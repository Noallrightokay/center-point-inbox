//! Saying that new mail has arrived, through the system's own notifications.
//!
//! What a notification says comes from strangers — a sender's name and a
//! subject line — and it is shown outside the app, where RATA's own escaping
//! does not reach. So it is made plain here: one line, no control characters,
//! no bidirectional-text controls (which can make a notification read
//! differently from what it says), a sensible length, and on Linux, where
//! notification servers read a little HTML in the body, no markup — a subject
//! of `<a href="…">Your bank</a>` must not become a link on the desktop.

/// The most characters of a title and of a body.
pub const TITLE_MAX: usize = 80;
pub const BODY_MAX: usize = 200;

/// Title and body, ready to show.
pub fn prepare(title: &str, body: &str) -> (String, String) {
    let title = plain(title, TITLE_MAX);
    let body = plain(body, BODY_MAX);
    (
        if title.is_empty() {
            "RATA".into()
        } else {
            title
        },
        markup_safe(&body),
    )
}

/// One line of plain text, at most `max` characters.
pub fn plain(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in s.chars() {
        if is_bidi(c) || (c.is_control() && !c.is_whitespace()) {
            continue;
        }
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(c);
    }
    if out.chars().count() > max {
        let cut: String = out.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    } else {
        out
    }
}

/// Characters that reorder the text around them.
fn is_bidi(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// On Linux the body may be read as markup; elsewhere it is plain already.
fn markup_safe(s: &str) -> String {
    if cfg!(target_os = "linux") {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strangers_words_are_shown_as_words() {
        let (t, b) = prepare(
            "Ann\u{202E}fdp.exe\n\tSmith\u{0007}",
            "<a href=\"https://phish.example\">Your bank</a> &\nmore",
        );
        assert_eq!(t, "Annfdp.exe Smith");
        if cfg!(target_os = "linux") {
            assert_eq!(
                b,
                "&lt;a href=\"https://phish.example\"&gt;Your bank&lt;/a&gt; &amp; more"
            );
        } else {
            assert!(b.starts_with("<a href"));
        }
    }

    #[test]
    fn long_or_empty_is_kept_in_bounds() {
        let (t, b) = prepare("   ", &"x".repeat(1000));
        assert_eq!(t, "RATA");
        assert_eq!(b.chars().count(), BODY_MAX);
        assert!(b.ends_with('…'));
        assert_eq!(plain("  café   au  lait ", 80), "café au lait");
    }
}
