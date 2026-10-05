//! Share to Slack (K3).
//!
//! Send only: a message, or files with a message, to a channel or a person
//! the customer picks in Share to Slack. Nothing here reads a message from
//! Slack; the only things asked of it are who and where the customer can
//! send to (`conversations.list`, `users.list`) and the sending itself
//! (`chat.postMessage`, the two-step file upload).
//!
//! **Signing in.** Slack's OAuth v2 with a *user* token and PKCE (Slack's
//! PKCE is generally available since 2026-03-30), so there is no client
//! secret anywhere and no server of RATA's in the path:
//!
//! 1. A one-shot listener on `127.0.0.1` at one of [`PORTS`]. Slack treats a
//!    `http://localhost` redirect as a desktop redirect once the app has PKCE
//!    turned on, but it matches the redirect exactly as registered, port
//!    and all, and refuses `127.0.0.1`. So the owner registers
//!    `http://localhost:<port>` for each of the three ports, and RATA uses
//!    the first it can listen on (`docs/CONNECTIONS-SETUP.md`). The
//!    browser's `localhost` reaches the IPv4 listener; a browser that tries
//!    `::1` first falls back to it when nothing answers there.
//! 2. The customer's own browser opens `slack.com/oauth/v2/authorize` with
//!    `user_scope`, a PKCE challenge (`S256`, the only method Slack takes)
//!    and a random `state`, through `links::classify`, never the webview.
//! 3. The code and the verifier go to `oauth.v2.access`, without a client
//!    secret, which answers with a user access token (12 hours) and a
//!    refresh token. Slack rotates these: each refresh spends the refresh
//!    token it was given and hands back a new one, and refresh tokens of a
//!    PKCE app expire after 30 days, so a customer who shares nothing for a
//!    month connects Slack again.
//!
//! The machinery is `oauth.rs`'s (the `state` check, the listener, the
//! gate that allows one sign-in at a time, the HTTPS client with the mail
//! connections' TLS, no redirects and a timeout); only Slack's own words and
//! answers are here.
//!
//! **Where each token goes.** Both tokens are kept in the keychain under a
//! service of their own (`vault::SLACK_SERVICE`), with the access token's
//! expiry, so a restart within twelve hours does not spend a refresh (Slack
//! allows only two live tokens from refreshes within that time), and in
//! memory while RATA runs. They are sent only to `slack.com` over TLS, as a
//! bearer token or, for the refresh token, in the form `oauth.v2.access`
//! reads. The store keeps only which workspace is connected and who signed
//! in, by Slack's ids. Every type that holds a token prints `<hidden>`, and
//! every sentence that could carry one goes through `oauth::scrub`.
//!
//! **Disconnect and Delete account win.** A sign-in or a refresh that
//! finishes after either writes nothing (SEC-7, SEC-8): both check, under
//! the list's lock, that the Slack generation (bumped by every connect,
//! disconnect and Delete account) and the epoch are still the ones they
//! began under, and Disconnect and Delete account empty the keychain under
//! that same lock.
//!
//! The client id is not a secret and is compiled in from
//! `RATA_SLACK_CLIENT_ID`, like Microsoft's. A build without it offers no
//! Slack at all, exactly as before K3.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rata_mail::{looks_disguised, safe_file_name};
use serde::Serialize;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::cloud::{Connected, Refusal, Service, Status, no_slack};
use crate::core::Rata;
use crate::oauth::{self, Attempt, Ended, Gate};
use crate::store::{SlackLink, Store, now};
use crate::vault::{self, Unreadable};

/// The Slack app's client id, from the build. Empty or unset: no Slack in
/// this build.
pub const CLIENT_ID: Option<&str> = option_env!("RATA_SLACK_CLIENT_ID");

pub const AUTHORIZE_URL: &str = "https://slack.com/oauth/v2/authorize";
/// Every Web API method is `<API_URL>/<method>`.
pub const API_URL: &str = "https://slack.com/api";
/// The only host the browser is ever sent to for signing in.
pub const LOGIN_HOST: &str = "slack.com";

/// What RATA asks Slack for, as the signed-in person: to post, to upload
/// files, to open a direct message, and to list the channels (public and
/// private), direct messages and people to choose from. Nothing that reads
/// a message.
pub const USER_SCOPES: &str =
    "chat:write,files:write,im:write,channels:read,groups:read,im:read,users:read";

/// The loopback ports Slack sends the browser back to, each registered by
/// the owner as `http://localhost:<port>`. Fixed, because Slack matches a
/// redirect exactly; three, so one in use by another program does not stop
/// the sign-in. Below every system's range of ports handed out at random.
pub const PORTS: [u16; 3] = [28417, 28418, 28419];

/// How many channels or people one Web API page asks for (Slack suggests no
/// more than 200), and how many pages are read at most: 3 000 of each.
const PAGE: &str = "200";
const PAGES_MAX: usize = 15;

/// The most one share carries: files in all, the files, and characters of
/// text (Slack cuts a message past 40 000).
pub const SHARE_MAX: usize = crate::core::READ_MAX;
pub const SHARE_FILES_MAX: usize = 10;
pub const TEXT_MAX: usize = 40_000;

/// The longest Slack's Retry-After is waited out for before RATA gives up
/// and says when to try again. Waited once per request, never in a loop.
const WAIT_MOST: u64 = 10;

/// The most a Web API answer may be: a page of 200 people with profiles is
/// a few hundred kilobytes.
const BODY_MAX: usize = 4 * 1024 * 1024;

/// Said when no Slack workspace is connected.
pub const NOT_CONNECTED: &str =
    "Slack is not connected. Connect it in Settings, under Connected accounts.";

/// The client id, when this build has one, checked for shape: it goes into
/// a URL. Slack's are digits with a dot (`1234567890.1234567890123`).
pub fn client_id() -> Option<&'static str> {
    usable_id(CLIENT_ID)
}

fn usable_id(raw: Option<&'static str>) -> Option<&'static str> {
    raw.map(str::trim).filter(|s| {
        !s.is_empty()
            && s.len() <= 64
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    })
}

/// An id as Slack writes them (`C0123ABCD`, `U…`, `D…`, `F…`, `T…`):
/// capital letters and digits, nothing a URL or a form could be bent with.
pub fn slack_id(id: &str) -> bool {
    (2..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

// ------------------------------------------------------------------ text

/// Characters dropped from anything sent to Slack or shown from it: every
/// control but the line breaks and tabs `for_slack` keeps, and everything
/// that draws nothing (`rata_mail::names::invisible`: bidi controls,
/// zero-width spaces and joiners, the byte-order mark, tag characters…),
/// except the variation selectors, which only pick how an emoji is drawn.
fn dropped(c: char) -> bool {
    (c.is_control() || rata_mail::names::invisible(c)) && !('\u{fe00}'..='\u{fe0f}').contains(&c)
}

/// Text as Slack must be sent it (K3). Slack reads `<!channel>`, `<!here>`,
/// `<@U…>`, `<#C…>` and `<https://x|label>` in a message as live mentions
/// and links, so `&`, `<` and `>` go as `&amp;`, `&lt;` and `&gt;` (`&`
/// first, which one pass over the text does by construction); and bidi
/// controls and zero-width characters are taken out, so a mail's text can
/// neither ping a whole channel nor show one address while linking to
/// another. Line breaks become `\n`.
pub fn for_slack(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\n' | '\t' => out.push(c),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if dropped(c) => {}
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

/// A name Slack gave (a workspace, a channel, a person), made one plain
/// line of at most `most` characters for the page, which draws it as text.
pub fn plain(name: &str, most: usize) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_whitespace() || c.is_control() {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
        } else if !dropped(c) {
            out.push(c);
        }
    }
    out.trim()
        .chars()
        .take(most)
        .collect::<String>()
        .trim()
        .to_string()
}

// ---------------------------------------------------------------- tokens

/// Slack's tokens for the signed-in person: the access token, the refresh
/// token that renews it (none for an app without rotation), and when the
/// access token stops working (seconds since 1970; none if it does not).
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_at: Option<u64>,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"<hidden>")
            .field("refresh", &self.refresh.as_ref().map(|_| "<hidden>"))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// A token as Slack issues them: printable ASCII, no spaces, not absurdly
/// long. Anything else is not kept and not sent.
fn token_ok(t: &str) -> bool {
    !t.is_empty() && t.len() <= 4096 && t.bytes().all(|b| b.is_ascii_graphic())
}

impl Tokens {
    /// As kept in the keychain.
    pub fn to_secret(&self) -> String {
        serde_json::json!({ "a": self.access, "r": self.refresh, "x": self.expires_at }).to_string()
    }

    pub fn from_secret(s: &str) -> Option<Tokens> {
        let v: Value = serde_json::from_str(s).ok()?;
        let access = v.get("a")?.as_str().filter(|t| token_ok(t))?.to_string();
        let refresh = match v.get("r") {
            None | Some(Value::Null) => None,
            Some(r) => Some(r.as_str().filter(|t| token_ok(t))?.to_string()),
        };
        let expires_at = v.get("x").and_then(Value::as_u64);
        Some(Tokens {
            access,
            refresh,
            expires_at,
        })
    }

    /// Whether the access token can still be used, with `oauth::EARLY`
    /// seconds in hand.
    pub fn fresh(&self, now: u64) -> bool {
        self.expires_at
            .is_none_or(|x| x > now.saturating_add(oauth::EARLY))
    }

    fn secrets(&self) -> Vec<&str> {
        let mut out = vec![self.access.as_str()];
        if let Some(r) = self.refresh.as_deref() {
            out.push(r);
        }
        out
    }
}

/// What connecting brought back: the tokens, the workspace and who signed
/// in.
#[derive(Debug, Clone)]
pub struct Granted {
    pub tokens: Tokens,
    pub team_id: String,
    pub team: String,
    pub user_id: String,
}

/// Why a Slack request did not succeed. Each needs a different answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The access token has run out (`token_expired`): renew it and try
    /// once more.
    Expired,
    /// The sign-in is no longer good (revoked, the person deactivated, the
    /// refresh token spent or 30 days old). Only connecting again fixes it,
    /// so nothing more is sent with it.
    Revoked(String),
    /// Slack asked RATA to slow down: wait this many seconds.
    Limited(u64),
    /// Slack could not be reached, or is having a bad morning. Nothing is
    /// wrong with the sign-in.
    Net(String),
    /// Refused for another reason: the refusal's kind and a sentence.
    Refused(&'static str, String),
}

/// One error code Slack sent, as a sentence and the answer it needs. The
/// code is the only part of Slack's answer that is used, and only when it
/// looks like one (`oauth::error_code`).
pub fn classify(code: &str) -> Fail {
    let code = oauth::error_code(code);
    match code.as_str() {
        "token_expired" => Fail::Expired,
        "invalid_auth" | "not_authed" | "account_inactive" | "token_revoked"
        | "invalid_refresh_token" | "invalid_grant" | "user_removed_from_team"
        | "team_disabled" => Fail::Revoked(format!("Slack ended RATA's sign-in ({code}).")),
        "ratelimited" => Fail::Limited(30),
        "internal_error" | "fatal_error" | "service_unavailable" | "request_timeout" => {
            Fail::Net(format!(
                "Slack is not answering properly right now ({code}). Try again in a minute."
            ))
        }
        "missing_scope" | "not_allowed_token_type" => Fail::Refused(
            "refused",
            "RATA's connection to Slack is missing a permission it needs. Disconnect Slack in Settings and connect it again."
                .into(),
        ),
        "channel_not_found" | "not_in_channel" | "user_not_found" | "user_not_visible" => {
            Fail::Refused(
                "not-found",
                "Slack says you are not in that conversation, or it no longer exists.".into(),
            )
        }
        "is_archived" => Fail::Refused("refused", "That channel is archived in Slack.".into()),
        "msg_too_long" => Fail::Refused(
            "too-large",
            "The message is too long for Slack. Shorten it and try again.".into(),
        ),
        "restricted_action"
        | "restricted_action_read_only_channel"
        | "restricted_action_thread_only_channel"
        | "restricted_action_non_threadable_channel"
        | "posting_to_general_channel_denied"
        | "cannot_dm_bot"
        | "ekm_access_denied"
        | "team_access_not_granted" => Fail::Refused(
            "refused",
            "Slack's settings for that workspace or conversation do not let you post there."
                .into(),
        ),
        _ => Fail::Refused("refused", format!("Slack refused it ({code}).")),
    }
}

fn refused(kind: &'static str, error: impl Into<String>) -> Refusal {
    Refusal::new(Service::Slack, kind, error)
}

/// A failure that is not about the sign-in, as the page is told it.
fn refusal(f: Fail) -> Refusal {
    match f {
        Fail::Expired | Fail::Revoked(_) => refused("not-connected", NOT_CONNECTED),
        Fail::Limited(s) => refused(
            "offline",
            format!(
                "Slack asked RATA to wait before sending more. Try again in {} seconds.",
                s.clamp(1, 3600)
            ),
        ),
        Fail::Net(why) => refused("offline", why),
        Fail::Refused(kind, why) => refused(kind, why),
    }
}

/// Said when Slack has ended the sign-in to `team`.
fn connect_again(team: &str) -> String {
    format!("Slack signed RATA out of {team}. Connect Slack again in Settings to share there.")
}

// ------------------------------------------------------------ in memory

/// What RATA holds for Share to Slack while it runs: the build's client id,
/// the sign-in in progress, the tokens, and the names last listed.
pub struct Slack {
    pub client_id: Option<String>,
    authorize_url: String,
    api: String,
    ports: Vec<u16>,
    /// Tests only: no proxy, and upload addresses on 127.0.0.1 over http.
    local: bool,
    /// How long one second of Retry-After is (shorter in tests).
    pub(crate) second: Duration,
    http: OnceLock<Result<reqwest::Client, String>>,
    gate: Gate,
    pub(crate) gap: Duration,
    /// Bumped by every connect, disconnect and Delete account, under the
    /// list's lock: what a sign-in or a refresh began under, and must still
    /// be when it writes.
    generation: AtomicU64,
    held: Mutex<Option<(u64, Tokens)>>,
    /// Held while a refresh is under way: two at once would spend the same
    /// refresh token, and the loser would read as revoked.
    refreshing: tokio::sync::Mutex<()>,
    /// Channel and person names by id, from the last listing, for saying
    /// where something was shared.
    names: Mutex<HashMap<String, String>>,
}

impl Slack {
    /// As this build was made.
    pub fn from_build() -> Slack {
        Slack::new(
            client_id().map(String::from),
            AUTHORIZE_URL,
            API_URL,
            &PORTS,
            false,
        )
    }

    /// A build without Slack.
    #[cfg(test)]
    pub fn off() -> Slack {
        Slack::new(None, AUTHORIZE_URL, API_URL, &PORTS, false)
    }

    pub fn new(
        client_id: Option<String>,
        authorize: &str,
        api: &str,
        ports: &[u16],
        local: bool,
    ) -> Slack {
        Slack {
            client_id,
            authorize_url: authorize.into(),
            api: api.trim_end_matches('/').into(),
            ports: ports.to_vec(),
            local,
            second: Duration::from_secs(1),
            http: OnceLock::new(),
            gate: Gate::default(),
            gap: oauth::BEGIN_GAP,
            generation: AtomicU64::new(0),
            held: Mutex::new(None),
            refreshing: tokio::sync::Mutex::new(()),
            names: Mutex::new(HashMap::new()),
        }
    }

    pub fn configured(&self) -> bool {
        self.client_id.is_some()
    }

    fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .get_or_init(|| oauth::client_for(!self.local, "Slack"))
            .as_ref()
            .map_err(Clone::clone)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// A new generation: whatever began under the last one writes nothing.
    /// Only under the list's lock.
    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn held(&self, generation: u64) -> Option<Tokens> {
        let held = self.held.lock().ok()?;
        held.as_ref()
            .filter(|(g, _)| *g == generation)
            .map(|(_, t)| t.clone())
    }

    fn hold(&self, generation: u64, tokens: Tokens) {
        if let Ok(mut held) = self.held.lock() {
            *held = Some((generation, tokens));
        }
    }

    fn drop_held(&self) {
        if let Ok(mut held) = self.held.lock() {
            *held = None;
        }
        if let Ok(mut names) = self.names.lock() {
            names.clear();
        }
    }

    fn name_of(&self, id: &str) -> Option<String> {
        self.names.lock().ok()?.get(id).cloned()
    }

    /// Start a sign-in: one at a time, and not one straight after another.
    pub fn begin(&self) -> Result<Arc<Notify>, String> {
        self.gate.begin(self.gap, "Slack")
    }

    /// Stop the sign-in waiting for the browser, if there is one.
    pub fn cancel(&self) -> bool {
        self.gate.cancel()
    }

    fn finish(&self, which: &Arc<Notify>) {
        self.gate.finish(which)
    }

    /// The listener Slack sends the browser back to: the first of the
    /// registered ports this computer lets RATA listen on.
    async fn listen(&self) -> Result<(TcpListener, u16), String> {
        for &port in &self.ports {
            if let Ok(l) = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
                && let Ok(addr) = l.local_addr()
            {
                return Ok((l, addr.port()));
            }
        }
        let ports: Vec<String> = self.ports.iter().map(u16::to_string).collect();
        Err(format!(
            "RATA could not get ready to hear back from Slack: another program is using ports {}. Close it and try again.",
            ports.join(", ")
        ))
    }

    /// Slack's page for this attempt.
    pub fn authorize_link(&self, client_id: &str, attempt: &Attempt) -> String {
        url::Url::parse_with_params(
            &self.authorize_url,
            &[
                ("client_id", client_id),
                // No bot: a desktop redirect may ask for user scopes only.
                ("scope", ""),
                ("user_scope", USER_SCOPES),
                ("redirect_uri", &attempt.redirect_uri),
                ("state", &attempt.state),
                ("code_challenge", &attempt.code_challenge()),
                ("code_challenge_method", "S256"),
            ],
        )
        .map(String::from)
        .unwrap_or_default()
    }

    /// One Web API method, as a form, with the token as a bearer token when
    /// there is one. Slack's own refusal comes back as a [`Fail`]; nothing
    /// in a sentence is ever a token.
    async fn call(
        &self,
        method: &str,
        token: Option<&str>,
        form: &[(&str, &str)],
    ) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        let mut req = http
            .post(format!("{}/{method}", self.api))
            .header(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .header("Accept", "application/json")
            .body(body);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let unreachable = |e: reqwest::Error| {
            Fail::Net(format!("Slack could not be reached: {}", e.without_url()))
        };
        let mut resp = req.send().await.map_err(unreachable)?;
        let status = resp.status().as_u16();
        let wait = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let mut got = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(unreachable)? {
            got.extend_from_slice(&chunk);
            if got.len() > BODY_MAX {
                return Err(Fail::Refused(
                    "refused",
                    "Slack sent far more than RATA asked for.".into(),
                ));
            }
        }
        if status == 429 {
            return Err(Fail::Limited(wait.unwrap_or(30)));
        }
        let Ok(v) = serde_json::from_slice::<Value>(&got) else {
            return Err(if status >= 500 {
                Fail::Net(format!(
                    "Slack is not answering properly right now (status {status}). Try again in a minute."
                ))
            } else {
                Fail::Refused(
                    "refused",
                    format!("Slack sent an answer RATA could not read (status {status})."),
                )
            });
        };
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(v);
        }
        match v.get("error").and_then(Value::as_str) {
            Some(code) => Err(match classify(code) {
                Fail::Limited(s) => Fail::Limited(wait.unwrap_or(s)),
                other => other,
            }),
            None if status >= 500 => Err(Fail::Net(format!(
                "Slack is not answering properly right now (status {status}). Try again in a minute."
            ))),
            None => Err(Fail::Refused(
                "refused",
                format!("Slack refused it (status {status})."),
            )),
        }
    }

    /// [`Slack::call`], waiting out one short Retry-After. Slack has done
    /// nothing with a request it answered `ratelimited`, so sending it again
    /// cannot post twice. A longer wait is said, not waited.
    async fn call_patient(
        &self,
        method: &str,
        token: Option<&str>,
        form: &[(&str, &str)],
    ) -> Result<Value, Fail> {
        match self.call(method, token, form).await {
            Err(Fail::Limited(s)) if s <= WAIT_MOST => {
                tokio::time::sleep(self.second * (s.max(1) as u32)).await;
                self.call(method, token, form).await
            }
            other => other,
        }
    }

    /// Trade the code the browser brought back for tokens.
    pub async fn exchange(
        &self,
        client_id: &str,
        code: &str,
        attempt: &Attempt,
    ) -> Result<Granted, Fail> {
        let got = self
            .call_patient(
                "oauth.v2.access",
                None,
                &[
                    ("client_id", client_id),
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("code_verifier", attempt.verifier()),
                    ("redirect_uri", &attempt.redirect_uri),
                ],
            )
            .await
            .map_err(|f| match f {
                // A code already used, or a verifier that does not match:
                // this sign-in failed, nothing to park.
                Fail::Revoked(why) | Fail::Refused(_, why) => {
                    Fail::Refused("refused", oauth::scrub(&why, &[code, attempt.verifier()]))
                }
                other => other,
            })?;
        read_granted(&got)
    }

    /// Fresh tokens from the refresh token, which this spends.
    pub async fn renew(&self, client_id: &str, refresh: &str) -> Result<Tokens, Fail> {
        let got = self
            .call_patient(
                "oauth.v2.access",
                None,
                &[
                    ("client_id", client_id),
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh),
                ],
            )
            .await
            .map_err(|f| match f {
                Fail::Expired => Fail::Revoked("Slack's sign-in has expired.".into()),
                other => other,
            })?;
        // Slack answers a refresh at the top level; read `authed_user` too,
        // as the first exchange answers.
        let from = if got.get("access_token").is_some() {
            &got
        } else {
            got.get("authed_user").unwrap_or(&got)
        };
        read_tokens(from, now())
            .ok_or_else(|| Fail::Refused("refused", "Slack's answer had no sign-in in it.".into()))
    }

    /// Send a file's bytes to the address `files.getUploadURLExternal`
    /// gave, which is Slack's own file host over https and nowhere else.
    /// The token does not go with them: the address carries its own
    /// permission.
    async fn upload(&self, to: &str, bytes: Vec<u8>) -> Result<(), Fail> {
        let url = url::Url::parse(to)
            .ok()
            .filter(|u| self.upload_ok(u))
            .ok_or_else(|| {
                Fail::Refused(
                    "refused",
                    "Slack named somewhere to upload the file that is not Slack's own, so RATA did not send it.".into(),
                )
            })?;
        let http = self.http().map_err(Fail::Net)?;
        let wait = Duration::from_secs(60 + (bytes.len() / (64 * 1024)) as u64);
        let unreachable = |e: reqwest::Error| {
            Fail::Net(format!(
                "The file could not be sent to Slack: {}",
                e.without_url()
            ))
        };
        let resp = http
            .post(url)
            .header("Content-Type", "application/octet-stream")
            .timeout(wait)
            .body(bytes)
            .send()
            .await
            .map_err(unreachable)?;
        let status = resp.status().as_u16();
        match status {
            200..=299 => Ok(()),
            429 => Err(Fail::Limited(30)),
            500.. => Err(Fail::Net(format!(
                "Slack did not take the file (status {status}). Try again in a minute."
            ))),
            _ => Err(Fail::Refused(
                "refused",
                format!("Slack did not take the file (status {status})."),
            )),
        }
    }

    fn upload_ok(&self, u: &url::Url) -> bool {
        if !u.username().is_empty() || u.password().is_some() {
            return false;
        }
        let host = u.host_str().unwrap_or("");
        if self.local && u.scheme() == "http" && host == "127.0.0.1" {
            return true;
        }
        u.scheme() == "https"
            && u.port().is_none()
            && (host == "slack.com" || host.ends_with(".slack.com"))
    }
}

/// Tokens as Slack writes them in `obj`: `access_token`, `refresh_token`
/// and `expires_in` (seconds; 43 200 for a rotating user token).
fn read_tokens(obj: &Value, now: u64) -> Option<Tokens> {
    if obj
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|t| !t.eq_ignore_ascii_case("user"))
    {
        return None;
    }
    let access = obj
        .get("access_token")?
        .as_str()
        .filter(|t| token_ok(t))?
        .to_string();
    let refresh = obj
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|t| token_ok(t))
        .map(String::from);
    let expires_at = obj
        .get("expires_in")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok()))
        .map(|s| oauth::expiry(now, s));
    Some(Tokens {
        access,
        refresh,
        expires_at,
    })
}

/// What `oauth.v2.access` answered the first time: the person's tokens
/// (in `authed_user`), the workspace, and that `chat:write` was granted.
pub fn read_granted(v: &Value) -> Result<Granted, Fail> {
    let bad = |w: &str| Fail::Refused("refused", w.to_string());
    let user = v
        .get("authed_user")
        .ok_or_else(|| bad("Slack connected, but gave RATA no sign-in for you."))?;
    let tokens = read_tokens(user, now())
        .ok_or_else(|| bad("Slack connected, but gave RATA no sign-in for you."))?;
    let user_id = user
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| slack_id(id))
        .ok_or_else(|| bad("Slack connected, but did not say who signed in."))?
        .to_string();
    let scopes: Vec<&str> = user
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .collect();
    if !scopes.contains(&"chat:write") {
        return Err(bad(
            "Slack connected without letting RATA post for you, so Share to Slack could not work. Nothing was kept.",
        ));
    }
    let team = v
        .get("team")
        .ok_or_else(|| bad("Slack connected, but did not say to which workspace."))?;
    let team_id = team
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| slack_id(id))
        .ok_or_else(|| bad("Slack connected, but did not say to which workspace."))?
        .to_string();
    let name = plain(team.get("name").and_then(Value::as_str).unwrap_or(""), 80);
    Ok(Granted {
        tokens,
        team: if name.is_empty() {
            "your Slack workspace".into()
        } else {
            name
        },
        team_id,
        user_id,
    })
}

/// Open Slack's sign-in page in the customer's browser, through the same
/// rules as every other link, and only if it is Slack's page over https.
pub fn open_sign_in(url: &str) -> Result<(), String> {
    match crate::links::classify(url) {
        Some(crate::links::Link::Web(u))
            if u.scheme() == "https" && u.host_str() == Some(LOGIN_HOST) =>
        {
            crate::links::open_in_browser(&u)
        }
        _ => Err("RATA would only open Slack's own sign-in page, and this was not it.".into()),
    }
}

// --------------------------------------------------------------- targets

/// Somewhere to send to, as the page lists it: a channel by its name
/// (without `#`), or a person by theirs. A person's id is their direct
/// message's when there is one, else their own (opened when sent to).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
}

/// The channels the person is in and the people they can write to, from
/// one listing of each: channels first, then people, each by name. Deleted
/// people, bots and Slackbot are left out; `me` is named "(you)".
pub fn targets_from(convs: &[Value], people: &[Value], me: &str) -> Vec<Target> {
    let truthy = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool) == Some(true);
    let mut ims: HashMap<String, String> = HashMap::new();
    let mut channels = Vec::new();
    for c in convs {
        let Some(id) = c.get("id").and_then(Value::as_str).filter(|i| slack_id(i)) else {
            continue;
        };
        if truthy(c, "is_im") {
            if let Some(user) = c
                .get("user")
                .and_then(Value::as_str)
                .filter(|u| slack_id(u))
                && !truthy(c, "is_user_deleted")
            {
                ims.insert(user.to_string(), id.to_string());
            }
            continue;
        }
        if truthy(c, "is_archived") || truthy(c, "is_mpim") {
            continue;
        }
        // A public channel is listed whether or not the person is in it; a
        // private one only when they are, and says so.
        let member = match c.get("is_member").and_then(Value::as_bool) {
            Some(m) => m,
            None => truthy(c, "is_private") || truthy(c, "is_group"),
        };
        if !member {
            continue;
        }
        let name = plain(c.get("name").and_then(Value::as_str).unwrap_or(""), 80);
        if name.is_empty() {
            continue;
        }
        channels.push(Target {
            id: id.to_string(),
            name,
            kind: "channel",
        });
    }
    let mut persons = Vec::new();
    for p in people {
        let Some(id) = p.get("id").and_then(Value::as_str).filter(|i| slack_id(i)) else {
            continue;
        };
        if truthy(p, "deleted")
            || truthy(p, "is_bot")
            || truthy(p, "is_app_user")
            || id == "USLACKBOT"
        {
            continue;
        }
        let profile = p.get("profile");
        let pick = |v: Option<&Value>, k: &str| {
            v.and_then(|v| v.get(k))
                .and_then(Value::as_str)
                .map(|s| plain(s, 80))
                .filter(|s| !s.is_empty())
        };
        let Some(mut name) = pick(profile, "display_name")
            .or_else(|| pick(profile, "real_name"))
            .or_else(|| pick(Some(p), "real_name"))
            .or_else(|| pick(Some(p), "name"))
        else {
            continue;
        };
        if id == me {
            name.push_str(" (you)");
        }
        persons.push(Target {
            id: ims.get(id).cloned().unwrap_or_else(|| id.to_string()),
            name,
            kind: "person",
        });
    }
    let by_name = |a: &Target, b: &Target| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    };
    channels.sort_by(by_name);
    persons.sort_by(by_name);
    channels.extend(persons);
    channels
}

/// A file to share, as the page sent it: its name, and the bytes it read
/// (an attachment through `read_attachment`, a document in Files, a file
/// made with Create file through `created_read`).
#[derive(Debug, Clone)]
pub struct ShareFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Where something was shared, for the page's toast: "#general in Acme".
/// Empty when RATA has not listed that target, and the page names it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Shared {
    #[serde(rename = "where")]
    pub place: String,
}

// ------------------------------------------------------------------ Rata

impl Rata {
    /// Share to Slack in the Settings list.
    pub(crate) fn slack_status(&self, pro: bool) -> Status {
        let mut s = Status {
            service: Service::Slack.key(),
            label: Service::Slack.label(),
            kind: Service::Slack.kind(),
            connected: false,
            account: None,
            note: None,
            available: self.slack.configured(),
        };
        if !(pro && s.available) {
            return s;
        }
        let link = self.store().lock().ok().and_then(|st| st.slack().cloned());
        if let Some(l) = link {
            if l.parked_at.is_some() {
                s.note = Some(connect_again(&l.team));
            } else {
                s.connected = true;
                s.account = Some(l.team);
            }
        }
        s
    }

    /// Licensed for Pro, in a build with Slack, with a workspace connected
    /// that Slack has not signed out.
    fn slack_ready(&self) -> Result<SlackLink, Refusal> {
        if !self.slack.configured() {
            return Err(no_slack());
        }
        self.may_connect(Service::Slack)?;
        let link = self
            .store()
            .lock()
            .ok()
            .and_then(|s| s.slack().cloned())
            .ok_or_else(|| refused("not-connected", NOT_CONNECTED))?;
        if link.parked_at.is_some() {
            return Err(refused("not-connected", connect_again(&link.team)));
        }
        Ok(link)
    }

    /// Whether `link`, as it was when something began under `generation`,
    /// is still the workspace connected. Under the list's lock.
    fn slack_still(&self, store: &Store, generation: u64, link: &SlackLink) -> bool {
        self.slack.generation() == generation
            && store.slack().is_some_and(|l| {
                l.connected_at == link.connected_at
                    && l.team_id == link.team_id
                    && l.parked_at.is_none()
            })
    }

    /// Slack ended the sign-in: nothing more is sent with it, and Settings
    /// says to connect again. Only while `link` is still the one connected,
    /// so a refusal never parks a workspace connected again meanwhile.
    fn park_slack(&self, generation: u64, link: &SlackLink) -> Refusal {
        if let Ok(mut store) = self.store().lock()
            && self.slack_still(&store, generation, link)
        {
            self.slack.drop_held();
            store.park_slack(now());
            let _ = store.save();
        }
        refused("not-connected", connect_again(&link.team))
    }

    /// Connect Slack: sign in in the browser, and keep the workspace only if
    /// nothing changed meanwhile. `open` shows the customer Slack's page
    /// (`open_sign_in` in the app).
    pub async fn connect_slack<O>(&self, open: O) -> Result<Connected, Refusal>
    where
        O: FnOnce(&str) -> Result<(), String>,
    {
        let Some(client_id) = self.slack.client_id.clone() else {
            return Err(no_slack());
        };
        self.may_connect(Service::Slack)?;
        // Read before the browser opens: a Disconnect or a Delete account
        // while the customer signs in means nothing is kept.
        let began = (self.epoch(), self.slack.generation());
        let failed = |e: String| refused("refused", e);
        let (listener, port) = self.slack.listen().await.map_err(failed)?;
        let attempt = Attempt::with_redirect(format!("http://localhost:{port}")).map_err(failed)?;
        let link = self.slack.authorize_link(&client_id, &attempt);
        // Nothing may return between `begin` and `finish`.
        let cancel = self.slack.begin().map_err(failed)?;
        let got: Result<Granted, Option<Refusal>> = async {
            open(&link).map_err(|e| Some(failed(e)))?;
            let code = oauth::wait_for_code_from(listener, &attempt, &cancel, "Slack")
                .await
                .map_err(|e| match e {
                    Ended::Cancelled => None,
                    Ended::TimedOut => Some(failed(
                        "RATA stopped waiting for Slack after five minutes. Try again when you are ready."
                            .into(),
                    )),
                    Ended::Denied => Some(failed(
                        "Connecting Slack was cancelled, so nothing was kept.".into(),
                    )),
                    Ended::Failed(why) => Some(failed(format!(
                        "Slack could not connect: {why}"
                    ))),
                })?;
            self.slack
                .exchange(&client_id, &code, &attempt)
                .await
                .map_err(|f| {
                    Some(match f {
                        Fail::Refused(_, why) => {
                            failed(format!("Slack did not finish connecting: {why}"))
                        }
                        other => refusal(other),
                    })
                })
        }
        .await;
        self.slack.finish(&cancel);
        match got {
            Ok(granted) => self.keep_slack(granted, began).map(Connected::Done),
            Err(None) => Ok(Connected::Cancelled { cancelled: true }),
            Err(Some(r)) => Err(r),
        }
    }

    /// Keep a workspace whose sign-in has just come back: the tokens in the
    /// keychain, then the workspace in the store, all under the list's lock
    /// and only if no Disconnect or Delete account ran since `began` (the
    /// epoch and the Slack generation read when it began) and the licence
    /// still allows it.
    fn keep_slack(&self, granted: Granted, began: (u64, u64)) -> Result<Status, Refusal> {
        let mut store = self
            .store()
            .lock()
            .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
        if self.epoch() != began.0 || self.slack.generation() != began.1 {
            return Err(refused(
                "refused",
                "Slack was disconnected, or Delete account ran, while you were signing in, so nothing was kept.",
            ));
        }
        if !self.connect_allowed_in(&store) {
            return Err(refused("plan", crate::cloud::NEED_PRO));
        }
        vault::put_slack_secret(self.vault(), &granted.tokens.to_secret())
            .map_err(|e| refused("disk", e))?;
        store.set_slack(Some(SlackLink {
            team_id: granted.team_id,
            team: granted.team,
            user_id: granted.user_id,
            connected_at: now(),
            parked_at: None,
        }));
        if let Err(e) = store.save() {
            // The workspace the saved file names, if any, finds no sign-in
            // and asks to be connected again.
            store.set_slack(None);
            let _ = vault::forget_slack_secret(self.vault());
            self.slack.bump();
            self.slack.drop_held();
            return Err(refused(
                "disk",
                format!("RATA could not save its settings ({}).", e.kind()),
            ));
        }
        self.slack.bump();
        self.slack.drop_held();
        self.slack.hold(self.slack.generation(), granted.tokens);
        drop(store);
        Ok(self.slack_status(true))
    }

    /// Stop a Slack sign-in waiting for the browser.
    pub fn cancel_slack(&self) -> bool {
        self.slack.cancel()
    }

    /// Disconnect Slack, on any plan: its keychain entries first, then the
    /// workspace in the store, under the list's lock, so a sign-in or a
    /// refresh still in flight finds the generation moved and writes
    /// nothing. A keychain that will not let go leaves Slack connected, with
    /// the reason. Answers the entry, and the tokens that were held, for
    /// `revoke_slack`.
    pub fn disconnect_slack(&self) -> Result<(Status, Option<Tokens>), Refusal> {
        self.slack.cancel();
        let held;
        {
            let mut store = self
                .store()
                .lock()
                .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
            held = self.slack.held(self.slack.generation()).or_else(|| {
                vault::get_slack_secret(self.vault())
                    .ok()
                    .and_then(|s| Tokens::from_secret(&s))
            });
            vault::forget_slack_secret(self.vault()).map_err(|e| {
                refused(
                    "disk",
                    format!(
                        "RATA could not remove its Slack sign-in from this computer's keychain, so Slack is still connected: {e}"
                    ),
                )
            })?;
            self.slack.bump();
            self.slack.drop_held();
            store.set_slack(None);
            store.save().map_err(|e| {
                refused(
                    "disk",
                    format!("RATA could not save its settings ({}).", e.kind()),
                )
            })?;
        }
        let pro = self.standing().plan.is_some_and(|p| p.connect);
        Ok((self.slack_status(pro), held))
    }

    /// After Disconnect: ask Slack to end the sign-in too, so the tokens
    /// RATA just forgot stop working everywhere. Best effort; the
    /// disconnect has already happened on this computer.
    pub async fn revoke_slack(&self, tokens: Tokens) {
        let _ = self
            .slack
            .call("auth.revoke", Some(&tokens.access), &[])
            .await;
    }

    /// Delete account's part (`forget_everything`), under the list's lock
    /// it already holds: the keychain entries, the workspace, what is held
    /// in memory, and a new generation. A keychain that will not let go is
    /// an error only when a workspace is connected, so a computer that never
    /// used Slack is not stopped by a keychain it never needed.
    pub(crate) fn forget_slack_in(&self, store: &mut Store) -> Result<(), String> {
        let forgot = vault::forget_slack_secret(self.vault());
        if store.slack().is_some() {
            forgot?;
        }
        self.slack.bump();
        self.slack.drop_held();
        store.set_slack(None);
        Ok(())
    }

    /// An access token for `link`, renewed when it is near its end, or when
    /// Slack refused `stale` for having run out. With whether it is new.
    async fn slack_token(
        &self,
        link: &SlackLink,
        stale: Option<&str>,
    ) -> Result<(String, bool), Refusal> {
        let _one = self.slack.refreshing.lock().await;
        let generation = self.slack.generation();
        let tokens = match self.slack.held(generation) {
            Some(t) => t,
            None => match vault::get_slack_secret(self.vault()) {
                Ok(s) => match Tokens::from_secret(&s) {
                    Some(t) => {
                        self.slack.hold(generation, t.clone());
                        t
                    }
                    None => return Err(self.park_slack(generation, link)),
                },
                Err(Unreadable::Missing(_)) => return Err(self.park_slack(generation, link)),
                Err(Unreadable::Locked(why)) => return Err(refused("refused", why)),
            },
        };
        if tokens.fresh(now()) && stale != Some(tokens.access.as_str()) {
            return Ok((tokens.access, stale.is_some()));
        }
        let (Some(refresh), Some(client_id)) =
            (tokens.refresh.clone(), self.slack.client_id.clone())
        else {
            return Err(self.park_slack(generation, link));
        };
        match self.slack.renew(&client_id, &refresh).await {
            Ok(mut next) => {
                if next.refresh.is_none() {
                    next.refresh = Some(refresh.clone());
                }
                // Kept only while the workspace is still the one this began
                // for: Disconnect or Delete account may have emptied the
                // keychain while Slack answered, and writing then would
                // leave a sign-in nothing in RATA could remove.
                let store = self
                    .store()
                    .lock()
                    .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
                if !self.slack_still(&store, generation, link) {
                    return Err(refused(
                        "not-connected",
                        "Slack was disconnected while RATA was renewing its sign-in, so nothing was kept.",
                    ));
                }
                // The refresh token just spent no longer works, so the new
                // one is held in memory even when the keychain will not
                // take it: this session goes on, and the next asks for
                // Slack to be connected again.
                let _ = vault::put_slack_secret(self.vault(), &next.to_secret());
                self.slack.hold(generation, next.clone());
                drop(store);
                Ok((next.access, true))
            }
            Err(Fail::Revoked(_) | Fail::Expired) => Err(self.park_slack(generation, link)),
            Err(other) => Err(refusal(hide(other, &tokens))),
        }
    }

    /// One Web API method with the workspace's token: renewed and tried
    /// once more if Slack says it ran out, and the sign-in parked if Slack
    /// has ended it.
    async fn slack_api(
        &self,
        link: &SlackLink,
        method: &str,
        form: &[(&str, &str)],
    ) -> Result<Value, Refusal> {
        let generation = self.slack.generation();
        let (token, new) = self.slack_token(link, None).await?;
        let hidden = |f: Fail, t: &str| match f {
            Fail::Net(w) => Fail::Net(oauth::scrub(&w, &[t])),
            Fail::Refused(k, w) => Fail::Refused(k, oauth::scrub(&w, &[t])),
            other => other,
        };
        match self.slack.call_patient(method, Some(&token), form).await {
            Ok(v) => Ok(v),
            Err(Fail::Expired) if !new => {
                let (token, _) = self.slack_token(link, Some(&token)).await?;
                match self.slack.call_patient(method, Some(&token), form).await {
                    Ok(v) => Ok(v),
                    Err(Fail::Expired | Fail::Revoked(_)) => Err(self.park_slack(generation, link)),
                    Err(e) => Err(refusal(hidden(e, &token))),
                }
            }
            Err(Fail::Expired | Fail::Revoked(_)) => Err(self.park_slack(generation, link)),
            Err(e) => Err(refusal(hidden(e, &token))),
        }
    }

    /// Every page of a listing method, as far as [`PAGES_MAX`] pages.
    async fn slack_pages(
        &self,
        link: &SlackLink,
        method: &str,
        form: &[(&str, &str)],
        field: &str,
    ) -> Result<Vec<Value>, Refusal> {
        let mut out = Vec::new();
        let mut cursor = String::new();
        for _ in 0..PAGES_MAX {
            let mut ask: Vec<(&str, &str)> = form.to_vec();
            ask.push(("limit", PAGE));
            if !cursor.is_empty() {
                ask.push(("cursor", &cursor));
            }
            let page = self.slack_api(link, method, &ask).await?;
            if let Some(items) = page.get(field).and_then(Value::as_array) {
                out.extend(items.iter().cloned());
            }
            let next = page
                .pointer("/response_metadata/next_cursor")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if next.is_empty() || next.len() > 1024 || next == cursor {
                break;
            }
            cursor = next;
        }
        Ok(out)
    }

    /// Where the customer can share to: the channels they are in and the
    /// people of the workspace.
    pub async fn slack_targets(&self) -> Result<Vec<Target>, Refusal> {
        let link = self.slack_ready()?;
        let convs = self
            .slack_pages(
                &link,
                "conversations.list",
                &[
                    ("types", "public_channel,private_channel,im"),
                    ("exclude_archived", "true"),
                ],
                "channels",
            )
            .await?;
        let people = self
            .slack_pages(&link, "users.list", &[], "members")
            .await?;
        let targets = targets_from(&convs, &people, &link.user_id);
        if let Ok(mut names) = self.slack.names.lock() {
            names.clear();
            for t in &targets {
                let shown = if t.kind == "channel" {
                    format!("#{}", t.name)
                } else {
                    t.name.clone()
                };
                names.insert(t.id.clone(), shown);
            }
        }
        Ok(targets)
    }

    /// Share `text`, and `files` with it, to `target`: a channel, a direct
    /// message, or a person (whose direct message is opened). Text alone is
    /// a message; with files, the files go up first and the text is posted
    /// with them. Everything is checked before anything is sent.
    pub async fn slack_share(
        &self,
        target: &str,
        text: &str,
        files: Vec<ShareFile>,
    ) -> Result<Shared, Refusal> {
        let link = self.slack_ready()?;
        let target = target.trim();
        if !slack_id(target) || !matches!(target.as_bytes()[0], b'C' | b'G' | b'D' | b'U' | b'W') {
            return Err(refused(
                "not-found",
                "RATA cannot send there. Choose a channel or a person from the list.",
            ));
        }
        let text = for_slack(text);
        if text.chars().count() > TEXT_MAX {
            return Err(refused(
                "too-large",
                "The message is too long for Slack. Shorten it and try again.",
            ));
        }
        if files.len() > SHARE_FILES_MAX {
            return Err(refused(
                "too-large",
                format!("Share at most {SHARE_FILES_MAX} files at a time."),
            ));
        }
        let total: usize = files.iter().map(|f| f.bytes.len()).sum();
        if total > SHARE_MAX {
            return Err(refused(
                "too-large",
                format!(
                    "Those files are too large to share from RATA together (the most is {} MB).",
                    SHARE_MAX / (1024 * 1024)
                ),
            ));
        }
        let mut named = Vec::with_capacity(files.len());
        for f in files {
            let name = safe_file_name(&f.name);
            if looks_disguised(&f.name) || looks_disguised(&name) {
                return Err(refused(
                    "refused",
                    format!(
                        "{name} is a program named to look like a document, so RATA will not share it."
                    ),
                ));
            }
            if f.bytes.is_empty() {
                return Err(refused("refused", format!("{name} is empty.")));
            }
            named.push((name, f.bytes));
        }
        if text.is_empty() && named.is_empty() {
            return Err(refused(
                "refused",
                "Write a message or choose a file to send.",
            ));
        }

        let channel = if matches!(target.as_bytes()[0], b'U' | b'W') {
            let opened = self
                .slack_api(&link, "conversations.open", &[("users", target)])
                .await?;
            opened
                .pointer("/channel/id")
                .and_then(Value::as_str)
                .filter(|id| slack_id(id))
                .ok_or_else(|| refused("refused", "Slack did not open a conversation with them."))?
                .to_string()
        } else {
            target.to_string()
        };

        if named.is_empty() {
            self.slack_api(
                &link,
                "chat.postMessage",
                &[
                    ("channel", &channel),
                    ("text", &text),
                    // A mail's links are not fetched by Slack's unfurler:
                    // a tracking link would tell its sender it was shared.
                    ("unfurl_links", "false"),
                    ("unfurl_media", "false"),
                    ("parse", "none"),
                    ("link_names", "false"),
                ],
            )
            .await?;
        } else {
            let mut uploaded = Vec::with_capacity(named.len());
            for (name, bytes) in named {
                let length = bytes.len().to_string();
                let got = self
                    .slack_api(
                        &link,
                        "files.getUploadURLExternal",
                        &[("filename", &name), ("length", &length)],
                    )
                    .await?;
                let (Some(url), Some(id)) = (
                    got.get("upload_url").and_then(Value::as_str),
                    got.get("file_id")
                        .and_then(Value::as_str)
                        .filter(|id| slack_id(id)),
                ) else {
                    return Err(refused(
                        "refused",
                        "Slack did not say where to upload the file.",
                    ));
                };
                self.slack.upload(url, bytes).await.map_err(refusal)?;
                uploaded.push(serde_json::json!({ "id": id, "title": name }));
            }
            let files_json = Value::Array(uploaded).to_string();
            let mut form: Vec<(&str, &str)> =
                vec![("files", &files_json), ("channel_id", &channel)];
            if !text.is_empty() {
                form.push(("initial_comment", &text));
            }
            self.slack_api(&link, "files.completeUploadExternal", &form)
                .await?;
        }
        Ok(Shared {
            place: self
                .slack
                .name_of(target)
                .map(|n| format!("{n} in {}", link.team))
                .unwrap_or_default(),
        })
    }
}

/// A failure's sentence without either token in it.
fn hide(f: Fail, tokens: &Tokens) -> Fail {
    let secrets = tokens.secrets();
    match f {
        Fail::Net(w) => Fail::Net(oauth::scrub(&w, &secrets)),
        Fail::Refused(k, w) => Fail::Refused(k, oauth::scrub(&w, &secrets)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::NO_SLACK;
    use crate::store::Store;
    use crate::vault::{Memory, Stuck, Vault};
    use rata_mail::Resolver;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    /// The licence fixtures of `core`'s tests, good until 2108.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const BASE: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNzg5NTczMjM1LCJleHAiOjQzODE1NzMyMzV9.tcfOa11iI2hOD5wozS1wS4if109Eg0lW5SuHi6XB0ZH8s0Yo4nBi9-g-av9MKjT4-xomtg3aa3xu1B9ngjXODg";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";

    const ACCESS: &str = "xoxe.xoxp-1-access-secret-0001";
    const REFRESH: &str = "xoxe-1-refresh-secret-0001";

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    // ------------------------------------------------ a scripted Slack

    /// One answer the scripted Slack gives: a status, headers and a body.
    /// `asked` fires once the request is read; `hold` keeps the answer
    /// until it is released, which is how a test puts a Disconnect in the
    /// middle of a refresh Slack is still answering.
    struct Reply {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
        asked: Option<oneshot::Sender<()>>,
        hold: Option<oneshot::Receiver<()>>,
    }

    fn ok(body: &str) -> Reply {
        Reply {
            status: 200,
            headers: Vec::new(),
            body: body.to_string(),
            asked: None,
            hold: None,
        }
    }

    /// One request as the scripted Slack saw it.
    #[derive(Debug, Clone)]
    struct Asked {
        path: String,
        auth: Option<String>,
        content_type: String,
        body: Vec<u8>,
    }

    impl Asked {
        fn form(&self) -> HashMap<String, String> {
            url::form_urlencoded::parse(&self.body)
                .into_owned()
                .collect()
        }
    }

    type Replies = Arc<Mutex<HashMap<String, VecDeque<Reply>>>>;

    /// A Web API on 127.0.0.1 that answers each path from its own queue, in
    /// order, and keeps every request. Unscripted paths are Slack's
    /// `unknown_method`.
    struct Scripted {
        base: String,
        replies: Replies,
        asked: Arc<Mutex<Vec<Asked>>>,
    }

    impl Scripted {
        async fn start() -> Scripted {
            let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
            let replies: Replies = Arc::default();
            let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
            let (r, a) = (replies.clone(), asked.clone());
            tokio::spawn(async move {
                loop {
                    let Ok((s, _)) = l.accept().await else { return };
                    tokio::spawn(serve(s, r.clone(), a.clone()));
                }
            });
            Scripted {
                base,
                replies,
                asked,
            }
        }

        /// The API's address, as `Slack::new` takes it.
        fn api(&self) -> String {
            format!("{}/api", self.base)
        }

        fn on(&self, path: &str, reply: Reply) {
            self.replies
                .lock()
                .unwrap()
                .entry(path.to_string())
                .or_default()
                .push_back(reply);
        }

        fn api_on(&self, method: &str, reply: Reply) {
            self.on(&format!("/api/{method}"), reply);
        }

        fn asked(&self) -> Vec<Asked> {
            self.asked.lock().unwrap().clone()
        }

        fn asked_for(&self, method: &str) -> Vec<Asked> {
            let path = format!("/api/{method}");
            self.asked()
                .into_iter()
                .filter(|a| a.path == path)
                .collect()
        }
    }

    async fn serve(mut s: tokio::net::TcpStream, replies: Replies, asked: Arc<Mutex<Vec<Asked>>>) {
        let mut got = Vec::new();
        let mut buf = [0u8; 8192];
        let (head_end, len) = loop {
            let n = s.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            got.extend_from_slice(&buf[..n]);
            if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&got[..end]).to_string();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                break (end, len);
            }
        };
        while got.len() < head_end + 4 + len {
            let n = s.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        let head = String::from_utf8_lossy(&got[..head_end]).to_string();
        let target = head.split(' ').nth(1).unwrap_or("").to_string();
        let path = target.split('?').next().unwrap_or("").to_string();
        let header = |name: &str| {
            head.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_string())
            })
        };
        asked.lock().unwrap().push(Asked {
            path: path.clone(),
            auth: header("authorization"),
            content_type: header("content-type").unwrap_or_default(),
            body: got[head_end + 4..].to_vec(),
        });
        let reply = replies
            .lock()
            .unwrap()
            .get_mut(&path)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| Reply {
                status: 404,
                headers: Vec::new(),
                body: r#"{"ok":false,"error":"unknown_method"}"#.into(),
                asked: None,
                hold: None,
            });
        if let Some(a) = reply.asked {
            let _ = a.send(());
        }
        if let Some(h) = reply.hold {
            let _ = h.await;
        }
        let mut out = format!(
            "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            reply.status,
            reply.body.len()
        );
        for (k, v) in &reply.headers {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        out.push_str("\r\n");
        out.push_str(&reply.body);
        let _ = s.write_all(out.as_bytes()).await;
        let _ = s.shutdown().await;
    }

    // ------------------------------------------------------- the app

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let p = std::env::temp_dir().join(format!(
                "rata-slack-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An app whose Slack is the scripted one (`api`), with a client id,
    /// on any free port, waiting a millisecond for each second Slack asks.
    fn slack_app<V: Vault + 'static>(
        dir: &Scratch,
        api: &str,
        licence: Option<&str>,
        vault: V,
    ) -> Rata {
        let mut app = Rata::new(
            Store::open(dir.0.join("mailboxes.json")),
            Box::new(vault),
            Resolver::system().expect("resolver"),
            Some(KEY),
        );
        let mut slack = Slack::new(
            Some("1234567890.1234567890123".into()),
            AUTHORIZE_URL,
            api,
            &[0],
            true,
        );
        slack.second = Duration::from_millis(1);
        slack.gap = Duration::ZERO;
        app.slack = slack;
        if let Some(l) = licence {
            app.set_licence(Some(l.into()), None).unwrap();
        }
        app
    }

    fn tokens(expires_at: Option<u64>) -> Tokens {
        Tokens {
            access: ACCESS.into(),
            refresh: Some(REFRESH.into()),
            expires_at,
        }
    }

    fn link() -> SlackLink {
        SlackLink {
            team_id: "T0ACME".into(),
            team: "Acme".into(),
            user_id: "U0ME".into(),
            connected_at: 100,
            parked_at: None,
        }
    }

    /// A workspace connected as `connect_slack` leaves one.
    fn connected(app: &Rata, t: &Tokens) {
        vault::put_slack_secret(app.vault(), &t.to_secret()).unwrap();
        let mut s = app.store().lock().unwrap();
        s.set_slack(Some(link()));
        s.save().unwrap();
    }

    fn later() -> Option<u64> {
        Some(now() + 43_200)
    }

    fn entry(app: &Rata) -> Status {
        app.connections_status()
            .into_iter()
            .find(|s| s.service == "slack")
            .unwrap()
    }

    const GRANTED: &str = r#"{"ok":true,"app_id":"A1","authed_user":{"id":"U0ME","scope":"chat:write,files:write,im:write,channels:read,groups:read,im:read,users:read","access_token":"xoxe.xoxp-1-access-secret-0001","token_type":"user","refresh_token":"xoxe-1-refresh-secret-0001","expires_in":43200},"team":{"id":"T0ACME","name":"Acme \u202eCorp"},"enterprise":null,"is_enterprise_install":false}"#;

    const ROTATED: &str = r#"{"ok":true,"app_id":"A1","access_token":"xoxe.xoxp-1-access-secret-0002","refresh_token":"xoxe-1-refresh-secret-0002","token_type":"user","expires_in":43200,"team":{"id":"T0ACME","name":"Acme"}}"#;

    /// The browser: follows the link Slack's page would send it back on,
    /// with `code` and the attempt's own state.
    fn browser(link: &str, code: &'static str) -> tokio::task::JoinHandle<String> {
        let u = url::Url::parse(link).unwrap();
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        let back = url::Url::parse(&q["redirect_uri"]).unwrap();
        assert_eq!(back.host_str(), Some("localhost"));
        let port = back.port().unwrap();
        let state = q["state"].clone();
        tokio::spawn(async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            s.write_all(
                format!(
                    "GET /?code={code}&state={state} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
            let mut got = String::new();
            s.read_to_string(&mut got).await.unwrap();
            got
        })
    }

    // ------------------------------------------------------ pure parts

    #[test]
    fn the_client_id_is_used_only_when_it_looks_like_one() {
        assert_eq!(
            usable_id(Some(" 1234567890.1234567890123 ")),
            Some("1234567890.1234567890123")
        );
        assert_eq!(usable_id(Some("")), None);
        assert_eq!(usable_id(None), None);
        assert_eq!(usable_id(Some("12.34&redirect_uri=evil")), None);
        assert!(slack_id("C0123ABCD") && slack_id("U0ME"));
        assert!(!slack_id("c0123") && !slack_id("C01/../x") && !slack_id("X") && !slack_id(""));
    }

    #[test]
    fn text_is_escaped_for_slack_and_nothing_invisible_goes() {
        // Mentions and links Slack would act on arrive as plain text.
        assert_eq!(
            for_slack("Hi <!channel> and <@U123> see <https://evil.example|https://bank.example>"),
            "Hi &lt;!channel&gt; and &lt;@U123&gt; see &lt;https://evil.example|https://bank.example&gt;"
        );
        // `&` first: text that already looks escaped stays as written.
        assert_eq!(for_slack("a & b &lt; c"), "a &amp; b &amp;lt; c");
        // Bidi controls, zero-width characters, the BOM and controls go;
        // line breaks are Slack's own; an emoji keeps its variation selector.
        assert_eq!(
            for_slack(
                "\u{feff}pay\u{202e}fdp.exe\u{202c} now\u{200b}!\r\nnext\rline\u{0007}\u{2028}end \u{2764}\u{fe0f}"
            ),
            "payfdp.exe now!\nnext\nline\nend \u{2764}\u{fe0f}"
        );
        assert_eq!(for_slack("  \u{200b}\n "), "");
        // Names are one plain line.
        assert_eq!(plain(" Acme\u{202e} \n Corp\u{200d} ", 80), "Acme Corp");
        assert_eq!(plain(&"x".repeat(200), 80).len(), 80);
    }

    #[test]
    fn the_sign_in_page_asks_for_user_scopes_with_pkce() {
        let slack = Slack::from_build();
        let a = Attempt::with_redirect("http://localhost:28417".into()).unwrap();
        let link = slack.authorize_link("1234.5678", &a);
        let u = url::Url::parse(&link).unwrap();
        assert_eq!(
            (u.scheme(), u.host_str(), u.path()),
            ("https", Some(LOGIN_HOST), "/oauth/v2/authorize")
        );
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "1234.5678");
        assert_eq!(q["scope"], "", "no bot scopes on a desktop redirect");
        let asked: Vec<&str> = q["user_scope"].split(',').collect();
        assert_eq!(
            asked,
            [
                "chat:write",
                "files:write",
                "im:write",
                "channels:read",
                "groups:read",
                "im:read",
                "users:read"
            ]
        );
        // Nothing that reads a message.
        assert!(!q["user_scope"].contains("history") && !q["user_scope"].contains(":read,chat"));
        assert_eq!(q["redirect_uri"], "http://localhost:28417");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], oauth::challenge(a.verifier()));
        assert_eq!(q["state"], a.state);
        assert!(!link.contains(a.verifier()));
        assert!(!q.contains_key("client_secret"));
        // The browser opens Slack's page and nothing else.
        assert!(open_sign_in("https://slack.com.evil.example/oauth").is_err());
        assert!(open_sign_in("http://slack.com/oauth/v2/authorize").is_err());
        assert!(open_sign_in("javascript:alert(1)").is_err());
        // The registered ports are fixed, and below every system's range of
        // ports handed out at random (32768 on Linux, 49152 elsewhere).
        assert!(PORTS.iter().all(|&p| (1024..32768).contains(&p)));
    }

    #[test]
    fn slacks_error_codes_get_the_answer_each_needs() {
        assert_eq!(classify("token_expired"), Fail::Expired);
        for code in [
            "token_revoked",
            "invalid_auth",
            "account_inactive",
            "invalid_refresh_token",
        ] {
            assert!(matches!(classify(code), Fail::Revoked(_)), "{code}");
        }
        assert!(matches!(classify("ratelimited"), Fail::Limited(_)));
        assert!(matches!(classify("internal_error"), Fail::Net(_)));
        assert!(matches!(
            classify("channel_not_found"),
            Fail::Refused("not-found", _)
        ));
        assert!(matches!(
            classify("msg_too_long"),
            Fail::Refused("too-large", _)
        ));
        assert!(matches!(
            classify("missing_scope"),
            Fail::Refused("refused", _)
        ));
        // A code is a code: words Slack (or anyone) put there are not shown.
        match classify("Call 0800 123 for help") {
            Fail::Refused(_, w) => assert!(!w.contains("0800"), "{w}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tokens_are_kept_whole_and_never_printed() {
        let t = tokens(Some(5_000));
        assert_eq!(Tokens::from_secret(&t.to_secret()), Some(t.clone()));
        let shown = format!("{t:?}");
        assert!(!shown.contains("secret"), "{shown}");
        let none = Tokens {
            refresh: None,
            expires_at: None,
            ..t.clone()
        };
        assert_eq!(Tokens::from_secret(&none.to_secret()), Some(none.clone()));
        assert!(
            none.fresh(u64::MAX - 1),
            "a token without an end does not run out"
        );
        assert!(t.fresh(5_000 - oauth::EARLY - 1) && !t.fresh(5_000 - oauth::EARLY));
        assert_eq!(Tokens::from_secret("xoxp-plain"), None);
        assert_eq!(Tokens::from_secret(r#"{"a":"has space"}"#), None);
    }

    #[test]
    fn the_targets_are_the_channels_you_are_in_and_the_people() {
        let convs: Vec<Value> = serde_json::from_str(
            r##"[
              {"id":"C0GEN","name":"general","is_channel":true,"is_member":true},
              {"id":"C0OUT","name":"not-mine","is_channel":true,"is_member":false},
              {"id":"C0OLD","name":"old","is_channel":true,"is_member":true,"is_archived":true},
              {"id":"G0SEC","name":"Secret\u202e","is_private":true},
              {"id":"D0ANN","is_im":true,"user":"U0ANN"},
              {"id":"D0GONE","is_im":true,"user":"U0GONE","is_user_deleted":true},
              {"id":"bad id","name":"x","is_member":true}
            ]"##,
        )
        .unwrap();
        let people: Vec<Value> = serde_json::from_str(
            r#"[
              {"id":"U0ANN","name":"ann","profile":{"display_name":"","real_name":"Ann Lee"}},
              {"id":"U0BOB","name":"bob","real_name":"Bob","profile":{"display_name":"bobby"}},
              {"id":"U0ME","name":"me","profile":{"real_name":"Me Myself"}},
              {"id":"U0GONE","name":"gone","deleted":true},
              {"id":"B0BOT","name":"bot","is_bot":true},
              {"id":"USLACKBOT","name":"slackbot","is_bot":false},
              {"id":"U0APP","name":"app","is_app_user":true}
            ]"#,
        )
        .unwrap();
        let t = targets_from(&convs, &people, "U0ME");
        let shown: Vec<(&str, &str, &str)> = t
            .iter()
            .map(|t| (t.id.as_str(), t.name.as_str(), t.kind))
            .collect();
        assert_eq!(
            shown,
            [
                ("C0GEN", "general", "channel"),
                ("G0SEC", "Secret", "channel"),
                // Ann has a direct message, so it is used; Bob is opened when
                // sent to.
                ("D0ANN", "Ann Lee", "person"),
                ("U0BOB", "bobby", "person"),
                ("U0ME", "Me Myself (you)", "person"),
            ]
        );
    }

    // ------------------------------------------------- against Slack

    #[test]
    fn connecting_signs_in_in_the_browser_and_keeps_no_token_in_the_store() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on("oauth.v2.access", ok(GRANTED));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = slack_app(&dir, &srv.api(), Some(PRO), vault.clone());
            assert!(!entry(&app).connected);
            let mut tab = None;
            let got = app
                .connect_slack(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap();
            let page = tab.unwrap().await.unwrap();
            assert!(page.contains("RATA has what it needs from Slack"), "{page}");
            let Connected::Done(st) = got else {
                panic!("{got:?}")
            };
            assert!(st.connected && st.available);
            assert_eq!(st.account.as_deref(), Some("Acme Corp"));

            // The code and verifier went to oauth.v2.access as a form, with
            // the redirect it came back on and no secret.
            let ex = &srv.asked_for("oauth.v2.access")[0];
            let form = ex.form();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["code"], "the-code");
            assert_eq!(form["client_id"], "1234567890.1234567890123");
            assert_eq!(form["code_verifier"].len(), 43);
            assert!(form["redirect_uri"].starts_with("http://localhost:"));
            assert!(!form.contains_key("client_secret"));
            assert!(ex.auth.is_none());
            assert!(
                ex.content_type
                    .starts_with("application/x-www-form-urlencoded")
            );

            // Both tokens in the keychain's own service; none in the store.
            let kept =
                Tokens::from_secret(&vault::get_slack_secret(vault.as_ref()).unwrap()).unwrap();
            assert_eq!(
                (kept.access.as_str(), kept.refresh.as_deref()),
                (ACCESS, Some(REFRESH))
            );
            assert!(kept.expires_at.unwrap() > now() + 40_000);
            let raw = std::fs::read_to_string(dir.0.join("mailboxes.json")).unwrap();
            assert!(raw.contains("T0ACME") && raw.contains("U0ME"), "{raw}");
            assert!(!raw.contains("xox") && !raw.contains("secret"), "{raw}");
            let all = serde_json::to_string(&app.connections_status()).unwrap();
            assert!(!all.contains("xox"), "{all}");
        });
    }

    #[test]
    fn a_sign_in_cancelled_or_refused_keeps_nothing() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = slack_app(&dir, &srv.api(), Some(PRO), vault.clone());
            // Cancel while the browser is open.
            let got = app
                .connect_slack(|_| {
                    assert!(app.cancel_slack());
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(!app.cancel_slack(), "nothing is left waiting");
            // Slack refuses the code: said, and nothing kept.
            srv.api_on(
                "oauth.v2.access",
                ok(r#"{"ok":false,"error":"invalid_code_verifier"}"#),
            );
            let mut tab = None;
            let e = app
                .connect_slack(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert_eq!(e.kind, "refused");
            assert!(e.error.contains("invalid_code_verifier"), "{}", e.error);
            // Granted without chat:write: nothing kept either.
            srv.api_on("oauth.v2.access", ok(&GRANTED.replace("chat:write,", "")));
            let mut tab = None;
            let e = app
                .connect_slack(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert!(e.error.contains("post for you"), "{}", e.error);
            assert!(vault.get_slack(vault::SLACK_ENTRY).is_err());
            assert!(app.store().lock().unwrap().slack().is_none());
            assert!(!entry(&app).connected);
        });
    }

    /// SEC-7/SEC-8 for Slack: a sign-in that comes back after Disconnect
    /// or Delete account writes nothing.
    #[test]
    fn a_sign_in_finishing_after_disconnect_or_delete_account_keeps_nothing() {
        let dir = Scratch::new();
        let vault = Arc::new(Memory::default());
        let app = slack_app(&dir, "http://127.0.0.1:9/api", Some(PRO), vault.clone());
        let granted = read_granted(&serde_json::from_str(GRANTED).unwrap()).unwrap();

        let began = (app.epoch(), app.slack.generation());
        app.disconnect_slack().unwrap();
        let e = app.keep_slack(granted.clone(), began).unwrap_err();
        assert!(e.error.contains("nothing was kept"), "{}", e.error);
        assert!(vault.get_slack(vault::SLACK_ENTRY).is_err());
        assert!(app.store().lock().unwrap().slack().is_none());

        let began = (app.epoch(), app.slack.generation());
        app.forget_everything().unwrap();
        app.set_licence(Some(PRO.into()), None).unwrap();
        assert!(app.keep_slack(granted.clone(), began).is_err());
        assert!(vault.get_slack(vault::SLACK_ENTRY).is_err());
        assert!(app.store().lock().unwrap().slack().is_none());

        // And one that began after them is kept.
        let began = (app.epoch(), app.slack.generation());
        assert!(app.keep_slack(granted.clone(), began).unwrap().connected);
        // A licence that became Base meanwhile keeps nothing.
        let began = (app.epoch(), app.slack.generation());
        app.set_licence(Some(BASE.into()), None).unwrap();
        assert_eq!(app.keep_slack(granted, began).unwrap_err().kind, "plan");
    }

    #[test]
    fn the_targets_are_read_page_by_page_with_the_token() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on(
                "conversations.list",
                ok(r#"{"ok":true,"channels":[{"id":"C0GEN","name":"general","is_member":true}],"response_metadata":{"next_cursor":"dGVhbTpDMDYx"}}"#),
            );
            srv.api_on(
                "conversations.list",
                ok(r#"{"ok":true,"channels":[{"id":"C0ALL","name":"all-hands","is_member":true},{"id":"D0ANN","is_im":true,"user":"U0ANN"}],"response_metadata":{"next_cursor":""}}"#),
            );
            srv.api_on(
                "users.list",
                ok(r#"{"ok":true,"members":[{"id":"U0ANN","profile":{"real_name":"Ann Lee"}}],"response_metadata":{"next_cursor":""}}"#),
            );
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            let t = app.slack_targets().await.unwrap();
            let ids: Vec<&str> = t.iter().map(|t| t.id.as_str()).collect();
            assert_eq!(ids, ["C0ALL", "C0GEN", "D0ANN"]);

            let pages = srv.asked_for("conversations.list");
            assert_eq!(pages.len(), 2);
            let first = pages[0].form();
            assert_eq!(first["types"], "public_channel,private_channel,im");
            assert_eq!(first["exclude_archived"], "true");
            assert_eq!(first["limit"], "200");
            assert!(!first.contains_key("cursor"));
            assert_eq!(pages[1].form()["cursor"], "dGVhbTpDMDYx");
            for a in srv.asked() {
                assert_eq!(a.auth.as_deref(), Some(&*format!("Bearer {ACCESS}")));
            }
            // No refresh was needed: the token had hours left.
            assert!(srv.asked_for("oauth.v2.access").is_empty());
        });
    }

    #[test]
    fn a_listing_stops_after_its_last_page_even_if_slack_does_not() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            for i in 0..PAGES_MAX + 3 {
                srv.api_on(
                    "conversations.list",
                    ok(&format!(r#"{{"ok":true,"channels":[],"response_metadata":{{"next_cursor":"c{i}"}}}}"#)),
                );
            }
            srv.api_on("users.list", ok(r#"{"ok":true,"members":[]}"#));
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            assert!(app.slack_targets().await.unwrap().is_empty());
            assert_eq!(srv.asked_for("conversations.list").len(), PAGES_MAX);
        });
    }

    #[test]
    fn an_access_token_near_its_end_is_rotated_and_the_new_pair_kept() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on("oauth.v2.access", ok(ROTATED));
            srv.api_on("conversations.list", ok(r#"{"ok":true,"channels":[]}"#));
            srv.api_on("users.list", ok(r#"{"ok":true,"members":[]}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = slack_app(&dir, &srv.api(), Some(PRO), vault.clone());
            connected(&app, &tokens(Some(now() + 30)));
            app.slack_targets().await.unwrap();

            let r = &srv.asked_for("oauth.v2.access")[0];
            let form = r.form();
            assert_eq!(form["grant_type"], "refresh_token");
            assert_eq!(form["refresh_token"], REFRESH);
            assert_eq!(form["client_id"], "1234567890.1234567890123");
            assert!(!form.contains_key("client_secret") && !form.contains_key("code_verifier"));
            // The listing used the new token, and the new pair is kept: the
            // old refresh token is spent.
            assert_eq!(
                srv.asked_for("conversations.list")[0].auth.as_deref(),
                Some("Bearer xoxe.xoxp-1-access-secret-0002")
            );
            let kept =
                Tokens::from_secret(&vault::get_slack_secret(vault.as_ref()).unwrap()).unwrap();
            assert_eq!(kept.refresh.as_deref(), Some("xoxe-1-refresh-secret-0002"));
            assert_eq!(kept.access, "xoxe.xoxp-1-access-secret-0002");

            // Slack saying a token ran out early renews it once, and retries.
            srv.api_on(
                "conversations.list",
                ok(r#"{"ok":false,"error":"token_expired"}"#),
            );
            srv.api_on("oauth.v2.access", ok(&ROTATED.replace("0002", "0003")));
            srv.api_on("conversations.list", ok(r#"{"ok":true,"channels":[]}"#));
            srv.api_on("users.list", ok(r#"{"ok":true,"members":[]}"#));
            app.slack_targets().await.unwrap();
            let lists = srv.asked_for("conversations.list");
            assert_eq!(lists.len(), 3);
            assert_eq!(
                lists[2].auth.as_deref(),
                Some("Bearer xoxe.xoxp-1-access-secret-0003")
            );
            assert_eq!(
                srv.asked_for("oauth.v2.access")[1].form()["refresh_token"],
                "xoxe-1-refresh-secret-0002"
            );
        });
    }

    #[test]
    fn a_revoked_sign_in_is_parked_and_said_and_a_network_failure_is_not() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on(
                "conversations.list",
                ok(r#"{"ok":false,"error":"token_revoked"}"#),
            );
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            let e = app.slack_targets().await.unwrap_err();
            assert_eq!(e.kind, "not-connected");
            assert!(e.error.contains("Connect Slack again"), "{}", e.error);
            assert!(
                app.store()
                    .lock()
                    .unwrap()
                    .slack()
                    .unwrap()
                    .parked_at
                    .is_some()
            );
            let st = entry(&app);
            assert!(!st.connected);
            assert!(st.note.unwrap().contains("Connect Slack again"));
            // Nothing more is sent with it.
            let before = srv.asked().len();
            assert_eq!(app.slack_targets().await.unwrap_err().kind, "not-connected");
            assert_eq!(
                app.slack_share("C0GEN", "hi", vec![])
                    .await
                    .unwrap_err()
                    .kind,
                "not-connected"
            );
            assert_eq!(srv.asked().len(), before);

            // A refresh token Slack no longer takes parks it too.
            srv.api_on(
                "oauth.v2.access",
                ok(r#"{"ok":false,"error":"invalid_refresh_token"}"#),
            );
            let dir2 = Scratch::new();
            let app2 = slack_app(&dir2, &srv.api(), Some(PRO), Memory::default());
            connected(&app2, &tokens(Some(1)));
            assert_eq!(
                app2.slack_targets().await.unwrap_err().kind,
                "not-connected"
            );
            assert!(
                app2.store()
                    .lock()
                    .unwrap()
                    .slack()
                    .unwrap()
                    .parked_at
                    .is_some()
            );

            // invalid_auth on a call: the same.
            srv.api_on(
                "chat.postMessage",
                ok(r#"{"ok":false,"error":"invalid_auth"}"#),
            );
            let dir3 = Scratch::new();
            let app3 = slack_app(&dir3, &srv.api(), Some(PRO), Memory::default());
            connected(&app3, &tokens(later()));
            assert_eq!(
                app3.slack_share("C0GEN", "hi", vec![])
                    .await
                    .unwrap_err()
                    .kind,
                "not-connected"
            );
            assert!(!entry(&app3).connected);

            // Slack not answering: said, and nothing parked.
            let dir4 = Scratch::new();
            let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let nowhere = format!("http://127.0.0.1:{}/api", gone.local_addr().unwrap().port());
            drop(gone);
            let app4 = slack_app(&dir4, &nowhere, Some(PRO), Memory::default());
            connected(&app4, &tokens(later()));
            let e = app4.slack_targets().await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(e.error.contains("could not be reached"), "{}", e.error);
            assert!(!e.error.contains("xox"), "{}", e.error);
            assert!(entry(&app4).connected);
            // A refresh that cannot reach Slack parks nothing either.
            connected(&app4, &tokens(Some(1)));
            app4.slack.drop_held();
            assert_eq!(app4.slack_targets().await.unwrap_err().kind, "offline");
            assert!(entry(&app4).connected);
        });
    }

    #[test]
    fn slow_down_is_waited_out_once_and_a_long_wait_is_said() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let limited = |secs: &str| Reply {
                status: 429,
                headers: vec![("Retry-After", secs.to_string())],
                body: r#"{"ok":false,"error":"ratelimited"}"#.into(),
                asked: None,
                hold: None,
            };
            srv.api_on("chat.postMessage", limited("1"));
            srv.api_on(
                "chat.postMessage",
                ok(r#"{"ok":true,"channel":"C0GEN","ts":"1.2"}"#),
            );
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            app.slack_share("C0GEN", "hello", vec![]).await.unwrap();
            assert_eq!(srv.asked_for("chat.postMessage").len(), 2);

            // Two minutes is not waited: said, once, and not parked.
            srv.api_on("chat.postMessage", limited("120"));
            let e = app.slack_share("C0GEN", "hello", vec![]).await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(e.error.contains("120 seconds"), "{}", e.error);
            assert_eq!(srv.asked_for("chat.postMessage").len(), 3);
            // Limited twice in a row: given up after the second.
            srv.api_on("chat.postMessage", limited("1"));
            srv.api_on("chat.postMessage", limited("1"));
            assert_eq!(
                app.slack_share("C0GEN", "hello", vec![])
                    .await
                    .unwrap_err()
                    .kind,
                "offline"
            );
            assert_eq!(srv.asked_for("chat.postMessage").len(), 5);
            assert!(entry(&app).connected);
        });
    }

    #[test]
    fn a_message_goes_escaped_to_the_channel_and_says_where() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on(
                "conversations.list",
                ok(r#"{"ok":true,"channels":[{"id":"C0GEN","name":"general","is_member":true}]}"#),
            );
            srv.api_on("users.list", ok(r#"{"ok":true,"members":[]}"#));
            srv.api_on("chat.postMessage", ok(r#"{"ok":true}"#));
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            app.slack_targets().await.unwrap();
            let shared = app
                .slack_share("C0GEN", "Re: invoice <!here>\r\nPay & go\u{202e}", vec![])
                .await
                .unwrap();
            assert_eq!(shared.place, "#general in Acme");
            let post = &srv.asked_for("chat.postMessage")[0];
            let form = post.form();
            assert_eq!(form["channel"], "C0GEN");
            assert_eq!(form["text"], "Re: invoice &lt;!here&gt;\nPay &amp; go");
            assert_eq!(form["unfurl_links"], "false");
            assert_eq!(form["unfurl_media"], "false");
            assert_eq!(form["parse"], "none");
            assert_eq!(post.auth.as_deref(), Some(&*format!("Bearer {ACCESS}")));
            assert_eq!(
                serde_json::to_value(&shared).unwrap(),
                serde_json::json!({"where": "#general in Acme"})
            );
        });
    }

    #[test]
    fn a_person_is_written_to_in_their_direct_message() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on(
                "conversations.open",
                ok(r#"{"ok":true,"channel":{"id":"D0BOB"}}"#),
            );
            srv.api_on("chat.postMessage", ok(r#"{"ok":true}"#));
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            let shared = app.slack_share("U0BOB", "hi Bob", vec![]).await.unwrap();
            assert_eq!(shared.place, "", "not listed: the page names it");
            assert_eq!(
                srv.asked_for("conversations.open")[0].form()["users"],
                "U0BOB"
            );
            assert_eq!(
                srv.asked_for("chat.postMessage")[0].form()["channel"],
                "D0BOB"
            );
        });
    }

    #[test]
    fn files_go_up_in_two_steps_with_the_message_beside_them() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            for (n, id) in [(1, "F0ONE"), (2, "F0TWO")] {
                srv.api_on(
                    "files.getUploadURLExternal",
                    ok(&format!(
                        r#"{{"ok":true,"upload_url":"{}/upload/{n}","file_id":"{id}"}}"#,
                        srv.base
                    )),
                );
                srv.on(&format!("/upload/{n}"), ok(&format!("OK - {n}")));
            }
            srv.api_on(
                "files.completeUploadExternal",
                ok(r#"{"ok":true,"files":[]}"#),
            );
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            app.slack_share(
                "C0GEN",
                "See <these>",
                vec![
                    ShareFile {
                        name: "../../report\u{202e}.pdf".into(),
                        bytes: b"%PDF-1".to_vec(),
                    },
                    ShareFile {
                        name: "notes.txt".into(),
                        bytes: b"hello".to_vec(),
                    },
                ],
            )
            .await
            .unwrap();
            let asks = srv.asked_for("files.getUploadURLExternal");
            assert_eq!(asks.len(), 2);
            let first = asks[0].form();
            assert_eq!(
                first["filename"],
                safe_file_name("../../report\u{202e}.pdf")
            );
            assert!(!first["filename"].contains('/') && !first["filename"].contains('\u{202e}'));
            assert_eq!(first["length"], "6");
            assert_eq!(asks[1].form()["length"], "5");
            // The bytes go to the upload address as they are, without the
            // token.
            let up: Vec<Asked> = srv
                .asked()
                .into_iter()
                .filter(|a| a.path.starts_with("/upload/"))
                .collect();
            assert_eq!(up.len(), 2);
            assert_eq!(up[0].body, b"%PDF-1");
            assert_eq!(up[1].body, b"hello");
            assert!(up.iter().all(|a| a.auth.is_none()));
            assert!(up[0].content_type.starts_with("application/octet-stream"));
            // Then one completion, to the channel, with the text escaped.
            let done = srv.asked_for("files.completeUploadExternal")[0].form();
            assert_eq!(done["channel_id"], "C0GEN");
            assert_eq!(done["initial_comment"], "See &lt;these&gt;");
            let files: Value = serde_json::from_str(&done["files"]).unwrap();
            assert_eq!(files[0]["id"], "F0ONE");
            assert_eq!(files[1]["id"], "F0TWO");
            assert_eq!(files[1]["title"], "notes.txt");
            // No message was posted besides.
            assert!(srv.asked_for("chat.postMessage").is_empty());

            // A file alone: no comment.
            srv.api_on(
                "files.getUploadURLExternal",
                ok(&format!(
                    r#"{{"ok":true,"upload_url":"{}/upload/3","file_id":"F0THREE"}}"#,
                    srv.base
                )),
            );
            srv.on("/upload/3", ok("OK"));
            srv.api_on("files.completeUploadExternal", ok(r#"{"ok":true}"#));
            app.slack_share(
                "C0GEN",
                "  ",
                vec![ShareFile {
                    name: "a.txt".into(),
                    bytes: b"a".to_vec(),
                }],
            )
            .await
            .unwrap();
            assert!(
                !srv.asked_for("files.completeUploadExternal")[1]
                    .form()
                    .contains_key("initial_comment")
            );
        });
    }

    #[test]
    fn a_file_goes_to_slacks_own_host_or_nowhere() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on(
                "files.getUploadURLExternal",
                ok(r#"{"ok":true,"upload_url":"https://files.slack.com.evil.example/upload/v1/x","file_id":"F0ONE"}"#),
            );
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            connected(&app, &tokens(later()));
            let e = app
                .slack_share("C0GEN", "x", vec![ShareFile { name: "a.txt".into(), bytes: b"a".to_vec() }])
                .await
                .unwrap_err();
            assert!(e.error.contains("not Slack's own"), "{}", e.error);
            assert!(srv.asked_for("files.completeUploadExternal").is_empty());
            // Which addresses count as Slack's.
            let real = Slack::from_build();
            let ok_url = |s: &str| real.upload_ok(&url::Url::parse(s).unwrap());
            assert!(ok_url("https://files.slack.com/upload/v1/abc"));
            assert!(!ok_url("http://files.slack.com/upload/v1/abc"));
            assert!(!ok_url("https://files.slack.com:8443/upload"));
            assert!(!ok_url("https://user@files.slack.com/upload"));
            assert!(!ok_url("https://slack.com.evil.example/upload"));
            assert!(!ok_url("http://127.0.0.1/upload"), "only the tests' own Slack");
        });
    }

    #[test]
    fn everything_is_checked_before_anything_is_sent() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = slack_app(&dir, &srv.api(), Some(PRO), Memory::default());
            // Not connected yet.
            let e = app.slack_targets().await.unwrap_err();
            assert_eq!((e.kind, e.error.as_str()), ("not-connected", NOT_CONNECTED));
            connected(&app, &tokens(later()));
            let file = |name: &str, n: usize| ShareFile {
                name: name.into(),
                bytes: vec![b'x'; n],
            };
            let long = "x".repeat(TEXT_MAX + 1);
            for (target, text, files, kind) in [
                ("", "hi", vec![], "not-found"),
                ("general", "hi", vec![], "not-found"),
                ("C0GEN&x=1", "hi", vec![], "not-found"),
                ("T0ACME", "hi", vec![], "not-found"),
                ("C0GEN", " \u{200b} ", vec![], "refused"),
                ("C0GEN", long.as_str(), vec![], "too-large"),
                ("C0GEN", "x", vec![file("invoice.pdf.exe", 3)], "refused"),
                ("C0GEN", "x", vec![file("empty.txt", 0)], "refused"),
                (
                    "C0GEN",
                    "x",
                    (0..=SHARE_FILES_MAX).map(|_| file("a.txt", 1)).collect(),
                    "too-large",
                ),
                (
                    "C0GEN",
                    "x",
                    vec![
                        file("a.bin", SHARE_MAX / 2 + 1),
                        file("b.bin", SHARE_MAX / 2 + 1),
                    ],
                    "too-large",
                ),
            ] {
                let e = app.slack_share(target, text, files).await.unwrap_err();
                assert_eq!(e.kind, kind, "{target:?} {text:.20?}: {}", e.error);
                assert_eq!(e.service, "slack");
            }
            assert!(srv.asked().is_empty(), "nothing reached Slack");
        });
    }

    #[test]
    fn only_pro_and_a_licence_share_and_disconnect_is_allowed_on_any_plan() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let none = slack_app(&dir, &srv.api(), None, vault.clone());
            assert_eq!(none.slack_targets().await.unwrap_err().kind, "unlicensed");
            assert_eq!(
                none.connect_slack(|_| Ok(())).await.unwrap_err().kind,
                "unlicensed"
            );
            let dir2 = Scratch::new();
            let base = slack_app(&dir2, &srv.api(), Some(BASE), vault.clone());
            connected(&base, &tokens(later()));
            assert_eq!(base.slack_targets().await.unwrap_err().kind, "plan");
            assert_eq!(
                base.slack_share("C0GEN", "x", vec![])
                    .await
                    .unwrap_err()
                    .kind,
                "plan"
            );
            let e = base
                .connect_slack(|_| panic!("no browser on Base"))
                .await
                .unwrap_err();
            assert_eq!(e.kind, "plan");
            let st = entry(&base);
            assert!(
                st.available && !st.connected,
                "Base sees the row, not the connection"
            );
            assert!(srv.asked().is_empty());
            // Disconnect works on Base, and empties the keychain.
            assert!(!base.disconnect_service("slack").unwrap().connected);
            assert!(vault.get_slack(vault::SLACK_ENTRY).is_err());
            assert!(base.store().lock().unwrap().slack().is_none());
        });
    }

    #[test]
    fn disconnect_forgets_the_keychain_first_and_asks_slack_to_end_it() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.api_on("auth.revoke", ok(r#"{"ok":true,"revoked":true}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Stuck::default());
            let app = slack_app(&dir, &srv.api(), Some(PRO), vault.clone());
            connected(&app, &tokens(later()));
            // A keychain that will not let go: still connected, said why.
            vault.stick(true);
            let e = app.disconnect_slack().unwrap_err();
            assert_eq!(e.kind, "disk");
            assert!(e.error.contains("still connected"), "{}", e.error);
            assert!(entry(&app).connected);
            vault.stick(false);
            let (st, held) = app.disconnect_slack().unwrap();
            assert!(!st.connected);
            assert!(vault::get_slack_secret(vault.as_ref()).is_err());
            assert!(app.store().lock().unwrap().slack().is_none());
            app.revoke_slack(held.unwrap()).await;
            let rv = &srv.asked_for("auth.revoke")[0];
            assert_eq!(rv.auth.as_deref(), Some(&*format!("Bearer {ACCESS}")));
            // Gone for good, after a restart too.
            let again = Store::open(dir.0.join("mailboxes.json"));
            assert!(again.slack().is_none());
            assert_eq!(app.slack_targets().await.unwrap_err().kind, "not-connected");
        });
    }

    /// SEC-7 for Slack: a rotation Slack answers after Disconnect or Delete
    /// account writes nothing back.
    #[test]
    fn a_rotation_answered_after_disconnect_or_delete_account_writes_nothing() {
        for delete_account in [false, true] {
            rt().block_on(async {
                let srv = Scripted::start().await;
                let (asked_tx, asked) = oneshot::channel();
                let (release, released) = oneshot::channel::<()>();
                srv.api_on(
                    "oauth.v2.access",
                    Reply {
                        asked: Some(asked_tx),
                        hold: Some(released),
                        ..ok(ROTATED)
                    },
                );
                let dir = Scratch::new();
                let vault = Arc::new(Memory::default());
                let app = slack_app(&dir, &srv.api(), Some(PRO), vault.clone());
                connected(&app, &tokens(Some(1)));
                let (listed, ()) = tokio::join!(app.slack_targets(), async {
                    asked.await.unwrap();
                    if delete_account {
                        app.forget_everything().unwrap();
                    } else {
                        app.disconnect_slack().unwrap();
                    }
                    release.send(()).unwrap();
                });
                let e = listed.unwrap_err();
                assert_eq!(e.kind, "not-connected", "{}", e.error);
                assert!(e.error.contains("nothing was kept"), "{}", e.error);
                assert!(
                    vault.get_slack(vault::SLACK_ENTRY).is_err(),
                    "nothing written back"
                );
                assert!(app.store().lock().unwrap().slack().is_none());
                assert!(app.slack.held(app.slack.generation()).is_none());
                assert!(srv.asked_for("conversations.list").is_empty());
            });
        }
    }

    #[test]
    fn delete_account_forgets_slack_and_a_stuck_keychain_keeps_the_licence() {
        let dir = Scratch::new();
        let vault = Arc::new(Stuck::default());
        let app = slack_app(&dir, "http://127.0.0.1:9/api", Some(PRO), vault.clone());
        // Never connected: a keychain that refuses does not stop it.
        vault.stick(true);
        app.forget_everything().unwrap();
        vault.stick(false);
        app.set_licence(Some(PRO.into()), None).unwrap();
        connected(&app, &tokens(later()));
        vault.stick(true);
        let e = app.forget_everything().unwrap_err();
        assert!(e.contains("Slack"), "{e}");
        assert!(
            app.standing().licensed,
            "the licence stays until Slack is gone"
        );
        vault.stick(false);
        app.forget_everything().unwrap();
        assert!(vault::get_slack_secret(vault.as_ref()).is_err());
        assert!(Store::open(dir.0.join("mailboxes.json")).slack().is_none());
        assert!(!app.standing().licensed);
    }

    /// A build without RATA_SLACK_CLIENT_ID is exactly as before K3.
    #[test]
    fn a_build_without_the_client_id_offers_no_slack_as_before() {
        rt().block_on(async {
            let dir = Scratch::new();
            let mut app = slack_app(&dir, "http://127.0.0.1:9/api", Some(PRO), Memory::default());
            app.slack = Slack::off();
            let st = entry(&app);
            assert_eq!(
                (st.available, st.connected, st.kind, st.account.is_none()),
                (false, false, "share", true)
            );
            for e in [
                app.slack_targets().await.unwrap_err(),
                app.slack_share("C0GEN", "x", vec![]).await.unwrap_err(),
                app.connect_slack(|_| panic!("no browser"))
                    .await
                    .unwrap_err(),
                app.service_of("slack").unwrap_err(),
                app.disconnect_service("slack").unwrap_err(),
            ] {
                assert_eq!((e.kind, e.error.as_str()), ("unavailable", NO_SLACK));
            }
            assert!(!app.cancel_slack());
            assert!(app.diagnostics(false, &[]).contains("Slack sharing no"));
            let mut on = app;
            on.slack = Slack::new(Some("1.2".into()), AUTHORIZE_URL, API_URL, &PORTS, false);
            assert!(entry(&on).available);
            assert!(on.diagnostics(false, &[]).contains("Slack sharing yes"));
            // Slack is never a folder.
            assert_eq!(on.cloud_list("slack", None).unwrap_err().kind, "refused");
        });
    }

    #[test]
    fn no_token_is_in_any_sentence() {
        let t = tokens(Some(1));
        let f = hide(Fail::Net(format!("proxy said {ACCESS} and {REFRESH}")), &t);
        let Fail::Net(w) = f else { panic!() };
        assert!(!w.contains(ACCESS) && !w.contains(REFRESH), "{w}");
    }
}
