//! Signing in with Microsoft (C2).
//!
//! Microsoft turned password sign-in to IMAP off — Outlook.com in September
//! 2024, Microsoft 365 before that — so an Outlook, Hotmail, Live or
//! Microsoft 365 mailbox opens only with an OAuth 2.0 access token. RATA gets
//! one the way a desktop app should, with no server of its own in the path:
//!
//! 1. A one-shot listener on `127.0.0.1` at a port the system picks.
//! 2. The customer's own browser — never the app's webview, which holds the
//!    bridge to everything — opens Microsoft's sign-in page, through the same
//!    rules as any link (`links::classify`), with a PKCE challenge
//!    (RFC 7636) and a random `state`.
//! 3. Microsoft sends the browser back to the listener with a code. The
//!    listener answers only a request carrying the right `state`, once,
//!    within five minutes, and says "you can close this tab".
//! 4. The code and the PKCE verifier go to Microsoft's token endpoint, which
//!    answers with an access token (about an hour) and a refresh token.
//!
//! **Where each token goes.** The refresh token is kept in the keychain, in
//! the slot a password would occupy (`vault::put_refresh`), and is sent only
//! to Microsoft's token endpoint. Access tokens are kept only in memory
//! ([`Microsoft`]) and are sent only to Microsoft's IMAP and SMTP servers,
//! over TLS, as XOAUTH2 (`rata_mail::credential`). Neither is written to
//! `mailboxes.json`, the page, the audit log or any message: every type that
//! holds one prints `<hidden>` for `{:?}`, and every sentence that could
//! carry one goes through [`scrub`].
//!
//! The client id is not a secret — a desktop app cannot keep one — and is
//! compiled in from `RATA_MS_CLIENT_ID`, like the licence key. A build
//! without it has no Microsoft sign-in, and says so in Add mailbox.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::time::{Instant, timeout};

/// The Entra application's id, from the build. Empty or unset: no Microsoft
/// sign-in in this build.
pub const CLIENT_ID: Option<&str> = option_env!("RATA_MS_CLIENT_ID");

/// `common`: personal Microsoft accounts and work or school ones alike.
pub const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
/// The only host the browser is ever sent to for signing in.
pub const LOGIN_HOST: &str = "login.microsoftonline.com";

/// IMAP and SMTP as the signed-in person, a refresh token so the customer
/// signs in once rather than hourly, and `openid email` for the one thing
/// RATA checks about who signed in: their address, so signing in as someone
/// else is named rather than failing as a refused token.
pub const SCOPES: &str = "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email";

/// How long the listener waits for the browser to come back.
pub const WAIT: Duration = Duration::from_secs(5 * 60);
/// An access token this close to expiring is renewed before use rather than
/// sent to a server that will refuse it part way through.
pub const EARLY: u64 = 120;

const HEAD_MAX: usize = 8 * 1024;
/// A browser sends its request line at once. Connections are answered one
/// at a time, so a program that connects and says nothing holds up the real
/// answer by this much (security review L1).
const HEAD_WAIT: Duration = Duration::from_secs(2);
/// How soon after one sign-in began another may: a page cannot open tab
/// after tab at Microsoft (security review L2).
pub const BEGIN_GAP: Duration = Duration::from_secs(3);
const HTTP_WAIT: Duration = Duration::from_secs(30);
const BODY_MAX: usize = 64 * 1024;

/// The client id, when this build has one. Checked for shape too: it goes
/// into a URL, and a stray quote or space from a misconfigured build would
/// otherwise go with it.
pub fn client_id() -> Option<&'static str> {
    usable_id(CLIENT_ID)
}

fn usable_id(raw: Option<&'static str>) -> Option<&'static str> {
    raw.map(str::trim).filter(|s| {
        !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

// ------------------------------------------------------------------- pieces

/// base64url without padding (RFC 4648 §5), as PKCE and JWTs use it.
pub fn b64url(bytes: &[u8]) -> String {
    rata_mail::words::base64_encode(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

fn b64url_decode(s: &str) -> Vec<u8> {
    rata_mail::words::base64(s.replace('-', "+").replace('_', "/").as_bytes())
}

/// `bytes` of the system's randomness, base64url.
pub fn random(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| {
        format!("This computer could not supply the randomness a sign-in needs: {e}")
    })?;
    Ok(b64url(&buf))
}

/// The PKCE `code_challenge` for a verifier: `S256`, base64url(SHA-256).
pub fn challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// Equal, taking as long whatever the first difference: `state` is compared
/// against whatever reaches the listener.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// `text` with every secret in `secrets` taken out. For sentences that come
/// from somewhere else — Microsoft's error descriptions, a server's
/// refusal — before they are shown or returned to the page.
pub fn scrub(text: &str, secrets: &[&str]) -> String {
    let mut out = text.to_string();
    for s in secrets {
        // Shorter is not a real token, and would blank out ordinary words.
        if s.len() >= 8 {
            out = out.replace(s, "[token hidden]");
        }
    }
    out
}

// ------------------------------------------------------------ one attempt

/// One sign-in: its `state`, its PKCE verifier and where Microsoft sends the
/// browser back. Made fresh for every attempt and used once.
pub struct Attempt {
    pub state: String,
    verifier: String,
    pub redirect_uri: String,
    pub deadline: Instant,
}

impl std::fmt::Debug for Attempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attempt")
            .field("redirect_uri", &self.redirect_uri)
            .field("state", &"<hidden>")
            .field("verifier", &"<hidden>")
            .finish()
    }
}

impl Attempt {
    /// 32 random bytes each: a 43-character verifier (RFC 7636's minimum
    /// length, and 256 bits), and a state nobody can guess.
    pub fn new(port: u16) -> Result<Self, String> {
        // Loopback, any port: Entra matches `http://127.0.0.1` whatever
        // the port for a desktop app, and the address is written here
        // exactly as registered apart from that.
        Attempt::with_redirect(format!("http://127.0.0.1:{port}"))
    }

    /// The same, sending the browser back to `redirect_uri`: for a provider
    /// that matches the address it was registered with exactly, port and
    /// all (Slack, `slack`).
    pub fn with_redirect(redirect_uri: String) -> Result<Self, String> {
        Ok(Attempt {
            state: random(32)?,
            verifier: random(32)?,
            redirect_uri,
            deadline: Instant::now() + WAIT,
        })
    }

    /// The PKCE `code_challenge` for this attempt's verifier.
    pub fn code_challenge(&self) -> String {
        challenge(&self.verifier)
    }

    /// The verifier itself, which goes only to the token endpoint.
    pub(crate) fn verifier(&self) -> &str {
        &self.verifier
    }

    /// Microsoft's sign-in page for this attempt. `login_hint` fills in the
    /// address the customer typed, so the right account is picked.
    pub fn authorize_url(&self, authorize: &str, client_id: &str, email: &str) -> String {
        self.authorize_url_for(authorize, client_id, SCOPES, Some(email))
    }

    /// Microsoft's sign-in page for this attempt, asking for `scopes`: the
    /// mailbox's (`SCOPES`) or OneDrive's (`onedrive::SCOPES`), which are
    /// for another resource and so a sign-in, and a token, of their own.
    pub fn authorize_url_for(
        &self,
        authorize: &str,
        client_id: &str,
        scopes: &str,
        login_hint: Option<&str>,
    ) -> String {
        let challenge = challenge(&self.verifier);
        let mut params = vec![
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("response_mode", "query"),
            ("scope", scopes),
            ("state", self.state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ];
        if let Some(hint) = login_hint {
            params.push(("login_hint", hint));
        }
        // Always a click in the browser: a browser already signed in to
        // Microsoft, with RATA approved, would otherwise finish the round
        // trip unseen (security review L2).
        params.push(("prompt", "select_account"));
        url::Url::parse_with_params(authorize, &params)
            .map(String::from)
            .unwrap_or_default()
    }
}

/// What the browser brought back.
#[derive(PartialEq, Eq)]
pub enum Callback {
    Code(String),
    /// The customer said no, or closed the consent screen.
    Denied,
    /// Microsoft said why it could not sign them in.
    Failed(String),
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Callback::Code(_) => f.write_str("Code(<hidden>)"),
            Callback::Denied => f.write_str("Denied"),
            Callback::Failed(w) => write!(f, "Failed({w:?})"),
        }
    }
}

/// A request that reached the listener: this sign-in's answer, or not.
#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    Ours(Callback),
    /// Anything without this attempt's `state` — the browser asking for a
    /// favicon, another program on the machine, a web page trying its luck.
    /// Answered and ignored; the listener keeps waiting.
    Stray,
}

/// Read the request-target of a request to the listener (`/?code=…&state=…`),
/// as Sign in with Microsoft hears it. The listener itself goes through
/// [`heard_with`]; this is what its tests read.
#[cfg(test)]
pub fn heard(target: &str, state: &str) -> Heard {
    heard_with(target, state, "Microsoft").0
}

/// Read the request-target of a request to the listener, for the provider
/// named `who` ("Microsoft", "Slack", "Google"), the only word that
/// differs, with every field of an answer that is this sign-in's: Google's
/// file chooser sends the files picked beside the code (`picked_file_ids`,
/// `google`). No fields for a stray request.
pub fn heard_with(target: &str, state: &str, who: &str) -> (Heard, HashMap<String, String>) {
    let stray = || (Heard::Stray, HashMap::new());
    if !target.starts_with('/') || target.len() > HEAD_MAX {
        return stray();
    }
    let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return stray();
    };
    if url.path() != "/" {
        return stray();
    }
    let mut seen: HashMap<String, String> = HashMap::new();
    for (k, v) in url.query_pairs() {
        // A repeated field is ambiguous, and nothing Microsoft sends.
        if seen.insert(k.into_owned(), v.into_owned()).is_some() {
            return stray();
        }
    }
    match seen.get("state") {
        Some(s) if same(s, state) => {}
        _ => return stray(),
    }
    let heard = if let Some(error) = seen.get("error") {
        if error == "access_denied" {
            Heard::Ours(Callback::Denied)
        } else {
            // The code only, never `error_description`: anything that
            // learned `state` could write that, and the form would show it
            // (security review L6).
            Heard::Ours(Callback::Failed(error_code(error)))
        }
    } else {
        match seen.get("code") {
            Some(code) if !code.is_empty() && code.len() <= 4096 => {
                Heard::Ours(Callback::Code(code.clone()))
            }
            _ => Heard::Ours(Callback::Failed(format!(
                "{who} sent the browser back without a sign-in code."
            ))),
        }
    };
    (heard, seen)
}

/// An OAuth error code as the redirect carried it (`invalid_request`,
/// `server_error`…) — which is only ever lowercase letters and underscores —
/// or nothing a stranger could have written.
pub(crate) fn error_code(code: &str) -> String {
    if !code.is_empty()
        && code.len() <= 60
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        code.into()
    } else {
        "an error it did not name".into()
    }
}

/// An error Microsoft named, made short and plain: the code, and the first
/// sentence of its description without the trace and correlation ids.
fn said(code: &str, description: Option<&str>) -> String {
    let plain = |s: &str, most: usize| -> String {
        s.chars()
            .filter(|c| !c.is_control())
            .take(most)
            .collect::<String>()
            .trim()
            .to_string()
    };
    let code = plain(code, 60);
    let desc = description
        .map(|d| {
            let d = d.split(['\r', '\n']).next().unwrap_or("");
            let d = d.split(" Trace ID").next().unwrap_or(d);
            plain(d, 200)
        })
        .unwrap_or_default();
    if desc.is_empty() {
        code
    } else {
        format!("{desc} ({code})")
    }
}

/// How waiting for the browser ended, when it did not bring a code.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    Cancelled,
    TimedOut,
    Denied,
    Failed(String),
}

/// Wait for Microsoft to send the browser back with this attempt's answer.
/// Every other request is answered and ignored; the first with the right
/// `state` ends it, as does `cancel` or the deadline.
pub async fn wait_for_code(
    listener: TcpListener,
    attempt: &Attempt,
    cancel: &Notify,
) -> Result<String, Ended> {
    wait_for_code_from(listener, attempt, cancel, "Microsoft").await
}

/// [`wait_for_code`], for the provider named `who`, which the browser tab
/// and a missing code name.
pub async fn wait_for_code_from(
    listener: TcpListener,
    attempt: &Attempt,
    cancel: &Notify,
    who: &str,
) -> Result<String, Ended> {
    wait_for_answer_from(listener, attempt, cancel, who)
        .await
        .map(|(code, _)| code)
}

/// [`wait_for_code_from`], with every field the browser brought back with
/// the code (`heard_with`), for the caller to read what it expects and
/// check it.
pub async fn wait_for_answer_from(
    listener: TcpListener,
    attempt: &Attempt,
    cancel: &Notify,
    who: &str,
) -> Result<(String, HashMap<String, String>), Ended> {
    loop {
        tokio::select! {
            _ = cancel.notified() => return Err(Ended::Cancelled),
            _ = tokio::time::sleep_until(attempt.deadline) => return Err(Ended::TimedOut),
            got = listener.accept() => {
                let Ok((stream, _)) = got else { continue };
                match answer(stream, &attempt.state, who).await {
                    (Heard::Ours(Callback::Code(code)), fields) => return Ok((code, fields)),
                    (Heard::Ours(Callback::Denied), _) => return Err(Ended::Denied),
                    (Heard::Ours(Callback::Failed(why)), _) => return Err(Ended::Failed(why)),
                    (Heard::Stray, _) => continue,
                }
            }
        }
    }
}

/// Read one request's head, answer it, and say what it was, with every
/// field of an answer that was this sign-in's.
async fn answer(mut stream: TcpStream, state: &str, who: &str) -> (Heard, HashMap<String, String>) {
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    let read = timeout(HEAD_WAIT, async {
        while head.len() < HEAD_MAX && !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
    })
    .await;
    let line = String::from_utf8_lossy(&head);
    let line = line.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (got, fields) = match (read, parts.next(), parts.next()) {
        (Ok(()), Some("GET"), Some(target)) => heard_with(target, state, who),
        _ => (Heard::Stray, HashMap::new()),
    };
    let (status, words) = match &got {
        Heard::Ours(Callback::Code(_)) => (
            "200 OK",
            format!(
                "RATA has what it needs from {who}. You can close this tab and go back to RATA."
            ),
        ),
        Heard::Ours(Callback::Denied) => (
            "200 OK",
            "Signing in was cancelled, so nothing was added. You can close this tab.".into(),
        ),
        Heard::Ours(Callback::Failed(_)) => (
            "200 OK",
            format!("{who} could not sign you in. You can close this tab; RATA says why."),
        ),
        Heard::Stray => ("404 Not Found", "Nothing here.".into()),
    };
    let _ = stream.write_all(page(status, &words).as_bytes()).await;
    let _ = stream.shutdown().await;
    (got, fields)
}

/// A tiny page with nothing external in it, and a policy that forbids
/// anything being fetched from it.
fn page(status: &str, words: &str) -> String {
    let body = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>RATA</title><body style=\"font:16px/1.5 system-ui,sans-serif;margin:15vh auto;max-width:32em;padding:0 16px;color:#15171C;background:#F5F6F8\"><p>{words}</p></body></html>"
    );
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

// ------------------------------------------------------------------ tokens

/// What Microsoft's token endpoint issued.
#[derive(Clone)]
pub struct Tokens {
    pub access: String,
    /// Microsoft sends a new one with most answers; kept when it does.
    pub refresh: Option<String>,
    pub expires_in: u64,
    /// The address Microsoft signed in, from the `id_token` — read, never
    /// kept.
    pub signed_in_as: Option<String>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"<hidden>")
            .field("refresh", &self.refresh.as_ref().map(|_| "<hidden>"))
            .field("expires_in", &self.expires_in)
            .field("signed_in_as", &self.signed_in_as)
            .finish()
    }
}

/// Why the token endpoint issued nothing. The three need different answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The sign-in itself is no longer good — withdrawn, expired from
    /// disuse, a password changed, consent removed. Only signing in again
    /// fixes it, so the mailbox is parked until they do.
    Revoked(String),
    /// Microsoft refused for another reason (this build's registration, a
    /// malformed request). Signing in again would not help.
    Refused(String),
    /// Microsoft could not be reached, or answered as though it were having
    /// a bad morning. Nothing is wrong with the sign-in: try later.
    Net(String),
}

#[derive(serde::Deserialize)]
struct Raw {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<serde_json::Value>,
    id_token: Option<String>,
    token_type: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Read the token endpoint's answer.
pub fn read_tokens(status: u16, body: &[u8]) -> Result<Tokens, TokenError> {
    read_tokens_from(status, body, "Microsoft")
}

/// [`read_tokens`], for the provider named `who` ("Microsoft", "Google"),
/// which every sentence names. Both answer as RFC 6749 section 5 says.
pub fn read_tokens_from(status: u16, body: &[u8], who: &str) -> Result<Tokens, TokenError> {
    let Ok(raw) = serde_json::from_slice::<Raw>(body) else {
        return Err(if status >= 500 || status == 429 {
            TokenError::Net(format!(
                "{who}'s sign-in service is not answering properly right now (status {status}). RATA will try again."
            ))
        } else {
            TokenError::Refused(format!(
                "{who}'s sign-in service sent an answer RATA could not read (status {status})."
            ))
        });
    };
    if let Some(code) = raw.error.as_deref() {
        let why = said(code, raw.error_description.as_deref());
        return Err(match code {
            "invalid_grant" | "interaction_required" | "consent_required" | "login_required" => {
                TokenError::Revoked(why)
            }
            "temporarily_unavailable" | "server_error" => TokenError::Net(why),
            _ => TokenError::Refused(why),
        });
    }
    if !(200..300).contains(&status) {
        return Err(if status >= 500 || status == 429 {
            TokenError::Net(format!(
                "{who}'s sign-in service is not answering properly right now (status {status}). RATA will try again."
            ))
        } else {
            TokenError::Refused(format!(
                "{who}'s sign-in service refused the request (status {status})."
            ))
        });
    }
    if raw
        .token_type
        .as_deref()
        .is_some_and(|t| !t.eq_ignore_ascii_case("bearer"))
    {
        return Err(TokenError::Refused(format!(
            "{who} issued a kind of sign-in RATA does not use."
        )));
    }
    let access = raw
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| TokenError::Refused(format!("{who}'s answer had no sign-in in it.")))?;
    let expires_in = match raw.expires_in {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|&n| n > 0)
    .unwrap_or(3600);
    Ok(Tokens {
        access,
        refresh: raw.refresh_token.filter(|t| !t.is_empty()),
        expires_in,
        signed_in_as: raw.id_token.as_deref().and_then(id_address),
    })
}

/// The address in an `id_token` — `email`, else `preferred_username`. Not
/// checked for a signature: it came straight from Microsoft's token endpoint
/// over TLS, which OpenID Connect accepts in place of one (Core §3.1.3.7),
/// and RATA uses it only to word an error.
fn id_address(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let claims: serde_json::Value = serde_json::from_slice(&b64url_decode(payload)).ok()?;
    ["email", "preferred_username"].iter().find_map(|k| {
        claims
            .get(*k)
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| v.contains('@') && v.len() <= 254 && !v.chars().any(char::is_control))
    })
}

/// One HTTPS client, for Microsoft's token endpoint. It trusts what the mail
/// connections trust (`rata_mail::imap::tls_client_config`), follows no
/// redirects — a form carrying a refresh token goes to the address it was
/// written for or nowhere — and gives up after half a minute.
pub fn client(use_proxy: bool) -> Result<reqwest::Client, String> {
    client_for(use_proxy, "Microsoft")
}

/// [`client`], for reaching the provider named `who`.
pub fn client_for(use_proxy: bool, who: &str) -> Result<reqwest::Client, String> {
    let tls = rata_mail::imap::tls_client_config()?;
    let mut b = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .timeout(HTTP_WAIT)
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("RATA/", env!("CARGO_PKG_VERSION")));
    if !use_proxy {
        b = b.no_proxy();
    }
    b.build()
        .map_err(|e| format!("RATA could not get ready to reach {who}: {e}"))
}

async fn post(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), TokenError> {
    post_to(http, url, form, "Microsoft").await
}

/// [`post`], to the provider named `who`.
async fn post_to(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
    who: &str,
) -> Result<(u16, Vec<u8>), TokenError> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let unreachable = |e: reqwest::Error| {
        TokenError::Net(format!(
            "{who}'s sign-in service could not be reached: {}",
            e.without_url()
        ))
    };
    let mut resp = http
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(unreachable)?;
    let status = resp.status().as_u16();
    let mut got = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(unreachable)? {
        got.extend_from_slice(&chunk);
        if got.len() > BODY_MAX {
            return Err(TokenError::Refused(format!(
                "{who}'s sign-in service sent far more than a sign-in."
            )));
        }
    }
    Ok((status, got))
}

/// One request to the token endpoint of the provider named `who`, with
/// `form` as it is (a provider whose form differs from Microsoft's: Google's
/// carries the desktop client's secret), its answer read as
/// [`read_tokens_from`] reads it, and every one of `secrets` taken out of
/// any sentence that comes back.
pub async fn token_request(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
    who: &str,
    secrets: &[&str],
) -> Result<Tokens, TokenError> {
    let (status, body) = post_to(http, url, form, who)
        .await
        .map_err(|e| hide_in(e, secrets))?;
    read_tokens_from(status, &body, who).map_err(|e| hide_in(e, secrets))
}

/// Trade the code the browser brought back for tokens.
pub async fn exchange(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    code: &str,
    attempt: &Attempt,
) -> Result<Tokens, TokenError> {
    exchange_for(http, token_url, client_id, code, attempt, SCOPES).await
}

/// [`exchange`], for `scopes`. Microsoft issues a token for one resource at
/// a time, so the mailbox's and OneDrive's are never asked for together.
pub async fn exchange_for(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    code: &str,
    attempt: &Attempt,
    scopes: &str,
) -> Result<Tokens, TokenError> {
    let (status, body) = post(
        http,
        token_url,
        &[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &attempt.redirect_uri),
            ("code_verifier", &attempt.verifier),
            ("scope", scopes),
        ],
    )
    .await?;
    read_tokens(status, &body).map_err(|e| hide_in(e, &[code, &attempt.verifier]))
}

/// A fresh access token from the refresh token.
pub async fn refresh(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<Tokens, TokenError> {
    refresh_for(http, token_url, client_id, refresh_token, SCOPES).await
}

/// [`refresh`], for `scopes`.
pub async fn refresh_for(
    http: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    refresh_token: &str,
    scopes: &str,
) -> Result<Tokens, TokenError> {
    let (status, body) = post(
        http,
        token_url,
        &[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", scopes),
        ],
    )
    .await?;
    read_tokens(status, &body).map_err(|e| hide_in(e, &[refresh_token]))
}

fn hide_in(e: TokenError, secrets: &[&str]) -> TokenError {
    match e {
        TokenError::Revoked(w) => TokenError::Revoked(scrub(&w, secrets)),
        TokenError::Refused(w) => TokenError::Refused(scrub(&w, secrets)),
        TokenError::Net(w) => TokenError::Net(scrub(&w, secrets)),
    }
}

// ----------------------------------------------------------- in memory

/// An access token, and when it stops working (seconds since 1970).
#[derive(Clone)]
pub struct Access {
    pub token: String,
    pub expires_at: u64,
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Access")
            .field("token", &"<hidden>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// When a token issued `now` for `expires_in` seconds stops working. A
/// nonsense lifetime is held to between a minute and a day.
pub fn expiry(now: u64, expires_in: u64) -> u64 {
    now.saturating_add(expires_in.clamp(60, 86_400))
}

/// Whether a token can still be used, leaving [`EARLY`] seconds in hand.
pub fn fresh(a: &Access, now: u64) -> bool {
    a.expires_at > now.saturating_add(EARLY)
}

/// Whether an operation a server refused for its token is worth one more
/// try: only with a token, only when the token was the refusal, and only
/// when the token was not just issued — a token Microsoft handed over a
/// moment ago would be refused again, and a second refresh would not change
/// that.
pub fn retry(with_token: bool, token_refused: bool, token_was_new: bool) -> bool {
    with_token && token_refused && !token_was_new
}

/// What RATA holds in memory for signing in with Microsoft: the build's
/// client id, the access tokens, and the one sign-in in progress.
pub struct Microsoft {
    pub client_id: Option<String>,
    pub authorize_url: String,
    pub token_url: String,
    use_proxy: bool,
    http: OnceLock<Result<reqwest::Client, String>>,
    access: Mutex<HashMap<String, Access>>,
    /// Held while a refresh is under way, so two refreshes of one mailbox
    /// never race each other with the same refresh token.
    pub refreshing: tokio::sync::Mutex<()>,
    gate: Gate,
    /// How long after one sign-in began another may.
    pub(crate) gap: Duration,
}

/// One sign-in at a time, and not one straight after another: the browser
/// tab that counts, for one provider. Sign in with Microsoft and Share to
/// Slack's sign-in (`slack`) each have a gate of their own.
#[derive(Default)]
pub struct Gate {
    pending: Mutex<Option<Arc<Notify>>>,
    /// When the last sign-in began.
    began: Mutex<Option<Instant>>,
}

impl Gate {
    /// Start a sign-in with the provider named `who`. There is only ever one
    /// listener and one browser tab that counts: another is refused while
    /// one waits (the form's Cancel ends that one first), and for `gap`
    /// after the last began, so a page cannot open the provider's page in
    /// tab after tab (security review L2).
    pub fn begin(&self, gap: Duration, who: &str) -> Result<Arc<Notify>, String> {
        let busy = || format!("RATA could not start signing in with {who}.");
        let mut pending = self.pending.lock().map_err(|_| busy())?;
        if pending.is_some() {
            return Err(format!(
                "A {who} sign-in is already waiting in your browser. Finish it there, or press Cancel first."
            ));
        }
        let mut began = self.began.lock().map_err(|_| busy())?;
        if began.is_some_and(|t| t.elapsed() < gap) {
            return Err(format!(
                "Wait a moment, then try signing in with {who} again."
            ));
        }
        *began = Some(Instant::now());
        let next = Arc::new(Notify::new());
        *pending = Some(next.clone());
        Ok(next)
    }

    /// Stop the sign-in in progress, if there is one.
    pub fn cancel(&self) -> bool {
        match self.pending.lock().ok().and_then(|mut p| p.take()) {
            Some(n) => {
                // Stored as a permit if the listener is not waiting yet.
                n.notify_one();
                true
            }
            None => false,
        }
    }

    /// A sign-in is over; forget it unless another has already replaced it.
    pub fn finish(&self, which: &Arc<Notify>) {
        if let Ok(mut p) = self.pending.lock()
            && p.as_ref().is_some_and(|n| Arc::ptr_eq(n, which))
        {
            p.take();
        }
    }
}

impl Microsoft {
    /// As this build was made.
    pub fn from_build() -> Self {
        Microsoft::new(
            client_id().map(String::from),
            AUTHORIZE_URL,
            TOKEN_URL,
            true,
        )
    }

    pub fn new(client_id: Option<String>, authorize: &str, token: &str, use_proxy: bool) -> Self {
        Microsoft {
            client_id,
            authorize_url: authorize.into(),
            token_url: token.into(),
            use_proxy,
            http: OnceLock::new(),
            access: Mutex::new(HashMap::new()),
            refreshing: tokio::sync::Mutex::new(()),
            gate: Gate::default(),
            gap: BEGIN_GAP,
        }
    }

    pub fn configured(&self) -> bool {
        self.client_id.is_some()
    }

    pub fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .get_or_init(|| client(self.use_proxy))
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn cached(&self, email: &str) -> Option<Access> {
        self.access.lock().ok()?.get(&key(email)).cloned()
    }

    pub fn keep(&self, email: &str, access: Access) {
        if let Ok(mut held) = self.access.lock() {
            held.insert(key(email), access);
        }
    }

    pub fn forget(&self, email: &str) {
        if let Ok(mut held) = self.access.lock() {
            held.remove(&key(email));
        }
    }

    /// Start a sign-in. There is only ever one listener and one browser tab
    /// that counts: another is refused while one waits (the form's Cancel
    /// ends that one first), and for `gap` after the last began, so a page
    /// cannot open Microsoft's page in tab after tab (security review L2).
    pub fn begin(&self) -> Result<Arc<Notify>, String> {
        self.gate.begin(self.gap, "Microsoft")
    }

    /// Stop the sign-in in progress, if there is one.
    pub fn cancel(&self) -> bool {
        self.gate.cancel()
    }

    /// A sign-in is over; forget it unless another has already replaced it.
    pub fn finish(&self, which: &Arc<Notify>) {
        self.gate.finish(which)
    }
}

fn key(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Open Microsoft's sign-in page in the customer's browser — through the
/// same rules as every other link, and only if it is Microsoft's page over
/// https. The app's own window never loads it.
pub fn open_sign_in(url: &str) -> Result<(), String> {
    match crate::links::classify(url) {
        Some(crate::links::Link::Web(u))
            if u.scheme() == "https" && u.host_str() == Some(LOGIN_HOST) =>
        {
            crate::links::open_in_browser(&u)
        }
        _ => Err("RATA would only open Microsoft's own sign-in page, and this was not it.".into()),
    }
}

/// For tests: Microsoft's token endpoint, as far as RATA can tell: a local server
/// that reads one form and answers with `reply`. Returns its address and
/// what it was sent.
#[cfg(test)]
pub(crate) async fn scripted_token_endpoint(
    reply: &'static str,
    status: &'static str,
) -> (String, tokio::task::JoinHandle<String>) {
    let (url, _asked, _release, task) = held_token_endpoint(reply, status, false).await;
    (url, task)
}

/// For tests: the same endpoint, but one that holds its answer once the
/// request is in, until told to go on. `asked` fires when the request has
/// been read; dropping or sending `release` lets the answer out. With
/// `hold` false it answers at once, as `scripted_token_endpoint` does. This
/// is how a test puts something (an Unlink, a Delete account) in the middle
/// of a renewal Microsoft is still answering.
#[cfg(test)]
pub(crate) async fn held_token_endpoint(
    reply: &'static str,
    status: &'static str,
    hold: bool,
) -> (
    String,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<String>,
) {
    let (asked_tx, asked) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/common/oauth2/v2.0/token",
        l.local_addr().unwrap().port()
    );
    let task = tokio::spawn(async move {
        let (mut s, _) = l.accept().await.unwrap();
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = s.read(&mut buf).await.unwrap();
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let len: usize = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if got.len() >= end + 4 + len || n == 0 {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let _ = asked_tx.send(());
        if hold {
            let _ = released.await;
        }
        let answer = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        s.write_all(answer.as_bytes()).await.unwrap();
        let _ = s.shutdown().await;
        String::from_utf8_lossy(&got).to_string()
    });
    (url, asked, release, task)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn pkce_matches_the_rfc_7636_example() {
        // RFC 7636, Appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        // And the verifier's own form: 32 random bytes are 43 characters of
        // base64url, the shortest RFC 7636 allows, with nothing to escape.
        let v = random(32).unwrap();
        assert_eq!(v.len(), 43);
        assert!(
            v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{v}"
        );
        assert_ne!(v, random(32).unwrap(), "two attempts, two verifiers");
    }

    #[test]
    fn the_client_id_is_used_only_when_it_looks_like_one() {
        assert_eq!(
            usable_id(Some(" 0f3a9c2e-1b4d-4e8f-9a7b-5c6d7e8f9a0b ")),
            Some("0f3a9c2e-1b4d-4e8f-9a7b-5c6d7e8f9a0b")
        );
        assert_eq!(usable_id(Some("")), None);
        assert_eq!(usable_id(None), None);
        assert_eq!(usable_id(Some("abc&redirect_uri=evil")), None);
    }

    #[test]
    fn the_sign_in_page_asks_for_exactly_what_rata_needs() {
        let a = Attempt::new(49152).unwrap();
        let link = a.authorize_url(AUTHORIZE_URL, "client-1", "Me@Outlook.com");
        let u = url::Url::parse(&link).unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host_str(), Some(LOGIN_HOST));
        assert_eq!(u.path(), "/common/oauth2/v2.0/authorize");
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:49152");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], challenge(&a.verifier));
        assert_eq!(q["state"], a.state);
        assert_eq!(q["login_hint"], "Me@Outlook.com");
        // Every sign-in needs a click in the browser, even one already
        // signed in to Microsoft with RATA approved (security review L2).
        assert_eq!(q["prompt"], "select_account");
        let scopes: Vec<&str> = q["scope"].split(' ').collect();
        for s in [
            "https://outlook.office.com/IMAP.AccessAsUser.All",
            "https://outlook.office.com/SMTP.Send",
            "offline_access",
        ] {
            assert!(scopes.contains(&s), "{s}");
        }
        // The verifier itself never leaves the machine except to the token
        // endpoint.
        assert!(!link.contains(&a.verifier));
        // And the address passes the same rules as any link, to the browser.
        match crate::links::classify(&link) {
            Some(crate::links::Link::Web(w)) => assert_eq!(w.host_str(), Some(LOGIN_HOST)),
            other => panic!("{other:?}"),
        }
        // Nothing but Microsoft's page is opened this way.
        assert!(open_sign_in("https://login.microsoftonline.com.evil.example/x").is_err());
        assert!(open_sign_in("http://login.microsoftonline.com/x").is_err());
        assert!(open_sign_in("javascript:alert(1)").is_err());
    }

    #[test]
    fn only_this_attempts_answer_is_taken() {
        let st = "the-state-0123456789";
        assert_eq!(
            heard(&format!("/?code=abc.def&state={st}"), st),
            Heard::Ours(Callback::Code("abc.def".into()))
        );
        // Percent-encoding is undone.
        assert_eq!(
            heard(
                &format!("/?code=M.C5%2Fx%3D&state={st}&session_state=z"),
                st
            ),
            Heard::Ours(Callback::Code("M.C5/x=".into()))
        );
        // A wrong, missing or repeated state is not ours, whatever it carries.
        assert_eq!(heard("/?code=abc&state=guess", st), Heard::Stray);
        assert_eq!(heard("/?code=abc", st), Heard::Stray);
        assert_eq!(
            heard(&format!("/?code=a&state={st}&state=other"), st),
            Heard::Stray
        );
        assert_eq!(heard("/?error=access_denied&state=guess", st), Heard::Stray);
        // Anything but the root.
        assert_eq!(heard(&format!("/favicon.ico?state={st}"), st), Heard::Stray);
        assert_eq!(heard("http://evil.example/", st), Heard::Stray);
    }

    #[test]
    fn a_refusal_or_a_missing_code_ends_the_attempt_with_why() {
        let st = "the-state-0123456789";
        assert_eq!(
            heard(
                &format!("/?error=access_denied&error_description=The+user+declined&state={st}"),
                st
            ),
            Heard::Ours(Callback::Denied)
        );
        match heard(
            &format!(
                "/?error=invalid_request&error_description=AADSTS50011%3A+The+redirect+URI+does+not+match.%0D%0ATrace+ID%3A+abc&state={st}"
            ),
            st,
        ) {
            // Only the code: whoever knows `state` could write the
            // description, and the form would show it (security review L6).
            Heard::Ours(Callback::Failed(w)) => assert_eq!(w, "invalid_request"),
            other => panic!("{other:?}"),
        }
        // Nor can the code itself carry words.
        match heard(&format!("/?error=Call+support+on+0800+123&state={st}"), st) {
            Heard::Ours(Callback::Failed(w)) => {
                assert!(!w.contains("0800") && !w.contains("support"), "{w}")
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            heard(&format!("/?state={st}"), st),
            Heard::Ours(Callback::Failed(
                "Microsoft sent the browser back without a sign-in code.".into()
            ))
        );
        assert_eq!(
            heard(&format!("/?code=&state={st}"), st),
            heard(&format!("/?state={st}"), st)
        );
    }

    #[test]
    fn the_listener_takes_one_answer_and_ignores_the_rest() {
        rt().block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let a = Attempt::new(port).unwrap();
            let state = a.state.clone();
            let cancel = Notify::new();
            let browser = tokio::spawn(async move {
                let ask = |target: String| async move {
                    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                    s.write_all(
                        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                    let mut got = String::new();
                    s.read_to_string(&mut got).await.unwrap();
                    got
                };
                let stray = ask("/favicon.ico".into()).await;
                let guess = ask("/?code=stolen&state=guess".into()).await;
                let ours = ask(format!("/?code=the-code&state={state}")).await;
                (stray, guess, ours)
            });
            let code = wait_for_code(listener, &a, &cancel).await;
            let (stray, guess, ours) = browser.await.unwrap();
            assert_eq!(code, Ok("the-code".into()));
            assert!(stray.starts_with("HTTP/1.1 404"), "{stray}");
            assert!(guess.starts_with("HTTP/1.1 404"), "{guess}");
            assert!(ours.starts_with("HTTP/1.1 200"), "{ours}");
            assert!(ours.contains("You can close this tab"), "{ours}");
            assert!(ours.contains("default-src 'none'"), "{ours}");
            // Nothing on the page is fetched from anywhere.
            assert!(!ours.contains("src=") && !ours.contains("href="), "{ours}");
            assert!(!ours.contains("the-code"), "{ours}");
        });
    }

    /// Security review L1: connections that open and say nothing cannot
    /// hold up the browser's answer for long.
    #[test]
    fn silent_connections_do_not_hold_up_the_answer() {
        rt().block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let a = Attempt::new(port).unwrap();
            let state = a.state.clone();
            let cancel = Notify::new();
            let started = Instant::now();
            let others = tokio::spawn(async move {
                let s1 = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                let s2 = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                s.write_all(
                    format!("GET /?code=the-code&state={state} HTTP/1.1\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
                let mut got = String::new();
                s.read_to_string(&mut got).await.unwrap();
                drop((s1, s2));
                got
            });
            assert_eq!(
                wait_for_code(listener, &a, &cancel).await,
                Ok("the-code".into())
            );
            assert!(others.await.unwrap().starts_with("HTTP/1.1 200"));
            assert!(
                started.elapsed() < Duration::from_secs(6),
                "{:?}",
                started.elapsed()
            );
        });
    }

    #[test]
    fn cancel_or_the_deadline_stops_the_wait() {
        rt().block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let a = Attempt::new(listener.local_addr().unwrap().port()).unwrap();
            let ms = Microsoft::new(None, AUTHORIZE_URL, TOKEN_URL, false);
            let n = ms.begin().unwrap();
            // Cancelled before the wait began still counts.
            assert!(ms.cancel());
            assert_eq!(wait_for_code(listener, &a, &n).await, Err(Ended::Cancelled));
            assert!(!ms.cancel(), "and a cancelled sign-in is gone");

            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let mut a = Attempt::new(listener.local_addr().unwrap().port()).unwrap();
            a.deadline = Instant::now() + Duration::from_millis(50);
            assert_eq!(
                wait_for_code(listener, &a, &Notify::new()).await,
                Err(Ended::TimedOut)
            );

            // Security review L2: straight after one began, another is
            // refused; and while one waits, so is another.
            let too_soon = ms.begin().unwrap_err();
            assert!(too_soon.contains("Wait a moment"), "{too_soon}");
            let mut ms = ms;
            ms.gap = Duration::ZERO;
            let first = ms.begin().unwrap();
            let busy = ms.begin().unwrap_err();
            assert!(busy.contains("already waiting"), "{busy}");
            // Finishing it frees the way; so does Cancel.
            ms.finish(&first);
            let second = ms.begin().unwrap();
            assert!(ms.cancel());
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let a = Attempt::new(listener.local_addr().unwrap().port()).unwrap();
            assert_eq!(
                wait_for_code(listener, &a, &second).await,
                Err(Ended::Cancelled)
            );
            assert!(ms.begin().is_ok());
        });
    }

    fn jwt(claims: &str) -> String {
        format!(
            "{}.{}.sig",
            b64url(br#"{"alg":"none"}"#),
            b64url(claims.as_bytes())
        )
    }

    #[test]
    fn the_token_answer_is_read_and_its_secrets_never_printed() {
        let body = format!(
            r#"{{"token_type":"Bearer","scope":"x","expires_in":3599,"ext_expires_in":3599,"access_token":"EwB-access-secret","refresh_token":"M.C5-refresh-secret","id_token":"{}"}}"#,
            jwt(r#"{"email":"Me@Outlook.com","preferred_username":"other@x.example"}"#)
        );
        let t = read_tokens(200, body.as_bytes()).unwrap();
        assert_eq!(t.access, "EwB-access-secret");
        assert_eq!(t.refresh.as_deref(), Some("M.C5-refresh-secret"));
        assert_eq!(t.expires_in, 3599);
        assert_eq!(t.signed_in_as.as_deref(), Some("me@outlook.com"));
        let shown = format!("{t:?}");
        assert!(!shown.contains("secret"), "{shown}");
        // expires_in as a string, as some endpoints send it; missing is an hour.
        let t = read_tokens(200, br#"{"access_token":"a","expires_in":"120"}"#).unwrap();
        assert_eq!((t.expires_in, t.refresh, t.signed_in_as), (120, None, None));
        assert_eq!(
            read_tokens(200, br#"{"access_token":"a"}"#)
                .unwrap()
                .expires_in,
            3600
        );
        // Only preferred_username: still an address.
        let body = format!(
            r#"{{"access_token":"a","id_token":"{}"}}"#,
            jwt(r#"{"preferred_username":"ann@contoso.example"}"#)
        );
        assert_eq!(
            read_tokens(200, body.as_bytes())
                .unwrap()
                .signed_in_as
                .as_deref(),
            Some("ann@contoso.example")
        );
    }

    #[test]
    fn a_refused_refresh_is_told_apart_from_a_bad_morning() {
        let revoked = br#"{"error":"invalid_grant","error_description":"AADSTS70008: The provided authorization code or refresh token has expired due to inactivity.\r\nTrace ID: 1\r\nCorrelation ID: 2"}"#;
        match read_tokens(400, revoked) {
            Err(TokenError::Revoked(w)) => {
                assert!(w.contains("AADSTS70008"), "{w}");
                assert!(!w.contains("Trace ID"), "{w}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            read_tokens(400, br#"{"error":"interaction_required"}"#),
            Err(TokenError::Revoked(_))
        ));
        assert!(matches!(
            read_tokens(503, br#"{"error":"temporarily_unavailable"}"#),
            Err(TokenError::Net(_))
        ));
        assert!(matches!(
            read_tokens(502, b"<html>Bad gateway</html>"),
            Err(TokenError::Net(_))
        ));
        assert!(matches!(
            read_tokens(400, br#"{"error":"invalid_client"}"#),
            Err(TokenError::Refused(_))
        ));
        assert!(matches!(
            read_tokens(200, br#"{"token_type":"mac","access_token":"a"}"#),
            Err(TokenError::Refused(_))
        ));
        assert!(matches!(
            read_tokens(200, br#"{"access_token":""}"#),
            Err(TokenError::Refused(_))
        ));
    }

    #[test]
    fn a_token_is_renewed_two_minutes_before_it_runs_out() {
        let a = Access {
            token: "t".into(),
            expires_at: expiry(1_000, 3_600),
        };
        assert_eq!(a.expires_at, 4_600);
        assert!(fresh(&a, 1_000));
        assert!(fresh(&a, 4_600 - EARLY - 1));
        assert!(!fresh(&a, 4_600 - EARLY));
        assert!(!fresh(&a, 9_999));
        // A lifetime of nothing, or of years, is held to something sensible.
        assert_eq!(expiry(10, 0), 70);
        assert_eq!(expiry(10, u64::MAX), 10 + 86_400);
        assert_eq!(expiry(u64::MAX, 60), u64::MAX);
        assert!(!format!("{a:?}").contains("\"t\""));
    }

    #[test]
    fn a_refused_token_is_renewed_and_tried_once_more_and_only_once() {
        // Refused, with a token held from before: renew and try again.
        assert!(retry(true, true, false));
        // A token just issued and refused anyway: another would be too.
        assert!(!retry(true, true, true));
        // Not refused, or not a token at all: nothing to renew.
        assert!(!retry(true, false, false));
        assert!(!retry(false, true, false));
    }

    #[test]
    fn a_secret_is_taken_out_of_whatever_is_said() {
        let said = "AADSTS9002313: Invalid request for code M.C5-code-secret-value";
        assert_eq!(
            scrub(said, &["M.C5-code-secret-value"]),
            "AADSTS9002313: Invalid request for code [token hidden]"
        );
        assert_eq!(scrub("a short word", &["a"]), "a short word");
    }

    #[test]
    fn the_code_and_verifier_go_to_the_token_endpoint_as_a_form() {
        rt().block_on(async {
            let (url, sent) = scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-a","refresh_token":"M.R-1"}"#,
                "200 OK",
            )
            .await;
            let http = client(false).unwrap();
            let a = Attempt::new(50000).unwrap();
            let t = exchange(&http, &url, "client-1", "the-code", &a).await.unwrap();
            assert_eq!(t.access, "EwB-a");
            let sent = sent.await.unwrap();
            assert!(sent.starts_with("POST /common/oauth2/v2.0/token HTTP/1.1"), "{sent}");
            let body = sent.split("\r\n\r\n").nth(1).unwrap();
            let form: HashMap<String, String> =
                url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["code"], "the-code");
            assert_eq!(form["code_verifier"], a.verifier);
            assert_eq!(challenge(&form["code_verifier"]), challenge(&a.verifier));
            assert_eq!(form["redirect_uri"], "http://127.0.0.1:50000");
            assert_eq!(form["client_id"], "client-1");
            assert!(form["scope"].contains("offline_access"));
            assert!(!form.contains_key("client_secret"), "a desktop app has none");
        });
    }

    #[test]
    fn a_refresh_refused_by_microsoft_says_so_without_the_token() {
        rt().block_on(async {
            let (url, sent) = scripted_token_endpoint(
                r#"{"error":"invalid_grant","error_description":"AADSTS50173: The provided grant has expired due to it being revoked, M.R-old-refresh-token was issued before."}"#,
                "400 Bad Request",
            )
            .await;
            let http = client(false).unwrap();
            match refresh(&http, &url, "client-1", "M.R-old-refresh-token").await {
                Err(TokenError::Revoked(w)) => {
                    assert!(w.contains("AADSTS50173"), "{w}");
                    assert!(!w.contains("M.R-old-refresh-token"), "{w}");
                }
                other => panic!("{other:?}"),
            }
            let sent = sent.await.unwrap();
            assert!(sent.contains("grant_type=refresh_token"), "{sent}");
            // Nobody listening: the network, never a revoked sign-in.
            let gone = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let url = format!("http://127.0.0.1:{}/token", gone.local_addr().unwrap().port());
            drop(gone);
            assert!(matches!(
                refresh(&http, &url, "client-1", "M.R-x-refresh").await,
                Err(TokenError::Net(_))
            ));
        });
    }
}
