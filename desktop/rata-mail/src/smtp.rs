//! Sending.
//!
//! Reading somebody's mail without being able to answer it is half a product,
//! and a message dragged between two inboxes has to leave from the one it
//! landed on or the transfer is theatre. The app password already stored for
//! reading is used for submission too (or, for a mailbox that signs in with
//! OAuth, the same access token, over `AUTH XOAUTH2` — see `xoauth2`),
//! which is how these providers work — the
//! message is sent by the customer's own mail server, from their own address,
//! so it lands in their Sent folder and passes SPF like anything else they
//! send.
//!
//! The protocol is spoken directly rather than through a library. That is a
//! deliberate trade and the reason is [`crate::resolve::resolve_public`]: every
//! SMTP crate takes a hostname and resolves it itself, which would mean the
//! guard judges one address and the socket opens to another. Submission is a
//! dozen commands, and owning them keeps the one property this crate exists to
//! keep — and one fewer dependency in the program that holds the customer's
//! mail password.
//!
//! Two ports, in this order:
//!
//! * **465**, implicit TLS — encrypted from the first byte.
//! * **587**, STARTTLS — starts in the clear and upgrades. Tried second because
//!   an attacker who can modify traffic can strip the upgrade offer; with 465
//!   there is nothing to strip. See [`starttls`] for what is checked before the
//!   upgrade.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;

use crate::compose::{Outgoing, message_id, render};
use crate::credential::{self, Credential};
use crate::discover::{SMTP_PORTS, smtp_candidates};
use crate::guard::HostVerdict;
use crate::imap::Account;
use crate::resolve::Resolver;
use crate::words;

const COMMAND: Duration = Duration::from_secs(20);
/// The body can be large and the server may be slow to accept it.
const DATA: Duration = Duration::from_secs(60);

/// The longest reply line RATA reads: twice what RFC 5321 allows.
const LINE_MAX: usize = 1024;

/// What a failure says in place of a secret a server repeated.
const HIDDEN: &str = "[hidden]";

/// A server's words, fit to put in front of the customer, who may paste them
/// into a bug report: the first line, without control or bidi-control
/// characters, at most 200 of them — the rule `imap.rs` keeps for NO and BAD
/// (`one_line`).
///
/// While signing in, `secrets` are what RATA sent (the password, the token
/// and the base64 each travelled as). Each is taken out wherever it appears,
/// in any case, and then so is every run of 16 or more base64 characters,
/// which is how a sign-in line cut short would still carry most of one. All
/// of that happens before the line is shortened, or a cut could leave part
/// of a secret that no longer matches.
fn said(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_string();
    if !secrets.is_empty() {
        for secret in secrets.iter().filter(|s| !s.is_empty()) {
            text = hide(&text, secret);
        }
        text = hide_base64_runs(&text);
    }
    let line: String = text
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .take(200)
        .collect();
    line.trim().to_string()
}

/// `text` with every occurrence of `secret` replaced, ignoring ASCII case.
/// Lower-casing ASCII never changes a string's length, so a match's place in
/// the lower-cased copy is its place in the original.
fn hide(text: &str, secret: &str) -> String {
    let (hay, needle) = (text.to_ascii_lowercase(), secret.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        out.push_str(&text[from..from + at]);
        out.push_str(HIDDEN);
        from += at + needle.len();
    }
    out.push_str(&text[from..]);
    out
}

/// `text` with every run of 16 or more base64 characters (and its padding)
/// replaced. Words that long are rare in what a server says while refusing a
/// sign-in; a credential in base64 is never shorter.
fn hide_base64_runs(text: &str) -> String {
    fn flush(run: &mut String, out: &mut String) {
        if run.trim_end_matches('=').len() >= 16 {
            out.push_str(HIDDEN);
        } else {
            out.push_str(run);
        }
        run.clear();
    }
    let is_b64 = |c: char| c.is_ascii_alphanumeric() || c == '+' || c == '/';
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if (is_b64(c) && !run.ends_with('=')) || (c == '=' && !run.is_empty()) {
            run.push(c);
        } else if is_b64(c) {
            // Padding ends a run; this starts the next.
            flush(&mut run, &mut out);
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The characters that reorder text on screen (U+200E, U+200F,
/// U+202A–U+202E, U+2066–U+2069).
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// How long a message of `len` bytes may take to upload, and then to be
/// accepted. A minute for any message, plus a second per 64 KiB: a 24 MB
/// message with attachments gets over six minutes, enough for a slow hotel
/// connection, where a flat minute would fail it halfway through every time.
fn data_timeout(len: usize) -> Duration {
    DATA + Duration::from_secs((len / (64 * 1024)) as u64)
}

/// What happened to a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Accepted, and by whom — worth showing, because "sent via
    /// smtp-mail.outlook.com" is the answer to "did it actually go?".
    ///
    /// `message_id` is the Message-ID RATA wrote into it, without the angle
    /// brackets — how the copy the provider files in Sent is recognised as
    /// this same message.
    Ok {
        via: String,
        id: String,
        message_id: String,
    },
    /// Refused before a socket was opened.
    Host(String),
    /// The server rejected the password. Every remaining candidate is the same
    /// password at another address, so this stops the search.
    Auth(String),
    /// The server did not accept the OAuth access token — expired, most
    /// likely. Stops the search like `Auth`, but the fix is a fresh token,
    /// not a new password: this is never a verdict on anything typed.
    OAuth(String),
    /// A server took the message and said no to it — a bad recipient, a size
    /// limit, a spam rule. Retrying changes nothing; the customer has to.
    Rejected(String),
    /// Nothing answered.
    Net(String),
}

/// One reply from the server: the code and everything it said.
#[derive(Debug, Clone)]
struct Reply {
    code: u16,
    text: String,
}

impl Reply {
    fn ok(&self) -> bool {
        (200..400).contains(&self.code)
    }
    /// 5xx is permanent — the server will say the same thing next time, so
    /// there is nothing to gain by trying another port or another name.
    fn permanent(&self) -> bool {
        self.code >= 500
    }
    /// Whether this reply is the server rejecting the password.
    ///
    /// Only ever a permanent (5xx) reply. A 4xx during sign-in — Gmail's
    /// `454 4.7.0 Too many login attempts, please try again later` — is the
    /// server asking for time, and calling it a wrong password marks the
    /// mailbox as needing a relink over what is really a rate limit.
    ///
    /// The codes are definitive wherever they appear. The words ("login",
    /// "password"…) are only believed while signing in: elsewhere they turn
    /// up in refusals that are about something else entirely.
    fn is_auth(&self, signing_in: bool) -> bool {
        self.permanent()
            && (matches!(self.code, 530 | 534 | 535 | 538)
                || (signing_in && crate::discover::is_auth_failure(&self.text)))
    }
}

/// The two stream shapes a submission connection can have. An enum rather than
/// a trait object because there are exactly two and they are known here.
enum Wire {
    Plain(BufReader<TcpStream>),
    // Boxed only because a TLS session is an order of magnitude larger than a
    // bare socket, and an enum is as big as its largest variant.
    Tls(Box<BufReader<TlsStream<TcpStream>>>),
}

macro_rules! on_wire {
    ($self:expr, $io:ident => $body:expr) => {
        match $self {
            Wire::Plain($io) => $body,
            Wire::Tls($io) => $body,
        }
    };
}

impl Wire {
    async fn say(&mut self, line: &str) -> Result<(), String> {
        let out = format!("{line}\r\n");
        on_wire!(self, io => {
            io.get_mut().write_all(out.as_bytes()).await.map_err(|e| e.to_string())?;
            io.get_mut().flush().await.map_err(|e| e.to_string())
        })
    }

    /// Read one complete reply. SMTP continues a reply across lines with
    /// `250-` and ends it with `250 `, so a single read is not enough.
    ///
    /// A line is read up to [`LINE_MAX`] bytes and no further: RFC 5321 caps
    /// one at 512, and a server that goes on is refused rather than waited
    /// on. An error may quote the line; whoever shows it passes it through
    /// [`said`].
    async fn hear(&mut self) -> Result<Reply, String> {
        let mut text = String::new();
        loop {
            let mut raw = Vec::new();
            let read = on_wire!(self, io => {
                (&mut *io).take(LINE_MAX as u64).read_until(b'\n', &mut raw).await
            });
            match read {
                Ok(0) => return Err("the server closed the connection".into()),
                Ok(_) => {}
                Err(e) => return Err(e.to_string()),
            }
            if raw.last() != Some(&b'\n') && raw.len() >= LINE_MAX {
                return Err(format!(
                    "the server sent a line longer than the {LINE_MAX} bytes RATA reads"
                ));
            }
            let line = String::from_utf8_lossy(&raw);
            let line = line.trim_end_matches(['\r', '\n']);
            // Three ASCII digits, checked as bytes: slicing a string at 3
            // panics when a character straddles it.
            let digits = line.as_bytes().get(..3);
            if !digits.is_some_and(|d| d.iter().all(u8::is_ascii_digit)) {
                return Err(format!("the server said something unexpected: {line}"));
            }
            let code = line[..3]
                .parse::<u16>()
                .map_err(|_| format!("the server said something unexpected: {line}"))?;
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(line[3..].trim_start_matches(['-', ' ']));
            // A space in the fourth column ends the reply; a hyphen continues
            // it. A line of exactly three characters ends it too.
            if line.as_bytes().get(3) != Some(&b'-') {
                return Ok(Reply { code, text });
            }
            if text.len() > 8192 {
                return Err("the server would not stop talking".into());
            }
        }
    }

    async fn ask(&mut self, line: &str) -> Result<Reply, String> {
        self.say(line).await?;
        self.hear().await
    }
}

/// Send one message, trying each submission host and port until one takes it.
pub async fn send(resolver: &Resolver, acct: &Account, msg: &Outgoing) -> Sent {
    // Sending as somebody else from this account would be forgery, and the
    // server would refuse it anyway — but refusing here says why.
    if !msg.from.as_str().eq_ignore_ascii_case(acct.email.trim()) {
        return Sent::Rejected(format!(
            "This message says it is from {}, but the account sending it is {}. Mail can only be sent from the account it belongs to.",
            msg.from.as_str(),
            acct.email
        ));
    }
    if msg.to.is_empty() {
        return Sent::Rejected("There is nobody to send this to.".into());
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let unique = message_id(msg, nanos);
    let written = format!("{unique}@{}", msg.from.domain());
    let body = render(msg, &now_rfc2822(), &unique);
    let to = envelope(&body, &msg.bcc);

    let hosts = smtp_candidates(&acct.host, &acct.email);
    let mut blocked: Option<String> = None;
    let mut last = String::new();

    for host in &hosts {
        for (port, implicit) in SMTP_PORTS {
            match attempt(resolver, acct, host, port, implicit, &body, &to).await {
                Sent::Ok { via, id, .. } => {
                    return Sent::Ok {
                        via,
                        id,
                        message_id: written,
                    };
                }
                // The password is wrong, or the message itself was refused.
                // Another port would produce the same answer, and repeating a
                // rejected password is how accounts get locked.
                done @ (Sent::Auth(_) | Sent::OAuth(_) | Sent::Rejected(_)) => return done,
                // A host pointing somewhere private is not retried on another
                // port, but a *different* host might still be fine.
                Sent::Host(why) => {
                    blocked.get_or_insert(why);
                    break;
                }
                Sent::Net(why) => last = why,
            }
        }
    }

    if let Some(why) = blocked {
        return Sent::Host(why);
    }
    let first = hosts
        .first()
        .map(String::as_str)
        .unwrap_or("the mail server");
    Sent::Net(format!(
        "Could not reach {first} to send: {}. Check with your provider that sending from a mail app is enabled for this account.",
        last.trim_end_matches('.')
    ))
}

/// One host, one port, start to finish.
async fn attempt(
    resolver: &Resolver,
    acct: &Account,
    host: &str,
    port: u16,
    implicit: bool,
    body: &str,
    to: &[String],
) -> Sent {
    let (name, addrs) = match crate::resolve::resolve_public(resolver, host).await {
        Ok(found) => found,
        Err(HostVerdict::NotFound) => return Sent::Net(HostVerdict::NotFound.explain(host)),
        Err(verdict) => return Sent::Host(verdict.explain(host)),
    };

    let tcp = match crate::imap::dial(&addrs, port, host).await {
        Ok(s) => s,
        Err(why) => return Sent::Net(why),
    };

    let mut wire = if implicit {
        match crate::imap::wrap_tls(tcp, &name, host).await {
            Ok(s) => Wire::Tls(Box::new(BufReader::new(s))),
            Err(why) => return Sent::Net(why),
        }
    } else {
        Wire::Plain(BufReader::new(tcp))
    };

    // The greeting comes first, unasked.
    match timeout(COMMAND, wire.hear()).await {
        Ok(Ok(r)) if r.ok() => {}
        Ok(Ok(r)) => {
            return Sent::Net(format!(
                "{host} would not accept mail: {}",
                said(&r.text, &[])
            ));
        }
        Ok(Err(e)) => return Sent::Net(format!("{host}: {}", said(&e, &[]))),
        Err(_) => return Sent::Net(format!("{host} did not answer in time.")),
    }

    let me = ehlo_name(&acct.email);
    let mut caps = match step(&mut wire, &format!("EHLO {me}"), host).await {
        Ok(r) => r.text,
        Err(sent) => return sent,
    };

    if !implicit {
        wire = match starttls(wire, &name, host).await {
            Ok(w) => w,
            Err(sent) => return sent,
        };
        // Everything the server said before the upgrade is discarded: it was
        // said in the clear and an attacker could have written it. RFC 3207
        // requires the client to re-issue EHLO, and this is why.
        caps = match step(&mut wire, &format!("EHLO {me}"), host).await {
            Ok(r) => r.text,
            Err(sent) => return sent,
        };
    }

    if let Err(sent) = authenticate(&mut wire, acct, &caps, host).await {
        return sent;
    }

    let envelope_from = format!("MAIL FROM:<{}>", acct.email.trim().to_ascii_lowercase());
    if let Err(sent) = step(&mut wire, &envelope_from, host).await {
        return sent;
    }
    // See `envelope`: the headers' own list, and Bcc.
    for rcpt in to {
        if let Err(sent) = step(&mut wire, &format!("RCPT TO:<{rcpt}>"), host).await {
            return sent;
        }
    }

    match step(&mut wire, "DATA", host).await {
        Ok(r) if r.code != 354 => {
            return Sent::Rejected(format!(
                "{host} would not take the message: {}",
                said(&r.text, &[])
            ));
        }
        Ok(_) => {}
        Err(sent) => return sent,
    }

    let allowed = data_timeout(body.len());
    if let Err(e) = timeout(allowed, wire.say(body))
        .await
        .unwrap_or(Err("timed out".into()))
    {
        return Sent::Net(format!("{host}: {}", said(&e, &[])));
    }
    let accepted = match timeout(allowed, wire.ask(".")).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Sent::Net(format!("{host}: {}", said(&e, &[]))),
        Err(_) => return Sent::Net(format!("{host} did not confirm the message in time.")),
    };
    let _ = timeout(COMMAND, wire.ask("QUIT")).await;

    if !accepted.ok() {
        return Sent::Rejected(format!(
            "{host} did not accept the message: {}",
            said(&accepted.text, &[])
        ));
    }
    Sent::Ok {
        via: format!("{host}:{port}"),
        id: said(&accepted.text, &[]),
        message_id: String::new(),
    }
}

/// Send a command and turn an unhappy answer into the right kind of failure.
async fn step(wire: &mut Wire, command: &str, host: &str) -> Result<Reply, Sent> {
    exchange(wire, command, host, None).await
}

/// The same, for the commands that carry the credentials — the only place a
/// refusal's wording is allowed to decide that the password was wrong.
/// `secrets` never appear in what a failure says (see [`said`]).
async fn sign_in_step(
    wire: &mut Wire,
    command: &str,
    host: &str,
    secrets: &[String],
) -> Result<Reply, Sent> {
    exchange(wire, command, host, Some(secrets)).await
}

/// `signing_in` carries what was sent to sign in, when that is what this is.
async fn exchange(
    wire: &mut Wire,
    command: &str,
    host: &str,
    signing_in: Option<&[String]>,
) -> Result<Reply, Sent> {
    let secrets = signing_in.unwrap_or_default();
    let clean = |text: &str| said(text, secrets);
    match timeout(COMMAND, wire.ask(command)).await {
        Ok(Ok(r)) if r.ok() => Ok(r),
        Ok(Ok(r)) if r.is_auth(signing_in.is_some()) => {
            Err(Sent::Auth(refused(host, &clean(&r.text))))
        }
        Ok(Ok(r)) if r.permanent() => Err(Sent::Rejected(format!(
            "{host} refused: {}",
            clean(&r.text)
        ))),
        // 4xx is the server asking to be tried again later, so the next
        // candidate is worth a go.
        Ok(Ok(r)) => Err(Sent::Net(format!(
            "{host} was not ready: {}",
            clean(&r.text)
        ))),
        Ok(Err(e)) => Err(Sent::Net(format!("{host}: {}", clean(&e)))),
        Err(_) => Err(Sent::Net(format!("{host} did not answer in time."))),
    }
}

fn refused(host: &str, why: &str) -> String {
    format!(
        "{host} refused the sign-in for sending. Some providers issue a separate app password for mail apps — regenerate it and relink the account. The server said: {}",
        why.trim()
    )
}

/// Upgrade a plaintext connection, having checked that nothing was injected
/// into it first.
///
/// The check is the point. A server's `220 Ready to start TLS` arrives in the
/// clear, and an attacker who can write to the connection can append further
/// commands to that response. Those bytes sit in the read buffer, and a client
/// that upgrades without looking will process them *after* the handshake as
/// though the server had sent them inside the encrypted session. That is
/// CVE-2011-0411 and it has been rediscovered in mail clients repeatedly since.
/// So: if there is a single unread byte before the handshake, the connection is
/// abandoned.
async fn starttls(wire: Wire, name: &str, host: &str) -> Result<Wire, Sent> {
    let mut wire = wire;
    let ready = match timeout(COMMAND, wire.ask("STARTTLS")).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(Sent::Net(format!("{host}: {}", said(&e, &[])))),
        Err(_) => return Err(Sent::Net(format!("{host} did not answer in time."))),
    };
    if !ready.ok() {
        return Err(Sent::Net(format!(
            "{host} will not encrypt the connection, and RATA will not send a password without one: {}",
            said(&ready.text, &[])
        )));
    }

    let io = match wire {
        Wire::Plain(io) => io,
        // Already encrypted; STARTTLS inside TLS is not a thing.
        Wire::Tls(io) => return Ok(Wire::Tls(io)),
    };
    if !io.buffer().is_empty() {
        return Err(Sent::Net(format!(
            "{host} sent something extra before encryption started, so RATA hung up rather than trusting the rest of the conversation."
        )));
    }

    match crate::imap::wrap_tls(io.into_inner(), name, host).await {
        Ok(s) => Ok(Wire::Tls(Box::new(BufReader::new(s)))),
        Err(why) => Err(Sent::Net(why)),
    }
}

/// Sign in, using whichever mechanism the server offered.
async fn authenticate(wire: &mut Wire, acct: &Account, caps: &str, host: &str) -> Result<(), Sent> {
    let upper = caps.to_ascii_uppercase();
    let user = acct.email.trim();
    let pass = match &acct.credential {
        Credential::Password(pass) => pass,
        Credential::OAuth { user, access_token } => {
            return xoauth2(wire, user, access_token, &upper, host)
                .await
                .map_err(|sent| redacted(sent, &acct.credential));
        }
    };

    // \0user\0pass, base64, for AUTH PLAIN.
    let mut plain = Vec::new();
    plain.push(0);
    plain.extend_from_slice(user.as_bytes());
    plain.push(0);
    plain.extend_from_slice(pass.as_bytes());
    let plain = words::base64_encode(&plain);
    let pass64 = words::base64_encode(pass.as_bytes());
    // Whatever a server repeats of these, a failure never says.
    let secrets = [pass.clone(), pass64.clone(), plain.clone()];

    if upper.contains("AUTH") && upper.contains("PLAIN") {
        // One round trip, so it is preferred.
        let command = format!("AUTH PLAIN {plain}");
        sign_in_step(wire, &command, host, &secrets).await?;
        return Ok(());
    }

    if upper.contains("AUTH") && upper.contains("LOGIN") {
        sign_in_step(wire, "AUTH LOGIN", host, &secrets).await?;
        let user64 = words::base64_encode(user.as_bytes());
        sign_in_step(wire, &user64, host, &secrets).await?;
        sign_in_step(wire, &pass64, host, &secrets).await?;
        return Ok(());
    }

    // Net rather than Auth, deliberately: nothing was rejected here, this
    // server simply is not offering to take a sign-in on this port. Calling it
    // an auth failure would abandon the other port and the other candidate
    // hosts, one of which is very likely the real submission server.
    Err(Sent::Net(format!(
        "{host} did not offer a way to sign in that RATA can use"
    )))
}

/// Present an OAuth access token with `AUTH XOAUTH2` (`caps` upper-cased).
///
/// In two steps — the mechanism, the server's empty `334`, then the string —
/// rather than on one line: that is how Microsoft documents it, and a token
/// runs to a couple of thousand characters, past what some servers take on
/// a command line.
///
/// A refused token may get a `334` carrying base64 JSON that says why
/// (Google sends `{"status":"401",…}`); SASL wants an answer before the
/// server finishes, so an empty line goes back and the final `535` is read.
/// A refusal is [`Sent::OAuth`], never [`Sent::Auth`]: whatever words it
/// comes in, the fix is a fresh token.
async fn xoauth2(
    wire: &mut Wire,
    user: &str,
    token: &str,
    caps: &str,
    host: &str,
) -> Result<(), Sent> {
    // Net, as for a server offering no password mechanism: another port or
    // host may well offer it.
    if !(caps.contains("AUTH") && caps.contains("XOAUTH2")) {
        return Err(Sent::Net(format!(
            "{host} did not offer a way to sign in with a token"
        )));
    }
    let Some(sasl) = credential::xoauth2(user, token) else {
        return Err(Sent::OAuth(format!(
            "The sign-in token RATA holds is not one {host} would accept."
        )));
    };

    let sasl = words::base64_encode(&sasl);
    // Whatever a server repeats of these, a failure never says.
    let secrets = [token.to_string(), sasl.clone()];

    let ready = talk(wire, "AUTH XOAUTH2", host, &secrets).await?;
    if ready.code != 334 {
        // Only the mechanism's name has been sent, so whatever this is, it is
        // not a verdict on the token.
        return Err(Sent::Net(format!(
            "{host} would not take a token sign-in: {}",
            said(&ready.text, &secrets)
        )));
    }

    let mut answer = talk(wire, &sasl, host, &secrets).await?;
    let mut status = None;
    if answer.code == 334 {
        status = credential::challenge_status(&words::base64(answer.text.as_bytes()));
        answer = talk(wire, "", host, &secrets).await?;
    }

    if (200..300).contains(&answer.code) {
        return Ok(());
    }
    if answer.permanent() {
        let status = status.map(|s| format!(" (status {s})")).unwrap_or_default();
        return Err(Sent::OAuth(format!(
            "{host} did not accept RATA's sign-in token for sending — it has expired or been withdrawn{status}. The server said: {}",
            said(&answer.text, &secrets)
        )));
    }
    // 4xx — Gmail's "too many login attempts" among them — is the server
    // asking for time, as it is for a password.
    Err(Sent::Net(format!(
        "{host} was not ready: {}",
        said(&answer.text, &secrets)
    )))
}

/// One command and its reply, whatever the reply says. Only a broken or
/// silent connection is a failure here, and it never repeats `secrets`.
async fn talk(wire: &mut Wire, line: &str, host: &str, secrets: &[String]) -> Result<Reply, Sent> {
    match timeout(COMMAND, wire.ask(line)).await {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(Sent::Net(format!("{host}: {}", said(&e, secrets)))),
        Err(_) => Err(Sent::Net(format!("{host} did not answer in time."))),
    }
}

/// `sent` with the credential's secret taken out of what it says.
fn redacted(sent: Sent, credential: &Credential) -> Sent {
    let r = |why: String| credential::redact(&why, credential);
    match sent {
        Sent::Host(why) => Sent::Host(r(why)),
        Sent::Auth(why) => Sent::Auth(r(why)),
        Sent::OAuth(why) => Sent::OAuth(r(why)),
        Sent::Rejected(why) => Sent::Rejected(r(why)),
        Sent::Net(why) => Sent::Net(r(why)),
        ok @ Sent::Ok { .. } => ok,
    }
}

/// What to call ourselves in EHLO. The customer's own domain: a submission
/// server does not check it, and the alternative is leaking the machine's
/// hostname to every server it talks to.
fn ehlo_name(email: &str) -> String {
    let d = crate::key::domain_of(email);
    if d.is_empty() { "localhost".into() } else { d }
}

/// The recipients, read back out of the rendered message: To and Cc, each
/// address once. Folded lines are joined back to their header first.
fn recipients(rendered: &str) -> Vec<String> {
    let head = rendered.split("\r\n\r\n").next().unwrap_or_default();
    let mut headers: Vec<String> = Vec::new();
    for line in head.split("\r\n") {
        match headers.last_mut() {
            Some(last) if line.starts_with(' ') || line.starts_with('\t') => last.push_str(line),
            _ => headers.push(line.to_string()),
        }
    }
    let mut out: Vec<String> = Vec::new();
    for header in &headers {
        let Some(rest) = header
            .strip_prefix("To:")
            .or_else(|| header.strip_prefix("Cc:"))
        else {
            continue;
        };
        for piece in rest.split(',') {
            let addr = piece.trim().trim_start_matches('<').trim_end_matches('>');
            if !addr.is_empty() && !out.iter().any(|a| a == addr) {
                out.push(addr.to_string());
            }
        }
    }
    out
}

/// Who the message is handed over for. Everyone in the headers is read back
/// out of the rendered message so there is exactly one visible list — an
/// envelope that disagrees with To: and Cc: is how mail goes to somebody the
/// customer cannot see. Bcc is the one deliberate exception, and so the only
/// addition: it is never in the headers (that is what makes it blind), and it
/// comes only from the customer's own Bcc line.
fn envelope(rendered: &str, bcc: &[crate::compose::Address]) -> Vec<String> {
    let mut out = recipients(rendered);
    for a in bcc {
        if !out.iter().any(|x| x == a.as_str()) {
            out.push(a.as_str().to_string());
        }
    }
    out
}

/// `Date:` as RFC 5322 wants it, in UTC.
///
/// Always `+0000`, and that is a choice rather than laziness: a local offset in
/// this header tells every correspondent, and every server in between, roughly
/// where the customer is sitting. It is the kind of thing a mail client leaks
/// without anybody deciding to.
fn now_rfc2822() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    rfc2822(secs)
}

/// One Unix timestamp as an RFC 5322 date. Written out because the alternative
/// was a timezone database this crate has no use for — the only zone here is
/// UTC, and the calendar arithmetic for that is a dozen lines.
fn rfc2822(unix_secs: i64) -> String {
    const DAY: i64 = 86_400;
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    // Floor division, so times before 1970 land on the right day rather than
    // the one after it.
    let days = unix_secs.div_euclid(DAY);
    let secs = unix_secs.rem_euclid(DAY);
    let (year, month, day) = civil_from_days(days);
    // 1 January 1970 was a Thursday, which is why the table starts there.
    let weekday = DAYS[days.rem_euclid(7) as usize];

    format!(
        "{weekday}, {day:02} {} {year:04} {:02}:{:02}:{:02} +0000",
        MONTHS[(month - 1) as usize],
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Days since the epoch to a calendar date. Howard Hinnant's algorithm, which
/// is correct for every proleptic Gregorian date and has no branches for leap
/// years — the century rules fall out of the arithmetic.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so that a leap day is the last day of the
    // year rather than the middle of it.
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Address;
    use crate::discover::IMAP_PORT;

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

    fn msg() -> Outgoing {
        Outgoing {
            from: Address::parse("owner@example.com").unwrap(),
            from_name: None,
            to: vec![Address::parse("someone@elsewhere.org").unwrap()],
            cc: vec![],
            bcc: vec![],
            subject: "Hello".into(),
            body: "Hi there.".into(),
            in_reply_to: None,
            attachments: vec![],
        }
    }

    #[test]
    fn sending_as_somebody_else_is_refused_before_anything_is_dialled() {
        rt().block_on(async {
            let r = Resolver::system().unwrap();
            let mut m = msg();
            m.from = Address::parse("someone.else@example.com").unwrap();
            match send(&r, &acct("imap.example.com"), &m).await {
                Sent::Rejected(why) => assert!(why.contains("can only be sent from"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_submission_host_pointing_inside_the_network_is_refused() {
        rt().block_on(async {
            let r = Resolver::system().unwrap();
            // The IMAP host is stored; the SMTP name is derived from it and has
            // never been checked by anything. That was the hole in the server
            // version, where sending was the one egress path with no guard.
            match send(&r, &acct("127.0.0.1"), &msg()).await {
                Sent::Host(why) => assert!(why.contains("private network"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_host_that_does_not_exist_is_a_network_failure_not_a_refusal() {
        rt().block_on(async {
            let r = Resolver::system().unwrap();
            let host = format!("imap.nx-{}.invalid", std::process::id());
            match send(&r, &acct(&host), &msg()).await {
                Sent::Net(why) => assert!(why.contains("Could not reach"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn the_envelope_recipients_are_the_ones_in_the_message() {
        // Not a second list that could drift from the visible one.
        let mut m = msg();
        m.to = vec![
            Address::parse("a@example.com").unwrap(),
            Address::parse("b@example.org").unwrap(),
        ];
        let rendered = render(&m, "d", "i");
        assert_eq!(recipients(&rendered), ["a@example.com", "b@example.org"]);
        // Nothing from the body is ever read as a recipient.
        m.body = "\r\nTo: <sneaky@example.net>\r\nCc: <sneaky@example.net>\r\n".into();
        let rendered = render(&m, "d", "i");
        assert_eq!(recipients(&rendered), ["a@example.com", "b@example.org"]);
        // Nor from a file's headers.
        m.attachments.push(crate::compose::File {
            name: "x.txt".into(),
            mime: "text/plain".into(),
            data: b"To: <sneaky@example.net>\r\n".to_vec(),
        });
        assert_eq!(
            recipients(&render(&m, "d", "i")),
            ["a@example.com", "b@example.org"]
        );
    }

    #[test]
    fn blind_copies_are_in_the_envelope_and_nowhere_in_the_message() {
        let mut m = msg();
        m.cc = vec![Address::parse("dee@example.org").unwrap()];
        m.bcc = vec![
            Address::parse("boss@example.net").unwrap(),
            Address::parse("dee@example.org").unwrap(),
        ];
        let rendered = render(&m, "d", "i");
        assert!(!rendered.contains("boss@example.net"), "{rendered}");
        assert!(!rendered.to_ascii_lowercase().contains("bcc"), "{rendered}");
        assert_eq!(
            envelope(&rendered, &m.bcc),
            [
                "someone@elsewhere.org",
                "dee@example.org",
                "boss@example.net"
            ]
        );
    }

    #[test]
    fn copied_and_folded_recipients_are_all_in_the_envelope_once() {
        let mut m = msg();
        m.to = (0..40)
            .map(|i| Address::parse(&format!("p{i}@example.org")).unwrap())
            .collect();
        m.cc = vec![
            Address::parse("dee@example.org").unwrap(),
            Address::parse("p3@example.org").unwrap(),
        ];
        let got = recipients(&render(&m, "d", "i"));
        assert_eq!(got.len(), 41, "{got:?}");
        assert_eq!(got[0], "p0@example.org");
        assert_eq!(got[39], "p39@example.org");
        assert_eq!(got[40], "dee@example.org");
    }

    #[test]
    fn a_reply_is_classified_by_what_it_means() {
        let r = |code, text: &str| Reply {
            code,
            text: text.into(),
        };
        assert!(r(250, "OK").ok());
        assert!(r(354, "Start mail input").ok());
        assert!(!r(535, "5.7.8 Username and Password not accepted").ok());
        assert!(r(535, "5.7.8 Username and Password not accepted").is_auth(true));
        assert!(r(530, "5.7.0 Authentication Required").is_auth(true));
        // The codes are definitive even outside sign-in.
        assert!(r(530, "5.7.0 Authentication Required").is_auth(false));
        // Permanent means the customer has to change something.
        assert!(r(550, "5.1.1 No such user").permanent());
        assert!(!r(550, "5.1.1 No such user").is_auth(true));
        // 4xx is "come back later", so another candidate is worth trying.
        assert!(!r(451, "4.7.1 Try again later").permanent());
    }

    #[test]
    fn being_asked_to_slow_down_is_not_a_wrong_password() {
        let r = |code, text: &str| Reply {
            code,
            text: text.into(),
        };
        // Exactly what Gmail sends when rate-limiting sign-ins. It says
        // "login", and it is a 4xx: come back later, not "wrong password".
        let slow = r(
            454,
            "4.7.0 Too many login attempts, please try again later.",
        );
        assert!(
            !slow.is_auth(true),
            "a rate limit read as a rejected password"
        );
        assert!(!slow.permanent());
        assert!(!r(454, "4.7.0 Temporary authentication failure").is_auth(true));
        assert!(!r(421, "4.7.0 Try again later, closing connection. (EHLO)").is_auth(false));
    }

    #[test]
    fn auth_words_outside_sign_in_do_not_condemn_the_password() {
        let r = |code, text: &str| Reply {
            code,
            text: text.into(),
        };
        // A refused recipient whose explanation happens to mention a login
        // page. It is about the recipient, not about this mailbox's password.
        let rcpt = r(
            550,
            "5.7.1 Recipient rejected, see https://example.com/login for policy",
        );
        assert!(!rcpt.is_auth(false));
        assert!(rcpt.permanent());
        // During sign-in the same wording is believed.
        assert!(r(550, "5.7.1 Invalid login or password").is_auth(true));
    }

    /// A server that says exactly what it is told to, so the client's half of
    /// the conversation can be tested without anybody else's mail server.
    ///
    /// The guard refuses loopback — deliberately, and that is not relaxed for a
    /// test — so these drive the wire directly rather than going through
    /// `send`. What is under test here is the protocol reading, which is where
    /// the subtle mistakes live.
    async fn scripted(script: &'static [&'static str]) -> Wire {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            for reply in script {
                if sock.write_all(reply.as_bytes()).await.is_err() {
                    return;
                }
                let _ = sock.flush().await;
                // Wait for the client's next command before the next reply,
                // except after the last one.
                let mut buf = [0u8; 512];
                if sock.read(&mut buf).await.unwrap_or(0) == 0 {
                    return;
                }
            }
        });
        Wire::Plain(BufReader::new(TcpStream::connect(addr).await.unwrap()))
    }

    #[test]
    fn a_reply_spread_over_several_lines_is_read_as_one() {
        rt().block_on(async {
            // What every real server sends in answer to EHLO.
            let mut wire = scripted(&["250-smtp.example.com at your service
250-SIZE 35882577
250-8BITMIME
250-AUTH LOGIN PLAIN
250 SMTPUTF8
"])
            .await;
            let r = wire.hear().await.unwrap();
            assert_eq!(r.code, 250);
            assert!(r.text.contains("AUTH LOGIN PLAIN"), "{}", r.text);
            assert!(r.text.contains("SMTPUTF8"), "{}", r.text);
            // And the whole thing counts as one reply, not five.
            assert!(r.ok());
        });
    }

    #[test]
    fn a_refusal_spread_over_several_lines_is_still_a_refusal() {
        rt().block_on(async {
            let mut wire = scripted(&["535-5.7.8 Username and Password not accepted.
535 5.7.8 For more information, go to support.example
"])
            .await;
            let r = wire.hear().await.unwrap();
            assert_eq!(r.code, 535);
            assert!(!r.ok() && r.is_auth(true), "{r:?}");
        });
    }

    #[test]
    fn nothing_injected_before_encryption_is_ever_acted_on() {
        rt().block_on(async {
            // The attack: the "220 Ready to start TLS" is in the clear, so
            // somebody who can write to the connection appends their own
            // commands to it. A client that upgrades without looking replays
            // those *inside* the encrypted session as though the server had
            // sent them. This is CVE-2011-0411, rediscovered in mail clients
            // every few years since.
            let wire = scripted(&[
                "220 example.com ESMTP
",
                "250-example.com
250 STARTTLS
",
                "220 2.0.0 Ready to start TLS
250 Injected by somebody else
",
            ])
            .await;
            let mut wire = wire;
            assert!(wire.hear().await.unwrap().ok(), "greeting");
            assert!(wire.ask("EHLO example.com").await.unwrap().ok(), "ehlo");

            match starttls(wire, "example.com", "example.com").await {
                Err(Sent::Net(why)) => {
                    assert!(why.contains("something extra before encryption"), "{why}");
                    assert!(why.contains("hung up"), "{why}");
                }
                Err(other) => panic!("wrong kind of failure: {other:?}"),
                Ok(_) => panic!("the injected command was not noticed"),
            }
        });
    }

    #[test]
    fn a_server_that_will_not_encrypt_never_sees_the_password() {
        rt().block_on(async {
            let wire = scripted(&[
                "220 example.com ESMTP
",
                "454 4.7.0 TLS not available
",
            ])
            .await;
            let mut wire = wire;
            assert!(wire.hear().await.unwrap().ok());
            match starttls(wire, "example.com", "example.com").await {
                Err(Sent::Net(why)) => assert!(why.contains("will not send a password"), "{why}"),
                Err(other) => panic!("wrong kind of failure: {other:?}"),
                Ok(_) => panic!("the password would have gone out unencrypted"),
            }
        });
    }

    #[test]
    fn a_server_talking_nonsense_is_given_up_on_rather_than_guessed_at() {
        rt().block_on(async {
            let mut wire = scripted(&["this is not SMTP at all
"])
            .await;
            assert!(wire.hear().await.is_err());

            let mut wire = scripted(&["
"])
            .await;
            assert!(wire.hear().await.is_err());
        });
    }

    #[test]
    fn a_connection_that_closes_mid_conversation_is_an_error_not_a_hang() {
        rt().block_on(async {
            let mut wire = scripted(&[]).await;
            let err = wire.hear().await.unwrap_err();
            assert!(err.contains("closed the connection"), "{err}");
        });
    }

    #[test]
    fn we_introduce_ourselves_as_the_domain_not_as_this_computer() {
        assert_eq!(ehlo_name("owner@example.com"), "example.com");
        assert_eq!(ehlo_name("nonsense"), "localhost");
    }

    #[test]
    fn the_date_header_is_the_shape_rfc_5322_asks_for() {
        let d = now_rfc2822();
        assert!(d.ends_with(" +0000"), "{d}");
        // "Wed, 16 Sep 2026 12:00:00 +0000"
        assert_eq!(d.split(' ').count(), 6, "{d}");
        assert_eq!(d.chars().filter(|c| *c == ':').count(), 2, "{d}");
    }

    #[test]
    fn the_calendar_is_right_including_the_awkward_cases() {
        // Checked against a real implementation rather than by hand: a leap
        // day, a non-leap century year's neighbour, a date before the epoch,
        // and the weekday, which is the part most likely to be quietly wrong.
        for (secs, expected) in [
            (0, "Thu, 01 Jan 1970 00:00:00 +0000"),
            (1_000_000_000, "Sun, 09 Sep 2001 01:46:40 +0000"),
            (1_789_564_800, "Wed, 16 Sep 2026 13:20:00 +0000"),
            (951_782_400, "Tue, 29 Feb 2000 00:00:00 +0000"),
            (4_102_444_800, "Fri, 01 Jan 2100 00:00:00 +0000"),
            (-1, "Wed, 31 Dec 1969 23:59:59 +0000"),
        ] {
            assert_eq!(rfc2822(secs), expected, "at {secs}");
        }
    }

    #[test]
    fn a_large_message_is_given_time_to_upload() {
        assert_eq!(data_timeout(2_000), DATA);
        let big = data_timeout(24 * 1024 * 1024);
        assert!(big >= Duration::from_secs(400), "{big:?}");
    }

    // ------------------------------------------------------- sign-in tests
    //
    // A server that answers each line with the next reply and records every
    // line it heard, so what RATA sends while signing in can be checked as
    // well as how it reads the answers. Line by line rather than read by
    // read: a token line is thousands of characters long.

    use std::sync::{Arc, Mutex};

    const TOKEN: &str = "EwBIA8l6BAAUbDba3x2OMJElkF7gJ4z/VbCPEz0AAZmp-secret";
    const CAPS: &str = "smtp.office365.com Hello SIZE 157286400 AUTH LOGIN XOAUTH2 8BITMIME";

    async fn scripted_sign_in(replies: Vec<String>) -> (Wire, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = sock.into_split();
            let mut lines = BufReader::new(r).lines();
            let mut replies = replies.into_iter();
            while let Ok(Some(line)) = lines.next_line().await {
                log.lock().unwrap().push(line);
                let Some(reply) = replies.next() else {
                    continue;
                };
                if w.write_all(format!("{reply}\r\n").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
        let wire = Wire::Plain(BufReader::new(TcpStream::connect(addr).await.unwrap()));
        (wire, seen)
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn oauth_acct() -> Account {
        Account {
            email: "me@outlook.com".into(),
            credential: Credential::oauth("me@outlook.com", TOKEN),
            host: "outlook.office365.com".into(),
            port: IMAP_PORT,
            label: "Outlook".into(),
        }
    }

    fn token_on_the_wire() -> String {
        words::base64_encode(&credential::xoauth2("me@outlook.com", TOKEN).unwrap())
    }

    #[test]
    fn a_token_signs_in_to_send_with_xoauth2() {
        rt().block_on(async {
            let (mut wire, log) =
                scripted_sign_in(lines(&["334 ", "235 2.7.0 Authentication successful"])).await;
            let signed = authenticate(&mut wire, &oauth_acct(), CAPS, "smtp.office365.com").await;
            assert!(signed.is_ok(), "{signed:?}");
            // The mechanism, then exactly the string — never AUTH PLAIN or
            // LOGIN, although the server offered LOGIN too.
            assert_eq!(
                *log.lock().unwrap(),
                ["AUTH XOAUTH2".to_string(), token_on_the_wire()]
            );
        });
    }

    #[test]
    fn an_expired_token_for_sending_is_an_oauth_failure_not_a_wrong_password() {
        rt().block_on(async {
            // Google's answer to an expired token: an error challenge, then
            // the same 535 it gives a wrong password.
            let challenge = words::base64_encode(
                br#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#,
            );
            let (mut wire, log) = scripted_sign_in(vec![
                "334 ".into(),
                format!("334 {challenge}"),
                "535-5.7.8 Username and Password not accepted. For more information, go to\r\n535 5.7.8  https://support.google.com/mail/?p=BadCredentials".into(),
            ])
            .await;
            match authenticate(&mut wire, &oauth_acct(), CAPS, "smtp.gmail.com").await {
                Err(Sent::OAuth(why)) => {
                    assert!(why.contains("status 401"), "{why}");
                    assert!(why.contains("not accepted"), "{why}");
                }
                other => panic!("wrong kind of failure: {other:?}"),
            }
            // The error challenge was answered with an empty line.
            let log = log.lock().unwrap();
            assert_eq!(log.len(), 3, "{log:?}");
            assert_eq!(log[2], "", "{log:?}");
        });
    }

    #[test]
    fn a_refused_token_for_sending_is_an_oauth_failure_not_a_wrong_password() {
        rt().block_on(async {
            // Microsoft's answer: a plain 535, no error challenge.
            let (mut wire, _) = scripted_sign_in(lines(&[
                "334 ",
                "535 5.7.3 Authentication unsuccessful [SN4PR0601CA0002.namprd06.prod.outlook.com]",
            ]))
            .await;
            match authenticate(&mut wire, &oauth_acct(), CAPS, "smtp.office365.com").await {
                Err(Sent::OAuth(why)) => {
                    assert!(why.contains("Authentication unsuccessful"), "{why}")
                }
                other => panic!("wrong kind of failure: {other:?}"),
            }
        });
    }

    #[test]
    fn a_server_asking_for_time_is_not_a_refused_token() {
        rt().block_on(async {
            let (mut wire, _) = scripted_sign_in(lines(&[
                "334 ",
                "454 4.7.0 Too many login attempts, please try again later.",
            ]))
            .await;
            assert!(matches!(
                authenticate(&mut wire, &oauth_acct(), CAPS, "smtp.gmail.com").await,
                Err(Sent::Net(_))
            ));
        });
    }

    #[test]
    fn a_server_that_does_not_offer_xoauth2_never_sees_the_token() {
        rt().block_on(async {
            let (mut wire, log) = scripted_sign_in(lines(&["235 ok"])).await;
            let caps = "mail.example.com Hello AUTH PLAIN LOGIN";
            assert!(matches!(
                authenticate(&mut wire, &oauth_acct(), caps, "mail.example.com").await,
                Err(Sent::Net(_))
            ));
            // Net, so the next port and host are still tried; and nothing,
            // above all no password mechanism, was attempted with the token.
            assert!(log.lock().unwrap().is_empty());

            // Nor one that refuses the mechanism before the token is sent.
            let (mut wire, log) =
                scripted_sign_in(lines(&["504 5.7.4 Unrecognized authentication type"])).await;
            assert!(matches!(
                authenticate(&mut wire, &oauth_acct(), CAPS, "mail.example.com").await,
                Err(Sent::Net(_))
            ));
            assert_eq!(*log.lock().unwrap(), ["AUTH XOAUTH2"]);
        });
    }

    #[test]
    fn a_token_is_never_in_what_a_sending_failure_says() {
        rt().block_on(async {
            let (mut wire, _) = scripted_sign_in(vec![
                "334 ".into(),
                format!("535 5.7.3 token {TOKEN} in {} refused", token_on_the_wire()),
            ])
            .await;
            let failed = authenticate(&mut wire, &oauth_acct(), CAPS, "smtp.office365.com").await;
            let Err(Sent::OAuth(why)) = &failed else {
                panic!("not an OAuth failure: {failed:?}");
            };
            for said in [
                why.clone(),
                format!("{failed:?}"),
                format!("{:?}", oauth_acct()),
            ] {
                assert!(!said.contains(TOKEN), "{said}");
                assert!(!said.contains("-secret"), "{said}");
                assert!(!said.contains(&token_on_the_wire()), "{said}");
            }
        });
    }

    #[test]
    fn a_password_still_signs_in_to_send_with_auth_plain() {
        rt().block_on(async {
            let (mut wire, log) = scripted_sign_in(lines(&["235 2.7.0 Accepted"])).await;
            let caps = "smtp.gmail.com at your service AUTH LOGIN PLAIN XOAUTH2";
            assert!(
                authenticate(&mut wire, &acct("imap.gmail.com"), caps, "smtp.gmail.com")
                    .await
                    .is_ok()
            );
            let first = log.lock().unwrap()[0].clone();
            assert!(first.starts_with("AUTH PLAIN "), "{first}");

            // And a 535 to a password is still a refused password.
            let (mut wire, _) =
                scripted_sign_in(lines(&["535 5.7.8 Username and Password not accepted"])).await;
            assert!(matches!(
                authenticate(&mut wire, &acct("imap.gmail.com"), caps, "smtp.gmail.com").await,
                Err(Sent::Auth(_))
            ));
        });
    }

    // ----------------------------------------------- what a server may make RATA say

    /// A server that says `bytes` at once and then keeps the connection open
    /// without another word, as a hostile one would.
    async fn saying(bytes: Vec<u8>) -> Wire {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let _ = sock.write_all(&bytes).await;
            let _ = sock.flush().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        Wire::Plain(BufReader::new(TcpStream::connect(addr).await.unwrap()))
    }

    const PASS: &str = "not-a-real-password";

    fn plain_wire() -> String {
        words::base64_encode(format!("\0owner@example.com\0{PASS}").as_bytes())
    }

    /// Every way a failure could still carry the password, whole or in part.
    fn assert_no_password(said: &str) {
        let pass64 = words::base64_encode(PASS.as_bytes());
        let wire = plain_wire();
        assert!(!said.to_ascii_lowercase().contains(PASS), "{said}");
        assert!(!said.contains(&pass64), "{said}");
        // No run of 12 characters from the wire, which is how a cut-short
        // echo would still give it away.
        for at in 0..wire.len().saturating_sub(12) {
            assert!(!said.contains(&wire[at..at + 12]), "{said}");
        }
    }

    #[test]
    fn a_reply_line_longer_than_rata_reads_is_refused_not_kept() {
        rt().block_on(async {
            let mut wire = saying(format!("250 {}\r\n", "A".repeat(5000)).into_bytes()).await;
            let err = wire.hear().await.expect_err("a 5 000-byte line was read");
            assert!(err.len() < 300, "{} bytes: {err}", err.len());

            // And one that never ends is not waited on until the timeout.
            let mut wire = saying(vec![b'2'; 64 * 1024]).await;
            let heard = timeout(Duration::from_secs(5), wire.hear()).await;
            let err = heard
                .expect("RATA waited for the end of an endless line")
                .expect_err("an endless line was read");
            assert!(err.len() < 300, "{} bytes: {err}", err.len());
        });
    }

    #[test]
    fn a_reply_that_is_not_ascii_where_the_code_goes_is_an_error_not_a_crash() {
        rt().block_on(async {
            for said in [
                "2\u{20ac}0 hello\r\n",
                "ab\u{20ac} hello\r\n",
                "\u{e9}\u{e9} hi\r\n",
            ] {
                let mut wire = saying(said.as_bytes().to_vec()).await;
                assert!(wire.hear().await.is_err(), "{said:?}");
            }
        });
    }

    #[test]
    fn what_a_server_said_is_one_short_plain_line() {
        rt().block_on(async {
            let long = format!(
                "550 5.1.1 \u{1b}[31mno\u{7} such \u{202e}user{}",
                " very".repeat(150)
            );
            let (mut wire, _) = scripted_sign_in(vec![long]).await;
            let Err(Sent::Rejected(why)) =
                step(&mut wire, "RCPT TO:<a@example.org>", "smtp.example.com").await
            else {
                panic!("not a refusal");
            };
            let said = why.split_once("refused: ").map(|(_, s)| s).unwrap_or(&why);
            assert!(
                said.chars().count() <= 200,
                "{} chars: {said}",
                said.chars().count()
            );
            assert!(said.starts_with("5.1.1 [31mno such user"), "{said}");
            assert!(
                !why.chars().any(|c| c.is_control() || c == '\u{202e}'),
                "{why:?}"
            );
        });
    }

    #[test]
    fn a_password_the_server_repeats_is_never_in_what_sending_says() {
        rt().block_on(async {
            let caps = "mail.example.com AUTH PLAIN LOGIN";
            let echoes = [
                // Refused, with the command and the password read back.
                format!("535 5.7.8 AUTH PLAIN {} refused for {PASS}", plain_wire()),
                // The same, shouted.
                format!("535 5.7.8 password {} is wrong", PASS.to_ascii_uppercase()),
                // The command cut short, as a server with a short buffer might.
                format!(
                    "535 5.7.8 line too long: AUTH PLAIN {}",
                    &plain_wire()[..30]
                ),
                // Not a verdict on the password, but still repeated.
                format!("454 4.7.0 try later, {PASS}"),
                format!("554 5.7.0 {PASS} refused"),
                // Not even a reply.
                format!("oops AUTH PLAIN {} {PASS}", plain_wire()),
            ];
            for echo in echoes {
                let (mut wire, _) = scripted_sign_in(vec![echo.clone()]).await;
                let failed = authenticate(
                    &mut wire,
                    &acct("mail.example.com"),
                    caps,
                    "mail.example.com",
                )
                .await;
                assert!(failed.is_err(), "{echo}");
                assert_no_password(&format!("{failed:?}"));
            }

            // AUTH LOGIN sends the password on a line of its own.
            let login_only = "mail.example.com AUTH LOGIN";
            let pass64 = words::base64_encode(PASS.as_bytes());
            let (mut wire, _) = scripted_sign_in(vec![
                "334 VXNlcm5hbWU6".into(),
                "334 UGFzc3dvcmQ6".into(),
                format!("535 5.7.8 {pass64} ({PASS}) not accepted"),
            ])
            .await;
            let failed = authenticate(
                &mut wire,
                &acct("mail.example.com"),
                login_only,
                "mail.example.com",
            )
            .await;
            assert!(matches!(failed, Err(Sent::Auth(_))), "{failed:?}");
            assert_no_password(&format!("{failed:?}"));
        });
    }

    #[test]
    fn a_token_the_server_repeats_cut_short_is_still_hidden() {
        rt().block_on(async {
            let wire = token_on_the_wire();
            let (mut w, _) = scripted_sign_in(vec![
                "334 ".into(),
                format!("535 5.7.3 line too long: {}", &wire[..wire.len() - 7]),
            ])
            .await;
            let failed = authenticate(&mut w, &oauth_acct(), CAPS, "smtp.office365.com").await;
            let said = format!("{failed:?}");
            for at in 0..wire.len().saturating_sub(16) {
                assert!(!said.contains(&wire[at..at + 16]), "{said}");
            }
        });
    }
}
