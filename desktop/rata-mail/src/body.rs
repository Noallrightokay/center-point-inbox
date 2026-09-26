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

use mail_parser::{MessageParser, MimeHeaders, PartType};

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

/// The text of one message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Body {
    /// Readable text, line breaks kept.
    pub text: String,
    /// True when the message goes on beyond what is shown — either past what
    /// was fetched or past [`BODY_CHARS`]. The rest is in the mailbox.
    pub truncated: bool,
}

/// Decode `raw` — a whole message, or its first `MESSAGE_BYTES` — into text.
///
/// `cut` says whether `raw` stopped short of the end of the message, which the
/// caller knows from how many bytes the server sent back.
pub fn read(raw: &[u8], cut: bool) -> Body {
    // A parser takes the first line as a header whatever it says, so text
    // with no header block would lose its opening line. Decide that here.
    if !starts_with_header(raw) {
        return fallback(raw, cut);
    }
    let Some(msg) = MessageParser::default().parse(raw) else {
        return fallback(raw, cut);
    };

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
            text: String::new(),
            truncated: cut,
        };
    };
    let part = &msg.parts[index as usize];
    let is_attachment =
        part.is_content_type("message", "rfc822") || part.attachment_name().is_some();
    if is_attachment {
        return Body {
            text: String::new(),
            truncated: cut,
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
    finish(&text, part_cut)
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
fn fallback(raw: &[u8], cut: bool) -> Body {
    let text = String::from_utf8_lossy(raw);
    let body = match text.find("\r\n\r\n").or_else(|| text.find("\n\n")) {
        Some(i) if starts_with_header(raw) => &text[i..],
        _ => &text[..],
    };
    finish(body, cut)
}

fn finish(text: &str, cut: bool) -> Body {
    let tidy = tidy(text);
    let mut out: String = tidy.chars().take(BODY_CHARS).collect();
    let over = out.len() < tidy.len();
    if over {
        // End on a whole line rather than mid-word.
        if let Some(i) = out.rfind('\n')
            && i > BODY_CHARS / 2
        {
            out.truncate(i);
        }
    }
    Body {
        text: out.trim_end().to_string(),
        truncated: cut || over,
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
