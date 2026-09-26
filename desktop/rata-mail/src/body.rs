//! A message's text, decoded.
//!
//! What comes off the wire is the raw message: headers, then a body that is
//! almost always MIME — a `multipart/alternative` holding a plain-text and an
//! HTML version, each in quoted-printable or base64, in whatever charset the
//! sender's software chose. Shown as it arrives, that is boundary lines,
//! `=20`s and blocks of base64, which is what RATA displayed until this module
//! existed.
//!
//! Parsing is `mail-parser`'s. What is decided here is which text to show,
//! what to do with the parts of a message that did not arrive (RATA fetches the
//! start of a message, not all of it), and keeping the result a sensible size
//! to hold on the customer's machine.

use mail_parser::{Message as Parsed, MessageParser, MimeHeaders, PartType};

use crate::{html, words};

/// How much of a message is fetched: its headers and the first 64 KiB. Enough
/// for the text of nearly any message, since the text part comes before the
/// attachments; not so much that a refresh of fifteen messages each carrying a
/// photograph downloads the photographs.
pub const MESSAGE_BYTES: usize = 64 * 1024;

/// How much decoded text is kept per message. RATA stores what it has fetched
/// on the customer's machine, and a newsletter can run to a hundred thousand
/// characters of text; this keeps any one message from crowding out the rest.
pub const BODY_CHARS: usize = 16_000;

/// How much text is shown when a message is opened in full. Not stored, so it
/// can be generous; still bounded, because a sender decides how long it is.
pub const WHOLE_CHARS: usize = 400_000;

/// The text of one message, and what is attached to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Body {
    /// Readable text, line breaks kept.
    pub text: String,
    /// True when the message goes on beyond what is shown — either past what
    /// was fetched or past the character limit. The rest is in the mailbox.
    pub truncated: bool,
    /// The attachments seen. From a partial fetch, those whose headers arrived.
    pub attachments: Vec<Attachment>,
    /// Whether the sender wrote an HTML version.
    pub has_html: bool,
    /// That HTML version, made safe — only from [`read_whole`], since it is for
    /// showing now and is never stored.
    pub html: Option<html::Safe>,
}

/// One attachment, as the reading pane lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Attachment {
    /// Its position among the message's attachments: what a download asks
    /// for. Stable between a partial and a whole fetch of the same message,
    /// since both parse the same structure from the same start.
    pub index: u32,
    /// The sender's file name, or one made up from the type. Not yet safe to
    /// use as a path — the app cleans it before anything is written.
    pub name: String,
    pub mime: String,
    /// Decoded size in bytes; 0 when only part of it was fetched.
    pub size: u64,
}

/// Decode `raw` — a whole message, or its first `MESSAGE_BYTES` — into text.
///
/// `cut` says whether `raw` stopped short of the end of the message, which the
/// caller knows from how many bytes the server sent back.
pub fn read(raw: &[u8], cut: bool) -> Body {
    read_capped(raw, cut, BODY_CHARS)
}

/// A whole message, opened: the same decoding with room for a long one, and
/// its HTML version made safe to show.
pub fn read_whole(raw: &[u8]) -> Body {
    let mut body = read_capped(raw, false, WHOLE_CHARS);
    if body.has_html
        && let Some(msg) = MessageParser::default().parse(raw)
    {
        body.html = formatted(&msg);
    }
    body
}

/// The HTML part, sanitised, with the pictures the message carries inline.
fn formatted(msg: &Parsed<'_>) -> Option<html::Safe> {
    let part = msg
        .html_body
        .first()
        .and_then(|&i| msg.parts.get(i as usize))?;
    let PartType::Html(source) = &part.body else {
        return None;
    };
    let inline: Vec<(String, String, Vec<u8>)> = msg
        .parts
        .iter()
        .filter_map(|p| {
            let cid = p
                .content_id()?
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            let mime = p
                .content_type()
                .map(|ct| format!("{}/{}", ct.c_type, ct.c_subtype.as_deref().unwrap_or("")))?
                .to_ascii_lowercase();
            Some((cid, mime, p.contents().to_vec()))
        })
        .collect();
    Some(html::safe(source, &inline))
}

/// One attachment's decoded bytes, by its [`Attachment::index`].
pub fn attachment(raw: &[u8], index: u32) -> Option<(Attachment, Vec<u8>)> {
    if !starts_with_header(raw) {
        return None;
    }
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachment(index)?;
    let info = describe(index, part, raw.len(), false);
    Some((info, part.contents().to_vec()))
}

fn read_capped(raw: &[u8], cut: bool, cap: usize) -> Body {
    // A parser takes the first line as a header whatever it says, so text
    // with no header block would lose its opening line. Decide that here.
    if !starts_with_header(raw) {
        return fallback(raw, cut, cap);
    }
    let Some(msg) = MessageParser::default().parse(raw) else {
        return fallback(raw, cut, cap);
    };
    let attachments = listed(&msg, raw.len(), cut);
    // mail-parser lists a plain-text part as the HTML body of a message that
    // has no HTML, so the part itself is what says.
    let has_html = msg
        .html_body
        .first()
        .and_then(|&i| msg.parts.get(i as usize))
        .is_some_and(|p| p.is_text_html());
    Body {
        attachments,
        has_html,
        ..text_of(&msg, raw, cut, cap)
    }
}

fn text_of(msg: &Parsed<'_>, raw: &[u8], cut: bool, cap: usize) -> Body {
    // The plain-text version where the sender wrote one, since that is what
    // they meant to be read as text; otherwise mail-parser's rendering of the
    // HTML version. Either way, never an attachment's text.
    let choice = msg
        .text_body
        .first()
        .copied()
        .or_else(|| msg.html_body.first().copied());
    let Some(index) = choice else {
        return Body {
            truncated: cut,
            ..Body::default()
        };
    };
    let part = &msg.parts[index as usize];
    let is_attachment =
        part.is_content_type("message", "rfc822") || part.attachment_name().is_some();
    if is_attachment {
        return Body {
            truncated: cut,
            ..Body::default()
        };
    }
    let text = match &part.body {
        PartType::Text(t) => t.to_string(),
        PartType::Html(h) => html::to_text(h),
        _ => String::new(),
    };

    // A part that runs to the last byte fetched was cut with it. One that ends
    // earlier arrived whole, even if an attachment after it did not.
    let part_cut = cut && part.offset_end as usize + 4 >= raw.len();
    finish(&text, part_cut, cap)
}

/// The attachments a person would call attachments. A picture embedded in the
/// message's own HTML — a logo, a signature image, marked inline and given a
/// Content-ID so the HTML can point at it — is part of the letter, not
/// something sent with it, and listing it would bury the one PDF that matters
/// under a row of signature icons.
fn listed(msg: &Parsed<'_>, raw_len: usize, cut: bool) -> Vec<Attachment> {
    (0..msg.attachments.len() as u32)
        .filter_map(|i| {
            let part = msg.attachment(i)?;
            let inline = part
                .content_disposition()
                .is_some_and(|d| d.c_type.eq_ignore_ascii_case("inline"));
            if inline && part.content_id().is_some() {
                return None;
            }
            Some(describe(i, part, raw_len, cut))
        })
        .collect()
}

fn describe(
    index: u32,
    part: &mail_parser::MessagePart<'_>,
    raw_len: usize,
    cut: bool,
) -> Attachment {
    let mime = part
        .content_type()
        .map(|ct| match &ct.c_subtype {
            Some(sub) => format!("{}/{}", ct.c_type, sub),
            None => ct.c_type.to_string(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
        .to_ascii_lowercase();
    let name = part
        .attachment_name()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| {
            part.message()
                .and_then(|m| m.subject())
                .map(|s| format!("{}.eml", s.trim()))
        })
        .unwrap_or_else(|| format!("attachment-{}{}", index + 1, extension_for(&mime)));
    // Only a part that ended before the cut has a size worth showing.
    let whole = !cut || (part.offset_end as usize + 4) < raw_len;
    Attachment {
        index,
        name,
        mime,
        size: if whole { part.len() as u64 } else { 0 },
    }
}

fn extension_for(mime: &str) -> &'static str {
    match mime {
        "application/pdf" => ".pdf",
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "text/plain" => ".txt",
        "text/calendar" => ".ics",
        "text/csv" => ".csv",
        "message/rfc822" => ".eml",
        "application/zip" => ".zip",
        _ => "",
    }
}

/// Whether `raw` opens with a header line — `Name: value`, the name made of
/// the characters RFC 5322 allows.
fn starts_with_header(raw: &[u8]) -> bool {
    let first = raw.split(|&b| b == b'\n').next().unwrap_or_default();
    match first.iter().position(|&b| b == b':') {
        Some(0) | None => false,
        Some(i) => first[..i]
            .iter()
            .all(|&b| b.is_ascii_graphic() && b != b':'),
    }
}

/// Not MIME at all, or too broken to parse: show the bytes as text, which is
/// what a plain RFC 822 message is — after its header block, if it has one.
fn fallback(raw: &[u8], cut: bool, cap: usize) -> Body {
    let text = String::from_utf8_lossy(raw);
    let body = match text.find("\r\n\r\n").or_else(|| text.find("\n\n")) {
        Some(i) if starts_with_header(raw) => &text[i..],
        _ => &text[..],
    };
    finish(body, cut, cap)
}

fn finish(text: &str, cut: bool, cap: usize) -> Body {
    let tidy = tidy(text);
    let mut out: String = tidy.chars().take(cap).collect();
    let over = out.len() < tidy.len();
    if over {
        // End on a whole line rather than mid-word.
        if let Some(i) = out.rfind('\n')
            && i > cap / 2
        {
            out.truncate(i);
        }
    }
    Body {
        text: out.trim_end().to_string(),
        truncated: cut || over,
        ..Body::default()
    }
}

/// Line breaks normalised, trailing space and invisible characters gone, and
/// no more than one blank line in a row — mail rendered from HTML is full of
/// empty paragraphs and zero-width spacers.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line: String = line
            .chars()
            .filter(|c| {
                !matches!(
                    c,
                    '\u{200b}'
                        | '\u{200c}'
                        | '\u{200d}'
                        | '\u{2060}'
                        | '\u{feff}'
                        | '\u{034f}'
                        | '\u{00ad}'
                )
            })
            .map(|c| if c == '\u{a0}' { ' ' } else { c })
            .collect();
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
            out.push('\n');
        } else {
            blank = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// One line for the message list: the text with quoted replies and
/// signatures' dashes left out where possible, whitespace collapsed.
pub fn preview(text: &str) -> String {
    let own: Vec<&str> = text
        .lines()
        .take_while(|l| l.trim_end() != "--")
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect();
    let joined = own.join(" ");
    // A link's address is kept in the message for the reader to follow; in a
    // one-line preview it only pushes the words out.
    let flat = joined
        .split_whitespace()
        .filter(|w| !(w.starts_with("<http") && w.ends_with('>')))
        .collect::<Vec<_>>()
        .join(" ");
    let flat = if flat.is_empty() {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        flat
    };
    words::clip(&flat, 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(headers: &str, body: &str) -> Vec<u8> {
        format!("From: Ann <ann@example.org>\r\nTo: me@example.com\r\nSubject: Hi\r\n{headers}\r\n{body}").into_bytes()
    }

    #[test]
    fn a_plain_message_reads_as_itself_with_its_line_breaks() {
        let b = read(
            &msg("", "Hello,\r\n\r\nSee you at 3.\r\n\r\nAnn\r\n"),
            false,
        );
        assert_eq!(b.text, "Hello,\n\nSee you at 3.\n\nAnn");
        assert!(!b.truncated);
    }

    #[test]
    fn quoted_printable_is_decoded() {
        let b = read(
            &msg(
                "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n",
                "Caf=C3=A9 at noon? It=E2=80=99s a long line that the sender's mail program =\r\nwrapped with a soft break.\r\n",
            ),
            false,
        );
        assert_eq!(
            b.text,
            "Café at noon? It’s a long line that the sender's mail program wrapped with a soft break."
        );
    }

    #[test]
    fn the_plain_part_of_a_multipart_message_is_shown_not_its_boundaries() {
        let raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"000abc\"\r\n",
            "--000abc\r\nContent-Type: text/plain; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\nSGkgdGhlcmUsCgpUaGUgZmlndXJlcyBhcmUgYXR0YWNoZWQu\r\n\r\n--000abc\r\nContent-Type: text/html; charset=\"UTF-8\"\r\n\r\n<div>Hi there,</div><div>The figures are attached.</div>\r\n--000abc--\r\n",
        );
        let b = read(&raw, false);
        assert_eq!(b.text, "Hi there,\n\nThe figures are attached.");
        assert!(!b.text.contains("000abc") && !b.text.contains("Content-Type"));
    }

    #[test]
    fn an_html_only_message_becomes_text_without_its_markup_or_styles() {
        let raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\n",
            "<html><head><style>p{color:red}</style></head><body><p>Your order has shipped.</p><p>Track it &amp; relax.</p></body></html>\r\n",
        );
        let b = read(&raw, false);
        assert!(b.text.contains("Your order has shipped."), "{:?}", b.text);
        assert!(b.text.contains("Track it & relax."), "{:?}", b.text);
        assert!(
            !b.text.contains('<') && !b.text.contains("color:red"),
            "{:?}",
            b.text
        );
    }

    #[test]
    fn a_legacy_charset_is_decoded() {
        let mut raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: 8bit\r\n",
            "",
        );
        raw.extend_from_slice(b"Gr\xfc\xdfe aus M\xfcnchen\r\n");
        assert_eq!(read(&raw, false).text, "Grüße aus München");

        let mut jp = msg(
            "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=iso-2022-jp\r\nContent-Transfer-Encoding: 7bit\r\n",
            "",
        );
        jp.extend_from_slice(b"\x1b$B$3$s$K$A$O\x1b(B\r\n");
        assert_eq!(read(&jp, false).text, "こんにちは");
    }

    #[test]
    fn an_attachment_is_never_shown_as_the_message() {
        let raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n",
            "--b1\r\nContent-Type: text/plain\r\n\r\nInvoice attached.\r\n--b1\r\nContent-Type: application/pdf; name=inv.pdf\r\nContent-Disposition: attachment; filename=inv.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b1--\r\n",
        );
        assert_eq!(read(&raw, false).text, "Invoice attached.");

        let only = msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n",
            "--b1\r\nContent-Type: text/plain; name=notes.txt\r\nContent-Disposition: attachment; filename=notes.txt\r\n\r\nsecret notes\r\n--b1--\r\n",
        );
        assert_eq!(read(&only, false).text, "");
    }

    fn with_attachments() -> Vec<u8> {
        msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n",
            concat!(
                "--b1\r\nContent-Type: multipart/related; boundary=b2\r\n\r\n",
                "--b2\r\nContent-Type: text/html\r\n\r\n<p>See attached.</p><img src=\"cid:logo\">\r\n",
                "--b2\r\nContent-Type: image/png; name=logo.png\r\nContent-Disposition: inline; filename=logo.png\r\nContent-ID: <logo>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n",
                "--b2--\r\n",
                "--b1\r\nContent-Type: application/pdf; name=\"Q3 figures.pdf\"\r\nContent-Disposition: attachment; filename=\"Q3 figures.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n",
                "--b1\r\nContent-Type: text/calendar\r\nContent-Disposition: attachment\r\n\r\nBEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
                "--b1--\r\n",
            ),
        )
    }

    #[test]
    fn attachments_are_listed_without_the_pictures_inside_the_letter() {
        let b = read_whole(&with_attachments());
        assert_eq!(b.text, "See attached.");
        let names: Vec<(&str, &str, u64)> = b
            .attachments
            .iter()
            .map(|a| (a.name.as_str(), a.mime.as_str(), a.size))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Q3 figures.pdf", "application/pdf", 9),
                ("attachment-3.ics", "text/calendar", 30)
            ]
        );
    }

    #[test]
    fn an_attachment_comes_back_as_its_decoded_bytes() {
        let raw = with_attachments();
        let pdf = read_whole(&raw).attachments[0].index;
        let (info, bytes) = attachment(&raw, pdf).unwrap();
        assert_eq!(info.name, "Q3 figures.pdf");
        assert_eq!(bytes, b"%PDF-1.4\n");
        assert!(attachment(&raw, 99).is_none());
        assert!(attachment(b"no headers", 0).is_none());
    }

    #[test]
    fn a_partly_fetched_attachment_is_listed_without_a_size() {
        let raw = with_attachments();
        let cut_at = raw.windows(8).position(|w| w == b"JVBERi0x").unwrap() + 4;
        let b = read(&raw[..cut_at], true);
        assert_eq!(b.attachments.len(), 1);
        assert_eq!(b.attachments[0].name, "Q3 figures.pdf");
        assert_eq!(b.attachments[0].size, 0);
    }

    #[test]
    fn an_opened_html_message_comes_with_its_safe_html_and_pictures() {
        let raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=r\r\n",
            concat!(
                "--r\r\nContent-Type: multipart/alternative; boundary=a\r\n\r\n",
                "--a\r\nContent-Type: text/plain\r\n\r\nYour order shipped.\r\n",
                "--a\r\nContent-Type: text/html\r\n\r\n<p onclick=\"steal()\">Your order <b>shipped</b>.</p><img src=\"cid:logo\"><script>steal()</script>\r\n",
                "--a--\r\n",
                "--r\r\nContent-Type: image/png\r\nContent-ID: <logo>\r\nContent-Disposition: inline\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw==\r\n",
                "--r--\r\n",
            ),
        );
        let listed = read(&raw, false);
        assert!(
            listed.has_html && listed.html.is_none(),
            "the list fetch only notes it"
        );
        let opened = read_whole(&raw);
        assert_eq!(opened.text, "Your order shipped.");
        let html = opened.html.expect("html");
        assert!(html.html.contains("<b>shipped</b>"), "{}", html.html);
        assert!(
            html.html.contains("data:image/png;base64,"),
            "{}",
            html.html
        );
        assert!(
            !html.html.contains("onclick") && !html.html.contains("<script"),
            "{}",
            html.html
        );
        assert!(!html.remote_images);
    }

    #[test]
    fn a_plain_message_has_no_html_to_show() {
        let b = read_whole(&msg("", "Just text.\r\n"));
        assert!(!b.has_html && b.html.is_none());
    }

    #[test]
    fn text_that_arrived_whole_is_not_marked_cut_when_an_attachment_was() {
        // The fetch stopped inside the attachment, after the text had ended.
        let raw = msg(
            "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n",
            "--b1\r\nContent-Type: text/plain\r\n\r\nPhotos from Saturday.\r\n--b1\r\nContent-Type: image/jpeg\r\nContent-Disposition: attachment; filename=a.jpg\r\nContent-Transfer-Encoding: base64\r\n\r\n/9j/4AAQSkZJRgABAQAAAQABAAD",
        );
        let b = read(&raw, true);
        assert_eq!(b.text, "Photos from Saturday.");
        assert!(!b.truncated);
    }

    #[test]
    fn text_cut_by_the_fetch_says_so() {
        let raw = msg("", &"A long letter. ".repeat(50));
        let b = read(&raw, true);
        assert!(b.truncated);
        assert!(b.text.starts_with("A long letter."));
    }

    #[test]
    fn a_very_long_message_is_kept_to_a_bounded_size_on_a_line_break() {
        let line = "Paragraph of a newsletter that goes on for a while.\n";
        let b = read(&msg("", &line.repeat(1000)), false);
        assert!(b.truncated);
        assert!(b.text.chars().count() <= BODY_CHARS);
        assert!(
            b.text.ends_with("a while."),
            "ends mid-line: {:?}",
            &b.text[b.text.len() - 20..]
        );
    }

    #[test]
    fn blank_runs_and_invisible_spacers_are_tidied() {
        let raw = msg(
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Top\u{200b}\r\n\r\n\r\n\r\n\u{a0}\r\nBottom   \r\n",
        );
        assert_eq!(read(&raw, false).text, "Top\n\nBottom");
    }

    #[test]
    fn something_that_is_not_mime_still_reads() {
        assert_eq!(
            read(b"no headers at all, just words", false).text,
            "no headers at all, just words"
        );
        assert_eq!(read(b"", false).text, "");
        assert_eq!(
            read(b"Hello there\n\nsecond para", false).text,
            "Hello there\n\nsecond para"
        );
    }

    #[test]
    fn no_message_makes_it_panic() {
        let pieces = [
            "From: a@b\r\n",
            "Content-Type: multipart/mixed; boundary=b\r\n",
            "Content-Type: text/html; charset=",
            "iso-2022-jp",
            "utf-8",
            "gbk",
            "\r\n",
            "\r\n\r\n",
            "--b\r\n",
            "--b--",
            "Content-Transfer-Encoding: base64\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n",
            "=C3=A9",
            "=\r\n",
            "=ZZ",
            "SGVsbG8=",
            "===",
            "<p>",
            "é",
            "\u{1b}$B",
            "\u{1b}(B",
            "\u{0}",
            "Content-Type: message/rfc822\r\n",
            "Subject: =?utf-8?B?w6k=?=\r\n",
        ];
        for seed in 0..2000 {
            let raw = crate::html::tests::junk(seed, &pieces, 40);
            for cut in [false, true] {
                let b = read(raw.as_bytes(), cut);
                assert!(b.text.chars().count() <= BODY_CHARS);
                let _ = preview(&b.text);
            }
        }
        // Raw bytes that are not UTF-8 at all.
        let bytes: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let _ = read(&bytes, true);
    }

    #[test]
    fn the_preview_is_the_writer_s_own_words() {
        let text = "Sounds good, Thursday it is.\n\nOn Tue, Ann wrote:\n> Are you free Thursday?\n> Let me know\n-- \nBen\nSent from my phone";
        assert_eq!(
            preview(text),
            "Sounds good, Thursday it is. On Tue, Ann wrote:"
        );
        assert_eq!(preview("> only a quote"), "> only a quote");
        assert_eq!(
            preview("Track it here: Track package <https://shop.example/t/1?u=x> today"),
            "Track it here: Track package today"
        );
    }
}
