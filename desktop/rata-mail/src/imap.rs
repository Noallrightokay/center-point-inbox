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

use crate::body;
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
    /// The server's number for this message, and the mailbox generation it
    /// belongs to. Together they are the only safe way to act on it later: a
    /// UID means nothing once UIDVALIDITY has changed, and could then name a
    /// different message entirely. Zero means unknown, and an unknown message
    /// is never acted on.
    pub uid: u32,
    pub uidvalidity: u32,
    /// The message's own `Message-ID`, without the angle brackets, so a reply
    /// can name what it answers and land in the same thread. Empty when the
    /// sender gave none.
    pub message_id: String,
    /// True when `body` is not the whole message: it ran past what is fetched
    /// or past what RATA keeps. The rest is in the mailbox.
    pub truncated: bool,
    /// Attachments whose headers were in what was fetched. Opening the message
    /// gives the full list.
    pub attachments: Vec<body::Attachment>,
    /// Where the sender asked for replies to go, when that is not the From
    /// address — a mailing list, a ticket system. Empty means reply to
    /// `from_addr`.
    pub reply_to: String,
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
    /// Only from [`fetch_older`]: the mailbox was rebuilt since RATA read it,
    /// so "older than UID n" no longer means anything. Refresh first.
    Stale(String),
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
/// plain `BODY[]` marks the message read, so merely refreshing would clear the
/// customer's unread count.
///
/// The whole message rather than `BODY[TEXT]`, because the text alone cannot
/// be decoded: its encoding, charset and MIME structure are declared in the
/// headers. Only the start of it, so attachments mostly stay on the server.
fn items() -> String {
    format!(
        "(UID FLAGS INTERNALDATE ENVELOPE BODY.PEEK[]<0.{}>)",
        body::MESSAGE_BYTES
    )
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

    let generation = mailbox.uid_validity.unwrap_or(0);
    let total = mailbox.exists;
    if total == 0 || limit == 0 {
        let _ = timeout(COMMAND, session.logout()).await;
        return Fetched::Messages(vec![]);
    }
    // `n:*` rather than `n:total`: the mailbox can grow between SELECT and
    // FETCH, and `*` means "whatever the last one is now".
    let from = total.saturating_sub(limit - 1).max(1);
    let range = format!("{from}:*");

    let messages = match read_range(&mut session, acct, &range, generation).await {
        Ok(m) => m,
        Err(failed) => return failed,
    };
    let _ = timeout(COMMAND, session.logout()).await;
    Fetched::Messages(messages)
}

/// The largest message RATA will download when one is opened. Mail providers
/// cap messages at 25–50 MB; anything larger is not mail a person reads.
pub const WHOLE_MAX: u32 = 60 * 1024 * 1024;

/// How long a whole message may take to arrive: a large attachment over a slow
/// connection is minutes, not the seconds a command normally gets.
const DOWNLOAD: Duration = Duration::from_secs(300);

/// One message, in full — for reading all of it and saving its attachments.
#[derive(Debug)]
pub enum Whole {
    /// The raw message, every byte.
    Raw(Vec<u8>),
    /// Not in the mailbox any more: deleted or moved elsewhere.
    Gone,
    /// Larger than [`WHOLE_MAX`]; its size in bytes.
    TooLarge(u32),
    Stale(String),
    Host(String),
    Auth(String),
    Net(String),
}

/// The whole of one message, by UID. Its size is asked first, so a message too
/// large to be reasonable is refused before any of it is downloaded.
pub async fn fetch_whole(resolver: &Resolver, acct: &Account, uid: u32, uidvalidity: u32) -> Whole {
    if uid == 0 || uidvalidity == 0 {
        return Whole::Stale("RATA has no server reference for this message.".into());
    }
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Whole::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why)) => {
            return Whole::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.pass).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Whole::Auth(revoked_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Whole::Net(unreachable_msg(acct, &why));
        }
    };
    let found = whole_in(&mut session, acct, uid, uidvalidity).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn whole_in<T>(session: &mut Session<T>, acct: &Account, uid: u32, uidvalidity: u32) -> Whole
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) if m.uid_validity == Some(uidvalidity) => {}
        Ok(Ok(_)) => {
            return Whole::Stale(
                "The mailbox has been reorganised since RATA last read it. Refresh, then open the message again.".into(),
            );
        }
        _ => return Whole::Net(unreachable_msg(acct, "its inbox could not be opened")),
    }
    let size = match timeout(
        COMMAND,
        session.uid_fetch(uid.to_string(), "(UID RFC822.SIZE)"),
    )
    .await
    {
        Ok(Ok(stream)) => {
            let got: Vec<_> = stream.collect().await;
            got.iter()
                .filter_map(|f| f.as_ref().ok())
                .find(|f| f.uid == Some(uid))
                .and_then(|f| f.size)
        }
        _ => return Whole::Net(unreachable_msg(acct, "the message could not be found")),
    };
    let Some(size) = size else { return Whole::Gone };
    if size > WHOLE_MAX {
        return Whole::TooLarge(size);
    }
    let stream = match timeout(
        COMMAND,
        session.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])"),
    )
    .await
    {
        Ok(Ok(s)) => s,
        _ => return Whole::Net(unreachable_msg(acct, "the message could not be downloaded")),
    };
    futures::pin_mut!(stream);
    let mut raw = None;
    loop {
        match timeout(DOWNLOAD, stream.next()).await {
            Ok(Some(Ok(f))) => {
                if f.uid == Some(uid)
                    && let Some(body) = f.body()
                {
                    raw = Some(body.to_vec());
                }
            }
            Ok(Some(Err(_))) => {
                return Whole::Net(unreachable_msg(
                    acct,
                    "the connection failed while downloading the message",
                ));
            }
            Ok(None) => break,
            Err(_) => {
                return Whole::Net(unreachable_msg(
                    acct,
                    "the server stopped sending the message",
                ));
            }
        }
    }
    raw.map_or(Whole::Gone, Whole::Raw)
}

/// Particular messages again, by UID, newest first — for mail RATA stored before
/// it could decode message bodies, which is otherwise never fetched again. A
/// UID no longer in the mailbox is simply absent from the answer.
pub async fn fetch_uids(
    resolver: &Resolver,
    acct: &Account,
    uids: &[u32],
    uidvalidity: u32,
) -> Fetched {
    let uids: Vec<u32> = uids.iter().copied().filter(|u| *u != 0).collect();
    if uids.is_empty() {
        return Fetched::Messages(vec![]);
    }
    if uidvalidity == 0 {
        return Fetched::Stale("RATA has no server reference for these messages.".into());
    }
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
    let found = uids_in(&mut session, acct, &uids, uidvalidity).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn uids_in<T>(
    session: &mut Session<T>,
    acct: &Account,
    uids: &[u32],
    uidvalidity: u32,
) -> Fetched
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) if m.uid_validity == Some(uidvalidity) => {}
        // A rebuilt mailbox numbers its messages afresh: these UIDs could now
        // name different messages, and their text must not be put under the
        // old ones' names.
        Ok(Ok(_)) => {
            return Fetched::Stale(
                "The mailbox has been reorganised since RATA last read it. Refresh to pick it up again.".into(),
            );
        }
        _ => return Fetched::Net(unreachable_msg(acct, "its inbox could not be opened")),
    }
    let stream = match timeout(COMMAND, session.uid_fetch(join(uids), items())).await {
        Ok(Ok(s)) => s,
        _ => return Fetched::Net(unreachable_msg(acct, "those messages could not be read")),
    };
    match collect(stream, acct, uidvalidity).await {
        // Only what was asked for: a server may volunteer others.
        Ok(m) => Fetched::Messages(m.into_iter().filter(|m| uids.contains(&m.uid)).collect()),
        Err(failed) => failed,
    }
}

/// Messages older than `before_uid` — the oldest one RATA already has — newest
/// first, at most `limit` of them. An empty list means there is nothing older.
///
/// Paged by position rather than by date or UID arithmetic, because both of
/// those break on a real mailbox. The messages at or above `before_uid` are
/// counted, and the block just below them is fetched by sequence number. That
/// holds when `before_uid` itself has since been deleted from another device,
/// and it is immune to IMAP's `n:*` quirk — asking for "70 onwards" when the
/// newest is 60 returns 60, which is filtered out rather than counted.
pub async fn fetch_older(
    resolver: &Resolver,
    acct: &Account,
    before_uid: u32,
    uidvalidity: u32,
    limit: u32,
) -> Fetched {
    if before_uid == 0 || uidvalidity == 0 {
        return Fetched::Stale(
            "RATA has no server reference for its oldest message yet. Refresh, then try again."
                .into(),
        );
    }
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
    let found = older_in(&mut session, acct, before_uid, uidvalidity, limit).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

/// The part of [`fetch_older`] that talks to a signed-in session.
async fn older_in<T>(
    session: &mut Session<T>,
    acct: &Account,
    before_uid: u32,
    uidvalidity: u32,
    limit: u32,
) -> Fetched
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let mailbox = match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) => m,
        _ => {
            return Fetched::Net(unreachable_msg(acct, "its inbox could not be opened"));
        }
    };
    if mailbox.uid_validity != Some(uidvalidity) {
        return Fetched::Stale(
            "The mailbox has been reorganised since RATA last read it. Refresh, then load older mail again.".into(),
        );
    }
    if mailbox.exists == 0 || limit == 0 {
        return Fetched::Messages(vec![]);
    }

    // How many messages sit at or above the oldest one RATA has.
    let newer = match timeout(COMMAND, session.uid_fetch(format!("{before_uid}:*"), "UID")).await {
        Ok(Ok(stream)) => {
            let found: Vec<_> = stream.collect().await;
            found
                .iter()
                .filter_map(|f| f.as_ref().ok()?.uid)
                .filter(|u| *u >= before_uid)
                .count() as u32
        }
        _ => return Fetched::Net(unreachable_msg(acct, "the inbox could not be counted")),
    };
    let older = mailbox.exists.saturating_sub(newer);
    if older == 0 {
        return Fetched::Messages(vec![]);
    }
    let from = older.saturating_sub(limit - 1).max(1);
    let range = format!("{from}:{older}");
    match read_range(session, acct, &range, uidvalidity).await {
        // Filtered again by UID: if something was expunged between the count
        // and the fetch, positions shift, and a message RATA already has must
        // not come back as "older".
        Ok(m) => Fetched::Messages(m.into_iter().filter(|m| m.uid < before_uid).collect()),
        Err(failed) => failed,
    }
}

/// FETCH one range of sequence numbers and turn it into messages, newest
/// first. Shared by the newest-first refresh and paging back through history.
async fn read_range<T>(
    session: &mut Session<T>,
    acct: &Account,
    range: &str,
    generation: u32,
) -> Result<Vec<Message>, Fetched>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let stream = match timeout(COMMAND, session.fetch(range, items())).await {
        Ok(Ok(s)) => s,
        // The stream borrows the session for as long as it exists, so the
        // caller cannot say goodbye politely on this path; dropping the
        // connection closes the socket.
        _ => {
            return Err(Fetched::Net(unreachable_msg(
                acct,
                "the inbox could not be listed",
            )));
        }
    };
    collect(stream, acct, generation).await
}

/// Messages out of a FETCH response, newest first.
async fn collect<S>(stream: S, acct: &Account, generation: u32) -> Result<Vec<Message>, Fetched>
where
    S: futures::Stream<Item = Result<async_imap::types::Fetch, ImapError>>,
{
    let mut messages: Vec<Message> = Vec::new();
    futures::pin_mut!(stream);
    // A message that will not parse is skipped rather than failing the
    // refresh: one malformed message must not cost the customer the other
    // fourteen. A connection that stops answering is different — what
    // arrived so far is an arbitrary slice of the inbox, and presenting it
    // as the inbox would be a refresh that silently lost mail.
    loop {
        match timeout(COMMAND, stream.next()).await {
            // A message flagged \Deleted is on its way out — deleted by RATA
            // on a server without UIDPLUS, or by another client — and showing
            // it would undo the delete on screen.
            Ok(Some(Ok(fetched))) if fetched.flags().any(|f| f == Flag::Deleted) => {}
            Ok(Some(Ok(fetched))) => messages.push(build(acct, &fetched, generation)),
            Ok(Some(Err(ImapError::Io(e)))) => {
                return Err(Fetched::Net(unreachable_msg(
                    acct,
                    &format!("the connection failed partway through the inbox ({e})"),
                )));
            }
            Ok(Some(Err(ImapError::ConnectionLost))) => {
                return Err(Fetched::Net(unreachable_msg(
                    acct,
                    "the connection was lost partway through the inbox",
                )));
            }
            Ok(Some(Err(_))) => {}
            Ok(None) => break,
            Err(_) => {
                return Err(Fetched::Net(unreachable_msg(
                    acct,
                    "the server stopped answering partway through the inbox",
                )));
            }
        }
    }
    messages.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(messages)
}

// ----------------------------------------------------------------------- act

/// Something the customer did to a message in RATA, done to the real mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Action {
    Read,
    Unread,
    Star,
    Unstar,
    /// Moved to the server's Trash — never deleted outright.
    Trash,
    /// Moved to the server's Archive (Gmail: "All Mail", which takes it out
    /// of the inbox and keeps it).
    Archive,
}

/// How acting on messages went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acted {
    /// `done` were changed. `gone` were no longer in the inbox — moved or
    /// deleted from another device — and were left alone; that is not a
    /// failure, the customer's intent already holds for them.
    Done {
        done: Vec<u32>,
        gone: Vec<u32>,
    },
    /// The mailbox was rebuilt since RATA read it, so its message numbers now
    /// mean something else — or RATA never had a server reference. Nothing was
    /// touched, which is the point.
    Stale(String),
    /// There is nowhere safe to put them: no Trash or Archive folder. Nothing
    /// was done rather than deleting anything for good.
    NoPlace(String),
    Auth(String),
    Host(String),
    Net(String),
}

/// Do `action` to messages `uids` in the inbox of `acct`, on one connection.
///
/// One sign-in for the whole set: a bulk delete of twenty is one conversation,
/// not twenty logins, which is what gets a mailbox rate-limited.
///
/// Two rules make this safe to run against somebody's real mail. It never
/// permanently deletes: "delete" is a move to the server's own Trash, and a
/// server without one is refused rather than expunged. And it never acts on a
/// number it cannot vouch for: the mailbox's UIDVALIDITY must still be the one
/// the messages were fetched under, and only the UIDs still present are
/// touched, otherwise the same number could name a different email.
pub async fn act(
    resolver: &Resolver,
    acct: &Account,
    uids: &[u32],
    uidvalidity: u32,
    action: Action,
) -> Acted {
    let uids: Vec<u32> = uids.iter().copied().filter(|u| *u != 0).collect();
    if uids.is_empty() || uidvalidity == 0 {
        return Acted::Stale(format!(
            "RATA does not have a server reference for this in {}, so the mailbox was left unchanged. Refresh and try again.",
            acct.email
        ));
    }
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Acted::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why)) => {
            return Acted::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.pass).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Acted::Auth(revoked_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Acted::Net(unreachable_msg(acct, &why));
        }
    };
    let done = apply(&mut session, &uids, uidvalidity, action).await;
    let _ = timeout(COMMAND, session.logout()).await;
    done
}

/// The part of [`act`] that talks to an already-signed-in session. Generic
/// over the stream so it can be driven by a scripted server in tests.
async fn apply<T>(session: &mut Session<T>, uids: &[u32], uidvalidity: u32, action: Action) -> Acted
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let failed = |what: &str, e: ImapError| Acted::Net(format!("The server would not {what}: {e}"));

    let mailbox = match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => return failed("open the inbox", e),
        Err(_) => return Acted::Net("The server did not open the inbox in time.".into()),
    };
    if mailbox.uid_validity != Some(uidvalidity) {
        return Acted::Stale(
            "The mailbox has been reorganised since RATA last read it, so its message numbers no longer match. Nothing was changed — refresh and try again.".into(),
        );
    }

    // Which of them are still there? A STORE or MOVE naming a UID that no
    // longer exists succeeds and does nothing, which would be reported as done.
    let asked = join(uids);
    let present: Vec<u32> = match timeout(COMMAND, session.uid_fetch(&asked, "UID")).await {
        Ok(Ok(stream)) => {
            let found: Vec<_> = stream.collect().await;
            let seen: Vec<u32> = found.iter().filter_map(|f| f.as_ref().ok()?.uid).collect();
            uids.iter().copied().filter(|u| seen.contains(u)).collect()
        }
        Ok(Err(e)) => return failed("look the messages up", e),
        Err(_) => return Acted::Net("The server did not answer in time.".into()),
    };
    let gone: Vec<u32> = uids
        .iter()
        .copied()
        .filter(|u| !present.contains(u))
        .collect();
    if present.is_empty() {
        return Acted::Done { done: vec![], gone };
    }
    let set = join(&present);

    let flag = |sign: char, name: &str| format!("{sign}FLAGS.SILENT ({name})");
    let stored = match action {
        Action::Read => Some(flag('+', "\\Seen")),
        Action::Unread => Some(flag('-', "\\Seen")),
        Action::Star => Some(flag('+', "\\Flagged")),
        Action::Unstar => Some(flag('-', "\\Flagged")),
        Action::Trash | Action::Archive => None,
    };
    if let Some(change) = stored {
        return match timeout(COMMAND, store(session, &set, &change)).await {
            Ok(Ok(())) => Acted::Done {
                done: present,
                gone,
            },
            Ok(Err(e)) => failed("update the messages", e),
            Err(_) => Acted::Net("The server did not answer in time.".into()),
        };
    }

    let Some(dest) = destination(session, action).await else {
        let what = if action == Action::Trash {
            "a Trash folder, so RATA left the messages where they are rather than delete them for good"
        } else {
            "an Archive folder, so RATA left the messages in the inbox"
        };
        return Acted::NoPlace(format!("This mailbox does not have {what}."));
    };

    let caps = match timeout(COMMAND, session.capabilities()).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return failed("list what it supports", e),
        Err(_) => return Acted::Net("The server did not answer in time.".into()),
    };
    let moved = if caps.has_str("MOVE") {
        timeout(COMMAND, session.uid_mv(&set, &dest)).await
    } else {
        // COPY, then flag the originals. Expunged only by UID: a plain EXPUNGE
        // would also remove every other message anybody had flagged \Deleted
        // in this inbox, which is not this action's to decide. Without
        // UIDPLUS the originals simply stay flagged, and RATA hides them.
        timeout(COMMAND, async {
            session.uid_copy(&set, &dest).await?;
            store(session, &set, "+FLAGS.SILENT (\\Deleted)").await?;
            if caps.has_str("UIDPLUS") {
                session.uid_expunge(&set).await?.collect::<Vec<_>>().await;
            }
            Ok(())
        })
        .await
    };
    match moved {
        Ok(Ok(())) => Acted::Done {
            done: present,
            gone,
        },
        Ok(Err(e)) => failed("move the messages", e),
        Err(_) => Acted::Net("The server did not answer in time.".into()),
    }
}

/// A UID set in IMAP's own syntax: `42,43,57`.
fn join(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// `UID STORE`, with the response stream drained so the session is ready for
/// the next command.
async fn store<T>(session: &mut Session<T>, set: &str, change: &str) -> Result<(), ImapError>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let stream = session.uid_store(set, change).await?;
    for item in stream.collect::<Vec<_>>().await {
        item?;
    }
    Ok(())
}

/// Where a Trash or Archive goes on this server, by the folder's declared
/// purpose (RFC 6154) rather than its name — "Trash", "Deleted Items",
/// "[Gmail]/Bin" and "Papierkorb" are all the same folder to a server that
/// says so. Archive falls back to Gmail's "All Mail", where moving a message
/// out of the inbox is exactly what archiving means.
async fn destination<T>(session: &mut Session<T>, action: Action) -> Option<String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    use async_imap::types::NameAttribute as A;
    let stream = timeout(COMMAND, session.list(Some(""), Some("*")))
        .await
        .ok()?
        .ok()?;
    let names: Vec<_> = stream.collect().await;
    let with = |want: &A| {
        names
            .iter()
            .flatten()
            .find(|n| n.attributes().contains(want) && !n.attributes().contains(&A::NoSelect))
    };
    let found = match action {
        Action::Trash => with(&A::Trash),
        Action::Archive => with(&A::Archive).or_else(|| with(&A::All)),
        _ => None,
    };
    found.map(|n| n.name().to_string())
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

/// A `Message-ID` fit to be written back into a reply's headers, or nothing.
///
/// Written by the sender, so checked rather than trusted: one `left@right`
/// token with no whitespace, control characters or brackets inside, and a sane
/// length. Anything else is dropped, and the reply simply goes out unthreaded —
/// a lost thread is a far smaller failure than a header a stranger wrote.
fn thread_id(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let id = text.trim().trim_start_matches('<').trim_end_matches('>');
    let clean = !id.is_empty()
        && id.len() < 400
        && id.contains('@')
        && !id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>');
    if clean { id.to_string() } else { String::new() }
}

/// One IMAP response into one message.
fn build(acct: &Account, f: &async_imap::types::Fetch, generation: u32) -> Message {
    let env = f.envelope();
    let sender = env.and_then(|e| e.from.as_ref()).and_then(|a| a.first());
    let address = |a: &async_imap::imap_proto::types::Address| {
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
    };

    let from_addr = sender.map(address).unwrap_or_default();

    // Servers fill Reply-To with the From address when the sender set none, so
    // only a different address is worth keeping.
    let reply_to = env
        .and_then(|e| e.reply_to.as_ref())
        .and_then(|a| a.first())
        .map(address)
        .filter(|r| !r.is_empty() && *r != from_addr)
        .unwrap_or_default();

    let message_id = env
        .and_then(|e| e.message_id.as_deref())
        .map(thread_id)
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

    let raw = f.body().unwrap_or_default();
    let text = body::read(raw, raw.len() >= body::MESSAGE_BYTES);
    let preview = body::preview(&text.text);

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
        body: if text.text.is_empty() {
            "(This message has no text RATA can show. Open it in your provider's own app to see it.)"
                .to_string()
        } else {
            text.text
        },
        truncated: text.truncated,
        attachments: text.attachments,
        preview,
        ts,
        unread,
        starred,
        // Not `uid` above, which falls back to the sequence number for the
        // id's sake. Acting on a sequence number as though it were a UID is
        // how the wrong message gets deleted.
        uid: f.uid.unwrap_or(0),
        uidvalidity: generation,
        message_id,
        reply_to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------- act tests
    //
    // A scripted IMAP server that answers like a real one and records every
    // command RATA sends. These tests are about what RATA *says* to somebody's
    // mailbox — above all, what it never says when something doesn't match.

    struct Script {
        caps: &'static str,
        list: &'static str,
        uidvalidity: u32,
        /// The UIDs the scripted inbox still holds.
        present: &'static [u32],
    }

    async fn scripted_session(
        script: Script,
    ) -> (Session<TcpStream>, Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut lines = BufReader::new(r).lines();
            w.write_all(b"* OK scripted IMAP ready\r\n").await.unwrap();
            while let Ok(Some(line)) = lines.next_line().await {
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                log.lock().unwrap().push(cmd.to_string());
                let up = cmd.to_ascii_uppercase();
                let body = if up.starts_with("SELECT") {
                    format!(
                        "* 3 EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n",
                        script.uidvalidity
                    )
                } else if up.starts_with("UID FETCH") {
                    // Answer only for the asked-for UIDs this inbox still has.
                    let asked = cmd.split_whitespace().nth(2).unwrap_or("");
                    asked
                        .split(',')
                        .filter_map(|u| u.parse::<u32>().ok())
                        .filter(|u| script.present.contains(u))
                        .enumerate()
                        .map(|(i, u)| format!("* {} FETCH (UID {u})\r\n", i + 1))
                        .collect()
                } else if up.starts_with("LIST") {
                    script.list.to_string()
                } else if up.starts_with("CAPABILITY") {
                    format!("* CAPABILITY IMAP4rev1 {}\r\n", script.caps)
                } else if up.starts_with("LOGOUT") {
                    "* BYE\r\n".to_string()
                } else {
                    String::new()
                };
                let reply = format!("{body}{tag} OK done\r\n");
                if w.write_all(reply.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        let tcp = TcpStream::connect(addr).await.unwrap();
        let mut client = async_imap::Client::new(tcp);
        client.read_response().await.unwrap();
        let session = client
            .login("me@example.com", "pw")
            .await
            .map_err(|(e, _)| e)
            .unwrap();
        (session, seen)
    }

    fn rt_act() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    const TRASH: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"\r\n";
    const GMAIL: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n";
    const NO_FOLDERS: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n";

    fn changed(log: &[String]) -> Vec<&String> {
        log.iter()
            .filter(|c| {
                let u = c.to_ascii_uppercase();
                u.starts_with("UID STORE")
                    || u.starts_with("UID MOVE")
                    || u.starts_with("UID COPY")
                    || u.contains("EXPUNGE")
            })
            .collect()
    }

    // ------------------------------------------------------- older-mail tests
    //
    // A scripted inbox with real positions and UIDs, answering FETCH with full
    // envelopes and bodies, and reproducing IMAP's `n:*` quirk. What is under
    // test is the paging arithmetic, which is where history gets duplicated
    // or skipped.

    async fn scripted_inbox(
        uids: &'static [u32],
        uidvalidity: u32,
    ) -> (Session<TcpStream>, Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut lines = BufReader::new(r).lines();
            w.write_all(b"* OK scripted IMAP ready\r\n").await.unwrap();
            let full = |seq: usize, uid: u32| {
                // What a real server sends for BODY[]: headers and a MIME
                // body, here quoted-printable UTF-8 as most mail programs write.
                let body = format!(
                    "From: Ann <ann@example.org>\r\nSubject: Subject {uid}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nBody of {uid} =E2=80=94 caf=C3=A9\r\n"
                );
                format!(
                    "* {seq} FETCH (UID {uid} FLAGS (\\Seen) INTERNALDATE \"01-Jan-2026 10:{:02}:00 +0000\" ENVELOPE (\"Thu, 1 Jan 2026 10:00:00 +0000\" \"Subject {uid}\" ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((NIL NIL \"me\" \"example.com\")) NIL NIL NIL \"<m{uid}@example.org>\") BODY[]<0> {{{}}}\r\n{body})\r\n",
                    uid / 2, // minutes that keep receive-order equal to UID order for 0..119
                    body.len()
                )
            };
            while let Ok(Some(line)) = lines.next_line().await {
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                log.lock().unwrap().push(cmd.to_string());
                let up = cmd.to_ascii_uppercase();
                let arg = cmd
                    .split_whitespace()
                    .nth(if up.starts_with("UID ") { 2 } else { 1 })
                    .unwrap_or("");
                let body = if up.starts_with("SELECT") {
                    format!(
                        "* {} EXISTS\r\n* OK [UIDVALIDITY {uidvalidity}] ok\r\n",
                        uids.len()
                    )
                } else if up.starts_with("UID FETCH") && up.contains("RFC822.SIZE") {
                    // UID 99 is a message far too large to download.
                    let want: u32 = arg.parse().unwrap_or(0);
                    uids.iter()
                        .enumerate()
                        .filter(|(_, u)| **u == want)
                        .map(|(i, u)| {
                            let size = if *u == 99 { u32::MAX } else { 400 };
                            format!("* {} FETCH (UID {u} RFC822.SIZE {size})\r\n", i + 1)
                        })
                        .collect()
                } else if up.starts_with("UID FETCH") && up.contains("BODY.PEEK") {
                    // Particular messages by UID, in full.
                    let want: Vec<u32> = arg.split(',').filter_map(|u| u.parse().ok()).collect();
                    uids.iter()
                        .enumerate()
                        .filter(|(_, u)| want.contains(u))
                        .map(|(i, u)| full(i + 1, *u))
                        .collect()
                } else if up.starts_with("UID FETCH") {
                    // "n:*" — every message with UID >= n, or, if there is
                    // none, the newest message anyway (RFC 3501's quirk).
                    let n: u32 = arg.trim_end_matches(":*").parse().unwrap_or(0);
                    let mut hits: Vec<(usize, u32)> = uids
                        .iter()
                        .enumerate()
                        .filter(|(_, u)| **u >= n)
                        .map(|(i, u)| (i + 1, *u))
                        .collect();
                    if hits.is_empty() && !uids.is_empty() {
                        hits.push((uids.len(), *uids.last().unwrap()));
                    }
                    hits.iter()
                        .map(|(seq, u)| format!("* {seq} FETCH (UID {u})\r\n"))
                        .collect()
                } else if up.starts_with("FETCH") {
                    let (a, b) = arg.split_once(':').unwrap();
                    let a: usize = a.parse().unwrap();
                    let b: usize = if b == "*" {
                        uids.len()
                    } else {
                        b.parse().unwrap()
                    };
                    (a..=b)
                        .filter(|s| *s >= 1 && *s <= uids.len())
                        .map(|s| full(s, uids[s - 1]))
                        .collect()
                } else if up.starts_with("LOGOUT") {
                    "* BYE\r\n".to_string()
                } else {
                    String::new()
                };
                if w.write_all(format!("{body}{tag} OK done\r\n").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
        let tcp = TcpStream::connect(addr).await.unwrap();
        let mut client = async_imap::Client::new(tcp);
        client.read_response().await.unwrap();
        let session = client
            .login("me@example.com", "pw")
            .await
            .map_err(|(e, _)| e)
            .unwrap();
        (session, seen)
    }

    fn me() -> Account {
        Account {
            email: "me@example.com".into(),
            pass: "pw".into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Me".into(),
        }
    }

    fn uids_of(f: Fetched) -> Vec<u32> {
        match f {
            Fetched::Messages(m) => m.iter().map(|m| m.uid).collect(),
            other => panic!("expected messages, got {other:?}"),
        }
    }

    #[test]
    fn older_mail_is_the_block_just_below_what_rata_has() {
        rt_act().block_on(async {
            // RATA holds 40, 50, 60; ask for two older than 40.
            let (mut s, log) = scripted_inbox(&[10, 20, 30, 40, 50, 60], 7).await;
            let got = older_in(&mut s, &me(), 40, 7, 2).await;
            assert_eq!(uids_of(got), vec![30, 20], "newest first, nothing RATA has");
            let log = log.lock().unwrap();
            assert!(log.iter().any(|c| c.starts_with("FETCH 2:3 ")), "{log:?}");
        });
    }

    #[test]
    fn older_mail_still_lines_up_when_the_anchor_was_deleted_elsewhere() {
        rt_act().block_on(async {
            // RATA's oldest was 40, which has since been deleted on a phone.
            let (mut s, _) = scripted_inbox(&[10, 20, 30, 50, 60], 7).await;
            assert_eq!(
                uids_of(older_in(&mut s, &me(), 40, 7, 2).await),
                vec![30, 20]
            );
        });
    }

    #[test]
    fn the_n_star_quirk_does_not_hide_or_duplicate_a_message() {
        rt_act().block_on(async {
            // "70:*" with nothing above 60 returns 60 anyway. Counting it would
            // skip 60; it must come back as older.
            let (mut s, _) = scripted_inbox(&[10, 20, 30, 40, 50, 60], 7).await;
            assert_eq!(
                uids_of(older_in(&mut s, &me(), 70, 7, 2).await),
                vec![60, 50]
            );
        });
    }

    #[test]
    fn at_the_start_of_the_mailbox_there_is_nothing_older() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20, 30], 7).await;
            assert_eq!(
                uids_of(older_in(&mut s, &me(), 10, 7, 50).await),
                Vec::<u32>::new()
            );
            assert!(
                !log.lock().unwrap().iter().any(|c| c.starts_with("FETCH")),
                "fetched when nothing was older"
            );
        });
    }

    #[test]
    fn older_mail_from_a_rebuilt_mailbox_is_refused() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20, 30], 7).await;
            assert!(matches!(
                older_in(&mut s, &me(), 30, 9, 50).await,
                Fetched::Stale(_)
            ));
            assert!(
                !log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c.to_ascii_uppercase().contains("FETCH"))
            );
        });
    }

    #[test]
    fn paging_all_the_way_back_returns_every_message_exactly_once() {
        rt_act().block_on(async {
            let inbox: &'static [u32] = &[3, 7, 11, 19, 23, 42, 57, 58, 61, 99];
            let mut seen: Vec<u32> = vec![99, 61]; // what the first refresh brought
            loop {
                let (mut s, _) = scripted_inbox(inbox, 7).await;
                let oldest = *seen.iter().min().unwrap();
                let page = uids_of(older_in(&mut s, &me(), oldest, 7, 3).await);
                if page.is_empty() {
                    break;
                }
                assert!(page.len() <= 3);
                for u in &page {
                    assert!(!seen.contains(u), "{u} came back twice");
                }
                seen.extend(page);
            }
            seen.sort();
            assert_eq!(seen, inbox.to_vec());
        });
    }

    fn done(uids: &[u32]) -> Acted {
        Acted::Done {
            done: uids.to_vec(),
            gone: vec![],
        }
    }

    #[test]
    fn a_bulk_action_touches_only_the_messages_still_there_on_one_connection() {
        rt_act().block_on(async {
            // 43 was deleted from a phone since RATA last looked.
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: TRASH,
                uidvalidity: 7,
                present: &[42, 57],
            })
            .await;
            assert_eq!(
                apply(&mut s, &[42, 43, 57], 7, Action::Trash).await,
                Acted::Done {
                    done: vec![42, 57],
                    gone: vec![43]
                }
            );
            let log = log.lock().unwrap();
            assert!(
                log.iter().any(|c| c == "UID MOVE 42,57 \"Deleted Items\""),
                "{log:?}"
            );
            assert!(
                !log.iter()
                    .any(|c| c.contains("43") && !c.starts_with("UID FETCH")),
                "{log:?}"
            );
            // One sign-in for the lot.
            assert_eq!(
                log.iter()
                    .filter(|c| c.to_ascii_uppercase().starts_with("LOGIN"))
                    .count(),
                1
            );
        });
    }

    #[test]
    fn marking_read_sets_seen_on_that_one_message() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE",
                list: TRASH,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Read).await, done(&[42]));
            let log = log.lock().unwrap();
            assert!(
                log.iter()
                    .any(|c| c == "UID STORE 42 +FLAGS.SILENT (\\Seen)"),
                "{log:?}"
            );
        });
    }

    #[test]
    fn a_rebuilt_mailbox_is_never_touched() {
        rt_act().block_on(async {
            // Fetched under generation 9; the server is now on 7. UID 42 may
            // be a different email entirely.
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE",
                list: TRASH,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert!(matches!(
                apply(&mut s, &[42], 9, Action::Trash).await,
                Acted::Stale(_)
            ));
            let log = log.lock().unwrap();
            assert!(
                changed(&log).is_empty(),
                "changed a mailbox it could not vouch for: {log:?}"
            );
        });
    }

    #[test]
    fn a_message_that_is_no_longer_there_is_not_reported_as_done() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE",
                list: TRASH,
                uidvalidity: 7,
                present: &[],
            })
            .await;
            // Not a failure: what the customer wanted is already true.
            assert_eq!(
                apply(&mut s, &[42], 7, Action::Trash).await,
                Acted::Done {
                    done: vec![],
                    gone: vec![42]
                }
            );
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    #[test]
    fn delete_moves_to_the_folder_the_server_calls_trash() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: TRASH,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Trash).await, done(&[42]));
            let log = log.lock().unwrap();
            assert!(
                log.iter().any(|c| c == "UID MOVE 42 \"Deleted Items\""),
                "{log:?}"
            );
            assert!(
                !log.iter()
                    .any(|c| c.to_ascii_uppercase().contains("EXPUNGE")),
                "{log:?}"
            );
        });
    }

    #[test]
    fn with_no_trash_folder_nothing_is_deleted() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: NO_FOLDERS,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert!(matches!(
                apply(&mut s, &[42], 7, Action::Trash).await,
                Acted::NoPlace(_)
            ));
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    #[test]
    fn gmail_archive_goes_to_all_mail_and_delete_to_its_trash() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: GMAIL,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Archive).await, done(&[42]));
            assert!(
                log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c == "UID MOVE 42 \"[Gmail]/All Mail\"")
            );
        });
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: GMAIL,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Trash).await, done(&[42]));
            assert!(
                log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c == "UID MOVE 42 \"[Gmail]/Trash\"")
            );
        });
    }

    #[test]
    fn without_move_only_that_message_is_ever_expunged() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "UIDPLUS",
                list: TRASH,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Trash).await, done(&[42]));
            let log = log.lock().unwrap();
            assert!(
                log.iter().any(|c| c == "UID COPY 42 \"Deleted Items\""),
                "{log:?}"
            );
            assert!(
                log.iter()
                    .any(|c| c == "UID STORE 42 +FLAGS.SILENT (\\Deleted)"),
                "{log:?}"
            );
            assert!(log.iter().any(|c| c == "UID EXPUNGE 42"), "{log:?}");
            // A bare EXPUNGE would take every \Deleted message with it.
            assert!(
                !log.iter().any(|c| c.eq_ignore_ascii_case("EXPUNGE")),
                "{log:?}"
            );
        });
        rt_act().block_on(async {
            // No UIDPLUS either: nothing is expunged at all.
            let (mut s, log) = scripted_session(Script {
                caps: "",
                list: TRASH,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(apply(&mut s, &[42], 7, Action::Trash).await, done(&[42]));
            assert!(
                !log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c.to_ascii_uppercase().contains("EXPUNGE"))
            );
        });
    }

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
        assert!(q.contains("BODY.PEEK[]<0."), "{q}");
        assert!(!q.contains(" BODY["), "{q}");
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

    // ---------------------------------------------------------- re-reading

    #[test]
    fn stored_mail_can_be_read_again_by_uid() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20, 30], 7).await;
            // 25 was never there; 20 and 30 are.
            let got = uids_in(&mut s, &me(), &[20, 25, 30], 7).await;
            let Fetched::Messages(m) = got else {
                panic!("{got:?}")
            };
            let mut uids: Vec<u32> = m.iter().map(|m| m.uid).collect();
            uids.sort();
            assert_eq!(uids, vec![20, 30]);
            assert!(
                m.iter().all(|m| m.body.contains("café")),
                "decoded: {:?}",
                m[0].body
            );
            let log = log.lock().unwrap();
            let fetch = log
                .iter()
                .find(|c| c.to_ascii_uppercase().starts_with("UID FETCH"))
                .unwrap();
            assert!(
                fetch.contains("20,25,30") && fetch.contains("BODY.PEEK[]"),
                "{fetch}"
            );
        });
    }

    #[test]
    fn stored_mail_is_not_re_read_from_a_rebuilt_mailbox() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20], 8).await;
            let got = uids_in(&mut s, &me(), &[10], 7).await;
            assert!(matches!(got, Fetched::Stale(_)), "{got:?}");
            assert!(
                !log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c.to_ascii_uppercase().starts_with("UID FETCH"))
            );
        });
    }

    // ------------------------------------------------------ opening in full

    #[test]
    fn an_opened_message_arrives_whole_and_decoded() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20], 7).await;
            let Whole::Raw(raw) = whole_in(&mut s, &me(), 20, 7).await else {
                panic!("expected the message")
            };
            assert_eq!(body::read_whole(&raw).text, "Body of 20 — café");
            let log = log.lock().unwrap();
            let asked: Vec<&String> = log
                .iter()
                .filter(|c| c.to_ascii_uppercase().starts_with("UID FETCH"))
                .collect();
            // Size first, then the message — and never a fetch that marks it read.
            assert!(asked[0].contains("RFC822.SIZE"), "{asked:?}");
            assert!(
                asked[1].contains("BODY.PEEK[]") && !asked[1].contains("<0."),
                "{asked:?}"
            );
        });
    }

    #[test]
    fn a_message_too_large_is_refused_before_it_is_downloaded() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[99], 7).await;
            assert!(matches!(
                whole_in(&mut s, &me(), 99, 7).await,
                Whole::TooLarge(_)
            ));
            assert!(!log.lock().unwrap().iter().any(|c| c.contains("BODY.PEEK")));
        });
    }

    #[test]
    fn a_message_that_has_gone_says_so() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_inbox(&[10], 7).await;
            assert!(matches!(whole_in(&mut s, &me(), 11, 7).await, Whole::Gone));
            let (mut s, _) = scripted_inbox(&[10], 8).await;
            assert!(matches!(
                whole_in(&mut s, &me(), 10, 7).await,
                Whole::Stale(_)
            ));
        });
    }

    // -------------------------------------------------------- reply threading

    #[test]
    fn a_message_id_is_kept_for_the_reply_to_thread_on() {
        assert_eq!(thread_id(b"<m42@example.org>"), "m42@example.org");
        assert_eq!(
            thread_id(b"  <CAF=x+y@mail.gmail.com>  "),
            "CAF=x+y@mail.gmail.com"
        );
    }

    #[test]
    fn a_message_id_that_could_smuggle_a_header_is_dropped() {
        // A reply writes this back into its own headers, so anything that could
        // break out of the angle brackets or the line is refused outright.
        assert_eq!(thread_id(b"<a@b>\r\nBcc: everyone@example.com"), "");
        assert_eq!(thread_id(b"<a@b> <c@d>"), "");
        assert_eq!(thread_id(b"<no-at-sign>"), "");
        assert_eq!(thread_id(b""), "");
        assert_eq!(thread_id(&[b'x'; 500]), "");
    }

    #[test]
    fn fetched_mail_carries_what_a_reply_needs() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_inbox(&[7, 9], 5).await;
            let got = read_range(&mut s, &me(), "1:2", 5).await.ok().unwrap();
            let seven = got.iter().find(|m| m.uid == 7).unwrap();
            assert_eq!(seven.message_id, "m7@example.org");
            // Decoded, not shown as the quoted-printable it arrived in.
            assert_eq!(seven.body, "Body of 7 — café");
            assert_eq!(seven.preview, "Body of 7 — café");
            assert!(!seven.truncated);
            // The fixture's Reply-To is the sender again, which is what servers
            // fill in when none was set; that is not worth keeping.
            assert_eq!(seven.reply_to, "");
        });
    }
}
