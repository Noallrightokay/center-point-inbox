//! Links, opened outside the app.
//!
//! The app's webview holds the bridge to everything in [`crate::commands`] —
//! the keychain, the mailboxes, sending. A page from the internet must never
//! load into it, so no link is ever followed here: a web address goes to the
//! customer's own browser, and only a web address does. Everything else a
//! link can name — `file:`, `javascript:`, `data:`, a custom scheme some
//! installed program registered — could run something, and is refused.
//!
//! An email address (`mailto:`) opens RATA's own composer instead of the
//! system's default mail program, which is not RATA and may not exist.

use tauri::Url;

/// Longer than any legitimate link; tracking links run to a couple of
/// thousand characters at most.
pub const URL_MAX: usize = 4096;
/// An address can be no longer than this (RFC 5321).
const ADDRESS_MAX: usize = 254;
const SUBJECT_MAX: usize = 200;

#[derive(Debug, PartialEq)]
pub enum Link {
    /// An http or https address, for the browser.
    Web(Url),
    /// Someone to write to, for the composer.
    Mail { to: String, subject: String },
}

/// What `raw` is, if it is anything RATA will act on.
pub fn classify(raw: &str) -> Option<Link> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > URL_MAX {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    match url.scheme() {
        "http" | "https" => web(url),
        "mailto" => mail(&url),
        _ => None,
    }
}

fn web(url: Url) -> Option<Link> {
    // No host, nowhere to go. And no user name or password in the address:
    // `https://bank.example@evil.example/` is the classic way of making a
    // link read as one site while going to another, and no real mail needs it.
    if url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(Link::Web(url))
}

fn mail(url: &Url) -> Option<Link> {
    let path = decode(url.path())?;
    // mailto: can list several people; the composer is started with the first.
    let to = path.split(',').next()?.trim();
    let (local, domain) = to.split_once('@')?;
    let plain = |s: &str| {
        !s.is_empty()
            && !s
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "<>\"(),;:@[]\\".contains(c))
    };
    if to.len() > ADDRESS_MAX || !plain(local) || !plain(domain) || !domain.contains('.') {
        return None;
    }
    let subject = url
        .query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case("subject"))
        .map(|(_, v)| {
            v.chars()
                .filter(|c| !c.is_control())
                .take(SUBJECT_MAX)
                .collect()
        })
        .unwrap_or_default();
    Some(Link::Mail {
        to: to.to_string(),
        subject,
    })
}

/// Percent-decoding for the address part of a `mailto:`, which `Url` leaves
/// encoded. `None` when the result is not text.
fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Hands a web address to the customer's browser.
pub fn open_in_browser(url: &Url) -> Result<(), String> {
    open::that_detached(url.as_str()).map_err(|e| format!("Your browser could not be opened: {e}"))
}

/// Where the app's own window may go.
#[derive(Debug, PartialEq)]
pub enum Navigation {
    /// The app's own pages, the frames inside them, and the files it makes
    /// for the customer to save (`blob:`, an export or a converted document).
    Stay,
    /// A link of RATA's own — the licence page, say — that belongs in the
    /// browser rather than in place of the app.
    Browser(Url),
    Refuse,
}

/// The app is served from `tauri://localhost` on macOS and Linux and
/// `http(s)://tauri.localhost` on Windows. Anything else would put an outside
/// page where the app was, with the bridge still attached to the window.
pub fn navigation(url: &Url) -> Navigation {
    match (url.scheme(), url.host_str()) {
        ("tauri" | "about" | "blob", _) => Navigation::Stay,
        ("http" | "https", Some("tauri.localhost")) => Navigation::Stay,
        ("http" | "https", Some("mailrata.org" | "www.mailrata.org")) => {
            match classify(url.as_str()) {
                Some(Link::Web(u)) => Navigation::Browser(u),
                _ => Navigation::Refuse,
            }
        }
        _ => Navigation::Refuse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_ok(s: &str) -> bool {
        matches!(classify(s), Some(Link::Web(_)))
    }

    #[test]
    fn web_addresses_go_to_the_browser() {
        assert!(web_ok("https://shop.example/order/123?t=abc"));
        assert!(web_ok("http://example.com"));
        assert!(web_ok("  HTTPS://Example.COM/x  "));
        match classify("https://xn--80ak6aa92e.com/") {
            Some(Link::Web(u)) => assert_eq!(u.host_str(), Some("xn--80ak6aa92e.com")),
            other => panic!("{other:?}"),
        }
        // A look-alike name is shown as what it really is.
        match classify("https://аpple.com/") {
            Some(Link::Web(u)) => assert!(u.host_str().unwrap().starts_with("xn--")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_else_runs() {
        for s in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "file:///etc/passwd",
            "file://C:/Windows/System32/calc.exe",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "smb://evil.example/share/x.exe",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x",
            "ftp://example.com/x",
            "tel:+15550100",
            "tauri://localhost/app.html",
            "/relative/path",
            "example.com",
            "",
            "https://",
        ] {
            assert_eq!(classify(s), None, "{s}");
        }
    }

    #[test]
    fn an_address_that_hides_where_it_goes_is_refused() {
        assert_eq!(classify("https://bank.example@evil.example/login"), None);
        assert_eq!(classify("https://user:pass@example.com/"), None);
    }

    #[test]
    fn a_very_long_link_is_refused() {
        let long = format!("https://example.com/{}", "a".repeat(URL_MAX));
        assert_eq!(classify(&long), None);
    }

    #[test]
    fn mailto_starts_a_message() {
        assert_eq!(
            classify("mailto:help@shop.example?subject=Order%20123"),
            Some(Link::Mail {
                to: "help@shop.example".into(),
                subject: "Order 123".into()
            })
        );
        assert_eq!(
            classify("mailto:first.last+tag@example.co.uk,other@example.com"),
            Some(Link::Mail {
                to: "first.last+tag@example.co.uk".into(),
                subject: String::new()
            })
        );
        assert_eq!(
            classify("MAILTO:a%40b.example"),
            Some(Link::Mail {
                to: "a@b.example".into(),
                subject: String::new()
            })
        );
        // Headers smuggled into the subject stay in the subject, minus the
        // line breaks that would make them headers.
        match classify("mailto:a@b.example?subject=Hi%0D%0ABcc:%20all@example.com") {
            Some(Link::Mail { subject, .. }) => {
                assert!(!subject.contains('\r') && !subject.contains('\n'))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_mailto_that_is_not_an_address_does_nothing() {
        for s in [
            "mailto:",
            "mailto:nobody",
            "mailto:a@b",
            "mailto:a%0D%0ABcc:x@y.example@b.example",
            "mailto:%FF@b.example",
            "mailto:%4",
            "mailto:\"a b\"@b.example",
            "mailto:<a@b.example>",
        ] {
            assert_eq!(classify(s), None, "{s}");
        }
    }

    #[test]
    fn the_window_stays_on_the_app() {
        let nav = |s: &str| navigation(&Url::parse(s).unwrap());
        assert_eq!(nav("tauri://localhost/app.html"), Navigation::Stay);
        assert_eq!(nav("http://tauri.localhost/app.html"), Navigation::Stay);
        assert_eq!(nav("https://tauri.localhost/auth.html"), Navigation::Stay);
        assert_eq!(nav("about:srcdoc"), Navigation::Stay);
        assert_eq!(nav("about:blank"), Navigation::Stay);
        assert_eq!(
            nav("blob:tauri://localhost/6f1c1d7e-0b6e-4a47-9d0a-2f1d3c4b5a69"),
            Navigation::Stay
        );
        assert!(matches!(
            nav("https://mailrata.org/account"),
            Navigation::Browser(_)
        ));
        for s in [
            "https://phish.example/login",
            "http://localhost:8080/",
            "https://tauri.localhost.evil.example/",
            "https://mailrata.org.evil.example/",
            "file:///etc/passwd",
            "data:text/html,x",
            "javascript:alert(1)",
        ] {
            assert_eq!(nav(s), Navigation::Refuse, "{s}");
        }
    }
}
