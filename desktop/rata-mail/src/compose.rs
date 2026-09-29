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
///
/// It may carry the name it was given with (`"Smith, Ann" <ann@example.org>`,
/// as the composer's suggestions write it), cleaned so that it can go into a
/// header as it is: see [`display_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    addr: String,
    name: Option<String>,
}

/// The longest display name kept, in characters. Enough for any real name,
/// and short enough that one address with its name, encoded, stays far
/// inside a header line's 998 bytes.
pub const NAME_MAX: usize = 64;

impl Address {
    /// Parse an address as typed. `None` when it is not one — which includes
    /// every attempt to smuggle a second header into it.
    pub fn parse(raw: &str) -> Option<Self> {
        let mut addr = raw.trim();
        let mut name = None;
        // "Name <a@b.c>" is what people paste. Take the angle brackets, and
        // keep the name in front of them if it is one a header can carry.
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
            name = display_name(&addr[..open])?;
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
        Some(Address {
            addr: addr.to_ascii_lowercase(),
            name,
        })
    }

    /// A list as a person would type it: `a@b.com, c@d.com`.
    ///
    /// All or nothing. A list where one address is malformed is refused whole
    /// rather than sent to the addresses that happened to parse, because a
    /// reply that reached three of its four recipients is a worse outcome than
    /// one that reached none and said so.
    ///
    /// A comma inside a quoted name (`"Smith, Ann" <ann@example.org>`) or
    /// inside the angle brackets belongs to that address, not between two;
    /// a quote left open refuses the list.
    pub fn parse_list(raw: &str) -> Option<Vec<Self>> {
        let mut out = Vec::new();
        for piece in split_list(raw)? {
            if piece.trim().is_empty() {
                continue;
            }
            out.push(Self::parse(piece)?);
        }
        if out.is_empty() { None } else { Some(out) }
    }

    pub fn as_str(&self) -> &str {
        &self.addr
    }

    /// The name it was given with, cleaned, if there was one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn domain(&self) -> String {
        domain_of(&self.addr)
    }
}

/// A typed list cut at the commas that separate addresses: not one inside
/// double quotes (where a backslash escapes the next character) or inside
/// angle brackets. `None` when a quote is never closed, since what the
/// customer meant cannot then be known.
fn split_list(raw: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let (mut quoted, mut angle, mut escaped, mut start) = (false, false, false, 0);
    for (i, c) in raw.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' if !angle => quoted = true,
            '<' => angle = true,
            '>' => angle = false,
            ',' if !angle => {
                out.push(&raw[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return None;
    }
    out.push(&raw[start..]);
    Some(out)
}

/// The name in front of `<a@b.c>`, as a header may carry it. `Some(None)`
/// when there is none worth keeping; `None` refuses the whole address.
///
/// A quoted name loses its quotes and its backslash escapes. A line break or
/// any other control character refuses the address: it is how a second
/// header is smuggled in, and no name has one. What would reorder text on
/// screen (bidi controls) goes, and so do `"`, `\`, `<` and `>`, so the name
/// can be written between quotes without escaping and never looks like an
/// address to anything reading the header back. Runs of blank space become
/// one, and the name is cut to [`NAME_MAX`] characters.
fn display_name(raw: &str) -> Option<Option<String>> {
    let raw = raw.trim();
    let inner = match raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => {
            let (mut out, mut escaped) = (String::new(), false);
            for c in inner.chars() {
                if escaped || c != '\\' {
                    out.push(c);
                    escaped = false;
                } else {
                    escaped = true;
                }
            }
            out
        }
        None => raw.to_string(),
    };
    if inner.chars().any(char::is_control) {
        return None;
    }
    let kept: String = inner
        .chars()
        .filter(|&c| !crate::credential::is_bidi_control(c) && !matches!(c, '"' | '\\' | '<' | '>'))
        .collect();
    let name: String = kept
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(NAME_MAX)
        .collect();
    let name = name.trim();
    Some((!name.is_empty()).then(|| name.to_string()))
}

/// A message on its way out.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub from: Address,
    /// What to show as the sender's name. Optional: without one the address
    /// stands on its own, which is correct, just plainer.
    pub from_name: Option<String>,
    pub to: Vec<Address>,
    /// Copied: seen by everyone, like To. Empty for none.
    pub cc: Vec<Address>,
    /// Copied blind: handed to the server for delivery and never written
    /// into the message, so nobody else sees them. Empty for none.
    pub bcc: Vec<Address>,
    pub subject: String,
    pub body: String,
    /// The message being replied to, if any, so mail clients thread it.
    pub in_reply_to: Option<String>,
    /// Files sent with it. Empty for a plain message.
    pub attachments: Vec<File>,
}

/// A file attached to an outgoing message.
#[derive(Debug, Clone)]
pub struct File {
    /// The name as the recipient will see it. Cleaned before it goes into a
    /// header; see [`file_name`].
    pub name: String,
    /// Its type, e.g. `application/pdf`. Anything that is not a well-formed
    /// `type/subtype` is sent as `application/octet-stream`.
    pub mime: String,
    pub data: Vec<u8>,
}

/// The most attachment data one message may carry. Most providers refuse
/// mail over 25 MB, and base64 makes attachments a third larger on the wire.
pub const ATTACH_MAX: usize = 18 * 1024 * 1024;

/// The full RFC 5322 message, CRLF throughout, ready for DATA.
///
/// `now_rfc2822` and `unique` are passed in rather than read here so the whole
/// thing can be tested byte for byte.
pub fn render(msg: &Outgoing, now_rfc2822: &str, unique: &str) -> String {
    render_as(msg, now_rfc2822, unique, None)
}

/// A draft as it is kept in the mailbox's Drafts folder, for IMAP `APPEND`.
///
/// The message [`render`] would send, with three differences, all of them
/// only here:
///
/// * **Bcc is written.** A draft is the customer's own copy, and finishing it
///   on another device must keep its blind copies. Nothing is sent from it:
///   sending renders the message again through [`render`], which never
///   writes Bcc.
/// * **`X-RATA-Draft: <id>` and `X-RATA-Draft-Rev: <n>`** say which draft
///   this is and which save of it. The id is how RATA proves a copy in
///   Drafts is its own before it replaces it; nothing without it ever is.
/// * **No dot-stuffing, and To may be empty.** An APPEND literal is stored
///   byte for byte, so a `.` doubled here would stay doubled; and a draft
///   need not be addressed to anyone yet.
///
/// `draft_id` must pass [`draft_id_ok`]: it goes into a header as it is.
pub fn render_draft(
    msg: &Outgoing,
    now_rfc2822: &str,
    unique: &str,
    draft_id: &str,
    rev: u32,
) -> String {
    render_as(msg, now_rfc2822, unique, Some((draft_id, rev)))
}

/// Whether a draft id is one RATA could have made: a UUID's characters
/// (lowercase hex and `-`), 16 to 64 of them. It is written into a header and
/// searched for, so nothing else may be one.
pub fn draft_id_ok(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

fn render_as(
    msg: &Outgoing,
    now_rfc2822: &str,
    unique: &str,
    draft: Option<(&str, u32)>,
) -> String {
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

    let _ = write!(out, "From: {from}\r\n");
    // A message sent always has someone in To (`smtp::send` refuses one
    // without); a draft may not yet.
    if draft.is_none() || !msg.to.is_empty() {
        addresses(&mut out, "To", &msg.to);
    }
    if !msg.cc.is_empty() {
        addresses(&mut out, "Cc", &msg.cc);
    }
    if let Some((id, rev)) = draft {
        if !msg.bcc.is_empty() {
            addresses(&mut out, "Bcc", &msg.bcc);
        }
        let _ = write!(out, "X-RATA-Draft: {id}\r\nX-RATA-Draft-Rev: {rev}\r\n");
    }
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
    if !msg.attachments.is_empty() {
        mixed(&mut out, msg, unique);
        return out;
    }
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
            // message early and hand the rest to the server as commands. A
            // draft is an APPEND literal, stored as it is, so not there.
            if draft.is_none() && line.starts_with('.') {
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

/// An address header, folded between addresses so that no line passes the
/// 998-byte limit however many there are. A fold only ever comes between two
/// addresses, never inside one.
///
/// A name goes in front of its address: between quotes when it is plain
/// ASCII (it holds no `"` or `\\`, see [`display_name`]), as RFC 2047
/// encoded-words otherwise, which have no quotes, commas or brackets in them.
/// Either way the only `<` and `>` in the header are around addresses, which
/// is what [`crate::smtp`] reads the envelope back out by.
fn addresses(out: &mut String, name: &str, list: &[Address]) {
    let mut line = name.len() + 1;
    let _ = write!(out, "{name}:");
    for (i, a) in list.iter().enumerate() {
        let piece = match a.name() {
            Some(shown) if shown.is_ascii() => format!("\"{shown}\" <{}>", a.as_str()),
            Some(shown) => format!("{} <{}>", words::encode_header(shown), a.as_str()),
            None => format!("<{}>", a.as_str()),
        };
        if i > 0 {
            out.push(',');
            line += 1;
        }
        if i > 0 && line + 1 + piece.len() > 76 {
            out.push_str("\r\n");
            line = 0;
        }
        out.push(' ');
        out.push_str(&piece);
        line += 1 + piece.len();
    }
    out.push_str("\r\n");
}

/// A message with files: `multipart/mixed`, the text first, then each file.
///
/// Every part is base64, the text included. That costs a few bytes and buys a
/// guarantee: the base64 alphabet has no `_`, so a boundary containing one can
/// never appear inside a part, whatever the customer wrote or attached.
fn mixed(out: &mut String, msg: &Outgoing, unique: &str) {
    let boundary = format!("=_rata_{unique}");
    let _ = write!(
        out,
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
    );
    out.push_str("This is a message in MIME format.\r\n");

    let _ = write!(out, "--{boundary}\r\n");
    out.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    let plain = msg.body.replace("\r\n", "\n").replace('\r', "\n");
    wrapped(out, plain.as_bytes());

    for file in &msg.attachments {
        let name = file_name(&file.name);
        let _ = write!(out, "--{boundary}\r\n");
        let _ = write!(
            out,
            "Content-Type: {}; name=\"{}\"\r\n",
            mime_type(&file.mime),
            ascii_name(&name)
        );
        let _ = write!(
            out,
            "Content-Disposition: attachment;\r\n\tfilename=\"{}\";\r\n\tfilename*=UTF-8''{}\r\n",
            ascii_name(&name),
            percent(&name)
        );
        out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        wrapped(out, &file.data);
    }
    let _ = write!(out, "--{boundary}--\r\n");
}

/// Base64 in lines of 76, as MIME requires.
fn wrapped(out: &mut String, data: &[u8]) {
    let encoded = words::base64_encode(data);
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
}

/// A file name fit for a header: the last path component only, no control
/// characters (a newline would end the header and start another), and short
/// enough that the header stays under the 998-byte line limit.
pub fn file_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return "attachment".into();
    }
    // Counted in bytes, not characters: the percent-encoded form is up to three
    // characters per byte, and it has to fit on one header line.
    if clean.len() <= NAME_BYTES {
        return clean.to_string();
    }
    let ext = clean
        .rfind('.')
        .map(|i| &clean[i..])
        .filter(|e| e.len() <= 12)
        .unwrap_or("");
    let mut keep = NAME_BYTES - ext.len();
    while !clean.is_char_boundary(keep) {
        keep -= 1;
    }
    format!("{}{ext}", &clean[..keep])
}

/// The longest file name sent, in UTF-8 bytes: room for any real name, and
/// short enough that every header carrying it stays under 998 characters.
const NAME_BYTES: usize = 150;

/// The name for the quoted `name=` and `filename=` parameters. An ASCII name
/// goes as it is, minus the quote and backslash that would end the quoted
/// string. Anything else goes as RFC 2047 encoded-words, which is not what the
/// MIME standard asks for in a parameter but is what Gmail and Outlook send and
/// what every mail program reads — while some read only this parameter and
/// never the standard `filename*`, and would show an ASCII stand-in instead.
fn ascii_name(name: &str) -> String {
    if name.is_ascii() {
        return name.chars().filter(|&c| c != '"' && c != '\\').collect();
    }
    words::encode_header(name)
}

/// RFC 2231's encoding for the exact name: UTF-8, percent-escaped except the
/// characters it allows as they are.
fn percent(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// A `type/subtype` made only of the characters a MIME token allows, or the
/// generic binary type.
fn mime_type(raw: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    match t.split_once('/') {
        Some((a, b)) if token(a) && token(b) => t,
        _ => "application/octet-stream".into(),
    }
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
            cc: vec![],
            bcc: vec![],
            subject: subject.into(),
            body: body.into(),
            in_reply_to: None,
            attachments: vec![],
        }
    }

    fn with_file(name: &str, mime: &str, data: &[u8]) -> Outgoing {
        let mut m = msg("Figures", "Attached.\n.\nThanks");
        m.attachments.push(File {
            name: name.into(),
            mime: mime.into(),
            data: data.to_vec(),
        });
        m
    }

    #[test]
    fn a_message_with_a_file_is_multipart_and_reads_back_whole() {
        let out = render(
            &with_file("Q3 figures.pdf", "application/pdf", b"%PDF-1.4\n"),
            "Sat, 26 Sep 2026 12:00:00 +0000",
            "abc123",
        );
        assert!(
            out.contains("Content-Type: multipart/mixed; boundary=\"=_rata_abc123\""),
            "{out}"
        );
        assert!(out.ends_with("--=_rata_abc123--\r\n"));
        assert!(out.split("\r\n").all(|l| l.len() <= 998));
        // What a receiving mail program makes of it.
        let parsed = crate::body::read_whole(out.as_bytes());
        assert_eq!(parsed.text, "Attached.\n.\nThanks");
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].name, "Q3 figures.pdf");
        assert_eq!(parsed.attachments[0].mime, "application/pdf");
        let (_, bytes) =
            crate::body::attachment(out.as_bytes(), parsed.attachments[0].index).unwrap();
        assert_eq!(bytes, b"%PDF-1.4\n");
    }

    #[test]
    fn a_file_name_cannot_add_a_header() {
        let out = render(
            &with_file(
                "x.pdf\"\r\nBcc: everyone@example.com\r\n",
                "application/pdf\r\nBcc: a@b.c",
                b"x",
            ),
            "Sat, 26 Sep 2026 12:00:00 +0000",
            "abc123",
        );
        assert!(!out.contains("\r\nBcc:"), "{out}");
        assert!(
            out.contains("Content-Type: application/octet-stream;"),
            "{out}"
        );
    }

    #[test]
    fn a_name_in_any_language_arrives_as_written() {
        let out = render(
            &with_file("résumé 履歴書.docx", "application/msword", b"x"),
            "Sat, 26 Sep 2026 12:00:00 +0000",
            "abc123",
        );
        assert!(
            out.contains("filename*=UTF-8''r%C3%A9sum%C3%A9%20"),
            "{out}"
        );
        let parsed = crate::body::read_whole(out.as_bytes());
        assert_eq!(parsed.attachments[0].name, "résumé 履歴書.docx");
    }

    #[test]
    fn file_names_are_cleaned_for_headers() {
        assert_eq!(
            file_name("C:\\Users\\me\\Desktop\\report.pdf"),
            "report.pdf"
        );
        assert_eq!(file_name("/home/me/a\nb.txt"), "ab.txt");
        assert_eq!(file_name("   "), "attachment");
        let long = file_name(&format!("{}.pdf", "a".repeat(300)));
        assert_eq!(long.len(), 150);
        assert!(long.ends_with(".pdf"));
        let cjk = file_name(&format!("{}.xlsx", "履".repeat(200)));
        assert!(cjk.len() <= 150 && cjk.ends_with(".xlsx"), "{cjk}");
        assert_eq!(mime_type("IMAGE/PNG"), "image/png");
        assert_eq!(
            mime_type("text/html; charset=x"),
            "application/octet-stream"
        );
        assert_eq!(mime_type(""), "application/octet-stream");
    }

    #[test]
    fn a_long_name_in_any_script_keeps_every_header_line_legal() {
        let out = render(
            &with_file(
                &format!("{}.xlsx", "履歴書".repeat(80)),
                "application/vnd.ms-excel",
                b"x",
            ),
            "Sat, 26 Sep 2026 12:00:00 +0000",
            "abc123",
        );
        let longest = out.split("\r\n").map(str::len).max().unwrap();
        assert!(longest <= 998, "a header line of {longest}");
        let parsed = crate::body::read_whole(out.as_bytes());
        assert!(
            parsed.attachments[0].name.ends_with(".xlsx"),
            "{}",
            parsed.attachments[0].name
        );
    }

    #[test]
    fn a_plain_message_is_unchanged_by_all_this() {
        let out = render(
            &msg("Hi", "Hello"),
            "Sat, 26 Sep 2026 12:00:00 +0000",
            "abc123",
        );
        assert!(
            out.contains(
                "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 7bit"
            ),
            "{out}"
        );
        assert!(!out.contains("multipart"));
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
    fn a_quoted_name_with_a_comma_is_one_address() {
        // What the composer writes when a suggestion is chosen (H3).
        let a = addr("\"Smith, Ann\" <Ann@Example.org>");
        assert_eq!(a.as_str(), "ann@example.org");
        assert_eq!(a.name(), Some("Smith, Ann"));

        let list = Address::parse_list(
            "\"Smith, Ann\" <ann@example.org>, Bob <bob@example.org>,dee@example.org",
        )
        .unwrap();
        assert_eq!(
            list.iter()
                .map(|a| (a.as_str(), a.name()))
                .collect::<Vec<_>>(),
            [
                ("ann@example.org", Some("Smith, Ann")),
                ("bob@example.org", Some("Bob")),
                ("dee@example.org", None)
            ]
        );
        // An escaped quote stays inside the name (as a plain character, since
        // the render writes the name between quotes), and the comma after it.
        let list = Address::parse_list(r#""Ann \"A, B\" Lee" <a@b.com>, c@d.com"#).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name(), Some("Ann A, B Lee"));
        // A quote never closed: what was meant cannot be known.
        assert!(Address::parse_list("\"Smith, Ann <ann@example.org>, b@c.com").is_none());
        // An unclosed bracket does not quietly swallow the next address.
        assert!(Address::parse_list("Ann <ann@example.org, b@c.com").is_none());
        // Empty quotes are no name, not a refusal.
        assert_eq!(addr("\"\" <a@b.com>").name(), None);
    }

    #[test]
    fn a_name_cannot_carry_a_header_or_disguise_itself() {
        for attack in [
            "\"Ann\r\nBcc: everyone@example.com\" <a@b.com>",
            "Ann\nBcc: everyone@example.com <a@b.com>",
            "\"Ann\u{0}\" <a@b.com>",
        ] {
            assert!(
                Address::parse(attack).is_none(),
                "{attack:?} should not parse"
            );
        }
        // Bidi controls, brackets and backslashes go; blank runs become one;
        // a long name is cut.
        assert_eq!(
            addr("\"\u{202e}Ann  <x> \\ Lee\" <a@b.com>").name(),
            Some("Ann x Lee")
        );
        let long = format!("\"{}\" <a@b.com>", "é".repeat(500));
        assert_eq!(addr(&long).name().unwrap().chars().count(), NAME_MAX);
    }

    #[test]
    fn names_are_written_in_front_of_their_addresses() {
        let mut m = msg("Hi", "Hello");
        m.to =
            Address::parse_list("\"Smith, Ann\" <ann@example.org>, Zoë <z@example.org>").unwrap();
        m.cc = vec![addr("cc@example.org")];
        let out = render(&m, "Wed, 16 Sep 2026 12:00:00 +0000", "abc");
        let to = out.split("\r\n").find(|l| l.starts_with("To:")).unwrap();
        // Plain ASCII is quoted as it is; anything else is encoded, so no
        // quote, comma or bracket of its own ever reaches the header.
        assert!(
            to.starts_with("To: \"Smith, Ann\" <ann@example.org>,"),
            "{out}"
        );
        assert!(
            out.contains(" =?UTF-8?B?Wm/Dqw==?= <z@example.org>"),
            "{out}"
        );
        assert!(out.contains("Cc: <cc@example.org>\r\n"), "{out}");

        // However many long names, no line passes 998 bytes, and every
        // address is still there.
        m.to = (0..100)
            .map(|i| {
                addr(&format!(
                    "\"{}\" <p{i}.{}@example.org>",
                    "名".repeat(80),
                    "x".repeat(200)
                ))
            })
            .collect();
        let out = render(&m, "d", "u");
        let head = out.split("\r\n\r\n").next().unwrap();
        assert!(
            head.split("\r\n").all(|l| l.len() <= 998),
            "a line is too long"
        );
        for i in 0..100 {
            assert!(head.contains(&format!("<p{i}.")), "p{i} is missing");
        }
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
    fn copied_addresses_are_in_a_cc_header_of_their_own() {
        let mut m = msg("Plan", "Hi");
        m.cc = vec![addr("dee@example.org"), addr("eli@example.net")];
        let out = render(&m, "d", "i");
        assert!(
            out.contains(
                "To: <someone@elsewhere.org>\r\nCc: <dee@example.org>, <eli@example.net>\r\n"
            ),
            "{out}"
        );
        // And none at all when nobody is copied.
        assert!(!render(&msg("Plan", "Hi"), "d", "i").contains("Cc:"));
        // A blind copy is never written into the message at all.
        m.bcc = vec![addr("boss@example.net")];
        let out = render(&m, "d", "i");
        assert!(
            !out.contains("boss@example.net") && !out.contains("Bcc"),
            "{out}"
        );
    }

    #[test]
    fn a_long_list_of_recipients_is_folded_into_legal_lines() {
        let mut m = msg("All hands", "Hi");
        m.to = (0..120)
            .map(|i| addr(&format!("person.number.{i}@department.example.org")))
            .collect();
        let out = render(&m, "d", "i");
        let head = out.split("\r\n\r\n").next().unwrap();
        assert!(head.split("\r\n").all(|l| l.len() <= 78), "{head}");
        // Every address is there, whole, once.
        for i in 0..120 {
            let want = format!("<person.number.{i}@department.example.org>");
            assert_eq!(head.matches(&want).count(), 1, "{want}");
        }
        // Continuation lines start with a space, which is what makes them one header.
        assert!(head.contains(",\r\n <person.number."), "{head}");
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

    const ID: &str = "0f8b6a52-3c1e-4d7a-9b2e-5a4c3d2e1f00";

    #[test]
    fn a_draft_keeps_its_bcc_and_says_whose_it_is_but_a_sent_message_never_has_bcc() {
        let mut m = msg("Plan", "Half a thought");
        m.cc = vec![addr("cy@example.org")];
        m.bcc = vec![addr("boss@example.net")];
        let draft = render_draft(&m, "d", "i", ID, 3);
        assert!(draft.contains("\r\nBcc: <boss@example.net>\r\n"), "{draft}");
        assert!(
            draft.contains(&format!("\r\nX-RATA-Draft: {ID}\r\n")),
            "{draft}"
        );
        assert!(draft.contains("\r\nX-RATA-Draft-Rev: 3\r\n"), "{draft}");
        // The headers come before the text, where a header belongs.
        let head = draft.split("\r\n\r\n").next().unwrap();
        assert!(head.contains("X-RATA-Draft:") && head.contains("Bcc:"));
        // What a mail program makes of it: the same message, Bcc and all.
        let parsed = mail_parser::MessageParser::default()
            .parse(draft.as_bytes())
            .unwrap();
        assert_eq!(
            parsed
                .bcc()
                .and_then(|a| a.first())
                .and_then(|a| a.address()),
            Some("boss@example.net")
        );
        // The message as sent is unchanged: no Bcc, no draft headers.
        let sent = render(&m, "d", "i");
        assert!(!sent.contains("Bcc:"), "{sent}");
        assert!(!sent.contains("X-RATA-Draft"), "{sent}");
    }

    #[test]
    fn a_draft_is_stored_as_written_and_need_not_be_addressed_yet() {
        let mut m = msg("Notes", "one\n.\n.two");
        m.to.clear();
        let draft = render_draft(&m, "d", "i", ID, 1);
        assert!(!draft.contains("\r\nTo:"), "{draft}");
        // An APPEND literal is stored byte for byte: no dot-stuffing.
        assert!(draft.ends_with("\r\n\r\none\r\n.\r\n.two\r\n"), "{draft}");
        // Sending still stuffs them.
        assert!(
            render(&msg("Notes", "one\n.\n.two"), "d", "i").ends_with("one\r\n..\r\n..two\r\n")
        );
    }

    /// The page repoints a draft's carried files at the copy just saved by
    /// position: the files in the order given, from 0. This is that order.
    #[test]
    fn a_draft_s_files_read_back_in_the_order_they_were_given_from_zero() {
        let mut m = with_file("a.pdf", "application/pdf", b"%PDF-1.4\n");
        m.attachments.push(File {
            name: "b.txt".into(),
            mime: "text/plain".into(),
            data: b"second".to_vec(),
        });
        let draft = render_draft(&m, "d", "i", ID, 1);
        let got = crate::body::read_whole(draft.as_bytes()).attachments;
        let seen: Vec<(u32, &str)> = got.iter().map(|a| (a.index, a.name.as_str())).collect();
        assert_eq!(seen, [(0, "a.pdf"), (1, "b.txt")]);
        let (_, data) = crate::body::attachment(draft.as_bytes(), 1).unwrap();
        assert_eq!(data, b"second");
    }

    #[test]
    fn only_an_id_rata_could_have_made_is_a_draft_id() {
        assert!(draft_id_ok(ID));
        for bad in [
            "",
            "short",
            "0F8B6A52-3C1E-4D7A-9B2E-5A4C3D2E1F00",
            "0f8b6a52 3c1e 4d7a 9b2e 5a4c3d2e1f00",
            "0f8b6a52-3c1e-4d7a\r\nBcc: x@y.z",
            "\"0f8b6a52-3c1e-4d7a-9b2e-5a4c3d2e1f00",
            &"a".repeat(65),
        ] {
            assert!(!draft_id_ok(bad), "{bad:?}");
        }
    }
}
