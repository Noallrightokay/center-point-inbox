//! Reading the mailbox.
//!
//! Two jobs, and the difference between them matters to the customer. `verify`
//! runs once when a mailbox is linked: it finds the server, proves the password
//! works, and says which provider it turned out to be. `fetch_inbox` runs on
//! every refresh and brings back the newest messages.
//!
//! Three things are worth knowing before reading the code:
//!
//! **A rejected sign-in stops the search.** Walking the remaining candidate
//! hosts after the server has said "wrong password" means presenting the same
//! wrong password three more times, and providers count that: Google and
//! Microsoft both lock an account for repeated failures. So an auth failure
//! breaks the loop, and only an unreachable server moves on to the next name.
//!
//! **The socket opens to an address the guard judged**, not to a name it judged
//! earlier. See [`resolve_public`].
//!
//! **Nothing here writes anything down.** The password is borrowed for the
//! length of a connection and the messages are returned to the caller. Where
//! they are stored is the app's decision, not this crate's.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_imap::error::Error as ImapError;
use async_imap::types::Flag;
use async_imap::{Client, Session};
use futures::StreamExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use crate::discover::{Candidate, IMAP_PORT, Source, is_auth_failure};
use crate::guard::HostVerdict;
use crate::key::{domain_of, mail_key};
use crate::resolve::{Discovery, Resolver, discover, resolve_public};
use crate::words;

/// A wrong hostname must fail in seconds rather than hanging a refresh while
/// three of them are tried in turn.
const CONNECT: Duration = Duration::from_secs(10);
const GREETING: Duration = Duration::from_secs(10);
const COMMAND: Duration = Duration::from_secs(20);

/// How much of a message body is pulled back for the preview line. Enough for a
/// sentence or two; small enough that fifteen of them are one small response.
const PREVIEW_BYTES: usize = 2048;

/// Everything needed to open one mailbox.
#[derive(Debug, Clone)]
pub struct Account {
    pub email: String,
    /// An app password, in almost every case. Held only for the call.
    pub pass: String,
    pub host: String,
    pub port: u16,
    /// What to call this account in the interface — "Gmail", "Work", the
    /// address itself.
    pub label: String,
}

/// One message, flattened to what a combined inbox actually shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message {
    /// The account is part of the id: the same uid in two different mailboxes
    /// must not collide once they are shown in one list.
    pub id: String,
    pub acct: String,
    pub acct_label: String,
    pub from_name: String,
    pub from_addr: String,
    pub subject: String,
    pub preview: String,
    pub body: String,
    /// Milliseconds since the epoch, newest first once sorted.
    pub ts: i64,
    pub unread: bool,
    pub starred: bool,
}

/// What a successful link found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Verified {
    pub host: String,
    pub port: u16,
    /// The provider, for saying "that is Google Workspace" rather than showing
    /// a hostname nobody recognises.
    pub label: String,
    pub help: Option<String>,
    pub source: Source,
}

/// The outcome of linking a mailbox. Every unhappy answer is a sentence the
/// customer can act on, and `NeedsHost` is the one that asks them for the one
/// piece of information nothing else could supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verify {
    Ok(Box<Verified>),
    /// Settled without connecting, or refused outright.
    Failed(String),
    /// A server answered and rejected the password.
    Refused(String),
    /// Nothing answered. Show the "server address" box.
    NeedsHost(String),
}

/// Why a refresh produced nothing. `Auth` is separated because the caller must
/// stop retrying that mailbox — an app password that was revoked will be
/// revoked on the next refresh too, and repeating a rejected sign-in is how a
/// provider decides to lock the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Messages(Vec<Message>),
    Host(String),
    Auth(String),
    Net(String),
}

/// How a connection attempt failed, internally.
#[derive(Debug)]
enum Trouble {
    /// Refused before a socket was opened. Never retried.
    Host(String),
    /// Nothing answered, or the conversation broke. Try the next candidate.
    Net(String),
    /// The server said no. Stop.
    Auth(String),
}

// ---------------------------------------------------------------- connection

type Tls = TlsStream<TcpStream>;

/// One TLS configuration for the process. Built once: assembling it reads the
/// platform's trust store, which is not something to do per refresh.
///
/// The platform verifier rather than a bundled root list, deliberately. A
/// desktop app lives behind whatever the customer's employer has installed, and
/// a company that terminates TLS at its firewall has put its own root in the
/// system store — trusting that store is what makes RATA work on a managed
/// laptop, and it is the same set of roots their existing mail client uses.
fn tls() -> Result<Arc<ClientConfig>, String> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            use rustls_platform_verifier::BuilderVerifierExt;
            // The provider is named rather than taken from the process default:
            // if a second one is ever linked in, `builder()` panics at the first
            // connection instead of failing here.
            let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| format!("TLS could not be set up: {e}"))?
                .with_platform_verifier()
                .map_err(|e| format!("This computer's certificate store could not be read: {e}"))
                .map(|b| Arc::new(b.with_no_client_auth()))
        })
        .clone()
}

/// Open a verified TLS connection to a host, having judged where it points.
async fn open(resolver: &Resolver, host: &str, port: u16) -> Result<Client<Tls>, Trouble> {
    let (name, addrs) = match resolve_public(resolver, host).await {
        Ok(found) => found,
        Err(HostVerdict::NotFound) => {
            // Worth trying the next name rather than stopping.
            return Err(Trouble::Net(HostVerdict::NotFound.explain(host)));
        }
        Err(verdict) => return Err(Trouble::Host(verdict.explain(host))),
    };

    let tcp = dial(&addrs, port, host).await.map_err(Trouble::Net)?;
    let stream = wrap_tls(tcp, &name, host).await.map_err(Trouble::Net)?;

    let mut client = Client::new(stream);
    // IMAP servers speak first; nothing may be sent before they have.
    match timeout(GREETING, client.read_response()).await {
        Ok(Ok(Some(_))) => Ok(client),
        Ok(Ok(None)) => Err(Trouble::Net(format!(
            "{host} closed the connection without answering."
        ))),
        Ok(Err(e)) => Err(Trouble::Net(format!(
            "{host} did not answer as a mail server: {e}"
        ))),
        Err(_) => Err(Trouble::Net(format!("{host} did not answer in time."))),
    }
}

/// Open a socket to one of the vetted addresses.
///
/// Each in turn, because a name with both an IPv4 and an IPv6 answer is common
/// and on a network carrying only one of the two the other is a dead end rather
/// than a failure. Shared with [`crate::smtp`], which needs exactly the same
/// guarantee: the socket goes to an address that was judged, never to a name
/// that gets resolved a second time.
pub(crate) async fn dial(addrs: &[IpAddr], port: u16, host: &str) -> Result<TcpStream, String> {
    let mut last = String::new();
    for ip in addrs {
        match timeout(CONNECT, TcpStream::connect(SocketAddr::new(*ip, port))).await {
            Ok(Ok(s)) => return Ok(s),
            Ok(Err(e)) => last = e.to_string(),
            Err(_) => last = "timed out".into(),
        }
    }
    Err(if last.is_empty() {
        format!("{host} could not be reached.")
    } else {
        format!("{host} could not be reached: {last}")
    })
}

/// Wrap a socket in TLS.
///
/// The certificate is checked against `name` — the hostname — not against the
/// address the socket went to. That is what makes connecting by address safe
/// rather than a way around TLS: the address decides where the packets go, the
/// certificate decides who is allowed to be there.
pub(crate) async fn wrap_tls(tcp: TcpStream, name: &str, host: &str) -> Result<Tls, String> {
    let server =
        ServerName::try_from(name.to_string()).map_err(|_| HostVerdict::Malformed.explain(host))?;
    let config = tls()?;
    timeout(CONNECT, TlsConnector::from(config).connect(server, tcp))
        .await
        .map_err(|_| format!("{host} did not finish its security handshake in time."))?
        .map_err(|e| format!("{host} could not be trusted: {e}"))
}

/// Sign in. A `NO` from the server during LOGIN is the server refusing the
/// credentials, whatever words it chooses to refuse them in — more reliable
/// than reading the message, which is why it is checked first.
async fn sign_in(client: Client<Tls>, email: &str, pass: &str) -> Result<Session<Tls>, Trouble> {
    match timeout(COMMAND, client.login(email, pass)).await {
        Ok(Ok(session)) => Ok(session),
        // A NO that says, in so many words, that the *server* is the problem
        // right now — RFC 5530's UNAVAILABLE, INUSE and LIMIT, or a plain
        // "try again later". Reading those as a wrong password parks the
        // mailbox behind a relink the customer does not need.
        Ok(Err((ImapError::No(why), _))) if is_temporary_refusal(&why) => Err(Trouble::Net(why)),
        Ok(Err((ImapError::No(why), _))) => Err(Trouble::Auth(why)),
        // BAD is a complaint about the conversation, not the credentials: the
        // server did not understand what was sent. It says nothing about the
        // password, so it must not be read as a verdict on it.
        Ok(Err((ImapError::Bad(why), _))) => Err(Trouble::Net(format!(
            "the server did not understand the sign-in: {why}"
        ))),
        Ok(Err((e, _))) => {
            let msg = e.to_string();
            if is_auth_failure(&msg) {
                Err(Trouble::Auth(msg))
            } else {
                Err(Trouble::Net(msg))
            }
        }
        Err(_) => Err(Trouble::Net("the sign-in did not finish in time".into())),
    }
}

/// Whether a `NO` to LOGIN is the server declining for now rather than
/// refusing the password. The response code is the reliable part where a
/// server sends one; the wording is the fallback where it does not.
fn is_temporary_refusal(why: &str) -> bool {
    let w = why.to_ascii_lowercase();
    // An explicit verdict on the credentials wins over any wording after it:
    // "[AUTHENTICATIONFAILED] … try again" is still a wrong password.
    if w.contains("authenticationfailed") || w.contains("authorizationfailed") {
        return false;
    }
    [
        "unavailable",
        "inuse",
        "[limit]",
        "try again",
        "temporar",
        "too many",
        "later",
    ]
    .iter()
    .any(|needle| w.contains(needle))
}

// -------------------------------------------------------------------- verify

/// Prove the credentials work — and find the server while we are at it —
/// before anything is saved. A typo then surfaces as a sign-in error rather
/// than a mailbox that silently never syncs.
pub async fn verify(
    resolver: &Resolver,
    email: &str,
    pass: &str,
    host_override: Option<&str>,
) -> Verify {
    let found = discover(resolver, email, host_override).await;
    let (hosts, filtered_by) = match found {
        // The domain's own DNS already answered the question, and the answer
        // was "there is no mailbox here". Trying anyway would spend three
        // timeouts arriving at a worse version of the same sentence.
        Discovery::Refuse(why) => return Verify::Failed(why),
        Discovery::Candidates { hosts, filtered_by } => (hosts, filtered_by),
    };

    let mut tried: Vec<String> = Vec::new();
    let mut last_net: Option<String> = None;
    let mut blocked: Option<String> = None;

    for cand in &hosts {
        // Recorded before the attempt, not after it. Only a host that reached
        // the point of being dialled ends up here, and if none of them answers
        // the customer is owed the list — "RATA tried the usual server names"
        // is not something an IT administrator can act on.
        tried.push(cand.host.clone());

        let client = match open(resolver, &cand.host, cand.port).await {
            Ok(c) => c,
            // Refused by the guard: this *name* points somewhere RATA will
            // not connect. A different name is a different place — a stale
            // private `imap.` record must not stop a public `mail.` from being
            // tried, which is exactly how `smtp::send` already behaves.
            Err(Trouble::Host(why)) => {
                blocked.get_or_insert(why);
                continue;
            }
            Err(Trouble::Net(why) | Trouble::Auth(why)) => {
                last_net = Some(why);
                continue;
            }
        };

        match sign_in(client, email, pass).await {
            Ok(mut session) => {
                let _ = timeout(COMMAND, session.logout()).await;
                return Verify::Ok(Box::new(Verified {
                    host: cand.host.clone(),
                    port: if cand.port == 0 { IMAP_PORT } else { cand.port },
                    label: cand.label.clone(),
                    help: cand.help.clone(),
                    source: cand.source,
                }));
            }
            // The password is wrong. Every remaining candidate is the same
            // password at another address, and providers count failures.
            Err(Trouble::Auth(_)) => return Verify::Refused(refusal(cand)),
            Err(Trouble::Net(why) | Trouble::Host(why)) => {
                last_net = Some(why);
                continue;
            }
        }
    }

    // Nothing answered, and at least one name pointed somewhere forbidden.
    // That is the more useful thing to say: it names a concrete problem.
    if let Some(why) = blocked {
        return Verify::Failed(why);
    }

    if let Some(given) = host_override {
        return Verify::Failed(format!(
            "Could not reach {given}. Check the server address with your mail provider."
        ));
    }

    let domain = domain_of(email);

    // A filter in front of the mailbox is the one failure where the domain's
    // DNS tells us something useful about why.
    if let Some(filter) = filtered_by {
        return Verify::NeedsHost(format!(
            "{domain} filters its mail through {filter}, which does not say where the mailbox itself is. Enter the IMAP server address below — your IT administrator will know it."
        ));
    }

    let names = if tried.is_empty() {
        "the usual server names".to_string()
    } else {
        tried.join(", ")
    };
    // The last thing that went wrong, carried rather than summarised away. A
    // blocked port, an expired certificate and no wifi at all produce the same
    // sentence otherwise, and only one of them is the customer's to fix.
    let because = match &last_net {
        Some(why) => format!(": {}", why.trim_end_matches('.')),
        None => String::new(),
    };
    Verify::NeedsHost(format!(
        "RATA checked {domain}’s DNS and tried {names} without finding a mailbox{because}. Enter the IMAP server address below — your provider or IT administrator lists it as \"IMAP server\" or \"incoming mail server\"."
    ))
}

/// What to tell someone whose password was refused. Almost always the same
/// cause — a normal account password where an app password is needed — so the
/// sentence says that, and says where theirs lives.
fn refusal(cand: &Candidate) -> String {
    let who = if cand.label.is_empty() {
        "The mail server"
    } else {
        &cand.label
    };
    match &cand.help {
        Some(help) => format!(
            "{who} rejected the sign-in. Use an app password, not your normal account password — {help}."
        ),
        None => format!(
            "{who} rejected the sign-in. Use an app password, not your normal account password."
        ),
    }
}

// --------------------------------------------------------------------- fetch

/// Everything wanted about a message, in one round trip. `PEEK` matters: a
/// plain `BODY[TEXT]` marks the message read, so merely refreshing would clear
/// the customer's unread count.
fn items() -> String {
    format!("(UID FLAGS INTERNALDATE ENVELOPE BODY.PEEK[TEXT]<0.{PREVIEW_BYTES}>)")
}

/// The newest `limit` messages in the inbox, newest first.
pub async fn fetch_inbox(resolver: &Resolver, acct: &Account, limit: u32) -> Fetched {
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };

    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Fetched::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };

    let mut session = match sign_in(client, &acct.email, &acct.pass).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Fetched::Auth(revoked_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };

    let mailbox = match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) => m,
        _ => {
            let _ = timeout(COMMAND, session.logout()).await;
            return Fetched::Net(format!(
                "{} did not sync — its inbox could not be opened. It will be tried again on the next refresh.",
                acct.email
            ));
        }
    };

    let total = mailbox.exists;
    if total == 0 || limit == 0 {
        let _ = timeout(COMMAND, session.logout()).await;
        return Fetched::Messages(vec![]);
    }
    // `n:*` rather than `n:total`: the mailbox can grow between SELECT and
    // FETCH, and `*` means "whatever the last one is now".
    let from = total.saturating_sub(limit - 1).max(1);
    let range = format!("{from}:*");

    let mut messages: Vec<Message> = Vec::new();
    {
        let stream = match timeout(COMMAND, session.fetch(&range, items())).await {
            Ok(Ok(s)) => s,
            // No logout on this path, and not from carelessness: the stream
            // borrows the session for as long as it exists, so the only way to
            // say goodbye politely would be to keep a connection that has
            // already failed. Dropping it closes the socket.
            _ => return Fetched::Net(unreachable_msg(acct, "the inbox could not be listed")),
        };
        futures::pin_mut!(stream);
        // A message that will not parse is skipped rather than failing the
        // refresh: one malformed message must not cost the customer the other
        // fourteen. A connection that stops answering is different — what
        // arrived so far is an arbitrary slice of the inbox, and presenting it
        // as the inbox would be a refresh that silently lost mail.
        loop {
            match timeout(COMMAND, stream.next()).await {
                Ok(Some(Ok(fetched))) => messages.push(build(acct, &fetched)),
                Ok(Some(Err(ImapError::Io(e)))) => {
                    return Fetched::Net(unreachable_msg(
                        acct,
                        &format!("the connection failed partway through the inbox ({e})"),
                    ));
                }
                Ok(Some(Err(ImapError::ConnectionLost))) => {
                    return Fetched::Net(unreachable_msg(
                        acct,
                        "the connection was lost partway through the inbox",
                    ));
                }
                Ok(Some(Err(_))) => {}
                Ok(None) => break,
                Err(_) => {
                    return Fetched::Net(unreachable_msg(
                        acct,
                        "the server stopped answering partway through the inbox",
                    ));
                }
            }
        }
    }
    let _ = timeout(COMMAND, session.logout()).await;

    messages.sort_by(|a, b| b.ts.cmp(&a.ts));
    Fetched::Messages(messages)
}

/// A refresh that failed for a reason worth keeping. The cause is carried
/// through rather than summarised away: "could not be reached" is the same
/// sentence for an expired certificate, a blocked port and a dead wifi
/// connection, and the customer can act on precisely one of those.
fn unreachable_msg(acct: &Account, why: &str) -> String {
    format!(
        "{} did not sync — {} could not be reached: {}. It will be tried again on the next refresh.",
        acct.email,
        acct.host,
        why.trim_end_matches('.')
    )
}

fn revoked_msg(acct: &Account, why: &str) -> String {
    format!(
        "{} rejected the sign-in — the app password has probably been revoked. Relink it in Accounts. The server said: {}",
        acct.email,
        why.trim()
    )
}

/// One IMAP response into one message.
fn build(acct: &Account, f: &async_imap::types::Fetch) -> Message {
    let env = f.envelope();
    let sender = env.and_then(|e| e.from.as_ref()).and_then(|a| a.first());

    let from_addr = sender
        .map(|a| {
            let mbox = a.mailbox.as_deref().unwrap_or_default();
            let host = a.host.as_deref().unwrap_or_default();
            if mbox.is_empty() || host.is_empty() {
                String::new()
            } else {
                format!(
                    "{}@{}",
                    String::from_utf8_lossy(mbox),
                    String::from_utf8_lossy(host)
                )
                .to_ascii_lowercase()
            }
        })
        .unwrap_or_default();

    let from_name = sender
        .and_then(|a| a.name.as_deref())
        .map(words::decode)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            if from_addr.is_empty() {
                "Unknown".to_string()
            } else {
                from_addr.clone()
            }
        });

    let subject = env
        .and_then(|e| e.subject.as_deref())
        .map(words::decode)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "(no subject)".to_string());

    let preview = f.text().map(words::plain).unwrap_or_default();

    // INTERNALDATE — when the server received it — rather than the `Date:`
    // header inside the message. The header is written by the sender and is
    // routinely wrong: a skewed clock or a spammer's future date would park a
    // message permanently at the top of the list.
    let ts = f.internal_date().map(|d| d.timestamp_millis()).unwrap_or(0);

    let mut unread = true;
    let mut starred = false;
    for flag in f.flags() {
        match flag {
            Flag::Seen => unread = false,
            Flag::Flagged => starred = true,
            _ => {}
        }
    }

    let uid = f.uid.unwrap_or(f.message);
    Message {
        id: format!("{}_{}", mail_key(&acct.email), uid),
        acct: acct.email.clone(),
        acct_label: if acct.label.is_empty() {
            acct.email.clone()
        } else {
            acct.label.clone()
        },
        from_name,
        from_addr,
        subject,
        body: if preview.is_empty() {
            format!("(preview unavailable)\n\n— Synced from {}.", acct.email)
        } else {
            format!("{preview}\n\n— Synced from {}.", acct.email)
        },
        preview: words::clip(&preview, 120),
        ts,
        unread,
        starred,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_asking_for_time_is_not_refusing_the_password() {
        use super::is_temporary_refusal as temp;
        // The shape async-imap hands over, around what real servers say.
        assert!(temp(
            r#"code: None, info: Some("[UNAVAILABLE] Temporary System Problem")"#
        ));
        assert!(temp(
            r#"code: None, info: Some("[ALERT] Too many simultaneous connections. (Failure)")"#
        ));
        assert!(temp(
            r#"code: None, info: Some("[INUSE] Mailbox in use, try again later")"#
        ));
        assert!(temp(
            r#"code: None, info: Some("[LIMIT] Too many connections")"#
        ));
    }

    #[test]
    fn a_refused_password_is_still_a_refused_password() {
        use super::is_temporary_refusal as temp;
        // Gmail, wrong app password.
        assert!(!temp(
            r#"code: None, info: Some("[AUTHENTICATIONFAILED] Invalid credentials (Failure)")"#
        ));
        // Outlook.
        assert!(!temp(r#"code: None, info: Some("LOGIN failed.")"#));
        // A credential verdict followed by friendly advice is still a verdict.
        assert!(!temp(
            r#"code: None, info: Some("[AUTHENTICATIONFAILED] Invalid credentials, try again")"#
        ));
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn acct(host: &str) -> Account {
        Account {
            email: "owner@example.com".into(),
            pass: "not-a-real-password".into(),
            host: host.into(),
            port: IMAP_PORT,
            label: "Work".into(),
        }
    }

    #[test]
    fn tls_is_configured_once_and_successfully() {
        // If the platform trust store cannot be read, nothing else in this
        // module can work, and it is better to learn that here.
        assert!(tls().is_ok(), "{:?}", tls().err());
    }

    #[test]
    fn a_private_server_address_is_refused_without_a_socket() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            for host in ["127.0.0.1", "192.168.1.1", "localhost", "::ffff:7f00:1"] {
                match fetch_inbox(&r, &acct(host), 15).await {
                    Fetched::Host(why) => assert!(
                        why.contains("private network") || why.contains("not a public"),
                        "{host}: {why}"
                    ),
                    other => panic!("{host} should have been refused outright: {other:?}"),
                }
            }
        });
    }

    #[test]
    fn a_host_that_does_not_exist_reads_as_a_network_failure_not_an_auth_one() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            let host = format!("nx-{}.invalid", std::process::id());
            match fetch_inbox(&r, &acct(&host), 15).await {
                // Net, emphatically not Auth: the caller stops retrying a
                // mailbox on Auth, and a DNS outage must not unlink everybody.
                Fetched::Net(why) => assert!(why.contains("did not sync"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn linking_a_provider_with_no_imap_never_opens_a_socket() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match verify(&r, "someone@proton.me", "x", None).await {
                Verify::Failed(why) => assert!(why.contains("no IMAP server"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_private_override_is_refused_rather_than_dialled() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match verify(&r, "me@example.com", "x", Some("192.168.1.1")).await {
                Verify::Failed(why) => assert!(why.contains("private network"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_domain_with_no_mail_anywhere_asks_for_the_server_address() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            let addr = format!("me@nx-{}.invalid", std::process::id());
            match verify(&r, &addr, "x", None).await {
                Verify::NeedsHost(why) => {
                    assert!(why.contains("IMAP server"), "{why}");
                    // It must name what was tried — "RATA tried the usual
                    // server names" is not a support call anyone can close.
                    assert!(
                        why.contains(&format!("imap.nx-{}", std::process::id())),
                        "{why}"
                    );
                    assert!(why.contains("DNS"), "{why}");
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn the_preview_fetch_peeks_so_a_refresh_cannot_mark_mail_read() {
        let q = items();
        assert!(q.contains("BODY.PEEK[TEXT]"), "{q}");
        assert!(!q.contains("BODY[TEXT]"), "{q}");
        assert!(
            q.contains("UID") && q.contains("FLAGS") && q.contains("ENVELOPE"),
            "{q}"
        );
    }

    #[test]
    fn a_refusal_names_the_provider_and_where_its_app_passwords_live() {
        let cand = Candidate {
            host: "imap.gmail.com".into(),
            port: IMAP_PORT,
            label: "Google Workspace".into(),
            help: Some("myaccount.google.com/apppasswords".into()),
            source: Source::Mx,
        };
        let msg = refusal(&cand);
        assert!(msg.contains("Google Workspace"), "{msg}");
        assert!(msg.contains("app password"), "{msg}");
        assert!(msg.contains("apppasswords"), "{msg}");
    }
}
