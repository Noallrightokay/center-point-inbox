//! Reading the mailbox.
//!
//! Two jobs, and the difference between them matters to the customer. `verify`
//! runs once when a mailbox is linked: it finds the server, proves the password
//! works, and says which provider it turned out to be. `fetch_newest` runs on
//! every refresh and brings back the newest messages — from the inbox, and
//! from the Sent folder, so a conversation shows both sides of it.
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
use async_imap::{Authenticator, Client, Session};
use futures::StreamExt;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use crate::body;
use crate::compose::{self, Outgoing};
use crate::credential::{self, Credential};
use crate::discover::{Candidate, IMAP_PORT, Source, is_auth_failure, is_microsoft};
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
    /// How to sign in: an app password in almost every case, an OAuth
    /// access token for a Microsoft mailbox. Held only for the call.
    pub credential: Credential,
    pub host: String,
    pub port: u16,
    /// What to call this account in the interface — "Gmail", "Work", the
    /// address itself.
    pub label: String,
}

/// Which folder of a mailbox a message lives in. A UID only means something
/// inside its own folder, so every request about a message names this too.
///
/// Serialised as `"inbox"`, `"sent"`, `"archive"`, `"junk"`, or
/// `{"named": "<server name>"}` for one of the customer's own folders.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Folder {
    #[default]
    Inbox,
    /// Mail the customer sent, from RATA or anywhere else — found by the
    /// purpose the server declares for it (RFC 6154), or failing that by the
    /// names providers give it.
    Sent,
    /// Mail archived out of the inbox. Only a folder that is an archive: Gmail
    /// has none — its "All Mail" holds the inbox and Sent too, and reading it
    /// would show every message twice — so on Gmail, Archive is All Mail
    /// searched for what is archived (see [`Archived`]).
    Archive,
    /// Spam, which is where real mail goes missing.
    Junk,
    /// Messages begun and not sent — on the phone, in webmail, anywhere —
    /// so they can be finished here.
    Drafts,
    /// One of the customer's own folders — a Gmail label is one too — by the
    /// name the server lists it under, exactly as LIST gave it (modified
    /// UTF-7 and all), since that is what SELECT needs. Only ever opened if
    /// it is still one of [`list_folders`]'s: never the inbox, never Trash,
    /// Drafts or a folder that is really Sent, Archive or Spam under another
    /// name, and never a name the server did not list.
    Named(String),
}

/// What the other folders a server keeps for itself are called, when it does
/// not say — so a server that declares nothing still does not show its Trash
/// and Drafts among the customer's own folders.
const SYSTEM_NAMES: &[&str] = &[
    "Trash",
    "Deleted",
    "Deleted Items",
    "Deleted Messages",
    "Bin",
    "Drafts",
    "Draft",
    "Outbox",
];

/// The most of the customer's own folders listed. Past this a folder list is
/// not something anyone picks from.
const FOLDERS_MAX: usize = 500;

/// What Sent is called on servers that do not say which folder it is.
const SENT_NAMES: &[&str] = &[
    "Sent",
    "Sent Items",
    "Sent Messages",
    "Sent Mail",
    "INBOX.Sent",
    "INBOX/Sent",
    "INBOX.Sent Items",
    "INBOX.Sent Messages",
];

/// What Archive is called on servers that do not say which folder it is.
const ARCHIVE_NAMES: &[&str] = &["Archive", "Archives", "INBOX.Archive", "INBOX/Archive"];

/// What Drafts is called on servers that do not say which folder it is.
const DRAFTS_NAMES: &[&str] = &["Drafts", "Draft", "INBOX.Drafts", "INBOX/Drafts"];

/// The most drafts a refresh lists by number. Nobody keeps more; a folder
/// past it is not a drafts folder anyone uses.
const DRAFTS_MAX: usize = 5000;

/// What Trash is called on servers that do not say which folder it is —
/// GreenMail's, and many a small host's. Only ever by the whole name: a
/// folder that merely contains the word ("Trash 2019", "Old Bin") is not it.
const TRASH_NAMES: &[&str] = &[
    "Trash",
    "Deleted Items",
    "Deleted Messages",
    "Bin",
    "INBOX.Trash",
    "INBOX/Trash",
];

/// What Spam is called on servers that do not say which folder it is.
const JUNK_NAMES: &[&str] = &[
    "Junk",
    "Spam",
    "Junk E-mail",
    "Junk Email",
    "Bulk Mail",
    "INBOX.Junk",
    "INBOX.Spam",
    "INBOX/Junk",
    "INBOX/Spam",
];

/// One message, flattened to what a combined inbox actually shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message {
    /// The account is part of the id: the same uid in two different mailboxes
    /// must not collide once they are shown in one list. So is the folder,
    /// for the same reason: `_sent_` for Sent.
    pub id: String,
    pub folder: Folder,
    pub acct: String,
    pub acct_label: String,
    pub from_name: String,
    pub from_addr: String,
    /// The first recipient — who a sent message went to.
    pub to_name: String,
    pub to_addr: String,
    /// Every address in To, and in Cc: what finishing a draft starts from.
    pub to_all: Vec<String>,
    pub cc: Vec<String>,
    /// Blind copies — only ever for a draft, whose Bcc is the customer's own
    /// and has to survive finishing it here. Empty for everything else.
    pub bcc: Vec<String>,
    /// The `Message-ID` this one answers, checked like `message_id`, so a
    /// draft of a reply stays in its thread when it is finished here.
    pub in_reply_to: String,
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
    /// For a draft RATA itself saved to Drafts, the id it wrote into it
    /// (`X-RATA-Draft`), so finishing it here saves over that copy rather
    /// than beside it. None for everything else.
    pub draft_id: Option<String>,
    /// True when `body` is not the whole message: it ran past what is fetched
    /// or past what RATA keeps. The rest is in the mailbox.
    pub truncated: bool,
    /// Attachments whose headers were in what was fetched. Opening the message
    /// gives the full list.
    pub attachments: Vec<body::Attachment>,
    /// Whether it has an HTML version, which opening it will show.
    pub html: bool,
    /// Where the sender asked for replies to go, when that is not the From
    /// address — a mailing list, a ticket system. Empty means reply to
    /// `from_addr`.
    pub reply_to: String,
    /// How to leave the mailing list it came from, from its
    /// `List-Unsubscribe` header ([`body::Unsubscribe`]). `None` when it has
    /// none RATA would act on, and for mail stored before this field.
    #[cfg_attr(feature = "serde", serde(default))]
    pub unsubscribe: Option<body::Unsubscribe>,
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
    /// A server answered and did not accept the OAuth token: get a fresh one.
    /// Never a verdict on a password.
    OAuth(String),
    /// Nothing answered. Show the "server address" box.
    NeedsHost(String),
    /// The mailbox is Microsoft's, which takes no password over IMAP: it signs
    /// in through Microsoft instead. Settled before the password was sent
    /// anywhere. Carries the provider, "Outlook" or "Microsoft 365".
    Microsoft(String),
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
    /// The OAuth access token was refused — expired, most likely, since they
    /// last about an hour. Get a fresh one and try again; unlike `Auth`, this
    /// says nothing about anything the customer typed.
    OAuth(String),
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
    /// The server said no to the password. Stop.
    Auth(String),
    /// The server said no to the OAuth token. Stop, and get a fresh one.
    OAuth(String),
}

impl Trouble {
    /// The same, with the credential's secret taken out of what it says.
    fn redacted(self, credential: &Credential) -> Trouble {
        let r = |why: String| credential::redact(&why, credential);
        match self {
            Trouble::Host(why) => Trouble::Host(r(why)),
            Trouble::Net(why) => Trouble::Net(r(why)),
            Trouble::Auth(why) => Trouble::Auth(r(why)),
            Trouble::OAuth(why) => Trouble::OAuth(r(why)),
        }
    }
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

/// The TLS configuration every connection RATA makes uses — the platform's
/// trust store, the named provider — for the app's one HTTPS client (signing
/// in with Microsoft), so that it trusts exactly what the mail connections do.
pub fn tls_client_config() -> Result<ClientConfig, String> {
    tls().map(|c| c.as_ref().clone())
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
            "{host} did not answer as a mail server: {}",
            io_reason(&e)
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

/// Sign in with whatever the mailbox signs in with. Whatever goes wrong is
/// said without the secret in it: every server sentence goes through
/// [`credential::said`], SMTP's rule, with what RATA sent as the secrets
/// (SEC-5).
async fn sign_in<T>(
    client: Client<T>,
    email: &str,
    credential: &Credential,
) -> Result<Session<T>, Trouble>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let secrets = sign_in_secrets(email, credential);
    // `redact` first, so an exact echo of a token still says so in its own
    // words; then the rule, which takes out everything else.
    let clean = |why: &str| credential::said(&credential::redact(why, credential), &secrets);
    match credential {
        Credential::Password(pass) => login(client, email, pass, &clean).await,
        Credential::OAuth { user, access_token } => xoauth2(client, user, access_token, &clean)
            .await
            .map_err(|t| t.redacted(credential)),
    }
}

/// What RATA sends to sign in, in every form a server's words could carry it
/// back. A password goes on the LOGIN line in the clear, quoted as
/// async-imap quotes it (`\` and `"` escaped); a token goes as the XOAUTH2
/// string in base64. And async-imap reports a NO or BAD as a `Debug` of the
/// server's text, which escapes `\` and `"` again — so each is also listed
/// as that `Debug` writes it. Longest first, so a whole LOGIN line goes as
/// one.
fn sign_in_secrets(email: &str, credential: &Credential) -> Vec<String> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let sent = match credential {
        Credential::Password(pass) => vec![
            format!("LOGIN \"{}\" \"{}\"", quote(email), quote(pass)),
            quote(pass),
            pass.clone(),
            words::base64_encode(pass.as_bytes()),
        ],
        Credential::OAuth { user, access_token } => {
            let mut sent = Vec::new();
            if let Some(sasl) = credential::xoauth2(user, access_token) {
                sent.push(words::base64_encode(&sasl));
            }
            sent.push(access_token.clone());
            sent
        }
    };
    let mut all = Vec::with_capacity(sent.len() * 2);
    for secret in sent {
        let debug = format!("{secret:?}");
        let debug = &debug[1..debug.len() - 1];
        if debug != secret {
            all.push(debug.to_string());
        }
        all.push(secret);
    }
    all
}

/// `XOAUTH2` as IMAP carries it: `AUTHENTICATE XOAUTH2`, the server's empty
/// `+`, then the one string [`credential::xoauth2`] builds.
///
/// Without SASL-IR (the string on the command line itself): RFC 3501 requires
/// every server to take this form, and SASL-IR would mean asking for the
/// capability list first. It costs one round trip, once per connection.
///
/// A refused token gets a second `+`, carrying base64 JSON that says why
/// (Google sends `{"status":"401",…}`). SASL requires an answer before the
/// server will finish, so the client sends an empty line and then reads the
/// tagged `NO`.
struct XOAuth2 {
    /// The string to send at the first `+`. Taken, so it is sent once.
    first: Option<Vec<u8>>,
    /// The server's error report, decoded, if it sent one.
    refused: Option<Vec<u8>>,
}

impl Authenticator for &mut XOAuth2 {
    type Response = Vec<u8>;

    fn process(&mut self, challenge: &[u8]) -> Vec<u8> {
        if let Some(first) = self.first.take() {
            return first;
        }
        // Any challenge after the token is the server saying no. An empty
        // answer acknowledges it, and the tagged NO follows.
        self.refused.get_or_insert_with(|| challenge.to_vec());
        Vec::new()
    }
}

/// Present an OAuth access token. A refusal is [`Trouble::OAuth`] — never
/// [`Trouble::Auth`], whatever words it comes in: the fix is a fresh token,
/// which the app can get by itself, not a new password from the customer.
///
/// `clean` makes a server's words fit to show (see [`sign_in`]).
async fn xoauth2<T>(
    client: Client<T>,
    user: &str,
    token: &str,
    clean: &impl Fn(&str) -> String,
) -> Result<Session<T>, Trouble>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let Some(sasl) = credential::xoauth2(user, token) else {
        return Err(Trouble::OAuth(
            "the sign-in token RATA holds is not one a mail server would accept".into(),
        ));
    };
    let mut auth = XOAuth2 {
        first: Some(sasl),
        refused: None,
    };
    let outcome = timeout(COMMAND, client.authenticate("XOAUTH2", &mut auth)).await;
    let status = auth
        .refused
        .as_deref()
        .and_then(credential::challenge_status);
    // The status is at most eight letters and digits (`challenge_status`).
    let with_status = |why: String| match &status {
        Some(s) => format!("{why} (status {s})"),
        None => why,
    };
    match outcome {
        Ok(Ok(session)) => Ok(session),
        // "Come back later" is the server, not the token — as with a password.
        Ok(Err((ImapError::No(why), _))) if is_temporary_refusal(&why) => {
            Err(Trouble::Net(clean(&why)))
        }
        Ok(Err((ImapError::No(why), _))) => Err(Trouble::OAuth(with_status(clean(&why)))),
        // The server's error report came first: that is its verdict, however
        // the exchange then ended.
        Ok(Err((e, _))) if auth.refused.is_some() => {
            Err(Trouble::OAuth(with_status(clean(&reason(&e)))))
        }
        // BAD is about the conversation — most likely a server that does not
        // know XOAUTH2 at all. Nothing was said about the token.
        Ok(Err((ImapError::Bad(why), _))) => Err(Trouble::Net(format!(
            "the server did not understand the sign-in: {}",
            clean(&why)
        ))),
        Ok(Err((e, _))) => Err(Trouble::Net(clean(&reason(&e)))),
        Err(_) => Err(Trouble::Net("the sign-in did not finish in time".into())),
    }
}

/// Sign in with a password. A `NO` from the server during LOGIN is the server
/// refusing the credentials, whatever words it chooses to refuse them in —
/// more reliable than reading the message, which is why it is checked first.
/// The server's words are judged as sent and shown only through `clean`
/// (see [`sign_in`]).
async fn login<T>(
    client: Client<T>,
    email: &str,
    pass: &str,
    clean: &impl Fn(&str) -> String,
) -> Result<Session<T>, Trouble>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match timeout(COMMAND, client.login(email, pass)).await {
        Ok(Ok(session)) => Ok(session),
        // A NO that says, in so many words, that the *server* is the problem
        // right now — RFC 5530's UNAVAILABLE, INUSE and LIMIT, or a plain
        // "try again later". Reading those as a wrong password parks the
        // mailbox behind a relink the customer does not need.
        Ok(Err((ImapError::No(why), _))) if is_temporary_refusal(&why) => {
            Err(Trouble::Net(clean(&why)))
        }
        Ok(Err((ImapError::No(why), _))) => Err(Trouble::Auth(clean(&why))),
        // BAD is a complaint about the conversation, not the credentials: the
        // server did not understand what was sent. It says nothing about the
        // password, so it must not be read as a verdict on it.
        Ok(Err((ImapError::Bad(why), _))) => Err(Trouble::Net(format!(
            "the server did not understand the sign-in: {}",
            clean(&why)
        ))),
        // Judged on everything the library said, reported in a line of RATA's.
        Ok(Err((e, _))) => {
            let msg = clean(&reason(&e));
            if !unreadable(&e) && is_auth_failure(&e.to_string()) {
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
///
/// With a password; [`verify_with`] takes any [`Credential`].
pub async fn verify(
    resolver: &Resolver,
    email: &str,
    pass: &str,
    host_override: Option<&str>,
) -> Verify {
    verify_with(
        resolver,
        email,
        &Credential::Password(pass.to_string()),
        host_override,
    )
    .await
}

/// [`verify`], signing in with `credential` — a password or an OAuth token.
pub async fn verify_with(
    resolver: &Resolver,
    email: &str,
    credential: &Credential,
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
    // The first provider RATA knows (from its table, or a recognised MX)
    // that could not be reached, and why. Its server address is not in
    // doubt, so a box asking for one would only send the customer looking
    // for something they already have.
    let mut known_down: Option<(Candidate, String)> = None;

    for cand in &hosts {
        // Recorded before the attempt, not after it. Only a host that reached
        // the point of being dialled ends up here, and if none of them answers
        // the customer is owed the list — "RATA tried the usual server names"
        // is not something an IT administrator can act on.
        tried.push(cand.host.clone());

        // Microsoft takes no password over IMAP. Sending one would only come
        // back "refused" — against a password that may well be right — and
        // count as a failed sign-in on the customer's account. Said before a
        // socket is opened.
        if matches!(credential, Credential::Password(_)) && is_microsoft(&cand.host) {
            return Verify::Microsoft(if cand.label.is_empty() {
                "Microsoft".into()
            } else {
                cand.label.clone()
            });
        }

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
            Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
                if known(cand) && known_down.is_none() {
                    known_down = Some((cand.clone(), why.clone()));
                }
                last_net = Some(why);
                continue;
            }
        };

        match sign_in(client, email, credential).await {
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
            Err(Trouble::Auth(why)) => return Verify::Refused(refusal(cand, &why)),
            // The same token goes to every candidate, so the same answer
            // would come back from each.
            Err(Trouble::OAuth(why)) => {
                let who = if cand.label.is_empty() {
                    "The mail server"
                } else {
                    &cand.label
                };
                return Verify::OAuth(format!(
                    "{who} did not accept the sign-in token. The server said: {}",
                    why.trim()
                ));
            }
            Err(Trouble::Net(why) | Trouble::Host(why)) => {
                if known(cand) && known_down.is_none() {
                    known_down = Some((cand.clone(), why.clone()));
                }
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
        return Verify::Failed(typed_host_failed(given, last_net.as_deref()));
    }

    if let Some((cand, why)) = known_down {
        return Verify::Failed(known_host_failed(&cand, &why));
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

/// What to tell someone whose typed server address did not work, with the
/// reason when there is one: an untrusted certificate, a refused port and a
/// wrong name are fixed in different places, and a refresh of the same server
/// already says which.
fn typed_host_failed(given: &str, why: Option<&str>) -> String {
    match why.map(str::trim).filter(|w| !w.is_empty()) {
        Some(why) => format!(
            "Could not connect to {given}: {}. Check the server address with your mail provider.",
            why.trim_end_matches('.')
        ),
        None => {
            format!("Could not reach {given}. Check the server address with your mail provider.")
        }
    }
}

/// Whether a candidate's server is one RATA knows — from its own table of
/// providers, or a provider it recognised in the domain's MX — rather than
/// one it guessed or was told.
fn known(cand: &Candidate) -> bool {
    matches!(cand.source, Source::Table | Source::Mx)
}

/// What to tell someone whose provider RATA knows but could not reach. The
/// server's address is not the problem, so this never asks for one: it names
/// the server and says why, and what usually fixes it.
fn known_host_failed(cand: &Candidate, why: &str) -> String {
    let who = if cand.label.is_empty() {
        cand.host.clone()
    } else {
        format!("{} ({})", cand.label, cand.host)
    };
    let why = why.trim().trim_end_matches('.');
    if why.is_empty() {
        format!("Could not reach {who}. Check your connection and try again in a few minutes.")
    } else {
        format!(
            "Could not reach {who}: {why}. Check your connection and try again in a few minutes."
        )
    }
}

/// What to tell someone whose password was refused. Almost always the same
/// cause — a normal account password where an app password is needed — so the
/// sentence says that, and says where theirs lives. The server's own words
/// follow (`said`: already one line, with no secret in it), because the
/// exception is worth reading: Gmail's "IMAP access is disabled for your
/// domain" has nothing to do with the password.
fn refusal(cand: &Candidate, said: &str) -> String {
    let who = if cand.label.is_empty() {
        "The mail server"
    } else {
        &cand.label
    };
    let advice = match &cand.help {
        Some(help) => format!(
            "{who} rejected the sign-in. Use an app password, not your normal account password — {help}."
        ),
        None => format!(
            "{who} rejected the sign-in. Use an app password, not your normal account password."
        ),
    };
    let said = said.trim();
    if said.is_empty() {
        advice
    } else {
        format!("{advice} The server said: {said}")
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

/// What RATA already holds from one folder: the newest UID it has, under
/// which UIDVALIDITY. Given this, a refresh downloads only what came after.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct Known {
    pub folder: Folder,
    pub uidvalidity: u32,
    pub since: u32,
}

/// A message RATA already has, as the server has it now.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Flags {
    pub id: String,
    pub unread: bool,
    pub starred: bool,
}

/// Messages a refresh left out: more arrived in a folder than one refresh
/// downloads ([`NEW_MAX`]), so those with UIDs above `floor` (the newest RATA
/// had) and below `top` (the oldest it downloaded now) are still to come —
/// by [`fetch_older`] from `top` down, page by page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Gap {
    pub folder: Folder,
    pub uidvalidity: u32,
    pub top: u32,
    pub floor: u32,
}

/// What a refresh found: messages new to RATA, whole, read/starred for
/// recent ones it already has, and any it had to leave for later.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Newest {
    pub messages: Vec<Message>,
    pub flags: Vec<Flags>,
    pub gaps: Vec<Gap>,
    /// Every draft in the Drafts folder now, by id, when it was read. A draft
    /// changes number each time it is saved elsewhere and goes when it is
    /// sent, so without this the drafts RATA holds only ever grow.
    pub drafts: Option<Vec<String>>,
    /// Gmail's archive, listed, when it was read: see [`Archived`].
    pub archived: Option<Archived>,
}

/// Which messages of Gmail's All Mail are archived, among those whose UID is
/// at least `floor`.
///
/// Gmail has no Archive folder: archiving takes a message out of the inbox
/// and leaves it in All Mail with the UID it arrived with. "Newer than the
/// newest held", which finds new mail everywhere else, never finds a message
/// archived today that arrived last week — so a refresh lists what is
/// archived near the top of All Mail, and the interface fetches what it does
/// not hold and drops what is no longer there (moved back to the inbox, or
/// deleted, on another device).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Archived {
    pub uidvalidity: u32,
    pub floor: u32,
    pub uids: Vec<u32>,
}

/// Gmail's search, over IMAP, for what is archived: in All Mail and in none
/// of the places RATA reads on their own. Any server but Gmail refuses it,
/// which leaves Archive out, as before.
const GMAIL_ARCHIVED: &str = "X-GM-RAW \"-in:inbox -in:sent -in:drafts\"";

/// How far below the top of All Mail, in UIDs, a refresh lists what is
/// archived. Roughly the last few weeks of mail for most people: a message
/// archived now that arrived before that turns up with Load older mail.
pub const ARCHIVE_WINDOW: u32 = 2000;

/// The most new messages one refresh downloads from one folder. A laptop
/// closed for a week comes back to hundreds; what is past this arrives with
/// Load older mail instead of in one enormous refresh.
pub const NEW_MAX: u32 = 200;

/// The newest mail of the inbox, Sent, Archive, Spam and Drafts, newest
/// first, on one connection.
///
/// For a folder RATA has read before (`known`), only messages newer than the
/// newest one it holds are downloaded, and the read and starred state of the
/// newest `limit` comes back without their text — so a refresh with nothing
/// new moves a few hundred bytes, and checking every few minutes costs
/// nothing. A folder RATA has not read, or whose UIDVALIDITY changed, gets its
/// newest `limit` messages whole.
///
/// The other folders are a bonus rather than a condition: a mailbox without
/// one, or where one will not open, still brings its inbox. The inbox is what
/// a refresh is for.
pub async fn fetch_newest(
    resolver: &Resolver,
    acct: &Account,
    limit: u32,
    known: &[Known],
) -> Result<Newest, Fetched> {
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };

    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Err(Fetched::Host(why)),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Err(Fetched::Net(unreachable_msg(acct, &why)));
        }
    };

    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Err(Fetched::Auth(revoked_msg(acct, &why))),
        Err(Trouble::OAuth(why)) => return Err(Fetched::OAuth(oauth_msg(acct, &why))),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Err(Fetched::Net(unreachable_msg(acct, &why)));
        }
    };

    let mut dial = Redial::new(Server { resolver, acct });
    let found = newest_everywhere(&mut session, &mut dial, acct, limit, known).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

/// The part of [`fetch_newest`] that talks to a signed-in session.
async fn newest_everywhere<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    limit: u32,
    known: &[Known],
) -> Result<Newest, Fetched> {
    let held = |folder: &Folder| known.iter().find(|k| k.folder == *folder);
    let Ok(Ok(mailbox)) = timeout(COMMAND, session.select("INBOX")).await else {
        return Err(Fetched::Net(format!(
            "{} did not sync — its inbox could not be opened. It will be tried again on the next refresh.",
            acct.email
        )));
    };
    let mut found = refresh_in(
        session,
        dial,
        acct,
        &Folder::Inbox,
        &mailbox,
        limit,
        held(&Folder::Inbox),
    )
    .await?;
    // The other folders are a bonus: any that is missing, will not open or
    // fails partway is left out, and the inbox still arrives. So is the rest
    // of them when a reply RATA could not read has left the connection
    // unusable and no other may be opened.
    if !dial.revive(session).await {
        return Ok(found);
    }
    let places = places(session).await;
    for folder in [Folder::Sent, Folder::Archive, Folder::Junk, Folder::Drafts] {
        let Some(name) = places.name(&folder) else {
            continue;
        };
        if !dial.revive(session).await {
            break;
        }
        let Selected::Open(mailbox) = select_name(session, name).await else {
            continue;
        };
        let read = if folder == Folder::Archive && places.all_mail {
            refresh_archived(session, dial, acct, &mailbox, limit, held(&folder)).await
        } else {
            refresh_in(session, dial, acct, &folder, &mailbox, limit, held(&folder)).await
        };
        if let Ok(mut more) = read {
            found.messages.append(&mut more.messages);
            found.flags.append(&mut more.flags);
            found.gaps.append(&mut more.gaps);
            if more.archived.is_some() {
                found.archived = more.archived;
            }
            if folder == Folder::Drafts && !dial.dead {
                found.drafts = all_ids(session, acct, &folder).await;
            }
        }
    }
    found.messages.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(found)
}

/// `UID SEARCH`, believed only when the server says it worked. The library's
/// own search reads a refusal (NO or BAD) as "found nothing", and "no drafts"
/// or "nothing archived" has the interface drop everything it holds there.
async fn search<T>(session: &mut Session<T>, query: &str) -> Option<Vec<u32>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    use async_imap::imap_proto::types::{MailboxDatum, Response, Status};
    let id = session
        .run_command(format!("UID SEARCH {query}"))
        .await
        .ok()?;
    let mut found = Vec::new();
    loop {
        let data = session.read_response().await.ok()??;
        match data.parsed() {
            Response::MailboxData(MailboxDatum::Search(uids)) => found.extend(uids.iter().copied()),
            Response::Done { tag, status, .. } if *tag == id => {
                return (*status == Status::Ok).then_some(found);
            }
            _ => {}
        }
    }
}

/// Every message of the folder just selected, by id — or nothing if the
/// server would not say, which must never read as "there are none".
async fn all_ids<T>(
    session: &mut Session<T>,
    acct: &Account,
    folder: &Folder,
) -> Option<Vec<String>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let uids = timeout(COMMAND, search(session, "ALL")).await.ok()??;
    let mut uids: Vec<u32> = uids.into_iter().filter(|u| *u != 0).collect();
    if uids.len() > DRAFTS_MAX {
        return None;
    }
    uids.sort_unstable();
    Some(
        uids.into_iter()
            .map(|u| message_key(acct, folder, u))
            .collect(),
    )
}

/// One folder of a refresh, just selected: see [`fetch_newest`].
async fn refresh_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    mailbox: &async_imap::types::Mailbox,
    limit: u32,
    known: Option<&Known>,
) -> Result<Newest, Fetched> {
    let generation = mailbox.uid_validity.unwrap_or(0);
    let Some(since) = known
        .filter(|k| k.since > 0 && generation != 0 && k.uidvalidity == generation)
        .map(|k| k.since)
    else {
        let messages = newest_in(session, dial, acct, folder, mailbox, limit).await?;
        return Ok(Newest {
            messages,
            ..Newest::default()
        });
    };
    if mailbox.exists == 0 || limit == 0 {
        return Ok(Newest::default());
    }
    let listing_failed = || Fetched::Net(unreachable_msg(acct, "the inbox could not be listed"));

    // Read and starred for the newest `limit`, as the server has them now.
    // No text: RATA has these already.
    let from = mailbox.exists.saturating_sub(limit - 1).max(1);
    let Ok(recent) = answered(session.fetch(format!("{from}:*"), "(UID FLAGS)")).await else {
        return Err(listing_failed());
    };
    let flags: Vec<Flags> = recent
        .iter()
        .filter_map(|f| Some((f.uid?, f)))
        .filter(|(uid, f)| *uid <= since && !f.flags().any(|x| x == Flag::Deleted))
        .map(|(uid, f)| Flags {
            id: message_key(acct, folder, uid),
            unread: !f.flags().any(|x| x == Flag::Seen),
            starred: f.flags().any(|x| x == Flag::Flagged),
        })
        .collect();

    // Everything after the newest message RATA holds. `n:*` returns the
    // newest message even when its UID is below n, so the answer is filtered.
    let Ok(after) =
        answered(session.uid_fetch(format!("{}:*", since.saturating_add(1)), "UID")).await
    else {
        return Err(listing_failed());
    };
    let mut fresh: Vec<u32> = after
        .iter()
        .filter_map(|f| f.uid)
        .filter(|u| *u > since)
        .collect();
    fresh.sort_unstable();
    fresh.dedup();
    let arrived = fresh.len();
    let fresh = &fresh[arrived.saturating_sub(NEW_MAX as usize)..];
    let gaps = if arrived > fresh.len() {
        vec![Gap {
            folder: folder.clone(),
            uidvalidity: generation,
            top: fresh[0],
            floor: since,
        }]
    } else {
        vec![]
    };
    if fresh.is_empty() {
        return Ok(Newest {
            flags,
            ..Newest::default()
        });
    }
    let messages = read_whole(
        session,
        dial,
        acct,
        folder,
        generation,
        Want::Uids(fresh),
        "the inbox could not be listed",
    )
    .await?
    .into_iter()
    .filter(|m| fresh.contains(&m.uid))
    .collect();
    Ok(Newest {
        messages,
        flags,
        gaps,
        drafts: None,
        archived: None,
    })
}

/// Gmail's archive, from All Mail just selected: like [`refresh_in`], but
/// only for the messages [`GMAIL_ARCHIVED`] finds, and with the listing that
/// lets the interface catch mail archived since it last looked.
async fn refresh_archived<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    mailbox: &async_imap::types::Mailbox,
    limit: u32,
    known: Option<&Known>,
) -> Result<Newest, Fetched> {
    let folder = Folder::Archive;
    let generation = mailbox.uid_validity.unwrap_or(0);
    if mailbox.exists == 0 || limit == 0 || generation == 0 {
        return Ok(Newest::default());
    }
    let unlisted = || Fetched::Net(unreachable_msg(acct, "its archive could not be listed"));
    let next = mailbox.uid_next.unwrap_or(u32::MAX);
    let floor = next.saturating_sub(ARCHIVE_WINDOW).max(1);
    let listed = archived(session, floor, None).await.ok_or_else(unlisted)?;

    let since = known
        .filter(|k| k.since > 0 && k.uidvalidity == generation)
        .map(|k| k.since);
    let (fresh, gaps) = match since {
        // Never read: the newest `limit` archived, searching further down if
        // the top of All Mail holds fewer than that.
        None if listed.len() >= limit as usize || floor == 1 => (tail(&listed, limit), vec![]),
        None => (
            newest_archived(session, next, limit)
                .await
                .ok_or_else(unlisted)?,
            vec![],
        ),
        Some(since) => {
            let after: Vec<u32> = listed.iter().copied().filter(|u| *u > since).collect();
            let kept = tail(&after, NEW_MAX);
            let gaps = if after.len() > kept.len() {
                vec![Gap {
                    folder: folder.clone(),
                    uidvalidity: generation,
                    top: kept[0],
                    floor: since,
                }]
            } else {
                vec![]
            };
            (kept, gaps)
        }
    };

    // Read and starred for the newest listed that RATA may hold already.
    let older: Vec<u32> = listed
        .iter()
        .copied()
        .filter(|u| !fresh.contains(u))
        .collect();
    let flags = flags_of(session, acct, &folder, &tail(&older, limit)).await?;
    let messages = by_uid(session, dial, acct, &folder, &fresh, generation).await?;
    Ok(Newest {
        messages,
        flags,
        gaps,
        drafts: None,
        archived: Some(Archived {
            uidvalidity: generation,
            floor,
            uids: listed,
        }),
    })
}

/// The archived UIDs of All Mail (just selected) from `lo` up to `hi`, or to
/// the top when `hi` is `None`, lowest first — or nothing if the server will
/// not search that way, which only Gmail does.
async fn archived<T>(session: &mut Session<T>, lo: u32, hi: Option<u32>) -> Option<Vec<u32>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let range = match hi {
        Some(hi) => format!("{lo}:{hi}"),
        None => format!("{lo}:*"),
    };
    let found = timeout(
        COMMAND,
        search(session, &format!("UID {range} {GMAIL_ARCHIVED}")),
    )
    .await
    .ok()??;
    // `lo:*` also names the newest message when its UID is below `lo`.
    let mut uids: Vec<u32> = found
        .into_iter()
        .filter(|u| *u >= lo && hi.is_none_or(|hi| *u <= hi))
        .collect();
    uids.sort_unstable();
    Some(uids)
}

/// The newest `want` archived messages below `below`, lowest first: the block
/// under it is searched, and a wider one each time that finds too few, so a
/// sparse archive under a busy inbox still fills a page.
async fn newest_archived<T>(session: &mut Session<T>, below: u32, want: u32) -> Option<Vec<u32>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let Some(hi) = below.checked_sub(1).filter(|h| *h > 0) else {
        return Some(vec![]);
    };
    let mut span = ARCHIVE_WINDOW;
    loop {
        let lo = hi.saturating_sub(span - 1).max(1);
        let found = archived(session, lo, Some(hi)).await?;
        if found.len() >= want as usize || lo == 1 {
            return Some(tail(&found, want));
        }
        span = span.saturating_mul(4);
    }
}

/// The last `n` of a list.
fn tail(list: &[u32], n: u32) -> Vec<u32> {
    list[list.len().saturating_sub(n as usize)..].to_vec()
}

/// Read and starred for `uids` of the folder just selected, without text.
async fn flags_of<T>(
    session: &mut Session<T>,
    acct: &Account,
    folder: &Folder,
    uids: &[u32],
) -> Result<Vec<Flags>, Fetched>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    if uids.is_empty() {
        return Ok(vec![]);
    }
    let Ok(got) = answered(session.uid_fetch(join(uids), "(UID FLAGS)")).await else {
        return Err(Fetched::Net(unreachable_msg(acct, &cannot_open(folder))));
    };
    Ok(got
        .iter()
        .filter_map(|f| Some((f.uid?, f)))
        .filter(|(uid, f)| uids.contains(uid) && !f.flags().any(|x| x == Flag::Deleted))
        .map(|(uid, f)| Flags {
            id: message_key(acct, folder, uid),
            unread: !f.flags().any(|x| x == Flag::Seen),
            starred: f.flags().any(|x| x == Flag::Flagged),
        })
        .collect())
}

/// `uids` of the folder just selected, whole, newest first — only those asked
/// for, since a server may volunteer others.
async fn by_uid<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    uids: &[u32],
    generation: u32,
) -> Result<Vec<Message>, Fetched> {
    if uids.is_empty() {
        return Ok(vec![]);
    }
    Ok(read_whole(
        session,
        dial,
        acct,
        folder,
        generation,
        Want::Uids(uids),
        "those messages could not be read",
    )
    .await?
    .into_iter()
    .filter(|m| uids.contains(&m.uid))
    .collect())
}

/// The newest `limit` messages of a folder just selected.
async fn newest_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    mailbox: &async_imap::types::Mailbox,
    limit: u32,
) -> Result<Vec<Message>, Fetched> {
    let generation = mailbox.uid_validity.unwrap_or(0);
    let total = mailbox.exists;
    if total == 0 || limit == 0 {
        return Ok(vec![]);
    }
    // `n:*` rather than `n:total`: the mailbox can grow between SELECT and
    // FETCH, and `*` means "whatever the last one is now".
    let from = total.saturating_sub(limit - 1).max(1);
    let range = format!("{from}:*");
    read_range(session, dial, acct, folder, &range, generation).await
}

/// How opening a folder went.
enum Selected {
    Open(async_imap::types::Mailbox),
    /// This mailbox has no such folder.
    Missing,
    Failed,
}

/// Open `folder` for reading and acting on.
async fn select<T>(session: &mut Session<T>, folder: &Folder) -> Selected
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    if *folder == Folder::Inbox {
        return select_name(session, "INBOX").await;
    }
    match name_of(session, folder).await {
        Some(name) => select_name(session, &name).await,
        None => Selected::Missing,
    }
}

/// The server's name for `folder` on this mailbox, if it has one. For one of
/// the customer's own folders that is its own name — but only while it is
/// still one of them: a page asking for "Deleted Items" or for a folder that
/// has since been removed gets nothing.
async fn name_of<T>(session: &mut Session<T>, folder: &Folder) -> Option<String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    if *folder == Folder::Inbox {
        return Some("INBOX".into());
    }
    let listed = listing(session).await?;
    let found = places_in(&listed);
    match folder {
        Folder::Named(want) => own_folders(&listed, &found)
            .into_iter()
            .find(|f| f.name == *want)
            .map(|f| f.name),
        other => found.name(other).map(str::to_string),
    }
}

async fn select_name<T>(session: &mut Session<T>, name: &str) -> Selected
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match timeout(COMMAND, session.select(name)).await {
        Ok(Ok(m)) => Selected::Open(m),
        _ => Selected::Failed,
    }
}

/// Where this server keeps Sent, Archive, Spam and Drafts, from one LIST.
#[derive(Debug, Default, PartialEq, Eq)]
struct Places {
    sent: Option<String>,
    archive: Option<String>,
    junk: Option<String>,
    drafts: Option<String>,
    /// Archive is Gmail's All Mail (`\All`, and no `\Archive`), which also
    /// holds the inbox, Sent and Drafts: only what [`GMAIL_ARCHIVED`] finds
    /// in it is archived.
    all_mail: bool,
}

impl Places {
    fn name(&self, folder: &Folder) -> Option<&str> {
        match folder {
            Folder::Inbox => Some("INBOX"),
            Folder::Sent => self.sent.as_deref(),
            Folder::Archive => self.archive.as_deref(),
            Folder::Junk => self.junk.as_deref(),
            Folder::Drafts => self.drafts.as_deref(),
            Folder::Named(_) => None,
        }
    }

    fn holds(&self, name: &str) -> bool {
        [&self.sent, &self.archive, &self.junk, &self.drafts]
            .iter()
            .any(|p| p.as_deref() == Some(name))
    }
}

/// Every folder the server lists, or nothing if it will not say.
async fn listing<T>(session: &mut Session<T>) -> Option<Vec<async_imap::types::Name>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    answered(session.list(Some(""), Some("*"))).await.ok()
}

async fn places<T>(session: &mut Session<T>) -> Places
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    listing(session)
        .await
        .map(|l| places_in(&l))
        .unwrap_or_default()
}

/// Whether a listed folder can be opened at all.
fn selectable(n: &async_imap::types::Name) -> bool {
    use async_imap::types::NameAttribute as A;
    !n.attributes().iter().any(|a| match a {
        A::NoSelect => true,
        A::Extension(x) => x.eq_ignore_ascii_case("\\NonExistent"),
        _ => false,
    })
}

/// Each folder by the purpose the server declares for it (RFC 6154), else by
/// the exact names providers give it — never by a name that merely contains
/// the word. None is ever the inbox, and no folder is two of them.
fn places_in(listed: &[async_imap::types::Name]) -> Places {
    use async_imap::types::NameAttribute as A;
    let usable: Vec<_> = listed
        .iter()
        .filter(|n| selectable(n) && !n.name().eq_ignore_ascii_case("INBOX"))
        .collect();
    let mut taken: Vec<String> = Vec::new();
    let mut pick = |attr: A, fallback: &[&str]| -> Option<String> {
        let by_attr = usable
            .iter()
            .find(|n| n.attributes().contains(&attr) && !taken.iter().any(|t| t == n.name()));
        let by_name = || {
            fallback.iter().find_map(|want| {
                usable.iter().find(|n| {
                    n.name().eq_ignore_ascii_case(want) && !taken.iter().any(|t| t == n.name())
                })
            })
        };
        let found = by_attr.or_else(by_name)?.name().to_string();
        taken.push(found.clone());
        Some(found)
    };
    let sent = pick(A::Sent, SENT_NAMES);
    let junk = pick(A::Junk, JUNK_NAMES);
    let drafts = pick(A::Drafts, DRAFTS_NAMES);
    let (archive, all_mail) = match pick(A::Archive, ARCHIVE_NAMES) {
        Some(archive) => (Some(archive), false),
        None => match pick(A::All, &[]) {
            Some(all) => (Some(all), true),
            None => (None, false),
        },
    };
    Places {
        sent,
        archive,
        junk,
        drafts,
        all_mail,
    }
}

/// One of the customer's own folders.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct OwnFolder {
    /// The server's name, exactly as listed — what [`Folder::Named`] carries.
    /// Not for showing: it may be in IMAP's modified UTF-7.
    pub name: String,
    /// For showing: decoded, its levels joined by " / ", and without the
    /// "INBOX." that some servers put in front of every folder.
    pub label: String,
}

/// The folders that are the customer's own, from a LIST: every folder that
/// can be opened, except the inbox and the ones the server keeps for itself —
/// Sent, Archive, Spam, Trash, Drafts, and Gmail's All Mail, Starred and
/// Important, which only show mail that is somewhere else already. Sorted by
/// how they read.
fn own_folders(listed: &[async_imap::types::Name], found: &Places) -> Vec<OwnFolder> {
    use async_imap::types::NameAttribute as A;
    let mut own: Vec<OwnFolder> = listed
        .iter()
        .filter(|n| selectable(n))
        .filter(|n| {
            !n.attributes().iter().any(|a| match a {
                A::All | A::Archive | A::Drafts | A::Flagged | A::Junk | A::Sent | A::Trash => true,
                A::Extension(x) => x.eq_ignore_ascii_case("\\Important"),
                _ => false,
            })
        })
        .filter(|n| !found.holds(n.name()))
        .filter(|n| {
            let bare = without_inbox(n.name(), n.delimiter());
            !bare.eq_ignore_ascii_case("INBOX")
                && !SYSTEM_NAMES.iter().any(|s| bare.eq_ignore_ascii_case(s))
        })
        .map(|n| OwnFolder {
            name: n.name().to_string(),
            label: folder_label(n.name(), n.delimiter()),
        })
        .collect();
    own.sort_by(|a, b| {
        a.label
            .to_lowercase()
            .cmp(&b.label.to_lowercase())
            .then(a.name.cmp(&b.name))
    });
    own.truncate(FOLDERS_MAX);
    own
}

/// A name without the "INBOX." (or "INBOX/") some servers put before every
/// folder. The inbox itself is left as it is.
fn without_inbox<'a>(name: &'a str, delimiter: Option<&str>) -> &'a str {
    match delimiter {
        // By `get`, not by slicing: a server that sends UTF-8 names can put a
        // character across byte 5, and slicing there would panic.
        Some(d) if !d.is_empty() && name.len() > 5 + d.len() => {
            match (name.get(..5), name.get(5..)) {
                (Some(head), Some(rest))
                    if head.eq_ignore_ascii_case("INBOX") && rest.starts_with(d) =>
                {
                    &rest[d.len()..]
                }
                _ => name,
            }
        }
        _ => name,
    }
}

/// How a folder's name reads: decoded, and its levels joined by " / ".
fn folder_label(name: &str, delimiter: Option<&str>) -> String {
    let bare = without_inbox(name, delimiter);
    let parts: Vec<String> = match delimiter {
        Some(d) if !d.is_empty() => bare.split(d).map(utf7_imap).collect(),
        _ => vec![utf7_imap(bare)],
    };
    parts.join(" / ")
}

/// IMAP's modified UTF-7 (RFC 3501 §5.1.3), which is how servers spell a
/// folder called "Entwürfe": `Entw&APw-rfe`. Anything that does not decode
/// is shown as it came.
fn utf7_imap(s: &str) -> String {
    fn sextet(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let chunk = &after[..end];
        rest = &after[end + 1..];
        if chunk.is_empty() {
            out.push('&');
            continue;
        }
        let mut bits = 0u32;
        let mut held = 0u32;
        let mut bytes = Vec::new();
        let mut ok = true;
        for c in chunk.bytes() {
            let Some(v) = sextet(c) else {
                ok = false;
                break;
            };
            bits = (bits << 6) | v;
            held += 6;
            if held >= 8 {
                held -= 8;
                bytes.push((bits >> held) as u8);
                bits &= (1 << held) - 1;
            }
        }
        if !ok || bytes.len() % 2 != 0 {
            out.push('&');
            out.push_str(chunk);
            out.push('-');
            continue;
        }
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        match String::from_utf16(&units) {
            Ok(text) => out.push_str(&text),
            Err(_) => {
                out.push('&');
                out.push_str(chunk);
                out.push('-');
            }
        }
    }
    out.push_str(rest);
    out
}

/// How listing a mailbox's folders went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listed {
    Folders(Vec<OwnFolder>),
    Host(String),
    Auth(String),
    /// See [`Fetched::OAuth`].
    OAuth(String),
    Net(String),
}

/// The customer's own folders in one mailbox — see [`own_folders`] for which
/// those are. Asked when the customer looks for them, not on every refresh.
pub async fn list_folders(resolver: &Resolver, acct: &Account) -> Listed {
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Listed::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Listed::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Listed::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Listed::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Listed::Net(unreachable_msg(acct, &why));
        }
    };
    let found = folders_in(&mut session, acct).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn folders_in<T>(session: &mut Session<T>, acct: &Account) -> Listed
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match listing(session).await {
        Some(listed) => Listed::Folders(own_folders(&listed, &places_in(&listed))),
        None => Listed::Net(unreachable_msg(acct, "its folders could not be listed")),
    }
}

/// The newest `limit` messages of one folder, newest first — how one of the
/// customer's own folders is read, when they open it.
pub async fn fetch_folder(
    resolver: &Resolver,
    acct: &Account,
    folder: Folder,
    limit: u32,
) -> Fetched {
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Fetched::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Fetched::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Fetched::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut dial = Redial::new(Server { resolver, acct });
    let found = folder_in(&mut session, &mut dial, acct, &folder, limit).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn folder_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    limit: u32,
) -> Fetched {
    match select(session, folder).await {
        Selected::Open(mailbox) => {
            match newest_in(session, dial, acct, folder, &mailbox, limit).await {
                Ok(m) => Fetched::Messages(m),
                Err(failed) => failed,
            }
        }
        Selected::Missing => Fetched::Stale(format!(
            "{} no longer has that folder — it was renamed or removed. Pick it again from Folders.",
            acct.email
        )),
        Selected::Failed => Fetched::Net(unreachable_msg(acct, &cannot_open(folder))),
    }
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
    /// See [`Fetched::OAuth`].
    OAuth(String),
    Net(String),
}

/// The whole of one message, by UID. Its size is asked first, so a message too
/// large to be reasonable is refused before any of it is downloaded.
pub async fn fetch_whole(
    resolver: &Resolver,
    acct: &Account,
    folder: Folder,
    uid: u32,
    uidvalidity: u32,
) -> Whole {
    if uid == 0 || uidvalidity == 0 {
        return Whole::Stale("RATA has no server reference for this message.".into());
    }
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Whole::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Whole::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Whole::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Whole::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Whole::Net(unreachable_msg(acct, &why));
        }
    };
    let found = whole_in(&mut session, acct, &folder, uid, uidvalidity).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn whole_in<T>(
    session: &mut Session<T>,
    acct: &Account,
    folder: &Folder,
    uid: u32,
    uidvalidity: u32,
) -> Whole
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match select(session, folder).await {
        Selected::Open(m) if m.uid_validity == Some(uidvalidity) => {}
        Selected::Open(_) | Selected::Missing => {
            return Whole::Stale(
                "The mailbox has been reorganised since RATA last read it. Refresh, then open the message again.".into(),
            );
        }
        Selected::Failed => return Whole::Net(unreachable_msg(acct, &cannot_open(folder))),
    }
    let size = match answered(session.uid_fetch(uid.to_string(), "(UID RFC822.SIZE)")).await {
        Ok(got) => got.iter().find(|f| f.uid == Some(uid)).and_then(|f| f.size),
        Err(_) => return Whole::Net(unreachable_msg(acct, "the message could not be found")),
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
    folder: Folder,
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
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Fetched::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Fetched::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut dial = Redial::new(Server { resolver, acct });
    let found = uids_in(&mut session, &mut dial, acct, &folder, &uids, uidvalidity).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

async fn uids_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    uids: &[u32],
    uidvalidity: u32,
) -> Fetched {
    match select(session, folder).await {
        Selected::Open(m) if m.uid_validity == Some(uidvalidity) => {}
        // A rebuilt mailbox numbers its messages afresh: these UIDs could now
        // name different messages, and their text must not be put under the
        // old ones' names.
        Selected::Open(_) | Selected::Missing => {
            return Fetched::Stale(
                "The mailbox has been reorganised since RATA last read it. Refresh to pick it up again.".into(),
            );
        }
        Selected::Failed => return Fetched::Net(unreachable_msg(acct, &cannot_open(folder))),
    }
    let read = read_whole(
        session,
        dial,
        acct,
        folder,
        uidvalidity,
        Want::Uids(uids),
        "those messages could not be read",
    )
    .await;
    match read {
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
    folder: Folder,
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
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Fetched::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Fetched::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Fetched::Net(unreachable_msg(acct, &why));
        }
    };
    let mut dial = Redial::new(Server { resolver, acct });
    let found = older_in(
        &mut session,
        &mut dial,
        acct,
        &folder,
        before_uid,
        uidvalidity,
        limit,
    )
    .await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

/// The part of [`fetch_older`] that talks to a signed-in session.
async fn older_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    before_uid: u32,
    uidvalidity: u32,
    limit: u32,
) -> Fetched {
    let mailbox = match select(session, folder).await {
        Selected::Open(m) => m,
        // A Sent folder that has gone has nothing older in it.
        Selected::Missing => return Fetched::Messages(vec![]),
        Selected::Failed => {
            return Fetched::Net(unreachable_msg(acct, &cannot_open(folder)));
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
    // Gmail's archive is a search of All Mail, not the whole of it.
    if *folder == Folder::Archive && places(session).await.all_mail {
        let Some(uids) = newest_archived(session, before_uid, limit).await else {
            return Fetched::Net(unreachable_msg(acct, "its archive could not be listed"));
        };
        return match by_uid(session, dial, acct, folder, &uids, uidvalidity).await {
            Ok(m) => Fetched::Messages(m),
            Err(failed) => failed,
        };
    }

    // How many messages sit at or above the oldest one RATA has.
    let newer = match answered(session.uid_fetch(format!("{before_uid}:*"), "UID")).await {
        Ok(found) => found
            .iter()
            .filter_map(|f| f.uid)
            .filter(|u| *u >= before_uid)
            .count() as u32,
        Err(_) => return Fetched::Net(unreachable_msg(acct, "the inbox could not be counted")),
    };
    let older = mailbox.exists.saturating_sub(newer);
    if older == 0 {
        return Fetched::Messages(vec![]);
    }
    let from = older.saturating_sub(limit - 1).max(1);
    let range = format!("{from}:{older}");
    match read_range(session, dial, acct, folder, &range, uidvalidity).await {
        // Filtered again by UID: if something was expunged between the count
        // and the fetch, positions shift, and a message RATA already has must
        // not come back as "older".
        Ok(m) => Fetched::Messages(m.into_iter().filter(|m| m.uid < before_uid).collect()),
        Err(failed) => failed,
    }
}

// -------------------------------------------------------------- server search

/// The longest search RATA sends a server, in characters. A search is a few
/// words; this keeps a pasted page from going out three times as literals.
pub const SEARCH_QUERY_MAX: usize = 200;

/// How many of the messages a server search finds are fetched: the newest.
pub const SEARCH_LIMIT: u32 = 50;

/// How long a server may take over a search. Longer than any other command:
/// a server without a text index reads every message to answer `TEXT`.
const SEARCH_TIME: Duration = Duration::from_secs(90);

/// What a search of one folder on the server found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Searched {
    /// The newest of them, whole, newest first: at most the `limit` asked for.
    pub messages: Vec<Message>,
    /// How many messages matched in all.
    pub matched: u32,
}

/// The query as it will be searched for, trimmed, or why it will not be.
/// Refused: nothing, more than [`SEARCH_QUERY_MAX`] characters, or a control
/// character. The query goes as a literal, so no character in it could
/// change the command, but a line break or a NUL is never something a
/// customer meant to look for, and a server may read one badly.
pub fn search_query(query: &str) -> Result<&str, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("Type something to search for.".into());
    }
    if q.chars().count() > SEARCH_QUERY_MAX {
        return Err(format!(
            "A search on the server can be at most {SEARCH_QUERY_MAX} characters."
        ));
    }
    if q.chars().any(char::is_control) {
        return Err("A search cannot contain a line break or another control character.".into());
    }
    Ok(q)
}

/// Messages in `folder` whose sender, subject or text contains `query`, as
/// the server finds them: one `UID SEARCH`, then the newest `limit` of what
/// it found, fetched whole by UID as [`fetch_uids`] fetches them, on the
/// same connection.
///
/// For mail RATA does not hold, older than anything it has read. The server
/// decides what matches (most ignore case; some match whole words only), and
/// RATA believes the answer only when the server says the search worked (the
/// rule of [`search`]): a refused search is an error, never "nothing found".
///
/// Not for one of the customer's own folders, nor for Gmail's archive, in
/// this version: a named folder is read only from its own view, and Gmail's
/// archive is All Mail searched for what is archived, so searching it would
/// take both searches in one. Both are refused before anything is searched.
pub async fn search_folder(
    resolver: &Resolver,
    acct: &Account,
    folder: Folder,
    query: &str,
    limit: u32,
) -> Result<Searched, Fetched> {
    let query = search_query(query).map_err(Fetched::Net)?;
    if matches!(folder, Folder::Named(_)) {
        return Err(Fetched::Net(not_searchable(&folder)));
    }
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Err(Fetched::Host(why)),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Err(Fetched::Net(unreachable_msg(acct, &why)));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Err(Fetched::Auth(revoked_msg(acct, &why))),
        Err(Trouble::OAuth(why)) => return Err(Fetched::OAuth(oauth_msg(acct, &why))),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Err(Fetched::Net(unreachable_msg(acct, &why)));
        }
    };
    let mut dial = Redial::new(Server { resolver, acct });
    let found = search_in(&mut session, &mut dial, acct, &folder, query, limit).await;
    let _ = timeout(COMMAND, session.logout()).await;
    found
}

/// The part of [`search_folder`] that talks to a signed-in session.
async fn search_in<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    query: &str,
    limit: u32,
) -> Result<Searched, Fetched> {
    let nothing = || Searched {
        messages: vec![],
        matched: 0,
    };
    let query = search_query(query).map_err(Fetched::Net)?;
    if matches!(folder, Folder::Named(_))
        || (*folder == Folder::Archive && places(session).await.all_mail)
    {
        return Err(Fetched::Net(not_searchable(folder)));
    }
    let mailbox = match select(session, folder).await {
        Selected::Open(m) => m,
        // A folder this mailbox does not have holds nothing to find.
        Selected::Missing => return Ok(nothing()),
        Selected::Failed => return Err(Fetched::Net(unreachable_msg(acct, &cannot_open(folder)))),
    };
    let generation = mailbox.uid_validity.unwrap_or(0);
    if mailbox.exists == 0 || limit == 0 {
        return Ok(nothing());
    }
    let mut uids = match timeout(SEARCH_TIME, text_search(session, query)).await {
        Ok(Ok(uids)) => uids,
        Ok(Err(why)) => {
            return Err(Fetched::Net(format!(
                "{} would not search {}. {why}",
                acct.email,
                place(folder)
            )));
        }
        Err(_) => {
            return Err(Fetched::Net(unreachable_msg(
                acct,
                "the search took too long to answer",
            )));
        }
    };
    uids.retain(|u| *u != 0);
    uids.sort_unstable();
    uids.dedup();
    let matched = u32::try_from(uids.len()).unwrap_or(u32::MAX);
    let newest = tail(&uids, limit);
    let messages = by_uid(session, dial, acct, folder, &newest, generation).await?;
    Ok(Searched { messages, matched })
}

/// `UID SEARCH OR OR FROM q SUBJECT q TEXT q`, with the query sent each time
/// as an IMAP literal (`{n}`, the server's go-ahead, then exactly n bytes),
/// never inside a quoted string, so nothing in it (a `"`, a `\`, a letter
/// outside ASCII) is ever read as part of the command.
///
/// `CHARSET UTF-8` goes with a query that is not ASCII, and only then:
/// every server searches US-ASCII without being told, and ASCII is already
/// UTF-8, so an ASCII query never needs it and never has it refused. A
/// server that refuses `CHARSET UTF-8` (a `NO [BADCHARSET]` or a `BAD`)
/// cannot search for a query that is not ASCII at all, since without it the
/// bytes would be read as US-ASCII, so there is no second try.
///
/// As in [`search`], only a tagged OK is believed; anything else, before or
/// after a literal, is a refusal, carrying the server's sentence as one line.
async fn text_search<T>(session: &mut Session<T>, query: &str) -> Result<Vec<u32>, String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    use async_imap::imap_proto::types::{MailboxDatum, Response, Status};
    let lost = || "The connection was lost.".to_string();
    let n = query.len();
    let charset = if query.is_ascii() {
        ""
    } else {
        "CHARSET UTF-8 "
    };
    let id = session
        .run_command(format!("UID SEARCH {charset}OR OR FROM {{{n}}}"))
        .await
        .map_err(|e| reason(&e))?;
    for next in [
        format!("{query} SUBJECT {{{n}}}"),
        format!("{query} TEXT {{{n}}}"),
        query.to_string(),
    ] {
        // The go-ahead for the literal, or a refusal before it is sent.
        loop {
            let data = session
                .read_response()
                .await
                .map_err(|e| io_reason(&e))?
                .ok_or_else(lost)?;
            match data.parsed() {
                Response::Continue { .. } => break,
                Response::Done {
                    tag,
                    code,
                    information,
                    ..
                } if *tag == id => return Err(refused_search(code, information.as_deref())),
                _ => {}
            }
        }
        // The literal, and what follows it up to the next one or the CRLF
        // that ends the command.
        session
            .run_command_untagged(next)
            .await
            .map_err(|e| reason(&e))?;
    }
    let mut found = Vec::new();
    loop {
        let data = session
            .read_response()
            .await
            .map_err(|e| io_reason(&e))?
            .ok_or_else(lost)?;
        match data.parsed() {
            Response::MailboxData(MailboxDatum::Search(uids)) => found.extend(uids.iter().copied()),
            Response::Done {
                tag,
                status,
                code,
                information,
            } if *tag == id => {
                return if *status == Status::Ok {
                    Ok(found)
                } else {
                    Err(refused_search(code, information.as_deref()))
                };
            }
            _ => {}
        }
    }
}

/// A server's reason for refusing a search, as one line of its own words.
fn refused_search(
    code: &Option<async_imap::imap_proto::types::ResponseCode<'_>>,
    said: Option<&str>,
) -> String {
    use async_imap::imap_proto::types::ResponseCode;
    if matches!(code, Some(ResponseCode::BadCharset(_))) {
        return "It cannot search for letters outside plain ASCII.".into();
    }
    let said = one_line(said.unwrap_or_default());
    if said.is_empty() {
        "It gave no reason.".into()
    } else {
        format!("It said: {said}")
    }
}

fn not_searchable(folder: &Folder) -> String {
    format!("RATA cannot search {} on the server yet.", place(folder))
}

/// FETCH one range of sequence numbers and turn it into messages, newest
/// first. Shared by the newest-first refresh and paging back through history.
async fn read_range<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    range: &str,
    generation: u32,
) -> Result<Vec<Message>, Fetched> {
    read_whole(
        session,
        dial,
        acct,
        folder,
        generation,
        Want::Range(range),
        "the inbox could not be listed",
    )
    .await
}

/// What one FETCH of messages brought.
struct Batch {
    /// Newest first.
    messages: Vec<Message>,
    /// A reply could not be read, so nothing more can be read on this
    /// connection (see [`unreadable`]): whatever was asked for and is not in
    /// `messages` is still to come.
    cut: bool,
}

/// Messages out of a FETCH response, newest first.
///
/// A reply the parser cannot read ends the batch but not the folder: the
/// connection is unusable after it, the mail is not, and [`read_whole`]
/// fetches the rest on a fresh one. A connection that really fails is
/// different — what arrived so far is an arbitrary slice of the folder, and
/// presenting it as the folder would be a refresh that silently lost mail.
async fn collect<S>(
    stream: S,
    acct: &Account,
    folder: &Folder,
    generation: u32,
) -> Result<Batch, Fetched>
where
    S: futures::Stream<Item = Result<async_imap::types::Fetch, ImapError>>,
{
    let mut messages: Vec<Message> = Vec::new();
    let mut cut = false;
    futures::pin_mut!(stream);
    let partway = |what: &str| {
        Fetched::Net(unreachable_msg(
            acct,
            &format!("{what} partway through {}", place(folder)),
        ))
    };
    loop {
        match timeout(COMMAND, stream.next()).await {
            // A message flagged \Deleted is on its way out — deleted by RATA
            // on a server without UIDPLUS, or by another client — and showing
            // it would undo the delete on screen.
            Ok(Some(Ok(fetched))) if fetched.flags().any(|f| f == Flag::Deleted) => {}
            Ok(Some(Ok(fetched))) => messages.push(build(acct, &fetched, folder, generation)),
            // Nothing more will come on this connection (see `unreadable`).
            Ok(Some(Err(e))) if unreadable(&e) => {
                cut = true;
                break;
            }
            Ok(Some(Err(ImapError::ConnectionLost))) => {
                return Err(partway("the connection was lost"));
            }
            Ok(Some(Err(e))) => {
                return Err(partway(&format!(
                    "the connection failed ({})",
                    reason(&e).trim_end_matches('.')
                )));
            }
            Ok(None) => break,
            Err(_) => return Err(partway("the server stopped answering")),
        }
    }
    messages.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(Batch { messages, cut })
}

// -------------------------------------------------- replies RATA cannot read

/// How many more connections one request may open after a reply it could
/// not read made its first unusable. Each is a sign-in, and providers count
/// those; past this, what is left is listed without its text.
const REDIALS: u8 = 2;

/// A message without its text: enough to list it and to find it again.
const LISTED: &str = "(UID FLAGS INTERNALDATE ENVELOPE)";

/// In place of the text of a message whose reply RATA could not read.
const UNREADABLE: &str = "RATA could not read this message as the server sent it. Opening it asks for the whole message again; if that fails too, read it in your provider's own app.";

/// In place of the text of a message RATA had no connection left to read.
const NOT_READ: &str = "RATA has not read the text of this message yet. Opening it fetches it.";

/// What async-imap says, in RATA's words, when its parser refuses a reply.
const UNPARSED: &str = "the server sent a reply RATA could not read";

/// Whether an error is async-imap's parser refusing a reply rather than the
/// connection failing.
///
/// No type tells the two apart. async-imap 0.11 (`imap_stream.rs`, `decode`)
/// turns a reply imap-proto rejects into
/// `io::Error::other(format!("{nom_error:?} during parsing of {reply:?}"))`:
/// an `Error::Io` like a reset socket, but of kind `Other` and always with
/// those words, which is what is matched. A socket or TLS failure has another
/// kind.
///
/// After that error, as after any, async-imap reads nothing more on the
/// connection (`read_closed`): every later answer ends at once, and a SELECT
/// then "succeeds" with an empty mailbox. So the connection is done with,
/// though the server and the mail are fine — the rest comes on a fresh one.
fn unreadable(e: &ImapError) -> bool {
    match e {
        ImapError::Parse(_) => true,
        ImapError::Io(io) => parse_failure(io),
        _ => false,
    }
}

fn parse_failure(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Other && e.to_string().contains(" during parsing of ")
}

/// What went wrong, in one short line and never in the server's own bytes.
/// async-imap's message for a reply it could not parse quotes all of it, and
/// the reply to a FETCH is the message: its headers, addresses and text.
fn reason(e: &ImapError) -> String {
    match e {
        ImapError::Io(io) => io_reason(io),
        ImapError::Parse(_) => UNPARSED.into(),
        ImapError::No(why) | ImapError::Bad(why) => one_line(why),
        ImapError::ConnectionLost => "the connection was lost".into(),
        _ => "the server's answer did not make sense to RATA".into(),
    }
}

fn io_reason(e: &std::io::Error) -> String {
    if parse_failure(e) {
        UNPARSED.into()
    } else {
        one_line(&e.to_string())
    }
}

/// The first line of `s`, without control or bidi-control characters, at
/// most 200 of them: [`credential::said`] with no secrets, the rule SMTP
/// keeps for every reply.
fn one_line(s: &str) -> String {
    credential::said(s, &[])
}

/// "the inbox", "the Sent folder"…: where something happened, for a sentence.
fn place(folder: &Folder) -> String {
    match folder {
        Folder::Inbox => "the inbox".into(),
        Folder::Sent => "the Sent folder".into(),
        Folder::Archive => "the Archive folder".into(),
        Folder::Junk => "the Spam folder".into(),
        Folder::Drafts => "the Drafts folder".into(),
        Folder::Named(name) => format!("the folder \u{201c}{}\u{201d}", utf7_imap(name)),
    }
}

/// Every item of one of async-imap's response streams, up to its end — or
/// its first error, which is returned instead. An error is where such a
/// stream ends (see [`unreadable`]), so what came before it is not the whole
/// answer, and a listing cut short must not pass for a complete one: "no
/// newer mail", "no longer there".
async fn drain<I>(
    stream: impl futures::Stream<Item = Result<I, ImapError>>,
) -> Result<Vec<I>, ImapError> {
    futures::pin_mut!(stream);
    let mut out = Vec::new();
    while let Some(item) = stream.next().await {
        out.push(item?);
    }
    Ok(out)
}

/// A command answered by a stream, and the whole answer, within [`COMMAND`].
/// `Err(None)` is a server that did not answer in time.
async fn answered<S, I>(
    sent: impl std::future::Future<Output = Result<S, ImapError>>,
) -> Result<Vec<I>, Option<ImapError>>
where
    S: futures::Stream<Item = Result<I, ImapError>>,
{
    match timeout(COMMAND, async { drain(sent.await?).await }).await {
        Ok(Ok(items)) => Ok(items),
        Ok(Err(e)) => Err(Some(e)),
        Err(_) => Err(None),
    }
}

/// Which messages of the folder just selected a FETCH asks for.
#[derive(Clone, Copy)]
enum Want<'a> {
    Uids(&'a [u32]),
    /// By position: `a:b`, or `a:*`.
    Range(&'a str),
}

/// Send a FETCH of `items` for `want`, or nothing if the server would not
/// take it.
async fn fetch_items<'s, T>(
    session: &'s mut Session<T>,
    want: Want<'_>,
    items: &str,
) -> Option<futures::stream::BoxStream<'s, Result<async_imap::types::Fetch, ImapError>>>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    Some(match want {
        Want::Uids(uids) => timeout(COMMAND, session.uid_fetch(join(uids), items))
            .await
            .ok()?
            .ok()?
            .boxed(),
        Want::Range(range) => timeout(COMMAND, session.fetch(range, items))
            .await
            .ok()?
            .ok()?
            .boxed(),
    })
}

/// How to open another signed-in connection to the same mailbox.
trait Connect {
    type Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send;
    fn connect(
        &self,
    ) -> impl std::future::Future<Output = Result<Session<Self::Stream>, String>> + Send;
}

/// The mailbox a public call was made for.
struct Server<'a> {
    resolver: &'a Resolver,
    acct: &'a Account,
}

impl Connect for Server<'_> {
    type Stream = Tls;
    async fn connect(&self) -> Result<Session<Tls>, String> {
        let acct = self.acct;
        let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
        let said = |t: Trouble| match t {
            Trouble::Host(why) | Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why) => {
                why
            }
        };
        let client = open(self.resolver, &acct.host, port).await.map_err(said)?;
        sign_in(client, &acct.email, &acct.credential)
            .await
            .map_err(said)
    }
}

/// Fresh connections for one request, at most [`REDIALS`] of them, and
/// whether the one in use can still be read.
struct Redial<C> {
    via: C,
    left: u8,
    /// A reply that could not be read has made the session in use unusable.
    dead: bool,
}

impl<C: Connect> Redial<C> {
    fn new(via: C) -> Self {
        Redial {
            via,
            left: REDIALS,
            dead: false,
        }
    }

    /// Replace `session` with a fresh one. Afterwards nothing is selected.
    async fn again(&mut self, session: &mut Session<C::Stream>) -> Result<(), String> {
        let Some(left) = self.left.checked_sub(1) else {
            return Err("RATA has already reconnected as often as one refresh may".into());
        };
        self.left = left;
        *session = self.via.connect().await?;
        self.dead = false;
        Ok(())
    }

    /// Before using `session` again: a fresh one if it is unusable. False if
    /// it is and none can be had.
    async fn revive(&mut self, session: &mut Session<C::Stream>) -> bool {
        !self.dead || self.again(session).await.is_ok()
    }
}

/// A fresh connection with `folder` open again under the same UIDVALIDITY.
async fn reopen<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    folder: &Folder,
    generation: u32,
) -> Result<(), String> {
    dial.again(session).await?;
    match select(session, folder).await {
        Selected::Open(m) if m.uid_validity.unwrap_or(0) == generation => Ok(()),
        Selected::Open(_) => Err(format!("{} was rebuilt meanwhile", place(folder))),
        Selected::Missing => Err(format!("{} has gone", place(folder))),
        Selected::Failed => Err(cannot_open(folder)),
    }
}

/// A message listed without its text, saying why.
fn marked(mut m: Message, why: &str) -> Message {
    m.body = why.into();
    m.preview = why.into();
    // What makes opening it fetch the whole message.
    m.truncated = true;
    m.attachments.clear();
    m.html = false;
    m
}

/// Whole messages (see [`items`]) of the folder just selected, newest first.
///
/// If a reply cannot be read, the rest is fetched on a fresh connection
/// ([`recover`]) and the message it belonged to is kept, [`marked`], rather
/// than costing the others. `failed` is what to say if the FETCH itself is
/// refused.
async fn read_whole<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    generation: u32,
    want: Want<'_>,
    failed: &str,
) -> Result<Vec<Message>, Fetched> {
    let Some(stream) = fetch_items(session, want, &items()).await else {
        return Err(Fetched::Net(unreachable_msg(acct, failed)));
    };
    let batch = collect(stream, acct, folder, generation).await?;
    let mut messages = batch.messages;
    if batch.cut {
        dial.dead = true;
        let mut rest = recover(session, dial, acct, folder, generation, want, &messages).await?;
        messages.append(&mut rest);
        messages.sort_by(|a, b| b.ts.cmp(&a.ts));
    }
    Ok(messages)
}

/// The rest of a FETCH cut short by a reply RATA could not read, on fresh
/// connections: what was asked for and did not arrive is listed without its
/// text, then fetched one message at a time, newest first, so a reply that
/// cannot be read is pinned to the one message it belongs to. That message
/// is kept, [`marked`] [`UNREADABLE`], and the next is fetched on another
/// connection; when this request may open no more, those left are kept
/// [`marked`] [`NOT_READ`].
async fn recover<C: Connect>(
    session: &mut Session<C::Stream>,
    dial: &mut Redial<C>,
    acct: &Account,
    folder: &Folder,
    generation: u32,
    want: Want<'_>,
    have: &[Message],
) -> Result<Vec<Message>, Fetched> {
    let gave_up = |why: String| {
        Fetched::Net(format!(
            "{} did not sync — {} sent a reply about a message in {} that RATA could not read, and {}. It will be tried again on the next refresh.",
            acct.email,
            acct.host,
            place(folder),
            why.trim_end_matches('.')
        ))
    };
    reopen(session, dial, folder, generation)
        .await
        .map_err(gave_up)?;
    let Some(stream) = fetch_items(session, want, LISTED).await else {
        return Err(gave_up(format!("{} could not be listed", place(folder))));
    };
    let listed = collect(stream, acct, folder, generation).await?;
    if listed.cut {
        dial.dead = true;
        return Err(gave_up("nor could a list of it without the text".into()));
    }
    let held: Vec<u32> = have.iter().map(|m| m.uid).collect();
    let mut rest: Vec<Message> = listed
        .messages
        .into_iter()
        .filter(|m| m.uid != 0 && !held.contains(&m.uid))
        .filter(|m| match want {
            Want::Uids(asked) => asked.contains(&m.uid),
            Want::Range(_) => true,
        })
        .collect();
    rest.sort_by(|a, b| b.uid.cmp(&a.uid));

    let mut out = Vec::with_capacity(rest.len());
    let mut rest = rest.into_iter();
    while let Some(m) = rest.next() {
        if dial.dead && reopen(session, dial, folder, generation).await.is_err() {
            dial.dead = true;
            out.push(marked(m, NOT_READ));
            out.extend(rest.by_ref().map(|m| marked(m, NOT_READ)));
            break;
        }
        let one = [m.uid];
        let Some(stream) = fetch_items(session, Want::Uids(&one), &items()).await else {
            // Not even asked: this connection is no good either.
            dial.dead = true;
            out.push(marked(m, NOT_READ));
            continue;
        };
        let got = collect(stream, acct, folder, generation).await?;
        if got.cut {
            dial.dead = true;
        }
        match got.messages.into_iter().find(|w| w.uid == m.uid) {
            Some(whole) => out.push(whole),
            None if got.cut => out.push(marked(m, UNREADABLE)),
            // Deleted meanwhile.
            None => {}
        }
    }
    Ok(out)
}

// --------------------------------------------------------------------- watch

/// How long one IDLE lasts before RATA ends it and starts another. Servers
/// may drop an IDLE after 30 minutes (RFC 2177 asks for a new one within
/// 29); a shorter one also finds out sooner that a sleeping laptop has lost
/// the connection.
pub const IDLE_FOR: Duration = Duration::from_secs(9 * 60);

/// What watching the inbox came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    /// New mail is in the inbox. Nothing about it: a refresh fetches it.
    Arrived,
    /// The wait ended with nothing new; the connection is still good.
    Quiet,
    /// The server has no IDLE. Checking every few minutes is all there is.
    Unsupported,
    Auth(String),
    /// See [`Fetched::OAuth`].
    OAuth(String),
    Host(String),
    /// The connection went; try again later.
    Net(String),
}

/// A signed-in connection with the inbox open, waiting to be told of new
/// mail (IMAP IDLE). It reads nothing from any message — a refresh does
/// that — and it keeps no password: that was needed only to sign in.
pub struct Watch {
    session: Option<Session<Tls>>,
    /// How many messages the inbox holds, as far as this connection knows.
    exists: u32,
}

/// Sign in to `acct` and open its inbox for [`Watch::wait`].
pub async fn watch(resolver: &Resolver, acct: &Account) -> Result<Watch, Watched> {
    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return Err(Watched::Host(why)),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Err(Watched::Net(unreachable_msg(acct, &why)));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Err(Watched::Auth(revoked_msg(acct, &why))),
        Err(Trouble::OAuth(why)) => return Err(Watched::OAuth(oauth_msg(acct, &why))),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Err(Watched::Net(unreachable_msg(acct, &why)));
        }
    };
    match ready(&mut session).await {
        Ok(exists) => Ok(Watch {
            session: Some(session),
            exists,
        }),
        Err(why) => {
            let _ = timeout(COMMAND, session.logout()).await;
            Err(why)
        }
    }
}

impl Watch {
    /// Wait up to `dur` for new mail. [`Watched::Arrived`] and
    /// [`Watched::Quiet`] leave the connection ready for the next wait;
    /// anything else means it is gone and a new [`watch`] is needed.
    pub async fn wait(&mut self, dur: Duration) -> Watched {
        let Some(session) = self.session.take() else {
            return Watched::Net("the connection was already closed".into());
        };
        let (session, said) = idle_until(session, &mut self.exists, dur).await;
        self.session = session;
        said
    }

    /// Say goodbye politely.
    pub async fn close(mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = timeout(COMMAND, s.logout()).await;
        }
    }
}

/// Whether this server can be watched, and the inbox open for it: how many
/// messages it holds now.
async fn ready<T>(session: &mut Session<T>) -> Result<u32, Watched>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let caps = match timeout(COMMAND, session.capabilities()).await {
        Ok(Ok(c)) => c,
        _ => {
            return Err(Watched::Net(
                "the server did not say what it supports".into(),
            ));
        }
    };
    if !caps.has_str("IDLE") {
        return Err(Watched::Unsupported);
    }
    match timeout(COMMAND, session.select("INBOX")).await {
        Ok(Ok(m)) => Ok(m.exists),
        _ => Err(Watched::Net("the inbox could not be opened".into())),
    }
}

/// IDLE until new mail arrives or `dur` is up, starting a fresh IDLE after
/// anything that is not new mail (a flag changed, a message went), so that
/// RATA marking a message read does not wake it. New mail is the inbox
/// growing past what it held: `exists` follows EXPUNGE down and EXISTS up.
async fn idle_until<T>(
    session: Session<T>,
    exists: &mut u32,
    dur: Duration,
) -> (Option<Session<T>>, Watched)
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    use async_imap::extensions::idle::IdleResponse;
    use async_imap::imap_proto::types::{MailboxDatum, Response};

    let lost = |what: &str| Watched::Net(format!("the connection was lost while {what}"));
    let end = tokio::time::Instant::now() + dur;
    let mut session = session;
    loop {
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return (Some(session), Watched::Quiet);
        }
        let mut handle = session.idle();
        if !matches!(timeout(COMMAND, handle.init()).await, Ok(Ok(()))) {
            return (None, lost("starting to wait"));
        }
        let mut arrived = false;
        {
            let (wait, _stop) = handle.wait_with_timeout(left);
            // The library's timeout starts again at every keepalive a server
            // sends; this one does not.
            match timeout(left + COMMAND, wait).await {
                Ok(Ok(IdleResponse::NewData(data))) => match data.parsed() {
                    Response::MailboxData(MailboxDatum::Exists(n)) => {
                        arrived = *n > *exists;
                        *exists = *n;
                    }
                    Response::Expunge(_) => *exists = exists.saturating_sub(1),
                    _ => {}
                },
                Ok(Ok(IdleResponse::Timeout | IdleResponse::ManualInterrupt)) | Err(_) => {}
                Ok(Err(_)) => return (None, lost("waiting")),
            }
        }
        session = match timeout(COMMAND, handle.done()).await {
            Ok(Ok(s)) => s,
            _ => return (None, lost("ending the wait")),
        };
        if arrived {
            return (Some(session), Watched::Arrived);
        }
    }
}

// ----------------------------------------------------------------------- act

/// Something the customer did to a message in RATA, done to the real mailbox.
///
/// Serialised as `"read"`, `"trash"`… or `{"move": <folder>}`.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Moved back to the inbox — from Spam ("not spam") or from Archive.
    Inbox,
    /// Moved to another folder — in practice one of the customer's own. The
    /// folder must be one this mailbox has; a name it does not list is
    /// refused rather than created.
    Move(Folder),
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
    /// Gmail's archive: `done` were copied where they were asked to go (a
    /// label, to Gmail) and are still archived, since taking a message out
    /// of All Mail deletes it everywhere. Nothing left the archive, so the
    /// page must not remember them as gone: they come back under Archive,
    /// as Gmail itself shows them.
    Copied {
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
    /// See [`Fetched::OAuth`].
    OAuth(String),
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
    folder: Folder,
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
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return Acted::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return Acted::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return Acted::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return Acted::Net(unreachable_msg(acct, &why));
        }
    };
    let done = apply(&mut session, &folder, &uids, uidvalidity, &action).await;
    let _ = timeout(COMMAND, session.logout()).await;
    done
}

/// The part of [`act`] that talks to an already-signed-in session. Generic
/// over the stream so it can be driven by a scripted server in tests.
async fn apply<T>(
    session: &mut Session<T>,
    folder: &Folder,
    uids: &[u32],
    uidvalidity: u32,
    action: &Action,
) -> Acted
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let failed = |what: &str, e: ImapError| {
        Acted::Net(format!("The server would not {what}: {}", reason(&e)))
    };

    let mailbox = match select(session, folder).await {
        Selected::Open(m) => m,
        Selected::Missing => {
            return Acted::Stale(
                "This mailbox no longer has that folder, so nothing was changed. Refresh and try again.".into(),
            );
        }
        Selected::Failed => {
            return Acted::Net(format!("The server would not {}.", cannot_open(folder)));
        }
    };
    if mailbox.uid_validity != Some(uidvalidity) {
        return Acted::Stale(
            "The mailbox has been reorganised since RATA last read it, so its message numbers no longer match. Nothing was changed — refresh and try again.".into(),
        );
    }

    // Which of them are still there? A STORE or MOVE naming a UID that no
    // longer exists succeeds and does nothing, which would be reported as done.
    let asked = join(uids);
    let present: Vec<u32> = match answered(session.uid_fetch(&asked, "UID")).await {
        Ok(found) => {
            let seen: Vec<u32> = found.iter().filter_map(|f| f.uid).collect();
            uids.iter().copied().filter(|u| seen.contains(u)).collect()
        }
        Err(Some(e)) => return failed("look the messages up", e),
        Err(None) => return Acted::Net("The server did not answer in time.".into()),
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
        Action::Trash | Action::Archive | Action::Inbox | Action::Move(_) => None,
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

    let already = match action {
        Action::Inbox => *folder == Folder::Inbox,
        Action::Archive => *folder == Folder::Archive,
        Action::Move(to) => to == folder,
        _ => false,
    };
    if already {
        return Acted::Done {
            done: present,
            gone,
        };
    }
    let Some(dest) = destination(session, action).await else {
        let what = match action {
            Action::Trash => {
                "a Trash folder, so RATA left the messages where they are rather than delete them for good"
            }
            Action::Move(_) => {
                "that folder any more — it was renamed or removed — so RATA left the messages where they are"
            }
            _ => "an Archive folder, so RATA left the messages in the inbox",
        };
        return Acted::NoPlace(format!("This mailbox does not have {what}."));
    };

    // In Gmail, taking a message out of All Mail deletes it everywhere. Back
    // to the inbox, or into a folder (a label, to Gmail), is a copy there:
    // the message gains the label and stays where it is.
    if *folder == Folder::Archive
        && matches!(action, Action::Inbox | Action::Move(_))
        && places(session).await.all_mail
    {
        return match timeout(COMMAND, session.uid_copy(&set, &dest)).await {
            Ok(Ok(())) => Acted::Copied {
                done: present,
                gone,
            },
            Ok(Err(e)) => failed("move the messages", e),
            Err(_) => Acted::Net("The server did not answer in time.".into()),
        };
    }
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
                // What it expunged is not needed; that it ends is.
                let _ = drain(session.uid_expunge(&set).await?).await;
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
    drain(session.uid_store(set, change).await?).await?;
    Ok(())
}

/// Where an action puts messages on this server.
///
/// Archive is wherever a refresh reads the archive from ([`places_in`]): the
/// folder the server declares `\Archive`, else one named exactly as
/// [`ARCHIVE_NAMES`] lists, else Gmail's "All Mail", where moving a message
/// out of the inbox is exactly what archiving means. So a server that
/// declares nothing, and shows its "Archive" in RATA, can also be archived to.
///
/// Trash is the folder the server declares `\Trash` (RFC 6154) — "Trash",
/// "Deleted Items", "[Gmail]/Bin" and "Papierkorb" alike — else, on a server
/// that declares none, one named exactly as [`TRASH_NAMES`] lists (see
/// [`trash_in`]). None at all is still a refusal the customer can see, never
/// a permanent delete.
async fn destination<T>(session: &mut Session<T>, action: &Action) -> Option<String>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    match action {
        Action::Inbox => Some("INBOX".into()),
        Action::Move(to) => name_of(session, to).await,
        Action::Archive => name_of(session, &Folder::Archive).await,
        Action::Trash => trash_in(&listing(session).await?),
        Action::Read | Action::Unread | Action::Star | Action::Unstar => None,
    }
}

/// The Trash in a LIST: the folder declared `\Trash`, else the first one
/// named exactly as [`TRASH_NAMES`] lists that can be opened, is not the
/// inbox, declares no other purpose, and is not what a refresh reads as
/// Sent, Archive, Spam or Drafts.
fn trash_in(listed: &[async_imap::types::Name]) -> Option<String> {
    use async_imap::types::NameAttribute as A;
    if let Some(n) = listed
        .iter()
        .find(|n| n.attributes().contains(&A::Trash) && selectable(n))
    {
        return Some(n.name().to_string());
    }
    let found = places_in(listed);
    let other_purpose = |n: &async_imap::types::Name| {
        n.attributes().iter().any(|a| {
            matches!(
                a,
                A::All | A::Archive | A::Drafts | A::Flagged | A::Junk | A::Sent
            )
        })
    };
    TRASH_NAMES.iter().find_map(|want| {
        listed
            .iter()
            .find(|n| {
                n.name().eq_ignore_ascii_case(want)
                    && selectable(n)
                    && !n.name().eq_ignore_ascii_case("INBOX")
                    && !other_purpose(n)
                    && !found.holds(n.name())
            })
            .map(|n| n.name().to_string())
    })
}

// -------------------------------------------------------------------- drafts

/// The copy of a draft RATA saved last: where it is in Drafts, and the id
/// RATA wrote into it (`X-RATA-Draft`), which is what proves it RATA's.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DraftRef {
    pub uid: u32,
    pub uidvalidity: u32,
    pub draft_id: String,
}

/// What became of the copy saved before, when a draft is saved again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Prior {
    /// None was named.
    None,
    /// It carried this draft's own id and was removed for good — the one
    /// thing RATA ever deletes permanently. On Gmail (`X-GM-EXT-1`) it was
    /// moved to Gmail's Trash instead, since an expunge there only removes
    /// the Drafts label; either way it has left Drafts.
    Replaced,
    /// It carried this draft's id and is flagged `\Deleted`, but the server
    /// has no UIDPLUS, so it was not expunged: a plain EXPUNGE would also
    /// remove every other message anybody had flagged there. RATA hides
    /// `\Deleted` mail, and the server removes it when something expunges.
    LeftFlagged,
    /// It could not be proven RATA's copy of this draft — another id or
    /// none, Drafts rebuilt since, or the check itself failed — so it was
    /// left exactly where it is.
    NotOurs,
    /// It was RATA's, but the server would not remove it. It is still there.
    Failed,
    /// It was no longer in Drafts — sent or deleted elsewhere.
    Gone,
}

/// How saving a draft to the mailbox went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftSaved {
    /// In Drafts now, as `saved`, under the message id a refresh gives it.
    Saved {
        saved: DraftRef,
        id: String,
        prior: Prior,
    },
    /// This mailbox has no Drafts folder. The draft stays in RATA only, as it
    /// always did; RATA never creates a folder.
    NoPlace(String),
    /// The server would not take it (over quota, too large), or the draft
    /// itself cannot be saved. Trying again unchanged will not help.
    Refused(String),
    Auth(String),
    /// See [`Fetched::OAuth`].
    OAuth(String),
    Host(String),
    Net(String),
}

/// Save a draft to the mailbox's Drafts folder, replacing RATA's own copy
/// saved before it, on one connection.
///
/// The draft is rendered as it would be sent ([`compose::render_draft`]:
/// attachments, Cc and Bcc kept) with `X-RATA-Draft: <draft_id>` and
/// `X-RATA-Draft-Rev: <rev>`, and APPENDed with `\Seen` and `\Draft` set:
/// a draft is the customer's own, never unread. Its new UID is the server's
/// `APPENDUID` answer where it gives one (UIDPLUS), else the newest message
/// in Drafts carrying this id.
///
/// `prior` is the copy saved before. It is permanently removed only when
/// every check holds: Drafts has the same UIDVALIDITY, the message at that
/// UID carries exactly one `X-RATA-Draft` header and it is this `draft_id`,
/// and it is not the copy just saved. Then it is flagged `\Deleted` and
/// expunged by its UID alone (`UID EXPUNGE`, UIDPLUS); without UIDPLUS it is
/// left flagged rather than running an EXPUNGE that would take other
/// messages with it. Anything else leaves it alone. Nothing else in RATA
/// ever deletes permanently.
pub async fn save_draft(
    resolver: &Resolver,
    acct: &Account,
    msg: &Outgoing,
    draft_id: &str,
    rev: u32,
    prior: Option<&DraftRef>,
) -> DraftSaved {
    if !compose::draft_id_ok(draft_id) {
        return DraftSaved::Refused(
            "RATA could not name this draft, so it was kept here and not saved to Drafts.".into(),
        );
    }
    if !msg.from.as_str().eq_ignore_ascii_case(acct.email.trim()) {
        return DraftSaved::Refused(format!(
            "This draft says it is from {}, but it was being saved to {}.",
            msg.from.as_str(),
            acct.email
        ));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = compose::message_id(msg, nanos);
    let raw = compose::render_draft(msg, &crate::smtp::now_rfc2822(), &unique, draft_id, rev);

    let port = if acct.port == 0 { IMAP_PORT } else { acct.port };
    let client = match open(resolver, &acct.host, port).await {
        Ok(c) => c,
        Err(Trouble::Host(why)) => return DraftSaved::Host(why),
        Err(Trouble::Net(why) | Trouble::Auth(why) | Trouble::OAuth(why)) => {
            return DraftSaved::Net(unreachable_msg(acct, &why));
        }
    };
    let mut session = match sign_in(client, &acct.email, &acct.credential).await {
        Ok(s) => s,
        Err(Trouble::Auth(why)) => return DraftSaved::Auth(revoked_msg(acct, &why)),
        Err(Trouble::OAuth(why)) => return DraftSaved::OAuth(oauth_msg(acct, &why)),
        Err(Trouble::Net(why) | Trouble::Host(why)) => {
            return DraftSaved::Net(unreachable_msg(acct, &why));
        }
    };
    let saved = save_in(&mut session, acct, &raw, draft_id, prior).await;
    let _ = timeout(COMMAND, session.logout()).await;
    saved
}

/// The part of [`save_draft`] that talks to a signed-in session, with the
/// draft already rendered.
async fn save_in<T>(
    session: &mut Session<T>,
    acct: &Account,
    raw: &str,
    draft_id: &str,
    prior: Option<&DraftRef>,
) -> DraftSaved
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let not_saved = |why: &str| {
        DraftSaved::Net(format!(
            "The draft was not saved to Drafts in {} — {}. It is kept here and will be saved again.",
            acct.email,
            why.trim_end_matches('.')
        ))
    };
    let Some(listed) = listing(session).await else {
        return not_saved("the server would not list its folders");
    };
    let Some(drafts) = places_in(&listed).drafts else {
        return DraftSaved::NoPlace(format!(
            "{} has no Drafts folder, so RATA keeps this draft on this computer only.",
            acct.email
        ));
    };
    let appended = match append(session, &drafts, raw).await {
        Appended::Done(uid) => uid,
        Appended::Refused(why) => {
            return DraftSaved::Refused(format!(
                "{} would not keep the draft in its Drafts folder: {}",
                acct.email,
                if why.is_empty() {
                    "it gave no reason".into()
                } else {
                    why
                }
            ));
        }
        Appended::Failed(why) => return not_saved(&why),
    };
    let Selected::Open(mailbox) = select_name(session, &drafts).await else {
        return not_saved("its Drafts folder could not be opened after saving");
    };
    let generation = mailbox.uid_validity.unwrap_or(0);
    let before = prior.filter(|p| p.uidvalidity == generation).map(|p| p.uid);
    let uid = match appended {
        Some((validity, uid)) if validity == generation && uid != 0 => Some(uid),
        // No APPENDUID: the newest copy carrying this id, which is the one
        // just saved — never the one before it.
        _ => {
            let found = timeout(
                COMMAND,
                search(
                    session,
                    &format!("UNDELETED HEADER X-RATA-Draft \"{draft_id}\""),
                ),
            )
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            let found: Vec<u32> = found
                .into_iter()
                .filter(|u| *u != 0 && Some(*u) != before)
                .collect();
            newest_carrying(session, &found, draft_id).await
        }
    };
    let Some(uid) = uid.filter(|_| generation != 0) else {
        return not_saved("the server took it but RATA could not find it again");
    };
    let prior = match prior {
        None => Prior::None,
        Some(p) => replace(session, p, draft_id, generation, uid).await,
    };
    DraftSaved::Saved {
        saved: DraftRef {
            uid,
            uidvalidity: generation,
            draft_id: draft_id.to_string(),
        },
        id: message_key(acct, &Folder::Drafts, uid),
        prior,
    }
}

/// Of `uids` in the Drafts folder just selected, the newest whose message
/// carries exactly one `X-RATA-Draft` header and it is `draft_id` — the
/// check [`replace`] makes. IMAP's HEADER search matches a substring
/// (P3-3), so a search hit alone could be another message whose header
/// merely contains the id, which Send or Discard would then put in the
/// Trash.
async fn newest_carrying<T>(session: &mut Session<T>, uids: &[u32], draft_id: &str) -> Option<u32>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    if uids.is_empty() {
        return None;
    }
    let found =
        answered(session.uid_fetch(join(uids), "(UID BODY.PEEK[HEADER.FIELDS (X-RATA-Draft)])"))
            .await
            .ok()?;
    found
        .iter()
        .filter(|f| header_values(f.header().unwrap_or_default(), "X-RATA-Draft") == [draft_id])
        .filter_map(|f| f.uid)
        .filter(|u| uids.contains(u))
        .max()
}

/// Remove `prior` from the Drafts folder just selected — only if it is
/// provably RATA's earlier copy of this draft (see [`save_draft`]).
async fn replace<T>(
    session: &mut Session<T>,
    prior: &DraftRef,
    draft_id: &str,
    generation: u32,
    saved: u32,
) -> Prior
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    if prior.uid == 0
        || prior.uid == saved
        || prior.uidvalidity != generation
        || prior.draft_id != draft_id
    {
        return Prior::NotOurs;
    }
    let set = prior.uid.to_string();
    let Ok(found) =
        answered(session.uid_fetch(&set, "(UID BODY.PEEK[HEADER.FIELDS (X-RATA-Draft)])")).await
    else {
        return Prior::NotOurs;
    };
    let Some(f) = found.iter().find(|f| f.uid == Some(prior.uid)) else {
        return Prior::Gone;
    };
    if header_values(f.header().unwrap_or_default(), "X-RATA-Draft") != [draft_id] {
        return Prior::NotOurs;
    }
    let caps = match timeout(COMMAND, session.capabilities()).await {
        Ok(Ok(c)) => c,
        _ => return Prior::Failed,
    };
    // Gmail (P3-2): expunging from [Gmail]/Drafts only takes the Drafts
    // label off, and the copy stays in All Mail — in the customer's quota,
    // and under RATA's own Archive. Moving it to Gmail's Trash takes it out
    // of All Mail, and Gmail empties the Trash after 30 days. The proof above
    // still decides what is moved. `destination` only LISTs, so Drafts stays
    // selected.
    if caps.has_str("X-GM-EXT-1") {
        let Some(trash) = destination(session, &Action::Trash).await else {
            return Prior::Failed;
        };
        return match timeout(COMMAND, session.uid_mv(&set, &trash)).await {
            Ok(Ok(())) => Prior::Replaced,
            _ => Prior::Failed,
        };
    }
    if !matches!(
        timeout(COMMAND, store(session, &set, "+FLAGS.SILENT (\\Deleted)")).await,
        Ok(Ok(()))
    ) {
        return Prior::Failed;
    }
    if !caps.has_str("UIDPLUS") {
        return Prior::LeftFlagged;
    }
    // Expunged by UID alone: never a plain EXPUNGE.
    match timeout(COMMAND, async {
        drain(session.uid_expunge(&set).await?).await
    })
    .await
    {
        Ok(Ok(_)) => Prior::Replaced,
        _ => Prior::LeftFlagged,
    }
}

/// How an APPEND went.
enum Appended {
    /// Stored; with UIDPLUS the server says where: (UIDVALIDITY, UID).
    Done(Option<(u32, u32)>),
    /// The server said no, in these words.
    Refused(String),
    Failed(String),
}

/// `APPEND` of `raw` to `mailbox`, flagged `\Seen \Draft`, reading the
/// server's `APPENDUID` if it gives one — which async-imap's own `append`
/// drops.
async fn append<T>(session: &mut Session<T>, mailbox: &str, raw: &str) -> Appended
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    use async_imap::imap_proto::types::{Response, ResponseCode, Status, UidSetMember};
    let Some(name) = quoted(mailbox) else {
        return Appended::Failed("its Drafts folder has a name RATA cannot use".into());
    };
    let lost = || Appended::Failed("the connection was lost".into());
    let run = async {
        let id = session
            .run_command(format!("APPEND {name} (\\Seen \\Draft) {{{}}}", raw.len()))
            .await
            .map_err(|e| Appended::Failed(reason(&e)))?;
        // Go ahead — or a refusal before anything is uploaded.
        loop {
            let data = session
                .read_response()
                .await
                .map_err(|e| Appended::Failed(io_reason(&e)))?
                .ok_or_else(lost)?;
            match data.parsed() {
                Response::Continue { .. } => break,
                Response::Done {
                    tag, information, ..
                } if *tag == id => {
                    return Err(Appended::Refused(one_line(
                        information.as_deref().unwrap_or_default(),
                    )));
                }
                _ => {}
            }
        }
        // The literal, then the CRLF that ends the command.
        session
            .run_command_untagged(raw)
            .await
            .map_err(|e| Appended::Failed(reason(&e)))?;
        loop {
            let data = session
                .read_response()
                .await
                .map_err(|e| Appended::Failed(io_reason(&e)))?
                .ok_or_else(lost)?;
            if let Response::Done {
                tag,
                status,
                code,
                information,
            } = data.parsed()
                && *tag == id
            {
                if *status != Status::Ok {
                    return Err(Appended::Refused(one_line(
                        information.as_deref().unwrap_or_default(),
                    )));
                }
                let at = match code {
                    Some(ResponseCode::AppendUid(validity, set)) => match set.first() {
                        Some(UidSetMember::Uid(u)) => Some((*validity, *u)),
                        Some(UidSetMember::UidRange(r)) => Some((*validity, *r.start())),
                        None => None,
                    },
                    _ => None,
                };
                return Ok(at);
            }
        }
    };
    match timeout(upload_time(raw.len()), run).await {
        Ok(Ok(at)) => Appended::Done(at),
        Ok(Err(e)) => e,
        Err(_) => Appended::Failed("the server did not answer in time".into()),
    }
}

/// How long an upload of `len` bytes may take: a minute, and a second more
/// for each 64 KiB, as SMTP's DATA gets.
fn upload_time(len: usize) -> Duration {
    Duration::from_secs(60 + (len / 65_536) as u64)
}

/// A folder name as an IMAP quoted string, or nothing for a name that cannot
/// be one (a line break or NUL would end the command).
fn quoted(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['\r', '\n', '\0']) {
        return None;
    }
    Some(format!(
        "\"{}\"",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// Every value of header `name` in a message's header block, unfolded and
/// trimmed.
fn header_values(raw: &[u8], name: &str) -> Vec<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .split("\n\n")
        .next()
        .unwrap_or_default();
    let mut fields: Vec<String> = Vec::new();
    for line in head.lines() {
        match fields.last_mut() {
            Some(last) if line.starts_with([' ', '\t']) => last.push_str(line),
            _ => fields.push(line.to_string()),
        }
    }
    fields
        .iter()
        .filter_map(|f| f.split_once(':'))
        .filter(|(n, _)| n.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim().to_string())
        .collect()
}

/// The id RATA wrote into a draft it saved, from the draft's own headers:
/// exactly one `X-RATA-Draft`, and one RATA could have made. Anything else
/// is a draft RATA did not save, which it never replaces.
fn rata_draft_id(raw: &[u8]) -> Option<String> {
    match header_values(raw, "X-RATA-Draft").as_slice() {
        [one] if compose::draft_id_ok(one) => Some(one.clone()),
        _ => None,
    }
}

/// "its inbox could not be opened", for whichever folder it was.
fn cannot_open(folder: &Folder) -> String {
    match folder {
        Folder::Inbox => "its inbox could not be opened".into(),
        Folder::Sent => "its Sent folder could not be opened".into(),
        Folder::Archive => "its Archive folder could not be opened".into(),
        Folder::Junk => "its Spam folder could not be opened".into(),
        Folder::Drafts => "its Drafts folder could not be opened".into(),
        Folder::Named(name) => format!(
            "its folder \u{201c}{}\u{201d} could not be opened",
            utf7_imap(name)
        ),
    }
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

/// What to say when a mailbox refused its OAuth token. Worded for the app to
/// act on rather than the customer: a fresh token usually fixes it without
/// them, and only if that is refused too do they need to sign in again.
fn oauth_msg(acct: &Account, why: &str) -> String {
    format!(
        "{} did not accept RATA's sign-in token — it has expired or been withdrawn. The server said: {}",
        acct.email,
        why.trim()
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

/// A short, stable tag for one of the customer's own folders, for message ids:
/// its name can hold anything, and the same UID in two folders must not
/// collide.
fn named_tag(name: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(name.as_bytes())
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The id RATA gives the message `uid` of `folder`. The mailbox and the
/// folder are part of it: the same UID in two mailboxes, or two folders of
/// one, must not collide once they are shown in one list.
fn message_key(acct: &Account, folder: &Folder, uid: u32) -> String {
    let place = match folder {
        Folder::Inbox => String::new(),
        Folder::Sent => "sent_".into(),
        Folder::Archive => "archive_".into(),
        Folder::Junk => "junk_".into(),
        Folder::Drafts => "drafts_".into(),
        Folder::Named(name) => format!("f{}_", named_tag(name)),
    };
    format!("{}_{place}{uid}", mail_key(&acct.email))
}

/// One IMAP response into one message.
fn build(
    acct: &Account,
    f: &async_imap::types::Fetch,
    folder: &Folder,
    generation: u32,
) -> Message {
    let env = f.envelope();
    let sender = env.and_then(|e| e.from.as_ref()).and_then(|a| a.first());
    let recipient = env.and_then(|e| e.to.as_ref()).and_then(|a| a.first());
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

    let to_addr = recipient.map(address).unwrap_or_default();
    let every = |list: Option<&Vec<async_imap::imap_proto::types::Address>>| -> Vec<String> {
        list.into_iter()
            .flatten()
            .map(address)
            .filter(|a| !a.is_empty())
            .take(100)
            .collect()
    };
    let to_all = every(env.and_then(|e| e.to.as_ref()));
    let cc = every(env.and_then(|e| e.cc.as_ref()));
    let bcc = if *folder == Folder::Drafts {
        every(env.and_then(|e| e.bcc.as_ref()))
    } else {
        vec![]
    };
    // Only the first: some clients list the whole chain here.
    let in_reply_to = env
        .and_then(|e| e.in_reply_to.as_deref())
        .map(|r| {
            let r = String::from_utf8_lossy(r);
            thread_id(r.split_whitespace().next().unwrap_or_default().as_bytes())
        })
        .unwrap_or_default();
    let to_name = recipient
        .and_then(|a| a.name.as_deref())
        .map(words::decode)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| to_addr.clone());

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
        id: message_key(acct, folder, uid),
        folder: folder.clone(),
        acct: acct.email.clone(),
        acct_label: if acct.label.is_empty() {
            acct.email.clone()
        } else {
            acct.label.clone()
        },
        from_name,
        from_addr,
        to_name,
        to_addr,
        to_all,
        cc,
        bcc,
        in_reply_to,
        subject,
        // A draft with no words yet is empty, not a message RATA cannot
        // read: finishing it must not start from that sentence.
        body: if text.text.is_empty() && *folder != Folder::Drafts {
            "(This message has no text RATA can show. Open it in your provider's own app to see it.)"
                .to_string()
        } else {
            text.text
        },
        truncated: text.truncated,
        attachments: text.attachments,
        html: text.has_html,
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
        draft_id: if *folder == Folder::Drafts {
            rata_draft_id(raw)
        } else {
            None
        },
        reply_to,
        unsubscribe: text.unsubscribe,
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

    // --------------------------------------------------------- watch tests
    //
    // A scripted server that answers IDLE: "+ idling", then round n's lines,
    // then nothing until DONE. What is under test is which of those wake RATA.

    async fn scripted_idle(
        caps: &'static str,
        rounds: Vec<Vec<&'static str>>,
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
            let mut idle_tag: Option<String> = None;
            let mut round = 0;
            while let Ok(Some(line)) = lines.next_line().await {
                if line == "DONE" {
                    log.lock().unwrap().push("DONE".into());
                    let tag = idle_tag.take().unwrap_or_default();
                    if w.write_all(format!("{tag} OK IDLE done\r\n").as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                log.lock().unwrap().push(cmd.to_string());
                let up = cmd.to_ascii_uppercase();
                if up == "IDLE" {
                    idle_tag = Some(tag.to_string());
                    let mut out = String::from("+ idling\r\n");
                    for l in rounds.get(round).cloned().unwrap_or_default() {
                        out.push_str(l);
                        out.push_str("\r\n");
                    }
                    round += 1;
                    if w.write_all(out.as_bytes()).await.is_err() {
                        return;
                    }
                    continue;
                }
                let body = if up.starts_with("SELECT") {
                    "* 3 EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n".to_string()
                } else if up.starts_with("CAPABILITY") {
                    format!("* CAPABILITY IMAP4rev1 {caps}\r\n")
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

    fn idles(log: &[String]) -> usize {
        log.iter()
            .filter(|c| c.eq_ignore_ascii_case("IDLE"))
            .count()
    }

    #[test]
    fn new_mail_wakes_the_watch() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_idle("IDLE", vec![vec!["* 4 EXISTS"]]).await;
            let mut exists = ready(&mut s).await.unwrap();
            assert_eq!(exists, 3);
            let (s, said) = idle_until(s, &mut exists, Duration::from_secs(5)).await;
            assert_eq!(said, Watched::Arrived);
            assert_eq!(exists, 4);
            assert!(s.is_some(), "the connection is kept for the next wait");
            let log = log.lock().unwrap();
            assert_eq!(idles(&log), 1);
            assert!(log.iter().any(|c| c == "DONE"), "{log:?}");
        });
    }

    #[test]
    fn a_flag_changing_does_not_wake_it_but_the_wait_goes_on() {
        rt_act().block_on(async {
            // RATA marking a message read, seen from this connection.
            let (mut s, log) = scripted_idle(
                "IDLE",
                vec![vec!["* 2 FETCH (FLAGS (\\Seen))"], vec![], vec![]],
            )
            .await;
            let mut exists = ready(&mut s).await.unwrap();
            let (s, said) = idle_until(s, &mut exists, Duration::from_millis(400)).await;
            assert_eq!(said, Watched::Quiet);
            assert!(s.is_some());
            assert_eq!(exists, 3);
            // It started a fresh IDLE after the flag change rather than stopping.
            assert_eq!(idles(&log.lock().unwrap()), 2);
        });
    }

    #[test]
    fn a_message_going_and_another_coming_is_new_mail() {
        rt_act().block_on(async {
            // One deleted elsewhere: not new mail. Then one arrives, back at 3.
            let (mut s, _) =
                scripted_idle("IDLE", vec![vec!["* 2 EXPUNGE"], vec!["* 3 EXISTS"]]).await;
            let mut exists = ready(&mut s).await.unwrap();
            let (_, said) = idle_until(s, &mut exists, Duration::from_secs(5)).await;
            assert_eq!(said, Watched::Arrived);
            assert_eq!(exists, 3);
        });
    }

    #[test]
    fn a_server_without_idle_is_not_watched() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_idle("MOVE", vec![]).await;
            assert_eq!(ready(&mut s).await, Err(Watched::Unsupported));
            // Nothing opened on a server that cannot be watched.
            assert!(
                !log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c.to_ascii_uppercase().starts_with("SELECT"))
            );
        });
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

    // ---------------------------------------------------- server search tests
    //
    // A scripted server that reads each command as bytes, literals and all,
    // and gives the go-ahead a literal waits for, so a test can say exactly
    // what RATA put on the wire. What is under test is that nothing a
    // customer types can become part of the command.

    /// How the scripted server answers `UID SEARCH`.
    #[derive(Clone, Copy)]
    enum SearchReply {
        /// `* SEARCH` with these UIDs, then OK.
        Found(&'static [u32]),
        /// This tagged answer (`NO …` or `BAD …`) once the command is whole.
        Refuse(&'static str),
        /// This tagged answer to the command's first line, with no go-ahead.
        RefuseAtOnce(&'static str),
    }

    /// The length of the literal a command line ends with (`… {n}`).
    fn literal_at_end(line: &[u8]) -> Option<usize> {
        let line = line.strip_suffix(b"\r\n")?.strip_suffix(b"}")?;
        let open = line.iter().rposition(|b| *b == b'{')?;
        std::str::from_utf8(&line[open + 1..]).ok()?.parse().ok()
    }

    async fn scripted_search(
        inbox: &'static [u32],
        list: &'static str,
        reply: SearchReply,
    ) -> (Session<TcpStream>, Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut r = BufReader::new(r);
            w.write_all(b"* OK scripted IMAP ready\r\n").await.unwrap();
            let full = |seq: usize, uid: u32| {
                let body = format!(
                    "From: Ann <ann@example.org>\r\nSubject: Found {uid}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nThe invoice, number {uid}\r\n"
                );
                format!(
                    "* {seq} FETCH (UID {uid} FLAGS (\\Seen) INTERNALDATE \"01-Jan-2026 10:{:02}:00 +0000\" ENVELOPE (\"Thu, 1 Jan 2026 10:00:00 +0000\" \"Found {uid}\" ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((NIL NIL \"me\" \"example.com\")) NIL NIL NIL \"<f{uid}@example.org>\") BODY[]<0> {{{}}}\r\n{body})\r\n",
                    uid % 60,
                    body.len()
                )
            };
            loop {
                // One command: its first line, and each literal with the
                // line that follows it.
                let mut cmd = Vec::new();
                let mut first = true;
                let mut early = false;
                loop {
                    let mut line = Vec::new();
                    if r.read_until(b'\n', &mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    cmd.extend_from_slice(&line);
                    let Some(n) = literal_at_end(&line) else {
                        break;
                    };
                    let searching = String::from_utf8_lossy(&line)
                        .to_ascii_uppercase()
                        .contains(" UID SEARCH ");
                    if first && searching && matches!(reply, SearchReply::RefuseAtOnce(_)) {
                        early = true;
                        break;
                    }
                    first = false;
                    w.write_all(b"+ go ahead\r\n").await.unwrap();
                    let mut bytes = vec![0; n];
                    r.read_exact(&mut bytes).await.unwrap();
                    cmd.extend_from_slice(&bytes);
                }
                let space = cmd.iter().position(|b| *b == b' ').unwrap_or(cmd.len());
                let tag = String::from_utf8_lossy(&cmd[..space]).to_string();
                let rest = cmd.get(space + 1..).unwrap_or_default().to_vec();
                log.lock().unwrap().push(rest.clone());
                let up = String::from_utf8_lossy(&rest).to_ascii_uppercase();
                let answer = if up.starts_with("LOGIN") {
                    format!("{tag} OK LOGIN done\r\n")
                } else if up.starts_with("LIST") {
                    format!("{list}{tag} OK LIST done\r\n")
                } else if up.starts_with("SELECT") {
                    format!(
                        "* {} EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n{tag} OK [READ-WRITE] SELECT done\r\n",
                        inbox.len()
                    )
                } else if up.starts_with("UID SEARCH") {
                    match reply {
                        SearchReply::Found(uids) => format!(
                            "* SEARCH {}\r\n{tag} OK SEARCH done\r\n",
                            join(uids).replace(',', " ")
                        ),
                        SearchReply::Refuse(said) => format!("{tag} {said}\r\n"),
                        SearchReply::RefuseAtOnce(said) => {
                            assert!(early);
                            format!("{tag} {said}\r\n")
                        }
                    }
                } else if up.starts_with("UID FETCH") {
                    let arg = up.split_whitespace().nth(2).unwrap_or("").to_string();
                    let want: Vec<u32> = arg.split(',').filter_map(|u| u.parse().ok()).collect();
                    let found: String = inbox
                        .iter()
                        .enumerate()
                        .filter(|(_, u)| want.contains(u))
                        .map(|(i, u)| full(i + 1, *u))
                        .collect();
                    format!("{found}{tag} OK FETCH done\r\n")
                } else if up.starts_with("LOGOUT") {
                    format!("* BYE\r\n{tag} OK LOGOUT done\r\n")
                } else {
                    format!("{tag} OK done\r\n")
                };
                if w.write_all(answer.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        (scripted_login(addr).await, seen)
    }

    const INBOX_ONLY: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n";

    /// The one `UID SEARCH` sent, exactly as it went, after its tag.
    fn the_search(log: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>) -> Vec<u8> {
        let log = log.lock().unwrap();
        let searches: Vec<&Vec<u8>> = log
            .iter()
            .filter(|c| c.starts_with(b"UID SEARCH"))
            .collect();
        assert_eq!(searches.len(), 1, "one search, never a second try: {log:?}");
        searches[0].clone()
    }

    fn searched_uids(got: &Searched) -> Vec<u32> {
        got.messages.iter().map(|m| m.uid).collect()
    }

    #[test]
    fn a_server_search_sends_the_query_as_literals_and_fetches_the_newest_found() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_search(
                &[3, 5, 8, 13, 21],
                INBOX_ONLY,
                SearchReply::Found(&[21, 3, 8, 13]),
            )
            .await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, " invoice ", 2)
                .await
                .unwrap();
            assert_eq!(got.matched, 4, "every match is counted");
            assert_eq!(searched_uids(&got), vec![21, 13], "the newest two, newest first");
            assert_eq!(got.messages[0].subject, "Found 21");
            assert_eq!(
                the_search(&log),
                b"UID SEARCH OR OR FROM {7}\r\ninvoice SUBJECT {7}\r\ninvoice TEXT {7}\r\ninvoice\r\n"
                    .to_vec(),
                "trimmed, and no CHARSET for plain ASCII"
            );
            let log = log.lock().unwrap();
            assert!(
                log.iter().any(|c| c.starts_with(b"UID FETCH 13,21 ")),
                "only the newest two are fetched: {log:?}"
            );
        });
    }

    #[test]
    fn a_quote_or_a_backslash_goes_as_bytes_of_a_literal() {
        rt_act().block_on(async {
            // Inside a quoted string, `a" OR ALL "` would have ended the
            // string and searched for everything.
            let q = r#"a" OR ALL "\b"#;
            let (mut s, log) =
                scripted_search(&[1, 2], INBOX_ONLY, SearchReply::Found(&[2])).await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, q, 50)
                .await
                .unwrap();
            assert_eq!(searched_uids(&got), vec![2]);
            let n = q.len();
            assert_eq!(
                the_search(&log),
                format!(
                    "UID SEARCH OR OR FROM {{{n}}}\r\n{q} SUBJECT {{{n}}}\r\n{q} TEXT {{{n}}}\r\n{q}\r\n"
                )
                .into_bytes()
            );
        });
    }

    #[test]
    fn an_accented_query_says_its_charset_and_counts_its_bytes() {
        rt_act().block_on(async {
            let (mut s, log) =
                scripted_search(&[1, 2], INBOX_ONLY, SearchReply::Found(&[1])).await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, "café", 50)
                .await
                .unwrap();
            assert_eq!(searched_uids(&got), vec![1]);
            // Five bytes, not four characters.
            assert_eq!(
                the_search(&log),
                "UID SEARCH CHARSET UTF-8 OR OR FROM {5}\r\ncafé SUBJECT {5}\r\ncafé TEXT {5}\r\ncafé\r\n"
                    .as_bytes()
                    .to_vec()
            );
        });
    }

    #[test]
    fn a_line_break_in_a_query_is_refused_before_anything_is_sent() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_search(&[1], INBOX_ONLY, SearchReply::Found(&[1])).await;
            for q in [
                "x\r\nA2 DELETE INBOX",
                "x\nDELETE",
                "tab\there",
                "nul\0",
                "",
            ] {
                let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, q, 50).await;
                assert!(matches!(got, Err(Fetched::Net(_))), "{q:?}: {got:?}");
            }
            let long = "a".repeat(SEARCH_QUERY_MAX + 1);
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, &long, 50).await;
            assert!(matches!(got, Err(Fetched::Net(_))), "{got:?}");
            // Two hundred accented letters are two hundred characters.
            assert!(search_query(&"é".repeat(SEARCH_QUERY_MAX)).is_ok());
            let log = log.lock().unwrap();
            assert!(
                log.len() == 1 && log[0].starts_with(b"LOGIN "),
                "nothing after signing in: {log:?}"
            );
        });
    }

    #[test]
    fn a_refused_search_is_an_error_never_nothing_found() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_search(
                &[1, 2],
                INBOX_ONLY,
                SearchReply::Refuse("NO [BADCHARSET (US-ASCII)] UTF-8 not supported"),
            )
            .await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, "café", 50).await;
            let Err(Fetched::Net(why)) = got else {
                panic!("{got:?}")
            };
            assert!(why.contains("outside plain ASCII"), "{why}");
            assert!(why.contains("me@example.com"), "{why}");
            // One search: without CHARSET the bytes would be read as ASCII.
            the_search(&log);
            let log = log.lock().unwrap();
            assert!(!log.iter().any(|c| c.starts_with(b"UID FETCH")), "{log:?}");

            let (mut s, _) =
                scripted_search(&[1], INBOX_ONLY, SearchReply::Refuse("BAD Invalid search")).await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, "plain", 50).await;
            let Err(Fetched::Net(why)) = got else {
                panic!("{got:?}")
            };
            assert!(why.contains("It said: Invalid search"), "{why}");
        });
    }

    #[test]
    fn a_search_refused_at_its_first_line_sends_no_literal() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_search(
                &[1],
                INBOX_ONLY,
                SearchReply::RefuseAtOnce("NO Search is switched off"),
            )
            .await;
            let got = search_in(
                &mut s,
                &mut no_redial(),
                &me(),
                &Folder::Inbox,
                "secret",
                50,
            )
            .await;
            let Err(Fetched::Net(why)) = got else {
                panic!("{got:?}")
            };
            assert!(why.contains("Search is switched off"), "{why}");
            assert_eq!(the_search(&log), b"UID SEARCH OR OR FROM {6}\r\n".to_vec());
            // The session is still usable: the next command is a command.
            assert!(s.noop().await.is_ok());
        });
    }

    #[test]
    fn named_folders_and_gmails_archive_are_not_searched_yet() {
        rt_act().block_on(async {
            let gmail = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n";
            let (mut s, log) = scripted_search(&[1], gmail, SearchReply::Found(&[1])).await;
            for folder in [Folder::Named("Receipts".into()), Folder::Archive] {
                let got = search_in(&mut s, &mut no_redial(), &me(), &folder, "x", 50).await;
                let Err(Fetched::Net(why)) = got else {
                    panic!("{got:?}")
                };
                assert!(why.contains("cannot search"), "{why}");
            }
            let log = log.lock().unwrap();
            assert!(!log.iter().any(|c| c.starts_with(b"UID SEARCH")), "{log:?}");
            assert!(!log.iter().any(|c| c.starts_with(b"SELECT")), "{log:?}");
        });
    }

    #[test]
    fn nothing_found_is_an_empty_answer() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_search(&[1, 2], INBOX_ONLY, SearchReply::Found(&[])).await;
            let got = search_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, "zzz", 50)
                .await
                .unwrap();
            assert_eq!(
                got,
                Searched {
                    messages: vec![],
                    matched: 0
                }
            );
        });
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
        let (addr, seen) = scripted_mailbox(uids, uidvalidity, Quirks::default()).await;
        (scripted_login(addr).await, seen)
    }

    /// How the scripted mailbox departs from a well-behaved server.
    #[derive(Clone, Copy, Default)]
    struct Quirks {
        /// Messages whose text comes as GreenMail 2.1 sends a partial fetch:
        /// `BODY[]<0>{n}`, without the space RFC 3501 puts before a literal,
        /// which the parser RATA uses rejects.
        greenmail: &'static [u32],
        /// The message partway through which the server hangs up.
        hang_up_at: Option<u32>,
        /// Messages from a mailing list: their headers carry a
        /// `List-Unsubscribe`, folded across lines, and a one-click
        /// `List-Unsubscribe-Post`.
        listed: &'static [u32],
    }

    /// Where a test's fresh connections come from: another sign-in to the
    /// scripted mailbox at `addr`, or none at all.
    struct Scripted(Option<std::net::SocketAddr>);

    impl Connect for Scripted {
        type Stream = TcpStream;
        async fn connect(&self) -> Result<Session<TcpStream>, String> {
            match self.0 {
                Some(addr) => Ok(scripted_login(addr).await),
                None => Err("there is nowhere to reconnect to".into()),
            }
        }
    }

    fn no_redial() -> Redial<Scripted> {
        Redial::new(Scripted(None))
    }

    fn redial_to(addr: std::net::SocketAddr) -> Redial<Scripted> {
        Redial::new(Scripted(Some(addr)))
    }

    async fn scripted_login(addr: std::net::SocketAddr) -> Session<TcpStream> {
        let tcp = TcpStream::connect(addr).await.unwrap();
        let mut client = async_imap::Client::new(tcp);
        client.read_response().await.unwrap();
        client
            .login("me@example.com", "pw")
            .await
            .map_err(|(e, _)| e)
            .unwrap()
    }

    /// A scripted inbox holding `uids` that answers every connection made to
    /// it, logging the commands of all of them in one list.
    async fn scripted_mailbox(
        uids: &'static [u32],
        uidvalidity: u32,
        quirks: Quirks,
    ) -> (std::net::SocketAddr, Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                log.lock().unwrap().push("(connected)".into());
                tokio::spawn(scripted_mailbox_line(
                    sock,
                    uids,
                    uidvalidity,
                    quirks,
                    log.clone(),
                ));
            }
        });
        (addr, seen)
    }

    /// One connection to [`scripted_mailbox`].
    async fn scripted_mailbox_line(
        sock: TcpStream,
        uids: &'static [u32],
        uidvalidity: u32,
        quirks: Quirks,
        log: Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        {
            let (r, mut w) = sock.into_split();
            let mut lines = BufReader::new(r).lines();
            w.write_all(b"* OK scripted IMAP ready\r\n").await.unwrap();
            let envelope = |uid: u32| {
                format!(
                    "UID {uid} FLAGS (\\Seen) INTERNALDATE \"01-Jan-2026 10:{:02}:00 +0000\" ENVELOPE (\"Thu, 1 Jan 2026 10:00:00 +0000\" \"Subject {uid}\" ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((NIL NIL \"me\" \"example.com\")) NIL NIL NIL \"<m{uid}@example.org>\")",
                    uid / 2, // minutes that keep receive-order equal to UID order for 0..119
                )
            };
            let full = |seq: usize, uid: u32| {
                // What a real server sends for BODY[]: headers and a MIME
                // body, here quoted-printable UTF-8 as most mail programs write.
                let list = if quirks.listed.contains(&uid) {
                    format!(
                        "List-Unsubscribe: <mailto:leave@list.example?subject=leave%20{uid}>,\r\n <https://list.example/u/{uid}>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n"
                    )
                } else {
                    String::new()
                };
                let body = format!(
                    "From: Ann <ann@example.org>\r\nSubject: Subject {uid}\r\n{list}MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nBody of {uid} =E2=80=94 caf=C3=A9\r\n"
                );
                let gap = if quirks.greenmail.contains(&uid) {
                    ""
                } else {
                    " "
                };
                format!(
                    "* {seq} FETCH ({} BODY[]<0>{gap}{{{}}}\r\n{body})\r\n",
                    envelope(uid),
                    body.len()
                )
            };
            let listed = |seq: usize, uid: u32| format!("* {seq} FETCH ({})\r\n", envelope(uid));
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
                } else if up.starts_with("UID FETCH") && up.contains("ENVELOPE") {
                    // Particular messages by UID, without their text.
                    let want: Vec<u32> = arg.split(',').filter_map(|u| u.parse().ok()).collect();
                    uids.iter()
                        .enumerate()
                        .filter(|(_, u)| want.contains(u))
                        .map(|(i, u)| listed(i + 1, *u))
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
                } else if up.starts_with("FETCH") && up.contains("ENVELOPE") && !up.contains("BODY")
                {
                    // A range of messages, without their text.
                    let (a, b) = arg.split_once(':').unwrap();
                    let a: usize = a.parse().unwrap();
                    let b: usize = if b == "*" {
                        uids.len()
                    } else {
                        b.parse().unwrap()
                    };
                    (a..=b)
                        .filter(|s| *s >= 1 && *s <= uids.len())
                        .map(|s| listed(s, uids[s - 1]))
                        .collect()
                } else if up.starts_with("FETCH") && !up.contains("BODY") {
                    // Read and starred only: even UIDs are read, every fifth
                    // is starred.
                    let (a, _) = arg.split_once(':').unwrap();
                    let a: usize = a.parse().unwrap();
                    (a.max(1)..=uids.len())
                        .map(|s| {
                            let u = uids[s - 1];
                            let mut f = Vec::new();
                            if u.is_multiple_of(2) {
                                f.push("\\Seen");
                            }
                            if u.is_multiple_of(5) {
                                f.push("\\Flagged");
                            }
                            format!("* {s} FETCH (UID {u} FLAGS ({}))\r\n", f.join(" "))
                        })
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
                // Hanging up halfway through a message's text: the socket is
                // closed with a literal still owed.
                if let Some(at) = quirks
                    .hang_up_at
                    .and_then(|u| body.find(&format!("Body of {u} ")))
                {
                    let _ = w.write_all(&body.as_bytes()[..at]).await;
                    return;
                }
                if w.write_all(format!("{body}{tag} OK done\r\n").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    fn me() -> Account {
        Account {
            email: "me@example.com".into(),
            credential: Credential::Password("pw".into()),
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
            let got = older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 40, 7, 2).await;
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
                uids_of(older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 40, 7, 2).await),
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
                uids_of(older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 70, 7, 2).await),
                vec![60, 50]
            );
        });
    }

    #[test]
    fn at_the_start_of_the_mailbox_there_is_nothing_older() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20, 30], 7).await;
            assert_eq!(
                uids_of(older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 10, 7, 50).await),
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
                older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 30, 9, 50).await,
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
                let page = uids_of(
                    older_in(
                        &mut s,
                        &mut no_redial(),
                        &me(),
                        &Folder::Inbox,
                        oldest,
                        7,
                        3,
                    )
                    .await,
                );
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

    /// What a move out of Gmail's archive is: a copy, still archived.
    fn copied(uids: &[u32]) -> Acted {
        Acted::Copied {
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
                apply(&mut s, &Folder::Inbox, &[42, 43, 57], 7, &Action::Trash).await,
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Read).await,
                done(&[42])
            );
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
                apply(&mut s, &Folder::Inbox, &[42], 9, &Action::Trash).await,
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
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                done(&[42])
            );
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
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                Acted::NoPlace(_)
            ));
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    /// GreenMail's LIST, and many a small host's: no special-use attribute on
    /// anything, so every folder is known only by its name.
    const UNDECLARED: &str =
        "* LIST () \".\" INBOX\r\n* LIST () \".\" Archive\r\n* LIST () \".\" Trash\r\n";

    #[test]
    fn archive_goes_where_a_refresh_reads_it_on_a_server_that_declares_nothing() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: UNDECLARED,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            // A refresh reads "Archive" as the archive, by its name…
            assert_eq!(places(&mut s).await.archive.as_deref(), Some("Archive"));
            // …so archiving files mail there, rather than refusing.
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Archive).await,
                done(&[42])
            );
            assert!(
                log.lock()
                    .unwrap()
                    .iter()
                    .any(|c| c == "UID MOVE 42 \"Archive\""),
                "{:?}",
                log.lock().unwrap()
            );
        });
    }

    #[test]
    fn delete_goes_to_a_folder_named_trash_on_a_server_that_declares_nothing() {
        rt_act().block_on(async {
            // GreenMail and many a small host declare no \Trash. Refusing
            // there left Delete doing nothing on the server at all (BUG-M).
            let (mut s, log) = scripted_session(Script {
                caps: "MOVE UIDPLUS",
                list: UNDECLARED,
                uidvalidity: 7,
                present: &[42],
            })
            .await;
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                done(&[42])
            );
            assert_eq!(changed(&log.lock().unwrap()), vec!["UID MOVE 42 \"Trash\""]);
        });
    }

    #[test]
    fn trash_by_name_is_the_whole_name_and_a_declared_trash_wins() {
        let row = |attrs: &str, name: &str| format!("* LIST ({attrs}) \"/\" \"{name}\"\r\n");
        let names = |rows: &[String]| {
            let raw = format!("* LIST () \"/\" \"INBOX\"\r\n{}", rows.concat());
            let list: &'static str = Box::leak(raw.into_boxed_str());
            rt_act().block_on(async {
                let (mut s, _) = scripted_session(Script {
                    caps: "MOVE UIDPLUS",
                    list,
                    uidvalidity: 7,
                    present: &[42],
                })
                .await;
                trash_in(&listing(&mut s).await.expect("a listing"))
            })
        };
        // Each of the names providers give it.
        for n in [
            "Trash",
            "Deleted Items",
            "Deleted Messages",
            "Bin",
            "INBOX/Trash",
        ] {
            assert_eq!(names(&[row("", n)]).as_deref(), Some(n), "{n}");
        }
        assert_eq!(names(&[row("", "trash")]).as_deref(), Some("trash"));
        // Never a name that merely contains the word, one that cannot be
        // opened, or one declared as something else.
        for rows in [
            vec![row("", "Trash 2019"), row("", "Old Bin")],
            vec![row("\\Noselect", "Trash")],
            vec![row("\\Sent", "Trash")],
        ] {
            assert_eq!(names(&rows), None, "{rows:?}");
        }
        // A declared Trash wins over one merely named so.
        assert_eq!(
            names(&[row("", "Trash"), row("\\Trash", "Papierkorb")]).as_deref(),
            Some("Papierkorb")
        );
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Archive).await,
                done(&[42])
            );
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                done(&[42])
            );
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                done(&[42])
            );
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
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[42], 7, &Action::Trash).await,
                done(&[42])
            );
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
            credential: Credential::Password("not-a-real-password".into()),
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
                match fetch_newest(&r, &acct(host), 15, &[]).await {
                    Err(Fetched::Host(why)) => assert!(
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
            match fetch_newest(&r, &acct(&host), 15, &[]).await {
                // Net, emphatically not Auth: the caller stops retrying a
                // mailbox on Auth, and a DNS outage must not unlink everybody.
                Err(Fetched::Net(why)) => assert!(why.contains("did not sync"), "{why}"),
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
    fn a_password_is_never_sent_to_microsoft() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            // A consumer address (from the table, no DNS) and Microsoft's
            // server typed by hand: both answered before any socket opens.
            match verify(&r, "someone@outlook.com", "right-password", None).await {
                Verify::Microsoft(label) => assert_eq!(label, "Outlook"),
                other => panic!("{other:?}"),
            }
            // Named for Microsoft, not for the address's own domain.
            match verify(&r, "me@example.com", "pw", Some("outlook.office365.com")).await {
                Verify::Microsoft(label) => assert_eq!(label, "Microsoft 365"),
                other => panic!("{other:?}"),
            }
            match verify(&r, "me@hotmail.com", "pw", Some("outlook.office365.com")).await {
                Verify::Microsoft(label) => assert_eq!(label, "Outlook"),
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
        let msg = refusal(&cand, "");
        assert!(msg.contains("Google Workspace"), "{msg}");
        assert!(msg.contains("app password"), "{msg}");
        assert!(msg.contains("apppasswords"), "{msg}");
        assert!(!msg.contains("The server said"), "{msg}");
    }

    #[test]
    fn a_refusal_keeps_what_the_server_said() {
        // Gmail's refusal when a Workspace administrator has turned IMAP off:
        // nothing to do with the password, and only the server says so.
        let cand = Candidate {
            host: "imap.gmail.com".into(),
            port: IMAP_PORT,
            label: "Gmail".into(),
            help: Some("myaccount.google.com/apppasswords".into()),
            source: Source::Table,
        };
        let msg = refusal(
            &cand,
            "[ALERT] IMAP access is disabled for your domain. Please contact your domain administrator.",
        );
        assert!(msg.contains("app password"), "{msg}");
        assert!(
            msg.ends_with(
                "The server said: [ALERT] IMAP access is disabled for your domain. Please contact your domain administrator."
            ),
            "{msg}"
        );
    }

    #[test]
    fn a_refused_password_at_link_time_carries_the_servers_words_and_no_secret() {
        // End to end through `login`: the words reach the sentence, and the
        // password a server echoes does not.
        rt_act().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                let (sock, _) = listener.accept().await.unwrap();
                let (r, mut w) = sock.into_split();
                let mut lines = BufReader::new(r).lines();
                w.write_all(b"* OK ready\r\n").await.unwrap();
                while let Ok(Some(line)) = lines.next_line().await {
                    let tag = line.split(' ').next().unwrap_or("*").to_string();
                    w.write_all(format!("{tag} NO [ALERT] IMAP access is disabled for your domain (you sent hunter2-secret)\r\n").as_bytes()).await.unwrap();
                }
            });
            let tcp = TcpStream::connect(addr).await.unwrap();
            let mut client = async_imap::Client::new(tcp);
            client.read_response().await.unwrap();
            let why = match sign_in(
                client,
                "me@example.com",
                &Credential::Password("hunter2-secret".into()),
            )
            .await
            {
                Err(Trouble::Auth(why)) => why,
                Err(other) => panic!("{other:?}"),
                Ok(_) => panic!("signed in"),
            };
            let cand = Candidate {
                host: "imap.gmail.com".into(),
                port: IMAP_PORT,
                label: "Gmail".into(),
                help: None,
                source: Source::Table,
            };
            let msg = refusal(&cand, &why);
            assert!(msg.contains("IMAP access is disabled for your domain"), "{msg}");
            assert!(!msg.contains("hunter2"), "{msg}");
        });
    }

    #[test]
    fn a_known_provider_that_cannot_be_reached_names_it_and_never_asks_for_a_host() {
        let gmail = Candidate {
            host: "imap.gmail.com".into(),
            port: IMAP_PORT,
            label: "Gmail".into(),
            help: None,
            source: Source::Table,
        };
        assert!(known(&gmail));
        let msg = known_host_failed(&gmail, "failed to lookup address information.");
        assert_eq!(
            msg,
            "Could not reach Gmail (imap.gmail.com): failed to lookup address information. Check your connection and try again in a few minutes."
        );
        assert!(!msg.contains("IMAP server address"), "{msg}");
        assert!(known(&Candidate {
            source: Source::Mx,
            ..gmail.clone()
        }));
        for source in [Source::Srv, Source::Guess, Source::Override] {
            assert!(!known(&Candidate {
                source,
                ..gmail.clone()
            }));
        }
    }

    #[test]
    fn a_typed_server_address_that_fails_says_why() {
        // What B1 saw on a real server: the handshake's reason, dropped.
        let msg = typed_host_failed(
            "mail.example.com",
            Some("mail.example.com could not be trusted: invalid peer certificate: UnknownIssuer"),
        );
        assert!(
            msg.starts_with("Could not connect to mail.example.com: "),
            "{msg}"
        );
        assert!(
            msg.contains("invalid peer certificate: UnknownIssuer."),
            "{msg}"
        );
        assert!(msg.contains("Check the server address"), "{msg}");
        // Nothing to add: the old sentence.
        assert_eq!(
            typed_host_failed("mail.example.com", None),
            "Could not reach mail.example.com. Check the server address with your mail provider."
        );
    }

    // ---------------------------------------------------------- re-reading

    #[test]
    fn stored_mail_can_be_read_again_by_uid() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&[10, 20, 30], 7).await;
            // 25 was never there; 20 and 30 are.
            let got = uids_in(
                &mut s,
                &mut no_redial(),
                &me(),
                &Folder::Inbox,
                &[20, 25, 30],
                7,
            )
            .await;
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
            let got = uids_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, &[10], 7).await;
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
            let Whole::Raw(raw) = whole_in(&mut s, &me(), &Folder::Inbox, 20, 7).await else {
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
                whole_in(&mut s, &me(), &Folder::Inbox, 99, 7).await,
                Whole::TooLarge(_)
            ));
            assert!(!log.lock().unwrap().iter().any(|c| c.contains("BODY.PEEK")));
        });
    }

    #[test]
    fn a_message_that_has_gone_says_so() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_inbox(&[10], 7).await;
            assert!(matches!(
                whole_in(&mut s, &me(), &Folder::Inbox, 11, 7).await,
                Whole::Gone
            ));
            let (mut s, _) = scripted_inbox(&[10], 8).await;
            assert!(matches!(
                whole_in(&mut s, &me(), &Folder::Inbox, 10, 7).await,
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
            let got = read_range(&mut s, &mut no_redial(), &me(), &Folder::Inbox, "1:2", 5)
                .await
                .ok()
                .unwrap();
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

    #[test]
    fn a_refresh_carries_how_to_leave_a_mailing_list() {
        // What `fetch_newest` does once signed in, against a mailbox where
        // one message came from a list.
        rt_act().block_on(async {
            let quirks = Quirks {
                listed: &[8],
                ..Quirks::default()
            };
            let (addr, log) = scripted_mailbox(&[7, 8], 5, quirks).await;
            let mut s = scripted_login(addr).await;
            let got = newest_everywhere(&mut s, &mut redial_to(addr), &me(), 15, &[])
                .await
                .unwrap_or_else(|e| panic!("the refresh failed: {e:?}"));
            let eight = got.messages.iter().find(|m| m.uid == 8).unwrap();
            let u = eight.unsubscribe.as_ref().expect("the list's header");
            assert_eq!(u.https.as_deref(), Some("https://list.example/u/8"));
            assert_eq!(
                u.mailto.as_deref(),
                Some("mailto:leave%40list.example?subject=leave%208")
            );
            let seven = got.messages.iter().find(|m| m.uid == 7).unwrap();
            assert_eq!(seven.unsubscribe, None);
            // Read, never acted on: the refresh asked the mailbox for
            // nothing it does not always ask for, and named no list address.
            let log = log.lock().unwrap();
            assert!(
                !log.iter()
                    .any(|c| c.to_ascii_lowercase().contains("list.example")),
                "{log:?}"
            );
        });
    }

    // ------------------------------------------- replies RATA cannot read
    //
    // GreenMail 2.1 answers a partial fetch as `BODY[]<0>{n}`, with no space
    // before the literal, and the parser RATA uses rejects it. That is one
    // server's quirk, but any reply the parser cannot read must cost the
    // message it belongs to — never the page, and never put the message's
    // own words into an error.

    /// Every word of the scripted mailbox's message `uid` that an error or a
    /// marked message must never show.
    fn words_of(uid: u32) -> [String; 4] {
        [
            format!("Body of {uid}"),
            format!("Subject {uid}"),
            "ann@example.org".into(),
            "FETCH".into(),
        ]
    }

    #[test]
    fn a_reply_the_parser_cannot_read_costs_that_message_not_the_refresh() {
        rt_act().block_on(async {
            let quirks = Quirks {
                greenmail: &[2],
                ..Quirks::default()
            };
            let (addr, log) = scripted_mailbox(&[1, 2, 3], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            let got = newest_everywhere(&mut s, &mut redial_to(addr), &me(), 15, &[]).await;
            let got = got.unwrap_or_else(|e| panic!("the refresh failed: {e:?}"));
            let mut uids: Vec<u32> = got.messages.iter().map(|m| m.uid).collect();
            uids.sort();
            assert_eq!(uids, vec![1, 2, 3], "every message is listed");
            for m in &got.messages {
                if m.uid == 2 {
                    assert!(m.truncated, "opening it fetches it whole");
                    assert_eq!(m.subject, "Subject 2", "listed as it is");
                    assert_eq!(m.body, UNREADABLE);
                    assert_eq!(m.preview, UNREADABLE);
                } else {
                    assert_eq!(m.body, format!("Body of {} — café", m.uid));
                    assert!(!m.truncated);
                }
            }
            // A fresh connection for what the broken one could not bring, and
            // no more than the refresh's allowance of them.
            let log = log.lock().unwrap();
            let connections = log.iter().filter(|c| *c == "(connected)").count();
            assert!((2..=1 + REDIALS as usize).contains(&connections), "{log:?}");
            // The one that could not be read was asked for on its own.
            assert!(
                log.iter()
                    .any(|c| c.starts_with("UID FETCH 2 ") && c.contains("BODY.PEEK")),
                "{log:?}"
            );
        });
    }

    #[test]
    fn a_server_whose_every_reply_is_unreadable_costs_two_more_sign_ins_at_most() {
        rt_act().block_on(async {
            let quirks = Quirks {
                greenmail: &[1, 2, 3],
                ..Quirks::default()
            };
            let (addr, log) = scripted_mailbox(&[1, 2, 3], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            let got = newest_everywhere(&mut s, &mut redial_to(addr), &me(), 15, &[])
                .await
                .unwrap_or_else(|e| panic!("the refresh failed: {e:?}"));
            // All listed, newest first pinned one by one while connections
            // were allowed, the rest said to be unread.
            let mut said: Vec<(u32, &str)> = got
                .messages
                .iter()
                .map(|m| (m.uid, m.body.as_str()))
                .collect();
            said.sort();
            assert_eq!(said, vec![(1, NOT_READ), (2, UNREADABLE), (3, UNREADABLE)]);
            assert!(got.messages.iter().all(|m| m.truncated));
            let log = log.lock().unwrap();
            let connections = log.iter().filter(|c| *c == "(connected)").count();
            assert_eq!(connections, 1 + REDIALS as usize, "{log:?}");
        });
    }

    #[test]
    fn async_imap_reads_nothing_more_after_a_reply_it_cannot_read() {
        // What `recover` is built on: after a reply its parser rejects,
        // async-imap reads nothing more on that connection — though here the
        // server's tagged OK is right behind it — and a SELECT then
        // "succeeds" with an empty mailbox. Were that connection used again,
        // a folder would read as empty. If an upgrade changes this,
        // recovering on a fresh connection is merely cautious.
        rt_act().block_on(async {
            let quirks = Quirks {
                greenmail: &[1],
                ..Quirks::default()
            };
            let (addr, _) = scripted_mailbox(&[1, 2], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            s.select("INBOX").await.unwrap();
            let got: Vec<_> = s.fetch("1:*", items()).await.unwrap().collect().await;
            assert_eq!(got.len(), 1, "one error, then the end: {got:?}");
            let Err(e) = &got[0] else {
                panic!("expected the parser's error")
            };
            assert!(unreadable(e), "{:.80}", e.to_string());
            assert_eq!(reason(e), UNPARSED);
            let after = s.select("INBOX").await.unwrap();
            assert_eq!((after.exists, after.uid_validity), (0, None));
        });
    }

    #[test]
    fn an_error_is_one_short_line_of_rata_s_own() {
        let parse = ImapError::Io(std::io::Error::other(
            "Error(Error { input: [42], code: TakeWhile1 }) during parsing of \"* 1 FETCH (BODY[]<0>{5}\\r\\nHello)\"",
        ));
        assert!(unreadable(&parse));
        assert_eq!(reason(&parse), UNPARSED);
        let reset = ImapError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        ));
        assert!(!unreadable(&reset));
        assert_eq!(reason(&reset), "connection reset by peer");
        let long = ImapError::No(format!("first line\r\nsecond {}", "x".repeat(500)));
        assert_eq!(reason(&long), "first line");
        let wide = ImapError::Bad("y".repeat(500));
        assert_eq!(reason(&wide).len(), 200);
    }

    #[test]
    fn a_page_of_older_mail_keeps_what_could_be_read() {
        rt_act().block_on(async {
            let quirks = Quirks {
                greenmail: &[20],
                ..Quirks::default()
            };
            let (addr, _) = scripted_mailbox(&[10, 20, 30, 40], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            let Fetched::Messages(got) = older_in(
                &mut s,
                &mut redial_to(addr),
                &me(),
                &Folder::Inbox,
                40,
                7,
                50,
            )
            .await
            else {
                panic!("older mail failed")
            };
            let mut uids: Vec<u32> = got.iter().map(|m| m.uid).collect();
            uids.sort();
            assert_eq!(uids, vec![10, 20, 30]);
            let twenty = got.iter().find(|m| m.uid == 20).unwrap();
            assert!(twenty.truncated);
            assert!(
                got.iter()
                    .find(|m| m.uid == 30)
                    .unwrap()
                    .body
                    .contains("Body of 30")
            );
        });
    }

    #[test]
    fn a_reply_that_cannot_be_read_is_never_quoted_in_an_error() {
        rt_act().block_on(async {
            // Nowhere to reconnect to: the refresh fails, and says so in a
            // line of its own words.
            let quirks = Quirks {
                greenmail: &[2],
                ..Quirks::default()
            };
            let (addr, _) = scripted_mailbox(&[1, 2, 3], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            let Err(Fetched::Net(why)) =
                newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[]).await
            else {
                panic!("expected the refresh to fail without a second connection")
            };
            for w in words_of(2) {
                assert!(!why.contains(&w), "{w:?} is in {why:?}");
            }
            assert!(why.contains("inbox"), "names the folder: {why}");
            assert!(
                !why.contains('\n') && why.len() < 400,
                "one short line: {why}"
            );
        });
    }

    #[test]
    fn a_connection_that_really_goes_fails_the_folder_without_quoting_mail() {
        rt_act().block_on(async {
            let quirks = Quirks {
                hang_up_at: Some(2),
                ..Quirks::default()
            };
            let (addr, _) = scripted_mailbox(&[1, 2, 3], 7, quirks).await;
            let mut s = scripted_login(addr).await;
            let Err(Fetched::Net(why)) =
                newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[]).await
            else {
                panic!("a connection that went partway must fail the refresh")
            };
            for w in words_of(2) {
                assert!(!why.contains(&w), "{w:?} is in {why:?}");
            }
            assert!(why.contains("inbox"), "names the folder: {why}");
        });
    }

    // ----------------------------------------------------------- sent tests
    //
    // A scripted server with an inbox and a Sent folder under whatever name
    // `list` gives it, each with its own UIDs and UIDVALIDITY. What is under
    // test is which folder RATA opens, and what it calls what it finds there.

    async fn scripted_folders(
        list: &'static str,
        sent: &'static str,
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
            // Which folder is selected: the inbox holds UIDs 1..=3 under
            // UIDVALIDITY 7, Sent holds 11..=12 under 8.
            let mut in_sent = false;
            let full = |seq: usize, uid: u32| {
                let body = format!(
                    "From: Me <me@example.com>\r\nTo: Bo Li <bo@example.org>\r\nSubject: Sent {uid}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nWhat I wrote in {uid}\r\n"
                );
                format!(
                    "* {seq} FETCH (UID {uid} FLAGS (\\Seen) INTERNALDATE \"01-Jan-2026 10:{:02}:00 +0000\" ENVELOPE (\"Thu, 1 Jan 2026 10:00:00 +0000\" \"Sent {uid}\" ((\"Me\" NIL \"me\" \"example.com\")) ((\"Me\" NIL \"me\" \"example.com\")) ((\"Me\" NIL \"me\" \"example.com\")) ((\"Bo Li\" NIL \"bo\" \"example.org\")(NIL NIL \"Cy\" \"Example.org\")) ((\"Dee\" NIL \"dee\" \"example.org\")) ((NIL NIL \"boss\" \"example.net\")) \"<orig@example.org> <older@example.org>\" \"<s{uid}@example.com>\") BODY[]<0> {{{}}}\r\n{body})\r\n",
                    uid,
                    body.len()
                )
            };
            while let Ok(Some(line)) = lines.next_line().await {
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                log.lock().unwrap().push(cmd.to_string());
                let up = cmd.to_ascii_uppercase();
                let body = if up.starts_with("LIST") {
                    list.to_string()
                } else if up.starts_with("SELECT") {
                    let name = cmd[6..].trim().trim_matches('"');
                    in_sent = name == sent;
                    if in_sent {
                        "* 2 EXISTS\r\n* OK [UIDVALIDITY 8] ok\r\n".to_string()
                    } else {
                        "* 3 EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n".to_string()
                    }
                } else if up.starts_with("FETCH") {
                    let uids: &[u32] = if in_sent { &[11, 12] } else { &[1, 2, 3] };
                    uids.iter()
                        .enumerate()
                        .map(|(i, u)| full(i + 1, *u))
                        .collect()
                } else if up.starts_with("UID FETCH") {
                    let uids: &[u32] = if in_sent { &[11, 12] } else { &[1, 2, 3] };
                    let asked = cmd.split_whitespace().nth(2).unwrap_or("");
                    asked
                        .split(',')
                        .filter_map(|u| u.parse::<u32>().ok())
                        .filter(|u| uids.contains(u))
                        .enumerate()
                        .map(|(i, u)| format!("* {} FETCH (UID {u})\r\n", i + 1))
                        .collect()
                } else if up.starts_with("UID SEARCH") {
                    if in_sent {
                        "* SEARCH 11 12\r\n".to_string()
                    } else {
                        "* SEARCH 1 2 3\r\n".to_string()
                    }
                } else if up.starts_with("CAPABILITY") {
                    "* CAPABILITY IMAP4rev1 MOVE\r\n".to_string()
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

    const OUTLOOK: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"\r\n";
    const GMAIL_SENT: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n";
    /// An older server that declares nothing: Sent is known by its name.
    const DOVECOT_OLD: &str = "* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Drafts\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Sent\"\r\n";
    /// A decoy: a folder merely containing the word is not Sent.
    const NO_SENT: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \"/\" \"Unsent ideas\"\r\n";

    fn selected(log: &[String]) -> Vec<String> {
        log.iter()
            .filter(|c| c.to_ascii_uppercase().starts_with("SELECT"))
            .map(|c| c[6..].trim().trim_matches('"').to_string())
            .collect()
    }

    #[test]
    fn sent_is_found_by_what_the_server_says_it_is() {
        rt_act().block_on(async {
            for (list, name) in [
                (OUTLOOK, "Sent Items"),
                (GMAIL_SENT, "[Gmail]/Sent Mail"),
                (DOVECOT_OLD, "INBOX.Sent"),
            ] {
                let (mut s, log) = scripted_folders(list, name).await;
                assert!(matches!(select(&mut s, &Folder::Sent).await, Selected::Open(m) if m.uid_validity == Some(8)), "{name}");
                assert_eq!(selected(&log.lock().unwrap()), vec![name.to_string()]);
            }
        });
    }

    #[test]
    fn a_mailbox_without_sent_opens_nothing() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(NO_SENT, "Sent").await;
            assert!(matches!(
                select(&mut s, &Folder::Sent).await,
                Selected::Missing
            ));
            assert!(
                selected(&log.lock().unwrap()).is_empty(),
                "nothing was opened"
            );
            // And the inbox is still read, with nothing from Sent.
            let Selected::Open(inbox) = select(&mut s, &Folder::Inbox).await else {
                panic!("the inbox opens");
            };
            let got = newest_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, &inbox, 15)
                .await
                .unwrap();
            assert_eq!(got.len(), 3);
            assert!(
                got.iter()
                    .all(|m| m.folder == Folder::Inbox && !m.id.contains("_sent_"))
            );
        });
    }

    #[test]
    fn sent_mail_has_ids_of_its_own_and_says_who_it_went_to() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_folders(OUTLOOK, "Sent Items").await;
            let Selected::Open(sent) = select(&mut s, &Folder::Sent).await else {
                panic!("Sent opens");
            };
            let got = newest_in(&mut s, &mut no_redial(), &me(), &Folder::Sent, &sent, 15)
                .await
                .unwrap();
            let ids: Vec<&str> = got.iter().map(|m| m.id.as_str()).collect();
            let key = mail_key("me@example.com");
            assert_eq!(
                ids,
                vec![format!("{key}_sent_12"), format!("{key}_sent_11")]
            );
            let m = &got[0];
            assert_eq!(m.folder, Folder::Sent);
            assert_eq!(
                (m.to_name.as_str(), m.to_addr.as_str()),
                ("Bo Li", "bo@example.org")
            );
            assert_eq!(m.uidvalidity, 8);
            assert_eq!(m.message_id, "s12@example.com");
            // Only a draft keeps its blind copies.
            assert!(m.bcc.is_empty());
            assert!(m.body.contains("What I wrote in 12"));
        });
    }

    // --------------------------------------------------------- drafts tests

    const GMAIL_DRAFTS: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\HasNoChildren \\Drafts) \"/\" \"[Gmail]/Drafts\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n";
    /// Drafts only by name, beside a decoy that merely contains the word.
    const DRAFTS_BY_NAME: &str = "* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Drafts\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Old drafts\"\r\n* LIST (\\HasNoChildren \\Trash) \".\" \"INBOX.Trash\"\r\n";

    #[test]
    fn drafts_are_found_by_what_the_server_says_or_by_name() {
        rt_act().block_on(async {
            for (list, name) in [
                (GMAIL_DRAFTS, "[Gmail]/Drafts"),
                (DRAFTS_BY_NAME, "INBOX.Drafts"),
            ] {
                let (mut s, log) = scripted_folders(list, name).await;
                assert!(
                    matches!(select(&mut s, &Folder::Drafts).await, Selected::Open(m) if m.uid_validity == Some(8)),
                    "{name}"
                );
                assert_eq!(selected(&log.lock().unwrap()), vec![name.to_string()]);
            }
            // Where there is none, nothing is opened — not "Old drafts".
            let (mut s, log) = scripted_folders(NO_SENT, "Drafts").await;
            assert!(matches!(
                select(&mut s, &Folder::Drafts).await,
                Selected::Missing
            ));
            assert!(selected(&log.lock().unwrap()).is_empty());
        });
    }

    #[test]
    fn a_draft_carries_everyone_it_is_for_and_the_thread() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_folders(GMAIL_DRAFTS, "[Gmail]/Drafts").await;
            let Selected::Open(drafts) = select(&mut s, &Folder::Drafts).await else {
                panic!("Drafts opens");
            };
            let got = newest_in(
                &mut s,
                &mut no_redial(),
                &me(),
                &Folder::Drafts,
                &drafts,
                15,
            )
            .await
            .unwrap();
            let key = mail_key("me@example.com");
            assert_eq!(got[0].id, format!("{key}_drafts_12"));
            assert_eq!(got[0].folder, Folder::Drafts);
            assert_eq!(got[0].to_all, vec!["bo@example.org", "cy@example.org"]);
            assert_eq!(got[0].cc, vec!["dee@example.org"]);
            assert_eq!(got[0].bcc, vec!["boss@example.net"]);
            // The first of a chain, checked like any id a stranger wrote.
            assert_eq!(got[0].in_reply_to, "orig@example.org");
            // Still the first recipient for showing.
            assert_eq!(got[0].to_addr, "bo@example.org");
        });
    }

    #[test]
    fn a_refresh_reads_drafts_and_lists_every_one_by_id() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(GMAIL_DRAFTS, "[Gmail]/Drafts").await;
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[])
                .await
                .unwrap();
            let key = mail_key("me@example.com");
            assert_eq!(
                found.drafts,
                Some(vec![format!("{key}_drafts_11"), format!("{key}_drafts_12")])
            );
            assert_eq!(
                found
                    .messages
                    .iter()
                    .filter(|m| m.folder == Folder::Drafts)
                    .count(),
                2
            );
            assert_eq!(
                selected(&log.lock().unwrap()),
                vec!["INBOX".to_string(), "[Gmail]/Drafts".to_string()]
            );
            // A mailbox without Drafts says nothing about drafts, rather than
            // "there are none".
            let (mut s, _) = scripted_folders(OUTLOOK, "Sent Items").await;
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[])
                .await
                .unwrap();
            assert_eq!(found.drafts, None);
        });
    }

    #[test]
    fn drafts_are_not_among_the_customers_own_folders() {
        rt_act().block_on(async {
            for list in [GMAIL_DRAFTS, DRAFTS_BY_NAME] {
                let (mut s, _) = scripted_folders(list, "none").await;
                let listed = listing(&mut s).await.unwrap();
                let own: Vec<String> = own_folders(&listed, &places_in(&listed))
                    .into_iter()
                    .map(|f| f.name)
                    .collect();
                assert!(
                    own.iter()
                        .all(|n| !n.ends_with("/Drafts") && n != "INBOX.Drafts"),
                    "{own:?}"
                );
            }
        });
    }

    #[test]
    fn a_finished_draft_goes_to_trash_from_drafts() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(GMAIL_DRAFTS, "[Gmail]/Drafts").await;
            assert_eq!(
                apply(&mut s, &Folder::Drafts, &[12], 8, &Action::Trash).await,
                Acted::Done {
                    done: vec![12],
                    gone: vec![]
                }
            );
            let log = log.lock().unwrap();
            assert_eq!(selected(&log), vec!["[Gmail]/Drafts".to_string()]);
            assert!(
                log.iter().any(|c| c
                    .to_ascii_uppercase()
                    .starts_with("UID MOVE 12 \"[GMAIL]/TRASH\"")),
                "{log:?}"
            );
        });
    }

    #[test]
    fn acting_on_sent_mail_happens_in_sent() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(OUTLOOK, "Sent Items").await;
            assert_eq!(
                apply(&mut s, &Folder::Sent, &[11], 8, &Action::Trash).await,
                Acted::Done {
                    done: vec![11],
                    gone: vec![]
                }
            );
            let log = log.lock().unwrap();
            assert_eq!(selected(&log), vec!["Sent Items".to_string()]);
            assert!(
                log.iter().any(|c| c
                    .to_ascii_uppercase()
                    .starts_with("UID MOVE 11 \"DELETED ITEMS\"")),
                "{log:?}"
            );
        });
    }

    #[test]
    fn a_sent_uid_is_never_used_in_the_inbox() {
        rt_act().block_on(async {
            // UID 2 exists in the inbox, under UIDVALIDITY 7. Asked about as
            // Sent (UIDVALIDITY 8), it must not touch the inbox's message 2.
            let (mut s, log) = scripted_folders(OUTLOOK, "Sent Items").await;
            assert!(matches!(
                apply(&mut s, &Folder::Sent, &[2], 7, &Action::Trash).await,
                Acted::Stale(_)
            ));
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    // --------------------------------------------------- archive and spam
    const OUTLOOK_ALL: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent Items\"\r\n* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\"\r\n* LIST (\\HasNoChildren \\Junk) \"/\" \"Junk Email\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"\r\n";
    const GMAIL_ALL: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n* LIST (\\HasNoChildren \\Junk) \"/\" \"[Gmail]/Spam\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n";
    /// Nothing declared; the real folders known by name, and decoys that
    /// only contain the words.
    const NAMES_ONLY: &str = "* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Spam reports\"\r\n* LIST (\\HasNoChildren) \".\" \"Old archive stuff\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Archive\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.spam\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Sent\"\r\n";

    #[test]
    fn archive_and_spam_are_found_by_what_they_are() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_folders(OUTLOOK_ALL, "x").await;
            assert_eq!(
                places(&mut s).await,
                Places {
                    sent: Some("Sent Items".into()),
                    archive: Some("Archive".into()),
                    junk: Some("Junk Email".into()),
                    drafts: None,
                    all_mail: false,
                }
            );
            // Gmail's All Mail is every message, inbox and Sent included:
            // it is the archive only through Gmail's search, and says so.
            let (mut s, _) = scripted_folders(GMAIL_ALL, "x").await;
            assert_eq!(
                places(&mut s).await,
                Places {
                    sent: Some("[Gmail]/Sent Mail".into()),
                    archive: Some("[Gmail]/All Mail".into()),
                    junk: Some("[Gmail]/Spam".into()),
                    drafts: None,
                    all_mail: true,
                }
            );
            let (mut s, _) = scripted_folders(NAMES_ONLY, "x").await;
            assert_eq!(
                places(&mut s).await,
                Places {
                    sent: Some("INBOX.Sent".into()),
                    archive: Some("INBOX.Archive".into()),
                    junk: Some("INBOX.spam".into()),
                    drafts: None,
                    all_mail: false,
                }
            );
        });
    }

    #[test]
    fn spam_has_ids_of_its_own_and_not_spam_moves_it_home() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(OUTLOOK_ALL, "Junk Email").await;
            let Selected::Open(junk) = select(&mut s, &Folder::Junk).await else {
                panic!("Spam opens");
            };
            let got = newest_in(&mut s, &mut no_redial(), &me(), &Folder::Junk, &junk, 15)
                .await
                .unwrap();
            let key = mail_key("me@example.com");
            assert_eq!(got[0].id, format!("{key}_junk_12"));
            assert_eq!(got[0].folder, Folder::Junk);
            assert_eq!(
                apply(&mut s, &Folder::Junk, &[11], 8, &Action::Inbox).await,
                Acted::Done {
                    done: vec![11],
                    gone: vec![]
                }
            );
            let log = log.lock().unwrap();
            assert!(
                log.iter()
                    .any(|c| c.to_ascii_uppercase().starts_with("UID MOVE 11 INBOX")
                        || c.to_ascii_uppercase().starts_with("UID MOVE 11 \"INBOX\"")),
                "{log:?}"
            );
            assert_eq!(
                selected(&log).last().map(String::as_str),
                Some("Junk Email")
            );
        });
    }

    #[test]
    fn moving_inbox_mail_to_the_inbox_does_nothing() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(OUTLOOK_ALL, "x").await;
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[2], 7, &Action::Inbox).await,
                Acted::Done {
                    done: vec![2],
                    gone: vec![]
                }
            );
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    // ------------------------------------------------------- own folders
    const GMAIL_LABELS: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n* LIST (\\Drafts \\HasNoChildren) \"/\" \"[Gmail]/Drafts\"\r\n* LIST (\\HasNoChildren \\Important) \"/\" \"[Gmail]/Important\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n* LIST (\\HasNoChildren \\Junk) \"/\" \"[Gmail]/Spam\"\r\n* LIST (\\Flagged \\HasNoChildren) \"/\" \"[Gmail]/Starred\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n* LIST (\\HasChildren) \"/\" \"Work\"\r\n* LIST (\\HasNoChildren) \"/\" \"Work/Clients\"\r\n* LIST (\\HasNoChildren) \"/\" \"Receipts\"\r\n* LIST (\\HasNoChildren) \"/\" \"Entw&APw-rfe\"\r\n";
    /// A server that declares nothing and keeps every folder under INBOX.
    const DOVECOT_TREE: &str = "* LIST (\\HasChildren) \".\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Drafts\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Sent\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Trash\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Spam\"\r\n* LIST (\\HasChildren) \".\" \"INBOX.Projects\"\r\n* LIST (\\HasNoChildren) \".\" \"INBOX.Projects.2026\"\r\n* LIST (\\HasNoChildren \\NonExistent) \".\" \"INBOX.Gone\"\r\n* LIST (\\HasNoChildren) \".\" \"Notes\"\r\n";

    fn labels(l: Listed) -> Vec<(String, String)> {
        let Listed::Folders(f) = l else {
            panic!("folders were listed: {l:?}")
        };
        f.into_iter().map(|f| (f.name, f.label)).collect()
    }

    #[test]
    fn own_folders_are_the_customers_and_nothing_the_server_keeps() {
        rt_act().block_on(async {
            let pair = |n: &str, l: &str| (n.to_string(), l.to_string());
            let (mut s, _) = scripted_folders(GMAIL_LABELS, "x").await;
            assert_eq!(
                labels(folders_in(&mut s, &me()).await),
                vec![
                    pair("Entw&APw-rfe", "Entwürfe"),
                    pair("Receipts", "Receipts"),
                    pair("Work", "Work"),
                    pair("Work/Clients", "Work / Clients"),
                ]
            );
            let (mut s, _) = scripted_folders(DOVECOT_TREE, "x").await;
            assert_eq!(
                labels(folders_in(&mut s, &me()).await),
                vec![
                    pair("Notes", "Notes"),
                    pair("INBOX.Projects", "Projects"),
                    pair("INBOX.Projects.2026", "Projects / 2026"),
                ]
            );
            // Outlook: its own folders are Sent, Archive, Spam and Trash.
            let (mut s, _) = scripted_folders(OUTLOOK_ALL, "x").await;
            assert_eq!(labels(folders_in(&mut s, &me()).await), vec![]);
        });
    }

    #[test]
    fn a_folder_of_the_customers_is_read_under_ids_of_its_own() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(GMAIL_LABELS, "Work/Clients").await;
            let work = Folder::Named("Work/Clients".into());
            let Fetched::Messages(got) =
                folder_in(&mut s, &mut no_redial(), &me(), &work, 50).await
            else {
                panic!("the folder is read");
            };
            let key = mail_key("me@example.com");
            let tag = named_tag("Work/Clients");
            assert_eq!(
                got.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
                vec![format!("{key}_f{tag}_12"), format!("{key}_f{tag}_11")]
            );
            assert!(got.iter().all(|m| m.folder == work && m.uidvalidity == 8));
            assert_ne!(named_tag("Work"), tag);
            assert_eq!(selected(&log.lock().unwrap()), vec!["Work/Clients"]);
        });
    }

    #[test]
    fn a_folder_that_is_not_the_customers_is_never_opened() {
        rt_act().block_on(async {
            for name in [
                "[Gmail]/Trash",
                "[Gmail]/All Mail",
                "[Gmail]/Sent Mail",
                "[Gmail]",
                "INBOX",
                "Nowhere",
            ] {
                let (mut s, log) = scripted_folders(GMAIL_LABELS, name).await;
                let asked = Folder::Named(name.into());
                assert!(
                    matches!(
                        folder_in(&mut s, &mut no_redial(), &me(), &asked, 50).await,
                        Fetched::Stale(_)
                    ),
                    "{name}"
                );
                assert!(
                    matches!(
                        apply(&mut s, &asked, &[11], 8, &Action::Trash).await,
                        Acted::Stale(_)
                    ),
                    "{name}"
                );
                let log = log.lock().unwrap();
                assert!(selected(&log).is_empty(), "{name}: {log:?}");
            }
        });
    }

    #[test]
    fn moving_to_a_folder_goes_there_and_nowhere_else() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_folders(GMAIL_LABELS, "Receipts").await;
            let to = |n: &str| Action::Move(Folder::Named(n.into()));
            assert_eq!(
                apply(&mut s, &Folder::Inbox, &[2], 7, &to("Receipts")).await,
                Acted::Done {
                    done: vec![2],
                    gone: vec![]
                }
            );
            // A folder the server keeps for itself, or one it does not have,
            // is not somewhere to move mail — and nothing is created.
            for name in ["[Gmail]/Trash", "Nowhere", "INBOX"] {
                assert!(
                    matches!(
                        apply(&mut s, &Folder::Inbox, &[2], 7, &to(name)).await,
                        Acted::NoPlace(_)
                    ),
                    "{name}"
                );
            }
            // Already there: nothing to do.
            assert_eq!(
                apply(
                    &mut s,
                    &Folder::Named("Receipts".into()),
                    &[11],
                    8,
                    &to("Receipts")
                )
                .await,
                Acted::Done {
                    done: vec![11],
                    gone: vec![]
                }
            );
            // And out of it again, home to the inbox.
            assert_eq!(
                apply(
                    &mut s,
                    &Folder::Named("Receipts".into()),
                    &[12],
                    8,
                    &Action::Inbox
                )
                .await,
                Acted::Done {
                    done: vec![12],
                    gone: vec![]
                }
            );
            let log = log.lock().unwrap();
            let moves: Vec<String> = changed(&log).into_iter().cloned().collect();
            assert_eq!(
                moves,
                vec![
                    "UID MOVE 2 \"Receipts\"".to_string(),
                    "UID MOVE 12 \"INBOX\"".to_string()
                ],
                "{log:?}"
            );
            assert!(
                !log.iter()
                    .any(|c| c.to_ascii_uppercase().starts_with("CREATE"))
            );
        });
    }

    #[test]
    fn folder_names_are_decoded_for_showing() {
        assert_eq!(utf7_imap("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(utf7_imap("Caf&AOk-"), "Café");
        assert_eq!(utf7_imap("&ZeVnLIqe-"), "日本語");
        assert_eq!(utf7_imap("Tom &- Jerry"), "Tom & Jerry");
        // What does not decode is shown as it came, never dropped.
        assert_eq!(utf7_imap("Odd &%%-"), "Odd &%%-");
        assert_eq!(utf7_imap("Open &AOk"), "Open &AOk");
        assert_eq!(
            folder_label("INBOX.Caf&AOk-.2026", Some(".")),
            "Café / 2026"
        );
        assert_eq!(folder_label("INBOXES", Some(".")), "INBOXES");
        // A server sending raw UTF-8 names must not bring RATA down.
        assert_eq!(folder_label("Entwürfe", Some("/")), "Entwürfe");
        assert_eq!(folder_label("Añoß.2026", Some(".")), "Añoß / 2026");
    }

    // ------------------------------------------------------- refreshing
    fn bodies_fetched(log: &[String]) -> Vec<String> {
        log.iter()
            .filter(|c| c.to_ascii_uppercase().contains("BODY.PEEK"))
            .cloned()
            .collect()
    }

    fn sorted_uids(m: &[Message]) -> Vec<u32> {
        let mut u: Vec<u32> = m.iter().map(|m| m.uid).collect();
        u.sort_unstable();
        u
    }

    fn inbox_known(uidvalidity: u32, since: u32) -> Vec<Known> {
        vec![Known {
            folder: Folder::Inbox,
            uidvalidity,
            since,
        }]
    }

    static TWENTY: [u32; 20] = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];

    #[test]
    fn a_refresh_with_nothing_new_downloads_no_text() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&TWENTY, 7).await;
            let got = newest_everywhere(&mut s, &mut no_redial(), &me(), 5, &inbox_known(7, 20))
                .await
                .unwrap();
            assert!(got.messages.is_empty());
            let key = mail_key("me@example.com");
            let flags: Vec<(String, bool, bool)> = got
                .flags
                .iter()
                .map(|f| (f.id.clone(), f.unread, f.starred))
                .collect();
            assert_eq!(
                flags,
                (16..=20)
                    .map(|u: u32| (
                        format!("{key}_{u}"),
                        !u.is_multiple_of(2),
                        u.is_multiple_of(5)
                    ))
                    .collect::<Vec<_>>(),
                "read and starred for the newest five, as the server has them"
            );
            assert!(bodies_fetched(&log.lock().unwrap()).is_empty());
        });
    }

    #[test]
    fn a_refresh_downloads_only_what_came_after() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_inbox(&TWENTY, 7).await;
            let got = newest_everywhere(&mut s, &mut no_redial(), &me(), 5, &inbox_known(7, 17))
                .await
                .unwrap();
            assert!(got.gaps.is_empty(), "three new messages leave nothing out");
            assert_eq!(sorted_uids(&got.messages), vec![18, 19, 20]);
            assert!(got.messages.iter().all(|m| m.body.contains("Body of")));
            // Flags only for what RATA already had.
            assert_eq!(got.flags.len(), 2);
            let log = log.lock().unwrap();
            let bodies = bodies_fetched(&log);
            assert_eq!(bodies.len(), 1, "{log:?}");
            assert!(
                bodies[0]
                    .to_ascii_uppercase()
                    .starts_with("UID FETCH 18,19,20 ")
            );
        });
    }

    #[test]
    fn an_unknown_or_rebuilt_folder_is_read_whole() {
        rt_act().block_on(async {
            for known in [vec![], inbox_known(9, 20), inbox_known(7, 0)] {
                let (mut s, _) = scripted_inbox(&TWENTY, 7).await;
                let got = newest_everywhere(&mut s, &mut no_redial(), &me(), 5, &known)
                    .await
                    .unwrap();
                assert_eq!(
                    sorted_uids(&got.messages),
                    vec![16, 17, 18, 19, 20],
                    "{known:?}"
                );
                assert!(got.flags.is_empty());
            }
        });
    }

    #[test]
    fn after_a_long_absence_a_refresh_is_still_bounded() {
        rt_act().block_on(async {
            let many: &'static [u32] =
                Box::leak((1..=300).collect::<Vec<u32>>().into_boxed_slice());
            let (mut s, _) = scripted_inbox(many, 7).await;
            let got = newest_everywhere(&mut s, &mut no_redial(), &me(), 5, &inbox_known(7, 50))
                .await
                .unwrap();
            // The newest of them, and where the rest are: above 50, below
            // the oldest one downloaded now.
            assert_eq!(
                sorted_uids(&got.messages),
                (300 - NEW_MAX + 1..=300).collect::<Vec<_>>()
            );
            assert_eq!(
                got.gaps,
                vec![Gap {
                    folder: Folder::Inbox,
                    uidvalidity: 7,
                    top: 300 - NEW_MAX + 1,
                    floor: 50
                }]
            );
            // Paging down from the top of the gap brings the next of them.
            let Fetched::Messages(next) =
                older_in(&mut s, &mut no_redial(), &me(), &Folder::Inbox, 101, 7, 50).await
            else {
                panic!("older mail")
            };
            assert_eq!(sorted_uids(&next), (51..=100).collect::<Vec<_>>());
        });
    }

    // ---------------------------------------------------- Gmail's archive

    /// Gmail, with one label of the customer's own.
    const GMAIL_BOX: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasChildren \\Noselect) \"/\" \"[Gmail]\"\r\n* LIST (\\All \\HasNoChildren) \"/\" \"[Gmail]/All Mail\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"\r\n* LIST (\\HasNoChildren \\Junk) \"/\" \"[Gmail]/Spam\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"[Gmail]/Trash\"\r\n* LIST (\\HasNoChildren) \"/\" \"Receipts\"\r\n";

    /// A Gmail mailbox: the inbox holds UIDs 1..=3 (UIDVALIDITY 7); All Mail
    /// holds 1..=10 (UIDVALIDITY 9, UIDNEXT 11), of which `archived` are
    /// archived. `searches` is false for a server that is not Gmail and
    /// refuses X-GM-RAW.
    async fn scripted_gmail(
        archived: &'static [u32],
        searches: bool,
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
            w.write_all(b"* OK scripted Gmail ready\r\n").await.unwrap();
            let mut open = String::new();
            let whole = |seq: usize, uid: u32| {
                let body = format!(
                    "From: Ann <ann@example.org>\r\nTo: Me <me@example.com>\r\nSubject: Mail {uid}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nText of {uid}\r\n"
                );
                format!(
                    "* {seq} FETCH (UID {uid} FLAGS (\\Seen) INTERNALDATE \"01-Jan-2026 10:{:02}:00 +0000\" ENVELOPE (\"Thu, 1 Jan 2026 10:00:00 +0000\" \"Mail {uid}\" ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Ann\" NIL \"ann\" \"example.org\")) ((\"Me\" NIL \"me\" \"example.com\")) NIL NIL NIL \"<m{uid}@example.org>\") BODY[]<0> {{{}}}\r\n{body})\r\n",
                    uid,
                    body.len()
                )
            };
            while let Ok(Some(line)) = lines.next_line().await {
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                log.lock().unwrap().push(cmd.to_string());
                let up = cmd.to_ascii_uppercase();
                let held: Vec<u32> = match open.as_str() {
                    "INBOX" => (1..=3).collect(),
                    "[Gmail]/All Mail" => (1..=10).collect(),
                    _ => vec![],
                };
                let body = if up.starts_with("LIST") {
                    GMAIL_BOX.to_string()
                } else if up.starts_with("SELECT") {
                    open = cmd[6..].trim().trim_matches('"').to_string();
                    match open.as_str() {
                        "INBOX" => {
                            "* 3 EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n* OK [UIDNEXT 4] ok\r\n"
                                .to_string()
                        }
                        "[Gmail]/All Mail" => {
                            "* 10 EXISTS\r\n* OK [UIDVALIDITY 9] ok\r\n* OK [UIDNEXT 11] ok\r\n"
                                .to_string()
                        }
                        _ => "* 0 EXISTS\r\n* OK [UIDVALIDITY 8] ok\r\n".to_string(),
                    }
                } else if up.starts_with("UID SEARCH") && !searches {
                    let _ = w
                        .write_all(format!("{tag} NO Search refused\r\n").as_bytes())
                        .await;
                    continue;
                } else if up.starts_with("UID SEARCH") && up.contains("X-GM-RAW") {
                    if !searches {
                        let _ = w
                            .write_all(format!("{tag} BAD Unknown search criterion\r\n").as_bytes())
                            .await;
                        continue;
                    }
                    // UID SEARCH UID lo:hi X-GM-RAW "..."
                    let range = cmd.split_whitespace().nth(3).unwrap_or("1:*");
                    let (lo, hi) = range.split_once(':').unwrap();
                    let lo: u32 = lo.parse().unwrap();
                    let hi: u32 = hi.parse().unwrap_or(u32::MAX);
                    let found: Vec<String> = archived
                        .iter()
                        .filter(|u| **u >= lo && **u <= hi)
                        .map(u32::to_string)
                        .collect();
                    format!("* SEARCH {}\r\n", found.join(" "))
                } else if up.starts_with("UID FETCH") {
                    let asked: Vec<u32> = cmd
                        .split_whitespace()
                        .nth(2)
                        .unwrap_or("")
                        .split(',')
                        .filter_map(|u| u.parse().ok())
                        .filter(|u| held.contains(u))
                        .collect();
                    asked
                        .iter()
                        .enumerate()
                        .map(|(i, u)| {
                            if up.contains("BODY") {
                                whole(i + 1, *u)
                            } else if up.contains("FLAGS") {
                                format!("* {} FETCH (UID {u} FLAGS (\\Flagged))\r\n", i + 1)
                            } else {
                                format!("* {} FETCH (UID {u})\r\n", i + 1)
                            }
                        })
                        .collect()
                } else if up.starts_with("FETCH") {
                    held.iter()
                        .enumerate()
                        .map(|(i, u)| whole(i + 1, *u))
                        .collect()
                } else if up.starts_with("CAPABILITY") {
                    "* CAPABILITY IMAP4rev1 MOVE UIDPLUS X-GM-EXT-1\r\n".to_string()
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

    fn archive_uids(found: &Newest) -> Vec<u32> {
        let mut uids: Vec<u32> = found
            .messages
            .iter()
            .filter(|m| m.folder == Folder::Archive)
            .map(|m| m.uid)
            .collect();
        uids.sort_unstable();
        uids
    }

    #[test]
    fn gmail_archive_is_all_mail_searched_for_what_is_archived() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], true).await;
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[])
                .await
                .unwrap();
            // Only the archived ones, not the inbox's or Sent's copies.
            assert_eq!(archive_uids(&found), vec![2, 4, 6, 9]);
            let key = mail_key("me@example.com");
            let m = found
                .messages
                .iter()
                .find(|m| m.uid == 9 && m.folder == Folder::Archive)
                .unwrap();
            assert_eq!(m.id, format!("{key}_archive_9"));
            assert_eq!(m.uidvalidity, 9);
            // The inbox still arrives as itself.
            assert_eq!(
                found
                    .messages
                    .iter()
                    .filter(|m| m.folder == Folder::Inbox)
                    .count(),
                3
            );
            // And the listing the interface uses to catch mail archived later.
            assert_eq!(
                found.archived,
                Some(Archived {
                    uidvalidity: 9,
                    floor: 1,
                    uids: vec![2, 4, 6, 9]
                })
            );
            let log = log.lock().unwrap();
            assert!(
                log.iter()
                    .any(|c| c == "UID SEARCH UID 1:* X-GM-RAW \"-in:inbox -in:sent -in:drafts\""),
                "{log:?}"
            );
            // Nothing but those was downloaded from All Mail.
            assert!(
                log.iter().any(|c| c.starts_with("UID FETCH 2,4,6,9 ")),
                "{log:?}"
            );
        });
    }

    #[test]
    fn a_gmail_archive_page_is_the_newest_archived() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_gmail(&[2, 4, 6, 9], true).await;
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 2, &[])
                .await
                .unwrap();
            assert_eq!(archive_uids(&found), vec![6, 9]);
            // The listing is still whole, so the rest can be picked up.
            assert_eq!(found.archived.unwrap().uids, vec![2, 4, 6, 9]);
        });
    }

    #[test]
    fn a_known_gmail_archive_downloads_only_what_is_newer_and_lists_the_rest() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_gmail(&[2, 4, 6, 9], true).await;
            let known = [Known {
                folder: Folder::Archive,
                uidvalidity: 9,
                since: 6,
            }];
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &known)
                .await
                .unwrap();
            assert_eq!(archive_uids(&found), vec![9]);
            // Read and starred for what RATA already has.
            let key = mail_key("me@example.com");
            let mut flagged: Vec<&str> = found
                .flags
                .iter()
                .filter(|f| f.id.contains("_archive_"))
                .map(|f| f.id.as_str())
                .collect();
            flagged.sort_unstable();
            assert_eq!(
                flagged,
                vec![
                    format!("{key}_archive_2"),
                    format!("{key}_archive_4"),
                    format!("{key}_archive_6")
                ]
            );
            assert!(found.flags.iter().all(|f| f.starred));
            // A message archived since, which kept its old UID, is in the
            // listing even though nothing newer than 6 names it.
            assert_eq!(found.archived.unwrap().uids, vec![2, 4, 6, 9]);
        });
    }

    #[test]
    fn older_gmail_archive_mail_pages_through_the_search() {
        rt_act().block_on(async {
            let (mut s, _) = scripted_gmail(&[2, 4, 6, 9], true).await;
            let Fetched::Messages(older) =
                older_in(&mut s, &mut no_redial(), &me(), &Folder::Archive, 6, 9, 50).await
            else {
                panic!("older archived mail")
            };
            assert_eq!(sorted_uids(&older), vec![2, 4]);
        });
    }

    #[test]
    fn a_server_that_is_not_gmail_keeps_its_all_folder_out_of_archive() {
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], false).await;
            let found = newest_everywhere(&mut s, &mut no_redial(), &me(), 15, &[])
                .await
                .unwrap();
            assert!(archive_uids(&found).is_empty());
            assert_eq!(found.archived, None);
            // The inbox is not held hostage by the refusal.
            assert_eq!(
                found
                    .messages
                    .iter()
                    .filter(|m| m.folder == Folder::Inbox)
                    .count(),
                3
            );
            // And nothing at all was read out of All Mail.
            let log = log.lock().unwrap();
            let mut in_all = false;
            for c in log.iter() {
                if c.starts_with("SELECT") {
                    in_all = c.contains("All Mail");
                }
                assert!(!(in_all && c.contains("FETCH")), "read from All Mail: {c}");
            }
        });
    }

    #[test]
    fn out_of_gmail_archive_is_a_label_never_a_move_out_of_all_mail() {
        // Back to the inbox: a copy there, which is Gmail's "move to inbox".
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], true).await;
            assert_eq!(
                apply(&mut s, &Folder::Archive, &[4], 9, &Action::Inbox).await,
                copied(&[4])
            );
            assert_eq!(changed(&log.lock().unwrap()), vec!["UID COPY 4 \"INBOX\""]);
        });
        // Into one of the customer's folders: the label, and it stays archived.
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], true).await;
            assert_eq!(
                apply(
                    &mut s,
                    &Folder::Archive,
                    &[4],
                    9,
                    &Action::Move(Folder::Named("Receipts".into()))
                )
                .await,
                copied(&[4])
            );
            assert_eq!(
                changed(&log.lock().unwrap()),
                vec!["UID COPY 4 \"Receipts\""]
            );
        });
        // Delete is still a move to Gmail's Trash, which is how Gmail deletes.
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], true).await;
            assert_eq!(
                apply(&mut s, &Folder::Archive, &[4], 9, &Action::Trash).await,
                done(&[4])
            );
            assert_eq!(
                changed(&log.lock().unwrap()),
                vec!["UID MOVE 4 \"[Gmail]/Trash\""]
            );
        });
        // And archiving what is archived already does nothing at all.
        rt_act().block_on(async {
            let (mut s, log) = scripted_gmail(&[2, 4, 6, 9], true).await;
            assert_eq!(
                apply(&mut s, &Folder::Archive, &[4], 9, &Action::Archive).await,
                done(&[4])
            );
            assert!(changed(&log.lock().unwrap()).is_empty());
        });
    }

    #[test]
    fn a_refused_search_is_never_read_as_nothing_there() {
        // The library's own search takes NO for "no results"; a draft list
        // read that way would have the interface drop every draft it holds.
        rt_act().block_on(async {
            let (mut s, _) = scripted_gmail(&[], false).await;
            assert!(matches!(
                select(&mut s, &Folder::Inbox).await,
                Selected::Open(_)
            ));
            assert_eq!(all_ids(&mut s, &me(), &Folder::Inbox).await, None);
            // The session is still in step for the next command.
            assert!(matches!(
                select(&mut s, &Folder::Inbox).await,
                Selected::Open(_)
            ));
        });
    }

    // ------------------------------------------------------- sign-in tests
    //
    // A scripted server for the sign-in alone: after its greeting it answers
    // each line RATA sends with the next reply, `{tag}` standing for the tag
    // of the first command, and records every line it heard. What is under
    // test is which kind of failure each answer becomes — above all that a
    // refused token is never read as a refused password.

    const TOKEN: &str = "EwBIA8l6BAAUbDba3x2OMJElkF7gJ4z/VbCPEz0AAZmp-secret";

    async fn scripted_sign_in(
        replies: Vec<String>,
    ) -> (Client<TcpStream>, Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut lines = BufReader::new(r).lines();
            w.write_all(b"* OK [CAPABILITY IMAP4rev1 AUTH=XOAUTH2] scripted IMAP ready\r\n")
                .await
                .unwrap();
            let mut tag = String::new();
            let mut replies = replies.into_iter();
            while let Ok(Some(line)) = lines.next_line().await {
                if tag.is_empty() {
                    tag = line.split(' ').next().unwrap_or_default().to_string();
                }
                log.lock().unwrap().push(line);
                let Some(reply) = replies.next() else {
                    continue;
                };
                let reply = format!("{}\r\n", reply.replace("{tag}", &tag));
                if w.write_all(reply.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        let tcp = TcpStream::connect(addr).await.unwrap();
        let mut client = async_imap::Client::new(tcp);
        client.read_response().await.unwrap();
        (client, seen)
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn token() -> Credential {
        Credential::oauth("me@outlook.com", TOKEN)
    }

    /// What goes on the wire for [`token`], base64 and all.
    fn token_on_the_wire() -> String {
        words::base64_encode(&credential::xoauth2("me@outlook.com", TOKEN).unwrap())
    }

    /// Google's error challenge for an expired token, as it sends it.
    fn expired_challenge() -> String {
        format!(
            "+ {}",
            words::base64_encode(
                br#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#
            )
        )
    }

    #[test]
    fn a_token_signs_in_with_xoauth2() {
        // "+ " is what Gmail and Microsoft send; some servers leave out the
        // space, and the parser has to take both.
        for go_ahead in ["+ ", "+"] {
            rt_act().block_on(async {
                let (client, log) =
                    scripted_sign_in(lines(&[go_ahead, "{tag} OK AUTHENTICATE completed."])).await;
                let signed = sign_in(client, "me@outlook.com", &token()).await;
                assert!(signed.is_ok(), "{go_ahead:?}: {:?}", signed.err());
                let log = log.lock().unwrap();
                assert_eq!(log.len(), 2, "{log:?}");
                assert!(log[0].ends_with(" AUTHENTICATE XOAUTH2"), "{log:?}");
                // Exactly the string, once, and never a LOGIN.
                assert_eq!(log[1], token_on_the_wire());
                assert!(!log.iter().any(|l| l.to_ascii_uppercase().contains("LOGIN")));
            });
        }
    }

    #[test]
    fn an_expired_token_is_an_oauth_failure_not_a_wrong_password() {
        rt_act().block_on(async {
            let (client, log) = scripted_sign_in(vec![
                "+ ".into(),
                expired_challenge(),
                "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)".into(),
            ])
            .await;
            match sign_in(client, "me@outlook.com", &token()).await {
                Err(Trouble::OAuth(why)) => {
                    assert!(why.contains("status 401"), "{why}");
                    assert!(why.contains("Invalid credentials"), "{why}");
                }
                Err(other) => panic!("wrong kind of failure: {other:?}"),
                Ok(_) => panic!("an expired token signed in"),
            }
            // The error challenge was answered with an empty line, which is
            // what makes the server send its NO rather than wait.
            let log = log.lock().unwrap();
            assert_eq!(log.len(), 3, "{log:?}");
            assert_eq!(log[2], "", "{log:?}");
        });
    }

    #[test]
    fn a_refused_token_is_an_oauth_failure_not_a_wrong_password() {
        rt_act().block_on(async {
            // Microsoft's answer: a plain NO, no error challenge.
            let (client, _) =
                scripted_sign_in(lines(&["+ ", "{tag} NO AUTHENTICATE failed."])).await;
            match sign_in(client, "me@outlook.com", &token()).await {
                Err(Trouble::OAuth(why)) => assert!(why.contains("AUTHENTICATE failed"), "{why}"),
                Err(other) => panic!("wrong kind of failure: {other:?}"),
                Ok(_) => panic!("a refused token signed in"),
            }
        });
    }

    #[test]
    fn a_busy_server_is_not_a_refused_token() {
        rt_act().block_on(async {
            let (client, _) = scripted_sign_in(lines(&[
                "+ ",
                "{tag} NO [UNAVAILABLE] Temporary server problem, try again later",
            ]))
            .await;
            assert!(matches!(
                sign_in(client, "me@outlook.com", &token()).await,
                Err(Trouble::Net(_))
            ));
        });
    }

    #[test]
    fn a_server_without_xoauth2_is_not_a_refused_token() {
        rt_act().block_on(async {
            let (client, log) =
                scripted_sign_in(lines(&["{tag} BAD Unsupported authentication mechanism"])).await;
            assert!(matches!(
                sign_in(client, "me@outlook.com", &token()).await,
                Err(Trouble::Net(_))
            ));
            // And the token itself was never sent.
            assert_eq!(log.lock().unwrap().len(), 1);
        });
    }

    #[test]
    fn a_token_that_could_forge_a_field_is_never_sent() {
        rt_act().block_on(async {
            let (client, log) = scripted_sign_in(lines(&["+ ", "{tag} OK"])).await;
            let forged = Credential::oauth("me@outlook.com", "tok\u{1}\u{1}");
            assert!(matches!(
                sign_in(client, "me@outlook.com", &forged).await,
                Err(Trouble::OAuth(_))
            ));
            assert!(log.lock().unwrap().is_empty());
        });
    }

    #[test]
    fn a_token_is_never_in_what_a_failure_says() {
        rt_act().block_on(async {
            // A server that echoes what it was sent, both ways.
            let (client, _) = scripted_sign_in(vec![
                "+ ".into(),
                format!(
                    "{{tag}} NO token {TOKEN} in {} refused",
                    token_on_the_wire()
                ),
            ])
            .await;
            let Err(Trouble::OAuth(why)) = sign_in(client, "me@outlook.com", &token()).await else {
                panic!("not an OAuth failure");
            };
            let acct = Account {
                email: "me@outlook.com".into(),
                credential: token(),
                host: "outlook.office365.com".into(),
                port: 993,
                label: "Outlook".into(),
            };
            let shown = Fetched::OAuth(oauth_msg(&acct, &why));
            for said in [
                format!("{shown:?}"),
                format!("{acct:?}"),
                format!("{:?}", Trouble::OAuth(why.clone())),
            ] {
                assert!(!said.contains(TOKEN), "{said}");
                assert!(!said.contains("-secret"), "{said}");
                assert!(!said.contains(&token_on_the_wire()), "{said}");
            }
            assert!(why.contains(credential::HIDDEN), "{why}");
        });
    }

    #[test]
    fn a_password_still_signs_in_with_login_and_a_no_is_still_auth() {
        rt_act().block_on(async {
            let (client, log) = scripted_sign_in(lines(&["{tag} OK LOGIN completed"])).await;
            let pw = Credential::Password("pw".into());
            assert!(sign_in(client, "me@example.com", &pw).await.is_ok());
            let first = log.lock().unwrap()[0].clone();
            assert!(first.contains(" LOGIN "), "{first}");

            let (client, _) = scripted_sign_in(lines(&[
                "{tag} NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)",
            ]))
            .await;
            assert!(matches!(
                sign_in(client, "me@example.com", &pw).await,
                Err(Trouble::Auth(_))
            ));
        });
    }

    // --------------------------------------- what a sign-in failure may say
    //
    // SEC-5: the IMAP half of what SEC-4 did for SMTP. A server's NO or BAD
    // to a sign-in reaches the customer, who may paste it into a bug report,
    // so it is one short plain line and never carries what RATA sent.

    /// A password with a quote and a backslash in it, so LOGIN has to escape
    /// it and the server's words, as async-imap reports them, escape it again.
    const PASS: &str = "not-a-real\"pass\\word";

    fn password() -> Credential {
        Credential::Password(PASS.into())
    }

    /// What a failed sign-in says, whatever kind of failure it is.
    fn why_of(signed: Result<Session<TcpStream>, Trouble>) -> String {
        match signed {
            Err(
                Trouble::Auth(why) | Trouble::OAuth(why) | Trouble::Net(why) | Trouble::Host(why),
            ) => why,
            Ok(_) => panic!("signed in"),
        }
    }

    /// Every form the password could still be in: as typed, as a Debug of the
    /// server's words writes it, and in base64 — whole, or eight characters
    /// of it in a row, in any case.
    fn assert_no_password(why: &str) {
        let lower = why.to_ascii_lowercase();
        let escaped = format!("{PASS:?}");
        let escaped = &escaped[1..escaped.len() - 1];
        for form in [PASS, escaped, &words::base64_encode(PASS.as_bytes())] {
            let form = form.to_ascii_lowercase();
            for at in 0..form.len().saturating_sub(8) {
                assert!(!lower.contains(&form[at..at + 8]), "{why}");
            }
        }
    }

    #[test]
    fn a_password_the_server_repeats_is_never_in_what_signing_in_says() {
        rt_act().block_on(async {
            let echoes = [
                // Refused, with the command and the password read back.
                format!("{{tag}} NO LOGIN \"me@example.com\" \"{PASS}\" refused for {PASS}"),
                // The same, shouted.
                format!("{{tag}} NO password {} is wrong", PASS.to_ascii_uppercase()),
                // Not a verdict on the password, but still repeated.
                format!("{{tag}} NO [UNAVAILABLE] try again later, {PASS}"),
                format!("{{tag}} BAD cannot parse {PASS}"),
                // In base64, which LOGIN never sends, but a server might.
                format!(
                    "{{tag}} NO {} not accepted",
                    words::base64_encode(PASS.as_bytes())
                ),
            ];
            for echo in echoes {
                let (client, _) = scripted_sign_in(vec![echo.clone()]).await;
                let why = why_of(sign_in(client, "me@example.com", &password()).await);
                assert_no_password(&why);
            }
        });
    }

    #[test]
    fn a_password_the_server_repeats_cut_short_is_still_hidden() {
        rt_act().block_on(async {
            // The LOGIN line cut five characters into the password, as a
            // server with a short buffer might.
            let (client, _) = scripted_sign_in(lines(&[
                "{tag} NO command too long: LOGIN \"me@example.com\" \"not-a",
            ]))
            .await;
            let why = why_of(sign_in(client, "me@example.com", &password()).await);
            assert!(!why.contains("not-a"), "{why}");

            // The password alone, cut short.
            let (client, _) =
                scripted_sign_in(lines(&["{tag} NO password not-a-real\"pa is wrong"])).await;
            let why = why_of(sign_in(client, "me@example.com", &password()).await);
            assert!(!why.contains("not-a-real"), "{why}");
            assert!(why.contains("is wrong"), "{why}");
        });
    }

    #[test]
    fn a_token_the_server_repeats_cut_short_or_in_another_case_is_still_hidden() {
        rt_act().block_on(async {
            let wire = token_on_the_wire();
            let (client, _) = scripted_sign_in(vec![
                "+ ".into(),
                format!("{{tag}} NO line too long: {}", &wire[..wire.len() - 7]),
            ])
            .await;
            let why = why_of(sign_in(client, "me@outlook.com", &token()).await);
            for at in 0..wire.len().saturating_sub(16) {
                assert!(!why.contains(&wire[at..at + 16]), "{why}");
            }

            let (client, _) = scripted_sign_in(vec![
                "+ ".into(),
                format!("{{tag}} NO token {} refused", TOKEN.to_ascii_lowercase()),
            ])
            .await;
            let why = why_of(sign_in(client, "me@outlook.com", &token()).await);
            assert!(!why.to_ascii_lowercase().contains("-secret"), "{why}");
            assert!(!why.to_ascii_lowercase().contains("ewbia8l6"), "{why}");
        });
    }

    #[test]
    fn an_over_long_sign_in_refusal_is_one_short_line() {
        rt_act().block_on(async {
            let long = format!(
                "[AUTHENTICATIONFAILED] Invalid credentials{}",
                " very".repeat(150)
            );
            // A password refused, a password not understood, a token refused.
            for (reply, cred) in [
                (format!("{{tag}} NO {long}"), password()),
                (format!("{{tag}} BAD {long}"), password()),
                (format!("{{tag}} NO {long}"), token()),
            ] {
                let mut replies = vec![reply];
                if cred.is_oauth() {
                    replies.insert(0, "+ ".into());
                }
                let (client, _) = scripted_sign_in(replies).await;
                let why = why_of(sign_in(client, "me@example.com", &cred).await);
                // RATA's own words may come first; the server's are at most 200.
                let said = why.split_once("sign-in: ").map(|(_, s)| s).unwrap_or(&why);
                assert!(
                    said.chars().count() <= 200,
                    "{} chars: {said}",
                    said.chars().count()
                );
                // A response code is the server's verdict, not a secret.
                assert!(
                    said.contains("[AUTHENTICATIONFAILED] Invalid credentials"),
                    "{said}"
                );
            }
        });
    }

    #[test]
    fn a_sign_in_refusal_with_bidi_controls_shows_none() {
        let bidi = |c: char| matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
        // What RATA makes of a NO or BAD, whatever brought it.
        for e in [
            ImapError::No("bad \u{202e}drowssap\u{2066} \u{200f}here".into()),
            ImapError::Bad("\u{202e}exe.fdp".into()),
        ] {
            let said = reason(&e);
            assert!(!said.chars().any(bidi), "{said:?}");
            assert!(
                said.contains("drowssap") || said.contains("exe.fdp"),
                "{said:?}"
            );
        }
        // Over the wire, imap-proto reads only ASCII in a NO's text, so the
        // controls never arrive as words: the reply is unreadable, and RATA
        // says so in its own.
        rt_act().block_on(async {
            for cred in [password(), token()] {
                let mut replies = lines(&["{tag} NO bad \u{202e}drowssap\u{2066} \u{200f}here"]);
                if cred.is_oauth() {
                    replies.insert(0, "+ ".into());
                }
                let (client, _) = scripted_sign_in(replies).await;
                let why = why_of(sign_in(client, "me@example.com", &cred).await);
                assert!(!why.chars().any(bidi), "{why:?}");
                assert_eq!(why, UNPARSED);
            }
        });
    }

    // ----------------------------------------------------------- draft tests
    //
    // A scripted Drafts folder that takes APPEND literals, keeps what it was
    // given, and answers SEARCH, FETCH, STORE and EXPUNGE from that. What is
    // under test is which copy RATA ever removes for good: only its own.

    const DRAFT_ID: &str = "0f8b6a52-3c1e-4d7a-9b2e-5a4c3d2e1f00";
    const OTHER_ID: &str = "9a7c2e10-0b1d-4e6f-8a3b-7c6d5e4f3a21";
    const DOVECOT_DRAFTS: &str = "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Drafts) \"/\" \"Drafts\"\r\n* LIST (\\HasNoChildren \\Trash) \"/\" \"Trash\"\r\n";

    #[derive(Clone)]
    struct DraftScript {
        caps: &'static str,
        list: &'static str,
        uidvalidity: u32,
        /// Answer APPEND with `[APPENDUID …]` (UIDPLUS).
        appenduid: bool,
        /// Refuse APPEND, as a mailbox over quota does.
        refuse: bool,
        /// Already in Drafts: UID and whole message.
        held: Vec<(u32, String)>,
        next_uid: u32,
        /// Match `UID SEARCH … HEADER X-RATA-Draft "<id>"` anywhere in the
        /// header's value, as IMAP's HEADER search does (RFC 3501: a
        /// substring), rather than only the exact line.
        substring: bool,
    }

    impl DraftScript {
        fn dovecot(held: Vec<(u32, String)>) -> Self {
            DraftScript {
                caps: "UIDPLUS MOVE",
                list: DOVECOT_DRAFTS,
                uidvalidity: 7,
                appenduid: true,
                refuse: false,
                held,
                next_uid: 20,
                substring: false,
            }
        }

        /// Gmail's Drafts: its folder names and its own extension.
        fn gmail(held: Vec<(u32, String)>) -> Self {
            DraftScript {
                caps: "UIDPLUS MOVE X-GM-EXT-1",
                list: GMAIL_DRAFTS,
                ..DraftScript::dovecot(held)
            }
        }
    }

    /// What the scripted Drafts folder holds, and every command it heard.
    #[derive(Default)]
    struct DraftBox {
        log: Vec<String>,
        /// UID, message, flagged \Deleted.
        held: Vec<(u32, String, bool)>,
        expunged: Vec<u32>,
        /// UID and the folder it was moved to.
        moved: Vec<(u32, String)>,
    }

    /// A draft as some client saved it, with `ids` as its X-RATA-Draft headers.
    fn stored_draft(ids: &[&str]) -> String {
        let mut raw =
            String::from("From: <me@example.com>\r\nTo: <bo@example.org>\r\nSubject: Plan\r\n");
        for id in ids {
            raw.push_str(&format!("X-RATA-Draft: {id}\r\n"));
        }
        raw.push_str(
            "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHalf a thought\r\n",
        );
        raw
    }

    fn outgoing_draft() -> Outgoing {
        let a = |s: &str| crate::compose::Address::parse(s).unwrap();
        Outgoing {
            from: a("me@example.com"),
            from_name: None,
            to: vec![a("bo@example.org")],
            cc: vec![a("cy@example.org")],
            bcc: vec![a("boss@example.net")],
            subject: "Plan".into(),
            body: "Half a thought, the second time".into(),
            in_reply_to: None,
            attachments: vec![],
        }
    }

    fn rendered(rev: u32) -> String {
        compose::render_draft(
            &outgoing_draft(),
            "Tue, 29 Sep 2026 10:00:00 +0000",
            "u1",
            DRAFT_ID,
            rev,
        )
    }

    fn ours(uid: u32) -> DraftRef {
        DraftRef {
            uid,
            uidvalidity: 7,
            draft_id: DRAFT_ID.into(),
        }
    }

    fn uid_arg(cmd: &str) -> u32 {
        cmd.split_whitespace().nth(2).unwrap().parse().unwrap()
    }

    async fn scripted_drafts(
        script: DraftScript,
    ) -> (Session<TcpStream>, Arc<std::sync::Mutex<DraftBox>>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let state = Arc::new(std::sync::Mutex::new(DraftBox {
            held: script
                .held
                .iter()
                .map(|(u, raw)| (*u, raw.clone(), false))
                .collect(),
            ..DraftBox::default()
        }));
        let shared = state.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut r = BufReader::new(r);
            let mut next = script.next_uid;
            w.write_all(b"* OK scripted IMAP ready\r\n").await.unwrap();
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end_matches(['\r', '\n']).to_string();
                let (tag, cmd) = line.split_once(' ').unwrap_or((&line, ""));
                let (tag, cmd) = (tag.to_string(), cmd.to_string());
                shared.lock().unwrap().log.push(cmd.clone());
                let up = cmd.to_ascii_uppercase();
                let mut done = format!("{tag} OK done\r\n");
                let body = if up.starts_with("APPEND") && script.refuse {
                    done = format!(
                        "{tag} NO [OVERQUOTA] Quota exceeded (mailbox for user is full)\r\n"
                    );
                    String::new()
                } else if up.starts_with("APPEND") {
                    let n: usize = cmd
                        .rsplit_once('{')
                        .and_then(|(_, n)| n.trim_end_matches('}').parse().ok())
                        .unwrap();
                    w.write_all(b"+ go ahead\r\n").await.unwrap();
                    let mut literal = vec![0u8; n];
                    r.read_exact(&mut literal).await.unwrap();
                    let mut end = String::new();
                    r.read_line(&mut end).await.unwrap();
                    assert_eq!(end, "\r\n", "the command ends right after the literal");
                    let uid = next;
                    next += 1;
                    shared.lock().unwrap().held.push((
                        uid,
                        String::from_utf8(literal).unwrap(),
                        false,
                    ));
                    if script.appenduid {
                        done = format!(
                            "{tag} OK [APPENDUID {} {uid}] Append completed.\r\n",
                            script.uidvalidity
                        );
                    }
                    String::new()
                } else if up.starts_with("SELECT") {
                    let n = shared.lock().unwrap().held.len();
                    format!(
                        "* {n} EXISTS\r\n* OK [UIDVALIDITY {}] ok\r\n",
                        script.uidvalidity
                    )
                } else if up.starts_with("UID SEARCH") {
                    let want = cmd.split('"').nth(1).unwrap_or("").to_string();
                    let found: Vec<String> = shared
                        .lock()
                        .unwrap()
                        .held
                        .iter()
                        .filter(|(_, raw, deleted)| {
                            let head = raw.split("\r\n\r\n").next().unwrap_or_default();
                            !deleted
                                && if script.substring {
                                    head.split("\r\n").any(|l| {
                                        l.to_ascii_lowercase().starts_with("x-rata-draft:")
                                            && l.contains(&want)
                                    })
                                } else {
                                    raw.contains(&format!("X-RATA-Draft: {want}\r\n"))
                                }
                        })
                        .map(|(u, _, _)| u.to_string())
                        .collect();
                    format!("* SEARCH {}\r\n", found.join(" "))
                } else if up.starts_with("UID FETCH") {
                    let held = shared.lock().unwrap().held.clone();
                    let mut out = String::new();
                    for asked in cmd.split_whitespace().nth(2).unwrap().split(',') {
                        let asked: u32 = asked.parse().unwrap();
                        let Some(i) = held.iter().position(|(u, _, _)| *u == asked) else {
                            continue;
                        };
                        let raw = &held[i].1;
                        if up.contains("HEADER.FIELDS") {
                            let mut fields: String = raw
                                .split("\r\n\r\n")
                                .next()
                                .unwrap()
                                .split("\r\n")
                                .filter(|l| l.to_ascii_lowercase().starts_with("x-rata-draft:"))
                                .map(|l| format!("{l}\r\n"))
                                .collect();
                            fields.push_str("\r\n");
                            out.push_str(&format!(
                                "* {} FETCH (UID {asked} BODY[HEADER.FIELDS (X-RATA-DRAFT)] {{{}}}\r\n{fields})\r\n",
                                i + 1,
                                fields.len()
                            ));
                        } else {
                            out.push_str(&format!(
                                "* {} FETCH (UID {asked} FLAGS (\\Draft \\Seen) INTERNALDATE \"29-Sep-2026 10:{:02}:00 +0000\" ENVELOPE (NIL \"Plan\" ((NIL NIL \"me\" \"example.com\")) NIL NIL ((NIL NIL \"bo\" \"example.org\")) ((NIL NIL \"cy\" \"example.org\")) ((NIL NIL \"boss\" \"example.net\")) NIL NIL) BODY[]<0> {{{}}}\r\n{raw})\r\n",
                                i + 1,
                                asked % 60,
                                raw.len()
                            ));
                        }
                    }
                    out
                } else if up.starts_with("UID STORE") {
                    let uid = uid_arg(&cmd);
                    for m in shared.lock().unwrap().held.iter_mut() {
                        if m.0 == uid && up.contains("\\DELETED") {
                            m.2 = true;
                        }
                    }
                    String::new()
                } else if up.starts_with("UID MOVE") {
                    let uid = uid_arg(&cmd);
                    let to = cmd.split('"').nth(1).unwrap_or_default().to_string();
                    let mut b = shared.lock().unwrap();
                    b.held.retain(|m| m.0 != uid);
                    b.moved.push((uid, to));
                    String::new()
                } else if up.starts_with("UID EXPUNGE") && !script.caps.contains("UIDPLUS") {
                    done = format!("{tag} BAD unknown command\r\n");
                    String::new()
                } else if up.starts_with("UID EXPUNGE") {
                    let uid = uid_arg(&cmd);
                    let mut b = shared.lock().unwrap();
                    let before = b.held.len();
                    b.held.retain(|m| !(m.0 == uid && m.2));
                    if b.held.len() < before {
                        b.expunged.push(uid);
                    }
                    String::new()
                } else if up.starts_with("LIST") {
                    script.list.to_string()
                } else if up.starts_with("CAPABILITY") {
                    format!("* CAPABILITY IMAP4rev1 {}\r\n", script.caps)
                } else if up.starts_with("LOGOUT") {
                    "* BYE\r\n".to_string()
                } else {
                    String::new()
                };
                if w.write_all(format!("{body}{done}").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
        (scripted_login(addr).await, state)
    }

    fn saved(d: DraftSaved) -> (DraftRef, String, Prior) {
        match d {
            DraftSaved::Saved { saved, id, prior } => (saved, id, prior),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_draft_is_appended_to_drafts_and_its_uid_read_from_appenduid() {
        rt_act().block_on(async {
            let (mut s, state) = scripted_drafts(DraftScript::dovecot(vec![])).await;
            let (at, id, prior) = saved(save_in(&mut s, &me(), &rendered(1), DRAFT_ID, None).await);
            assert_eq!(at, ours(20));
            assert_eq!(id, format!("{}_drafts_20", mail_key("me@example.com")));
            assert_eq!(prior, Prior::None);
            let b = state.lock().unwrap();
            let appends: Vec<_> = b.log.iter().filter(|c| c.starts_with("APPEND")).collect();
            assert_eq!(appends.len(), 1, "{:?}", b.log);
            assert!(
                appends[0].starts_with("APPEND \"Drafts\" (\\Seen \\Draft) {"),
                "{appends:?}"
            );
            // The server said where it went, so nothing is searched for.
            assert!(
                !b.log.iter().any(|c| c.starts_with("UID SEARCH")),
                "{:?}",
                b.log
            );
            // Stored byte for byte as rendered: Bcc and RATA's id in it.
            let (_, raw, _) = &b.held[0];
            assert_eq!(*raw, rendered(1));
            assert!(raw.contains("\r\nBcc: <boss@example.net>\r\n"));
            assert!(raw.contains(&format!("\r\nX-RATA-Draft: {DRAFT_ID}\r\n")));
        });
    }

    #[test]
    fn without_appenduid_the_new_copy_is_found_by_its_header_never_the_old_one() {
        rt_act().block_on(async {
            let mut script = DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))]);
            script.appenduid = false;
            let (mut s, state) = scripted_drafts(script).await;
            let (at, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!(at, ours(20));
            assert_eq!(prior, Prior::Replaced);
            let b = state.lock().unwrap();
            let asked = format!("UID SEARCH UNDELETED HEADER X-RATA-Draft \"{DRAFT_ID}\"");
            assert!(b.log.contains(&asked), "{:?}", b.log);
            assert_eq!(b.expunged, vec![3]);
        });
    }

    #[test]
    fn the_copy_before_is_removed_for_good_only_when_it_carries_this_drafts_id() {
        rt_act().block_on(async {
            let (mut s, state) =
                scripted_drafts(DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))])).await;
            let (at, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!((at.uid, prior), (20, Prior::Replaced));
            let b = state.lock().unwrap();
            assert!(
                b.log
                    .iter()
                    .any(|c| c == "UID FETCH 3 (UID BODY.PEEK[HEADER.FIELDS (X-RATA-Draft)])"),
                "{:?}",
                b.log
            );
            assert!(
                b.log
                    .iter()
                    .any(|c| c == "UID STORE 3 +FLAGS.SILENT (\\Deleted)"),
                "{:?}",
                b.log
            );
            assert!(b.log.iter().any(|c| c == "UID EXPUNGE 3"), "{:?}", b.log);
            // Never a bare EXPUNGE, which takes every \Deleted message with it.
            assert!(
                !b.log.iter().any(|c| c.eq_ignore_ascii_case("EXPUNGE")),
                "{:?}",
                b.log
            );
            assert_eq!(b.expunged, vec![3]);
            // Exactly one copy is left: the one just saved.
            assert_eq!(b.held.iter().map(|m| m.0).collect::<Vec<_>>(), vec![20]);
        });
    }

    #[test]
    fn a_copy_that_is_not_provably_rata_s_is_never_touched() {
        // Another draft's id, no id at all (saved by another app), and two ids
        // of which one is this draft's (not something RATA writes).
        for held in [
            stored_draft(&[OTHER_ID]),
            stored_draft(&[]),
            stored_draft(&[OTHER_ID, DRAFT_ID]),
        ] {
            rt_act().block_on(async {
                let (mut s, state) =
                    scripted_drafts(DraftScript::dovecot(vec![(3, held.clone())])).await;
                let (at, _, prior) =
                    saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
                assert_eq!((at.uid, prior), (20, Prior::NotOurs));
                let b = state.lock().unwrap();
                assert!(changed(&b.log).is_empty(), "{:?}", b.log);
                assert_eq!(b.held.len(), 2);
                assert!(b.held.iter().all(|m| !m.2));
            });
        }
    }

    #[test]
    fn a_rebuilt_drafts_folder_or_a_mismatched_reference_leaves_the_copy_alone() {
        let cases = [
            // Drafts was rebuilt since: UID 3 may be anything now.
            DraftRef {
                uid: 3,
                uidvalidity: 6,
                draft_id: DRAFT_ID.into(),
            },
            // A reference to another draft's copy.
            DraftRef {
                uid: 3,
                uidvalidity: 7,
                draft_id: OTHER_ID.into(),
            },
            // No reference at all.
            DraftRef {
                uid: 0,
                uidvalidity: 7,
                draft_id: DRAFT_ID.into(),
            },
        ];
        for prior in cases {
            rt_act().block_on(async {
                let (mut s, state) =
                    scripted_drafts(DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))]))
                        .await;
                let (_, _, got) =
                    saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&prior)).await);
                assert_eq!(got, Prior::NotOurs, "{prior:?}");
                let b = state.lock().unwrap();
                // Not even looked at: nothing is fetched, flagged or expunged.
                assert!(
                    !b.log.iter().any(|c| c.starts_with("UID FETCH")),
                    "{:?}",
                    b.log
                );
                assert!(changed(&b.log).is_empty(), "{:?}", b.log);
            });
        }
    }

    #[test]
    fn a_copy_already_gone_is_said_to_be_gone() {
        rt_act().block_on(async {
            let (mut s, state) = scripted_drafts(DraftScript::dovecot(vec![])).await;
            let (_, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!(prior, Prior::Gone);
            assert!(changed(&state.lock().unwrap().log).is_empty());
        });
    }

    #[test]
    fn without_uidplus_the_copy_before_is_only_flagged() {
        rt_act().block_on(async {
            let mut script = DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))]);
            script.caps = "MOVE";
            script.appenduid = false;
            let (mut s, state) = scripted_drafts(script).await;
            let (at, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!((at.uid, prior), (20, Prior::LeftFlagged));
            let b = state.lock().unwrap();
            assert!(
                b.log
                    .iter()
                    .any(|c| c == "UID STORE 3 +FLAGS.SILENT (\\Deleted)"),
                "{:?}",
                b.log
            );
            assert!(
                !b.log
                    .iter()
                    .any(|c| c.to_ascii_uppercase().contains("EXPUNGE")),
                "{:?}",
                b.log
            );
            assert!(b.held.iter().any(|m| m.0 == 3 && m.2));
        });
    }

    #[test]
    fn with_no_drafts_folder_nothing_is_saved_or_created() {
        rt_act().block_on(async {
            let mut script = DraftScript::dovecot(vec![]);
            script.list = TRASH;
            let (mut s, state) = scripted_drafts(script).await;
            let got = save_in(&mut s, &me(), &rendered(1), DRAFT_ID, Some(&ours(3))).await;
            assert!(
                matches!(got, DraftSaved::NoPlace(ref why) if why.contains("no Drafts folder")),
                "{got:?}"
            );
            let b = state.lock().unwrap();
            assert!(
                !b.log.iter().any(|c| {
                    let u = c.to_ascii_uppercase();
                    u.starts_with("APPEND") || u.starts_with("CREATE") || u.starts_with("UID")
                }),
                "{:?}",
                b.log
            );
        });
    }

    #[test]
    fn a_refused_save_says_why_and_removes_nothing() {
        rt_act().block_on(async {
            let mut script = DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))]);
            script.refuse = true;
            let (mut s, state) = scripted_drafts(script).await;
            let got = save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await;
            assert!(
                matches!(got, DraftSaved::Refused(ref why) if why.contains("Quota exceeded")),
                "{got:?}"
            );
            let b = state.lock().unwrap();
            assert!(changed(&b.log).is_empty(), "{:?}", b.log);
            assert_eq!(b.held.len(), 1);
        });
    }

    #[test]
    fn gmail_keeps_drafts_in_its_own_drafts_folder() {
        rt_act().block_on(async {
            let mut script = DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID]))]);
            script.list = GMAIL_DRAFTS;
            let (mut s, state) = scripted_drafts(script).await;
            let (_, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!(prior, Prior::Replaced);
            let b = state.lock().unwrap();
            assert!(
                b.log
                    .iter()
                    .any(|c| c.starts_with("APPEND \"[Gmail]/Drafts\" (\\Seen \\Draft) {")),
                "{:?}",
                b.log
            );
            assert!(
                b.log.iter().any(|c| c == "SELECT \"[Gmail]/Drafts\""),
                "{:?}",
                b.log
            );
        });
    }

    #[test]
    fn on_gmail_the_copy_before_goes_to_the_trash_not_into_all_mail() {
        // P3-2: expunging from [Gmail]/Drafts only takes the Drafts label
        // off, and the copy stays in All Mail. Gmail's Trash takes it out.
        rt_act().block_on(async {
            let (mut s, state) =
                scripted_drafts(DraftScript::gmail(vec![(3, stored_draft(&[DRAFT_ID]))])).await;
            let (at, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!((at.uid, prior), (20, Prior::Replaced));
            let b = state.lock().unwrap();
            // Proven first, exactly as anywhere else.
            assert!(
                b.log
                    .iter()
                    .any(|c| c == "UID FETCH 3 (UID BODY.PEEK[HEADER.FIELDS (X-RATA-Draft)])"),
                "{:?}",
                b.log
            );
            assert_eq!(b.moved, vec![(3, "[Gmail]/Trash".to_string())]);
            assert!(
                b.log.iter().any(|c| c == "UID MOVE 3 \"[Gmail]/Trash\""),
                "{:?}",
                b.log
            );
            // Neither flagged nor expunged.
            assert!(
                !b.log.iter().any(|c| {
                    let u = c.to_ascii_uppercase();
                    u.starts_with("UID STORE") || u.contains("EXPUNGE")
                }),
                "{:?}",
                b.log
            );
            assert!(b.expunged.is_empty());
        });

        // And a copy that is not provably RATA's is not moved either.
        rt_act().block_on(async {
            let (mut s, state) =
                scripted_drafts(DraftScript::gmail(vec![(3, stored_draft(&[OTHER_ID]))])).await;
            let (_, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!(prior, Prior::NotOurs);
            let b = state.lock().unwrap();
            assert!(changed(&b.log).is_empty(), "{:?}", b.log);
            assert!(b.moved.is_empty());
        });
    }

    #[test]
    fn without_appenduid_a_message_whose_header_only_contains_the_id_is_not_taken() {
        // P3-3: IMAP's HEADER search matches a substring, so the newest
        // match can be another message whose X-RATA-Draft merely contains
        // this draft's id. RATA must not take that one as its copy.
        rt_act().block_on(async {
            let near_miss = stored_draft(&[&format!("x{DRAFT_ID}")]);
            let mut script =
                DraftScript::dovecot(vec![(3, stored_draft(&[DRAFT_ID])), (30, near_miss)]);
            script.appenduid = false;
            script.substring = true;
            let (mut s, state) = scripted_drafts(script).await;
            let (at, _, prior) =
                saved(save_in(&mut s, &me(), &rendered(2), DRAFT_ID, Some(&ours(3))).await);
            assert_eq!(at, ours(20));
            assert_eq!(prior, Prior::Replaced);
            let b = state.lock().unwrap();
            assert_eq!(b.expunged, vec![3]);
            // The near miss is where it was, untouched.
            assert!(b.held.iter().any(|m| m.0 == 30 && !m.2), "{:?}", b.held);
        });
    }

    #[test]
    fn a_draft_rata_saved_is_read_back_with_its_id_and_its_bcc() {
        rt_act().block_on(async {
            let (mut s, _) =
                scripted_drafts(DraftScript::dovecot(vec![(3, stored_draft(&[]))])).await;
            let (at, id, _) = saved(save_in(&mut s, &me(), &rendered(1), DRAFT_ID, None).await);
            let got = by_uid(
                &mut s,
                &mut no_redial(),
                &me(),
                &Folder::Drafts,
                &[at.uid, 3],
                7,
            )
            .await
            .unwrap();
            let mine = got.iter().find(|m| m.uid == at.uid).unwrap();
            assert_eq!(mine.id, id);
            assert_eq!(mine.draft_id.as_deref(), Some(DRAFT_ID));
            assert_eq!(mine.bcc, vec!["boss@example.net"]);
            assert_eq!(mine.body, "Half a thought, the second time");
            // Another app's draft carries no id: RATA will never replace it.
            assert_eq!(got.iter().find(|m| m.uid == 3).unwrap().draft_id, None);
        });
        assert_eq!(
            rata_draft_id(stored_draft(&[DRAFT_ID]).as_bytes()).as_deref(),
            Some(DRAFT_ID)
        );
        assert_eq!(
            rata_draft_id(stored_draft(&[DRAFT_ID, DRAFT_ID]).as_bytes()),
            None
        );
        assert_eq!(
            rata_draft_id(stored_draft(&["\"quoted\" value"]).as_bytes()),
            None
        );
        // A line in the text is not a header.
        assert_eq!(
            rata_draft_id(format!("Subject: x\r\n\r\nX-RATA-Draft: {DRAFT_ID}\r\n").as_bytes()),
            None
        );
    }

    #[test]
    fn saving_refuses_an_id_rata_did_not_make_before_dialling() {
        rt_act().block_on(async {
            let r = Resolver::system().unwrap();
            let got = save_draft(
                &r,
                &acct("10.0.0.1"),
                &outgoing_draft(),
                "x\r\nBcc: a@b.c",
                1,
                None,
            )
            .await;
            assert!(matches!(got, DraftSaved::Refused(_)), "{got:?}");
        });
    }
}
