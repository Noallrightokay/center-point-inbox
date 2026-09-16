//! Sending.
//!
//! Reading somebody's mail without being able to answer it is half a product,
//! and a message dragged between two inboxes has to leave from the one it
//! landed on or the transfer is theatre. The app password already stored for
//! reading is used for submission too, which is how these providers work — the
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

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;

use crate::compose::{message_id, render, Outgoing};
use crate::discover::{smtp_candidates, SMTP_PORTS};
use crate::guard::HostVerdict;
use crate::imap::Account;
use crate::resolve::Resolver;
use crate::words;

const COMMAND: Duration = Duration::from_secs(20);
/// The body can be large and the server may be slow to accept it.
const DATA: Duration = Duration::from_secs(60);

/// What happened to a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Accepted, and by whom — worth showing, because "sent via
    /// smtp-mail.outlook.com" is the answer to "did it actually go?".
    Ok { via: String, id: String },
    /// Refused before a socket was opened.
    Host(String),
    /// The server rejected the password. Every remaining candidate is the same
    /// password at another address, so this stops the search.
    Auth(String),
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
    fn is_auth(&self) -> bool {
        matches!(self.code, 530 | 534 | 535 | 538) || crate::discover::is_auth_failure(&self.text)
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
    async fn hear(&mut self) -> Result<Reply, String> {
        let mut text = String::new();
        loop {
            let mut line = String::new();
            let read = on_wire!(self, io => io.read_line(&mut line).await);
            match read {
                Ok(0) => return Err("the server closed the connection".into()),
                Ok(_) => {}
                Err(e) => return Err(e.to_string()),
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.len() < 3 {
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
    let body = render(msg, &now_rfc2822(), &message_id(msg, nanos));

    let hosts = smtp_candidates(&acct.host, &acct.email);
    let mut blocked: Option<String> = None;
    let mut last = String::new();

    for host in &hosts {
        for (port, implicit) in SMTP_PORTS {
            match attempt(resolver, acct, host, port, implicit, &body).await {
                Sent::Ok { via, id } => return Sent::Ok { via, id },
                // The password is wrong, or the message itself was refused.
                // Another port would produce the same answer, and repeating a
                // rejected password is how accounts get locked.
                done @ (Sent::Auth(_) | Sent::Rejected(_)) => return done,
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
    let first = hosts.first().map(String::as_str).unwrap_or("the mail server");
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
        Ok(Ok(r)) => return Sent::Net(format!("{host} would not accept mail: {}", r.text)),
        Ok(Err(e)) => return Sent::Net(format!("{host}: {e}")),
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
    // Recipients are read back out of the rendered message so there is exactly
    // one list: an envelope that disagrees with the To: header is how mail goes
    // to somebody the customer cannot see.
    for rcpt in recipients(body) {
        if let Err(sent) = step(&mut wire, &format!("RCPT TO:<{rcpt}>"), host).await {
            return sent;
        }
    }

    match step(&mut wire, "DATA", host).await {
        Ok(r) if r.code != 354 => {
            return Sent::Rejected(format!("{host} would not take the message: {}", r.text))
        }
        Ok(_) => {}
        Err(sent) => return sent,
    }

    if let Err(e) = timeout(DATA, wire.say(body)).await.unwrap_or(Err("timed out".into())) {
        return Sent::Net(format!("{host}: {e}"));
    }
    let accepted = match timeout(DATA, wire.ask(".")).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Sent::Net(format!("{host}: {e}")),
        Err(_) => return Sent::Net(format!("{host} did not confirm the message in time.")),
    };
    let _ = timeout(COMMAND, wire.ask("QUIT")).await;

    if !accepted.ok() {
        return Sent::Rejected(format!("{host} did not accept the message: {}", accepted.text));
    }
    Sent::Ok {
        via: format!("{host}:{port}"),
        id: accepted.text.clone(),
    }
}

/// Send a command and turn an unhappy answer into the right kind of failure.
async fn step(wire: &mut Wire, command: &str, host: &str) -> Result<Reply, Sent> {
    match timeout(COMMAND, wire.ask(command)).await {
        Ok(Ok(r)) if r.ok() => Ok(r),
        Ok(Ok(r)) if r.is_auth() => Err(Sent::Auth(refused(host, &r.text))),
        Ok(Ok(r)) if r.permanent() => Err(Sent::Rejected(format!("{host} refused: {}", r.text))),
        // 4xx is the server asking to be tried again later, so the next
        // candidate is worth a go.
        Ok(Ok(r)) => Err(Sent::Net(format!("{host} was not ready: {}", r.text))),
        Ok(Err(e)) => Err(Sent::Net(format!("{host}: {e}"))),
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
        Ok(Err(e)) => return Err(Sent::Net(format!("{host}: {e}"))),
        Err(_) => return Err(Sent::Net(format!("{host} did not answer in time."))),
    };
    if !ready.ok() {
        return Err(Sent::Net(format!(
            "{host} will not encrypt the connection, and RATA will not send a password without one: {}",
            ready.text
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

    if upper.contains("AUTH") && upper.contains("PLAIN") {
        // \0user\0pass, base64. One round trip, so it is preferred.
        let mut secret = Vec::new();
        secret.push(0);
        secret.extend_from_slice(user.as_bytes());
        secret.push(0);
        secret.extend_from_slice(acct.pass.as_bytes());
        let command = format!("AUTH PLAIN {}", words::base64_encode(&secret));
        step(wire, &command, host).await?;
        return Ok(());
    }

    if upper.contains("AUTH") && upper.contains("LOGIN") {
        step(wire, "AUTH LOGIN", host).await?;
        step(wire, &words::base64_encode(user.as_bytes()), host).await?;
        step(wire, &words::base64_encode(acct.pass.as_bytes()), host).await?;
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

/// What to call ourselves in EHLO. The customer's own domain: a submission
/// server does not check it, and the alternative is leaking the machine's
/// hostname to every server it talks to.
fn ehlo_name(email: &str) -> String {
    let d = crate::key::domain_of(email);
    if d.is_empty() { "localhost".into() } else { d }
}

/// The recipients, read back out of the rendered message.
fn recipients(rendered: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in rendered.split("\r\n") {
        if line.is_empty() {
            break; // end of headers
        }
        let Some(rest) = line.strip_prefix("To: ") else {
            continue;
        };
        for piece in rest.split(',') {
            let addr = piece.trim().trim_start_matches('<').trim_end_matches('>');
            if !addr.is_empty() {
                out.push(addr.to_string());
            }
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
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
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

    fn msg() -> Outgoing {
        Outgoing {
            from: Address::parse("owner@example.com").unwrap(),
            from_name: None,
            to: vec![Address::parse("someone@elsewhere.org").unwrap()],
            subject: "Hello".into(),
            body: "Hi there.".into(),
            in_reply_to: None,
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
        m.body = "\r\nTo: <sneaky@example.net>\r\n".into();
        let rendered = render(&m, "d", "i");
        assert_eq!(recipients(&rendered), ["a@example.com", "b@example.org"]);
    }

    #[test]
    fn a_reply_is_classified_by_what_it_means() {
        let r = |code, text: &str| Reply { code, text: text.into() };
        assert!(r(250, "OK").ok());
        assert!(r(354, "Start mail input").ok());
        assert!(!r(535, "5.7.8 Username and Password not accepted").ok());
        assert!(r(535, "5.7.8 Username and Password not accepted").is_auth());
        assert!(r(530, "5.7.0 Authentication Required").is_auth());
        // Permanent means the customer has to change something.
        assert!(r(550, "5.1.1 No such user").permanent());
        assert!(!r(550, "5.1.1 No such user").is_auth());
        // 4xx is "come back later", so another candidate is worth trying.
        assert!(!r(451, "4.7.1 Try again later").permanent());
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
            let mut wire = scripted(&[
                "250-smtp.example.com at your service
250-SIZE 35882577
250-8BITMIME
250-AUTH LOGIN PLAIN
250 SMTPUTF8
",
            ])
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
            let mut wire = scripted(&[
                "535-5.7.8 Username and Password not accepted.
535 5.7.8 For more information, go to support.example
",
            ])
            .await;
            let r = wire.hear().await.unwrap();
            assert_eq!(r.code, 535);
            assert!(!r.ok() && r.is_auth(), "{r:?}");
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
"]).await;
            assert!(wire.hear().await.is_err());

            let mut wire = scripted(&["
"]).await;
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
}
