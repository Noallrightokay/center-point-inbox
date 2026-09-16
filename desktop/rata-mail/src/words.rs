//! Making a mail header readable.
//!
//! A subject line arrives from IMAP as raw header bytes, and anything outside
//! ASCII is wrapped in RFC 2047 encoded-words:
//!
//! ```text
//! =?UTF-8?B?SW52b2ljZSDigJMgTWFyY2g=?=      "Invoice – March"
//! =?iso-8859-1?Q?Caf=E9_ferm=E9?=            "Café fermé"
//! ```
//!
//! The JavaScript never had to think about this — imapflow decoded headers on
//! the way past. Here nothing does, and skipping it is not cosmetic: a good
//! share of real mail has an accent, an em dash or an emoji in the subject, and
//! every one of those messages would show the customer a line of `=?UTF-8?B?`
//! instead of a subject. So it is decoded here, with no new dependency.
//!
//! Anything that is not a well-formed encoded-word is passed through unchanged,
//! which is the only safe failure: a subject shown slightly wrong still tells
//! you what the message is.

/// Decode a header value: encoded-words become text, everything else is left
/// as it was.
pub fn decode(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    // Whitespace between two adjacent encoded-words is separator, not content
    // (RFC 2047 §6.2) — a long subject split across several words must not come
    // back with gaps in the middle of it.
    let mut pending_space: Option<String> = None;
    let mut last_was_word = false;

    while i < raw.len() {
        if raw[i] == b'=' && i + 1 < raw.len() && raw[i + 1] == b'?'
            && let Some((text, next)) = word_at(raw, i)
        {
            if !last_was_word && let Some(sp) = pending_space.take() {
                out.push_str(&sp);
            }
            pending_space = None;
            out.push_str(&text);
            last_was_word = true;
            i = next;
            continue;
        }
        let ch = raw[i];
        if ch.is_ascii_whitespace() {
            // Hold it back until we know whether an encoded-word follows.
            pending_space.get_or_insert_with(String::new).push(ch as char);
            i += 1;
            continue;
        }
        if let Some(sp) = pending_space.take() {
            out.push_str(&sp);
        }
        last_was_word = false;
        // A raw non-ASCII byte in a header is out of spec but common; UTF-8 is
        // the overwhelmingly likely intent, so gather the run and decode it.
        //
        // The first byte is taken unconditionally, and that is not a detail:
        // when it is the `=` of something that only *looks* like an encoded
        // word, a scan that stopped at `=?` would stop immediately, consume
        // nothing, and spin here forever. Always move at least one byte.
        let start = i;
        i += 1;
        while i < raw.len()
            && !raw[i].is_ascii_whitespace()
            && !(raw[i] == b'=' && raw.get(i + 1) == Some(&b'?'))
        {
            i += 1;
        }
        out.push_str(&String::from_utf8_lossy(&raw[start..i]));
    }
    if let Some(sp) = pending_space {
        out.push_str(&sp);
    }
    out
}

/// One `=?charset?enc?text?=` starting at `at`. `None` when it is not actually
/// one, in which case the caller passes the bytes through.
fn word_at(raw: &[u8], at: usize) -> Option<(String, usize)> {
    let rest = &raw[at + 2..];
    let q1 = find(rest, b'?')?;
    let charset = &rest[..q1];
    let enc = *rest.get(q1 + 1)?;
    if rest.get(q1 + 2)? != &b'?' {
        return None;
    }
    let body_from = q1 + 3;
    let end = find_seq(&rest[body_from..], b"?=")? + body_from;
    let body = &rest[body_from..end];
    // An encoded-word may not contain whitespace; if it does, this is not one.
    if body.iter().any(|b| b.is_ascii_whitespace()) || charset.is_empty() {
        return None;
    }

    let bytes = match enc.to_ascii_uppercase() {
        b'B' => base64(body),
        b'Q' => quoted(body),
        _ => return None,
    };
    Some((to_text(charset, &bytes), at + 2 + end + 2))
}

/// Bytes to text, for the charsets that actually turn up in mail headers.
fn to_text(charset: &[u8], bytes: &[u8]) -> String {
    let cs = String::from_utf8_lossy(charset).to_ascii_lowercase();
    match cs.as_str() {
        // Latin-1 maps byte-for-byte onto the first 256 code points. Windows-1252
        // differs only in 0x80–0x9f; decoding it as Latin-1 turns a smart quote
        // into an invisible control character rather than mojibake, which is the
        // better of the two wrong answers and needs no table.
        "iso-8859-1" | "latin1" | "iso8859-1" | "windows-1252" | "cp1252" => {
            bytes.iter().map(|&b| b as char).collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn find(hay: &[u8], needle: u8) -> Option<usize> {
    hay.iter().position(|&b| b == needle)
}

fn find_seq(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Base64, ignoring anything outside the alphabet. Written out rather than
/// pulled in: it is twenty lines, and a dependency in the app that reads the
/// customer's mail is a dependency worth not having.
fn base64(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &c in input {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue, // padding, and anything malformed
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Quoted-printable as encoded-words use it: `_` is a space, `=XX` is a byte.
fn quoted(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < input.len() => match hex(input[i + 1], input[i + 2]) {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(b'=');
                    i += 1;
                }
            },
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn hex(a: u8, b: u8) -> Option<u8> {
    let n = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    Some(n(a)? * 16 + n(b)?)
}

/// A body down to something showable in a one-line preview: tags out,
/// whitespace collapsed.
pub fn plain(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if in_tag => {}
            _ => out.push(ch),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cut to `n` characters, not bytes — slicing a String by bytes panics in the
/// middle of anything non-ASCII, which is exactly the mail this module exists
/// for.
pub fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> String {
        decode(s.as_bytes())
    }

    #[test]
    fn plain_ascii_is_left_exactly_as_it_was() {
        assert_eq!(d("Invoice for March"), "Invoice for March");
        assert_eq!(d(""), "");
        assert_eq!(d("Re: [ticket-42] status?"), "Re: [ticket-42] status?");
    }

    #[test]
    fn the_two_encodings_real_mail_uses() {
        assert_eq!(d("=?UTF-8?B?SW52b2ljZSDigJMgTWFyY2g=?="), "Invoice – March");
        assert_eq!(d("=?iso-8859-1?Q?Caf=E9_ferm=E9?="), "Café fermé");
        assert_eq!(d("=?utf-8?q?Caf=C3=A9?="), "Café");
        // Emoji, which is where a byte-wise clip would later panic.
        assert_eq!(d("=?UTF-8?B?8J+OiSBEb25l?="), "🎉 Done");
    }

    #[test]
    fn a_word_in_the_middle_of_a_line() {
        assert_eq!(d("Re: =?UTF-8?B?Q2Fmw6k=?= closing"), "Re: Café closing");
    }

    #[test]
    fn adjacent_words_join_without_a_gap() {
        // A long subject split across words: the space between them is
        // separator, not content, so "Café" must not come back as "Caf é".
        assert_eq!(d("=?utf-8?q?Caf?= =?utf-8?q?=C3=A9?="), "Café");
        // But a real space either side of a single word survives.
        assert_eq!(d("a =?utf-8?q?b?= c"), "a b c");
    }

    #[test]
    fn something_that_only_looks_like_an_encoded_word_is_passed_through() {
        for raw in [
            "=?UTF-8?B?",              // truncated
            "=?UTF-8?X?abc?=",         // no such encoding
            "=??B?abc?=",              // no charset
            "=?UTF-8?B?ab cd?=",       // whitespace inside
            "what =? about this",
        ] {
            assert_eq!(d(raw), raw, "{raw:?} should survive untouched");
        }
    }

    #[test]
    fn every_input_makes_progress() {
        // The first cut of this loop consumed nothing when it met a `=?` that
        // did not open a valid encoded word, and spun on it forever — a bug
        // that fails as a hung refresh rather than an error, which is the worst
        // way for one to fail. Every string here ends in that state.
        for raw in ["=?", "=?=?=?", "a=?b", "=?x?Y?z?= =?", "==??", "=?UTF-8?B?ab cd?=x"] {
            let out = d(raw);
            assert!(!out.is_empty(), "{raw:?} produced nothing");
        }
    }

    #[test]
    fn an_unknown_charset_still_produces_readable_text() {
        // Rather than an error or an empty subject.
        assert_eq!(d("=?koi8-r?B?dGVzdA==?="), "test");
    }

    #[test]
    fn a_body_becomes_one_line_of_prose() {
        assert_eq!(
            plain(b"<p>Hello   <b>there</b></p>\r\n\r\n<div>again</div>"),
            "Hello there again"
        );
        assert_eq!(plain(b"   "), "");
    }

    #[test]
    fn clipping_counts_characters_rather_than_bytes() {
        // The panic this exists to prevent: "é" is two bytes, "🎉" is four.
        assert_eq!(clip("héllo", 2), "hé");
        assert_eq!(clip("🎉🎉🎉", 2), "🎉🎉");
        assert_eq!(clip("short", 50), "short");
    }
}
