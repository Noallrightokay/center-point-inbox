//! Turning a reply into a message an SMTP server will accept.
//!
//! Nodemailer did this on the server. Nothing here does, so it is written out —
//! and the parts that look fussy are the parts that are load-bearing.
//!
//! **Addresses and headers are checked, not trusted.** A recipient address and
//! a subject line both arrive from whatever the customer typed, and both end up
//! in headers. A newline in either of them ends the header and starts a new
//! one, so `Bcc:` can be added to a message by typing it into the subject box.
//! [`Address::parse`] and [`words::encode_header`] are where that stops.
//!
//! **The body is dot-stuffed.** In SMTP a line consisting of a single `.` ends
//! the message, so a message whose own text contains such a line would be cut
//! off there and the rest interpreted as SMTP commands. Every line starting
//! with `.` gets a second one, which the receiving server removes.
//!
//! **Line endings are CRLF, everywhere.** A bare newline in DATA is not legal
//! and different servers disagree about what to do with one.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::key::domain_of;
use crate::words;

/// One address, already checked. There is no way to build one that is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address(String);

impl Address {
    /// Parse an address as typed. `None` when it is not one — which includes
    /// every attempt to smuggle a second header into it.
    pub fn parse(raw: &str) -> Option<Self> {
        let mut addr = raw.trim();
        // "Name <a@b.c>" is what people paste. Take the angle brackets.
        if let Some(open) = addr.rfind('<') {
            let close = addr.rfind('>')?;
            if close < open {
                return None;
            }
            // Nothing may follow the closing bracket. Without this,
            // `"a" <b@c.com>, d@e.com` parses as b@c.com alone and d@e.com is
            // silently dropped — the customer sees the message sent and one of
            // the two recipients never hears about it. Refusing is the only
            // honest answer; [`Address::parse_list`] is how a list is meant to
            // be given.
            if !addr[close + 1..].trim().is_empty() {
                return None;
            }
            addr = addr[open + 1..close].trim();
        }
        let addr = addr.trim();
        if addr.len() < 3 || addr.len() > 254 {
            return None;
        }
        // Exactly one @, something either side, and a dot in the domain.
        let (local, domain) = addr.split_once('@')?;
        if local.is_empty() || domain.contains('@') || !domain.contains('.') {
            return None;
        }
        if domain.starts_with('.') || domain.ends_with('.') || domain.starts_with('-') {
            return None;
        }
        // Whitespace, controls and angle brackets are the injection vector, and
        // none of them belongs in a bare address anyway.
        if addr.chars().any(|c| {
            c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | ';' | '"')
        }) {
            return None;
        }
        Some(Address(addr.to_ascii_lowercase()))
    }

    /// A list as a person would type it: `a@b.com, c@d.com`.
    ///
    /// All or nothing. A list where one address is malformed is refused whole
    /// rather than sent to the addresses that happened to parse, because a
    /// reply that reached three of its four recipients is a worse outcome than
    /// one that reached none and said so.
    pub fn parse_list(raw: &str) -> Option<Vec<Self>> {
        let mut out = Vec::new();
        for piece in raw.split(',') {
            if piece.trim().is_empty() {
                continue;
            }
            out.push(Self::parse(piece)?);
        }
        if out.is_empty() { None } else { Some(out) }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn domain(&self) -> String {
        domain_of(&self.0)
    }
}

/// A message on its way out.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub from: Address,
    /// What to show as the sender's name. Optional: without one the address
    /// stands on its own, which is correct, just plainer.
    pub from_name: Option<String>,
    pub to: Vec<Address>,
    pub subject: String,
    pub body: String,
    /// The message being replied to, if any, so mail clients thread it.
    pub in_reply_to: Option<String>,
}

/// The full RFC 5322 message, CRLF throughout, ready for DATA.
///
/// `now_rfc2822` and `unique` are passed in rather than read here so the whole
/// thing can be tested byte for byte.
pub fn render(msg: &Outgoing, now_rfc2822: &str, unique: &str) -> String {
    let mut out = String::with_capacity(msg.body.len() + 512);

    let from = match &msg.from_name {
        // A display name is a quoted string, and the quoting has to survive a
        // name with a quote in it.
        Some(name) if !name.trim().is_empty() => {
            let shown = words::encode_header(name)
                .replace('\\', "")
                .replace('"', "'");
            format!("\"{}\" <{}>", shown, msg.from.as_str())
        }
        _ => format!("<{}>", msg.from.as_str()),
    };

    let to = msg
        .to
        .iter()
        .map(|a| format!("<{}>", a.as_str()))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = write!(out, "From: {from}\r\n");
    let _ = write!(out, "To: {to}\r\n");
    let _ = write!(out, "Subject: {}\r\n", words::encode_header(&msg.subject));
    let _ = write!(out, "Date: {now_rfc2822}\r\n");
    let _ = write!(out, "Message-ID: <{unique}@{}>\r\n", msg.from.domain());
    if let Some(parent) = &msg.in_reply_to {
        // Whatever the parent's Message-ID was, it came off the wire and is not
        // ours to trust.
        let id = words::encode_header(parent);
        let id = id.trim_matches(|c| c == '<' || c == '>');
        if !id.is_empty() && id.len() < 400 {
            let _ = write!(out, "In-Reply-To: <{id}>\r\nReferences: <{id}>\r\n");
        }
    }
    out.push_str("MIME-Version: 1.0\r\n");
    out.push_str("Content-Type: text/plain; charset=utf-8\r\n");

    // 7bit where the body allows it, because it stays readable in a raw
    // message; base64 where it does not, because 8BITMIME is advertised by most
    // servers and guaranteed by none, and a long line is illegal either way.
    let plain = msg.body.replace("\r\n", "\n").replace('\r', "\n");
    let simple = plain.is_ascii() && plain.split('\n').all(|l| l.len() <= 900);
    if simple {
        out.push_str("Content-Transfer-Encoding: 7bit\r\n\r\n");
        for line in plain.split('\n') {
            // Dot-stuffing: a line of a single "." would otherwise end the
            // message early and hand the rest to the server as commands.
            if line.starts_with('.') {
                out.push('.');
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
    } else {
        out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        let encoded = words::base64_encode(plain.as_bytes());
        // Base64 has no dots and no long lines once wrapped, so neither problem
        // above can arise.
        for chunk in encoded.as_bytes().chunks(76) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
            out.push_str("\r\n");
        }
    }
    out
}

/// A Message-ID that will not collide with anyone else's.
///
/// Not random — there is no source of randomness in this crate and adding one
/// for this would be silly. A digest of the clock, the process and the message
/// itself is unique enough for an identifier whose only job is to be different
/// from the last one.
pub fn message_id(msg: &Outgoing, nanos: u128) -> String {
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(msg.from.as_str().as_bytes());
    h.update(msg.subject.as_bytes());
    h.update(msg.body.as_bytes());
    h.finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> Address {
        Address::parse(s).unwrap_or_else(|| panic!("{s} should parse"))
    }

    fn msg(subject: &str, body: &str) -> Outgoing {
        Outgoing {
            from: addr("owner@example.com"),
            from_name: None,
            to: vec![addr("someone@elsewhere.org")],
            subject: subject.into(),
            body: body.into(),
            in_reply_to: None,
        }
    }

    #[test]
    fn the_addresses_people_actually_paste() {
        assert_eq!(addr("Owner@Example.com").as_str(), "owner@example.com");
        assert_eq!(
            addr("Jane Doe <jane@example.com>").as_str(),
            "jane@example.com"
        );
        assert_eq!(
            addr("  spaced@example.com  ").as_str(),
            "spaced@example.com"
        );
        assert_eq!(
            addr("a.b+tag@sub.example.co.uk").domain(),
            "sub.example.co.uk"
        );
    }

    #[test]
    fn an_address_cannot_carry_a_second_header_into_the_message() {
        for attack in [
            "a@b.com\r\nBcc: everyone@example.com",
            "a@b.com\nBcc: everyone@example.com",
            "a@b.com, everyone@example.com",
            "a@b.com everyone@example.com",
        ] {
            assert!(
                Address::parse(attack).is_none(),
                "{attack:?} should not parse"
            );
        }
    }

    #[test]
    fn a_second_recipient_is_never_quietly_dropped() {
        // This one parsed as c@d.com alone, and e@f.com would simply never have
        // received the message — no error, nothing in the interface, the reply
        // just does not arrive.
        assert!(Address::parse("\"a\" <c@d.com>, e@f.com").is_none());
        assert!(Address::parse("Jane <jane@example.com> and Bob").is_none());

        // The way to give more than one.
        let list = Address::parse_list("Jane <jane@example.com>, bob@example.org").unwrap();
        assert_eq!(
            list.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
            ["jane@example.com", "bob@example.org"]
        );
        // One bad address refuses the whole list rather than sending to the
        // rest.
        assert!(Address::parse_list("jane@example.com, nonsense").is_none());
        assert!(Address::parse_list("  ,  ").is_none());
        assert!(Address::parse_list("").is_none());
        // A trailing comma is a typo, not a failure.
        assert_eq!(Address::parse_list("a@b.com,").unwrap().len(), 1);
    }

    #[test]
    fn rubbish_is_refused_rather_than_sent_to() {
        for bad in [
            "",
            "   ",
            "nobody",
            "@example.com",
            "a@",
            "a@b",
            "a@@b.com",
            "a@.com",
            "a@b.",
        ] {
            assert!(Address::parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_plain_message_comes_out_readable() {
        let m = render(
            &msg("Invoice", "Hello there.\nThanks."),
            "Wed, 16 Sep 2026 12:00:00 +0000",
            "abc",
        );
        assert!(m.contains("From: <owner@example.com>\r\n"), "{m}");
        assert!(m.contains("To: <someone@elsewhere.org>\r\n"), "{m}");
        assert!(m.contains("Subject: Invoice\r\n"), "{m}");
        assert!(m.contains("Message-ID: <abc@example.com>\r\n"), "{m}");
        assert!(m.contains("Content-Transfer-Encoding: 7bit\r\n\r\n"), "{m}");
        assert!(m.ends_with("Hello there.\r\nThanks.\r\n"), "{m:?}");
        // Not one bare newline anywhere.
        assert!(!m.replace("\r\n", "").contains('\n'), "{m:?}");
    }

    #[test]
    fn a_line_of_a_single_dot_cannot_end_the_message_early() {
        // Without stuffing, everything after this line would be handed to the
        // SMTP server as commands.
        let m = render(&msg("x", "before\n.\nafter"), "d", "i");
        assert!(m.contains("\r\nbefore\r\n..\r\nafter\r\n"), "{m:?}");
        // And the subtler one: a line that merely starts with a dot.
        let m = render(&msg("x", ".hidden"), "d", "i");
        assert!(m.contains("\r\n..hidden\r\n"), "{m:?}");
    }

    #[test]
    fn a_subject_cannot_add_a_bcc() {
        let m = render(&msg("Hi\r\nBcc: everyone@example.com", "body"), "d", "i");
        assert!(!m.contains("Bcc:\r\n") && !m.contains("\r\nBcc:"), "{m}");
        assert!(
            m.contains("Subject: HiBcc: everyone@example.com\r\n"),
            "{m}"
        );
    }

    #[test]
    fn a_body_that_needs_it_is_encoded_rather_than_sent_raw() {
        let m = render(&msg("x", "Voilà — a café ☕"), "d", "i");
        assert!(m.contains("Content-Transfer-Encoding: base64\r\n"), "{m}");
        let body = m.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
        assert_eq!(
            String::from_utf8(words::base64(body.as_bytes())).unwrap(),
            "Voilà — a café ☕"
        );
        // A very long line is illegal in 7bit too, encoded or not.
        let long = render(&msg("x", &"a".repeat(2000)), "d", "i");
        assert!(
            long.contains("base64"),
            "a 2000-character line must not go out raw"
        );
        assert!(
            long.split("\r\n").all(|l| l.len() <= 998),
            "a line exceeded the limit"
        );
    }

    #[test]
    fn a_display_name_survives_a_quote_in_it() {
        let mut m = msg("x", "y");
        m.from_name = Some("O\"Brien \\ Co".into());
        let out = render(&m, "d", "i");
        let line = out.lines().next().unwrap();
        assert!(
            line.starts_with("From: \"") && line.ends_with("<owner@example.com>"),
            "{line}"
        );
        // One opening and one closing quote, and nothing that escapes them.
        assert_eq!(line.matches('"').count(), 2, "{line}");
        assert!(!line.contains('\\'), "{line}");
    }

    #[test]
    fn a_reply_threads_but_does_not_trust_what_it_is_replying_to() {
        let mut m = msg("Re: x", "y");
        m.in_reply_to = Some("<parent@example.com>\r\nBcc: everyone@example.com".into());
        let out = render(&m, "d", "i");
        assert!(out.contains("In-Reply-To: <parent@example.com"), "{out}");
        assert!(!out.contains("\r\nBcc:"), "{out}");
    }

    #[test]
    fn two_messages_do_not_share_an_id() {
        let a = msg("x", "y");
        assert_ne!(message_id(&a, 1), message_id(&a, 2));
        assert_ne!(message_id(&a, 1), message_id(&msg("x", "z"), 1));
        assert_eq!(message_id(&a, 1).len(), 32);
    }
}
