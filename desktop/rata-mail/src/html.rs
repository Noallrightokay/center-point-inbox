//! An HTML email as text.
//!
//! RATA shows mail as text, and most mail is HTML: receipts, newsletters,
//! password resets, anything a company sends. The job here is to keep what a
//! person reads and acts on — the words in their order, paragraphs and table
//! rows on their own lines, and every link's address, because "Verify your
//! email" is useless without the URL behind it — and drop what they never see:
//! styles, scripts, hidden preheaders, tracking pixels.
//!
//! This is deliberately a reader, not a renderer or a sanitiser. Its output is
//! plain text that the interface escapes; nothing it produces is ever treated
//! as markup, so a hostile message can at worst make its own text look odd.

/// Elements whose content is never shown.
const SKIP: &[&str] = &[
    "head", "style", "script", "title", "noscript", "template", "svg", "xml",
];

/// Elements that start on a new line and end one.
const BLOCK: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "aside",
    "nav",
    "center",
    "blockquote",
    "pre",
    "address",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "ul",
    "ol",
    "dl",
    "dt",
    "dd",
    "hr",
    "body",
    "html",
];

/// Elements with a blank line after them.
const PARAGRAPH: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "blockquote",
    "ul",
    "ol",
    "hr",
];

/// Void elements: no closing tag, so never on the hidden-element stack.
const VOID: &[&str] = &[
    "br", "img", "hr", "meta", "link", "input", "col", "area", "base", "wbr", "source", "track",
    "embed", "param",
];

pub fn to_text(html: &str) -> String {
    let mut out = Out::default();
    let mut rest = html;
    // Innermost element whose content is being skipped, and how deeply that
    // same tag is nested inside itself, so its own closing tag is the one that
    // ends the skip.
    let mut hidden: Option<(String, usize)> = None;
    let mut link: Option<(String, usize)> = None;

    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if hidden.is_none() {
                out.text(rest);
            }
            break;
        };
        if hidden.is_none() {
            out.text(&rest[..lt]);
        }
        rest = &rest[lt..];

        // Comments, conditional comments, CDATA, doctype.
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map_or("", |i| &r[i + 3..]);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            rest = rest.find('>').map_or("", |i| &rest[i + 1..]);
            continue;
        }

        let tag = match Tag::parse(rest) {
            Ok(tag) => tag,
            // A '<' that opens nothing — "a < b" — is text.
            Err(Miss::NotATag) => {
                if hidden.is_none() {
                    out.text("<");
                }
                rest = &rest[1..];
                continue;
            }
            // Too long to be a tag worth reading, or a quote that never closes:
            // skip to the next '>', as a browser would. With none left at all,
            // the message was cut off inside this tag.
            Err(Miss::Unfinished) => match rest.find('>') {
                Some(i) => {
                    rest = &rest[i + 1..];
                    continue;
                }
                None => break,
            },
        };
        rest = &rest[tag.len..];
        let name = tag.name.as_str();

        if let Some((ref h, ref mut depth)) = hidden {
            if name == h {
                if tag.closing {
                    *depth -= 1;
                    if *depth == 0 {
                        hidden = None;
                    }
                } else if !tag.self_closing {
                    *depth += 1;
                }
            }
            continue;
        }

        if !tag.closing
            && !tag.self_closing
            && !VOID.contains(&name)
            && (SKIP.contains(&name) || tag.hidden)
        {
            hidden = Some((name.to_string(), 1));
            continue;
        }

        match (name, tag.closing) {
            ("br", _) => out.newline(),
            ("td" | "th", false) => out.cell(),
            ("tr", _) => out.newline(),
            ("li", false) => {
                out.newline();
                out.raw("• ");
            }
            ("img", false) => {
                if let Some(alt) = tag.attr("alt") {
                    let alt = decode_entities(alt);
                    let alt = alt.trim();
                    if !alt.is_empty() {
                        out.text(alt);
                        out.text(" ");
                    }
                }
            }
            ("a", false) => {
                link = tag
                    .attr("href")
                    .map(|h| (decode_entities(h).trim().to_string(), out.buf.len()));
            }
            ("a", true) => {
                if let Some((href, start)) = link.take() {
                    // `get`, not indexing: the text is a stranger's, and a panic
                    // here would take the whole refresh down with one message.
                    let shown = out.buf.get(start..).unwrap_or("").trim().to_string();
                    if let Some(target) = worth_showing(&href, &shown) {
                        out.text(" ");
                        out.raw(&format!("<{target}>"));
                    }
                }
            }
            (n, closing)
                if n.len() == 2 && n.starts_with('h') && n.as_bytes()[1].is_ascii_digit() =>
            {
                if closing {
                    out.paragraph();
                } else {
                    out.newline();
                }
            }
            (n, closing) if BLOCK.contains(&n) => {
                if closing && PARAGRAPH.contains(&n) {
                    out.paragraph();
                } else {
                    out.newline();
                }
                if n == "hr" {
                    out.paragraph();
                }
            }
            _ => {}
        }
    }
    out.finish()
}

/// The address to print after a link's text, or nothing when printing it adds
/// nothing: the text already is the address, or the link goes nowhere a person
/// could follow.
fn worth_showing(href: &str, shown: &str) -> Option<String> {
    let lower = href.to_ascii_lowercase();
    let target = if let Some(addr) = lower.strip_prefix("mailto:") {
        addr.split('?').next().unwrap_or("").to_string()
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        href.to_string()
    } else {
        return None;
    };
    if target.is_empty() || shown.is_empty() {
        return None;
    }
    let bare = |s: &str| {
        s.trim_end_matches('/')
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .to_ascii_lowercase()
    };
    if bare(&target) == bare(shown) || shown.to_ascii_lowercase().contains(&bare(&target)) {
        return None;
    }
    Some(target)
}

/// Text as it accumulates: HTML whitespace collapsed, line structure kept.
#[derive(Default)]
struct Out {
    buf: String,
}

impl Out {
    fn text(&mut self, s: &str) {
        let s = decode_entities(s);
        for (i, word) in s
            .split(|c: char| c.is_whitespace() && c != '\u{a0}')
            .enumerate()
        {
            if i > 0 && !self.buf.ends_with([' ', '\n']) && !self.buf.is_empty() {
                self.buf.push(' ');
            }
            self.buf.push_str(word);
        }
    }
    fn raw(&mut self, s: &str) {
        self.buf.push_str(s);
    }
    fn newline(&mut self) {
        self.trim_spaces();
        if !self.buf.is_empty() && !self.buf.ends_with('\n') {
            self.buf.push('\n');
        }
    }
    fn paragraph(&mut self) {
        self.newline();
        if !self.buf.is_empty() && !self.buf.ends_with("\n\n") {
            self.buf.push('\n');
        }
    }
    fn cell(&mut self) {
        self.trim_spaces();
        if !self.buf.is_empty() && !self.buf.ends_with('\n') {
            self.buf.push_str("  ");
        }
    }
    fn trim_spaces(&mut self) {
        while self.buf.ends_with(' ') {
            self.buf.pop();
        }
    }
    fn finish(self) -> String {
        self.buf
            .lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }
}

/// The longest tag read as one. Anything longer — an inline image as a data
/// URI, or junk — is skipped rather than parsed.
const MAX_TAG: usize = 4096;

enum Miss {
    /// Not the start of a tag at all: text that happens to contain '<'.
    NotATag,
    /// Starts like a tag but its end was not found within `MAX_TAG`.
    Unfinished,
}

struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
    hidden: bool,
    attrs: String,
    len: usize,
}

impl Tag {
    /// A tag at the start of `s`, or `None` if `s` does not open one.
    fn parse(s: &str) -> Result<Tag, Miss> {
        let b = s.as_bytes();
        let closing = b.get(1) == Some(&b'/');
        let start = if closing { 2 } else { 1 };
        if !b.get(start).is_some_and(|c| c.is_ascii_alphabetic()) {
            return Err(Miss::NotATag);
        }
        // The end is the first '>' outside a quoted attribute value, looked for
        // only so far: the input is a stranger's, and an unbounded search from
        // every '<' is how a message full of stray quotes stalls a refresh.
        let mut quote = None;
        let mut end = None;
        for (i, &c) in b.iter().enumerate().skip(start).take(MAX_TAG) {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(c),
                (None, b'>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end.ok_or(Miss::Unfinished)?;
        let inner = &s[start..end];
        let name_len = inner
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':'))
            .unwrap_or(inner.len());
        let name = inner[..name_len].to_ascii_lowercase();
        let attrs = inner[name_len..].to_string();
        let self_closing = attrs.trim_end().ends_with('/');
        let style = attr_in(&attrs, "style")
            .unwrap_or("")
            .to_ascii_lowercase()
            .replace(' ', "");
        let hidden = style.contains("display:none")
            || style.contains("visibility:hidden")
            || attr_in(&attrs, "hidden").is_some();
        Ok(Tag {
            name,
            closing,
            self_closing,
            hidden,
            attrs,
            len: end + 1,
        })
    }

    fn attr(&self, name: &str) -> Option<&str> {
        attr_in(&self.attrs, name)
    }
}

/// An attribute's value from a tag's attribute text. A bare attribute is
/// present with an empty value.
fn attr_in<'a>(attrs: &'a str, want: &str) -> Option<&'a str> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let before = i;
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let name = &attrs[ns..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let vs = i + 1;
                    let ve = attrs[vs..].find(q as char).map_or(attrs.len(), |e| vs + e);
                    i = (ve + 1).min(b.len());
                    &attrs[vs..ve]
                }
                _ => {
                    let vs = i;
                    while i < b.len() && !b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    &attrs[vs..i]
                }
            }
        } else {
            ""
        };
        if name.eq_ignore_ascii_case(want) {
            return Some(value);
        }
        // Never stuck, and never stepping into the middle of a character: the
        // attribute text is a stranger's, and a slice inside 'é' panics.
        if i == before {
            i += attrs[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// Character references to characters. Unknown names are left as written.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map(|e| e + 1);
        let (body, len) = match end {
            Some(e) if e > 1 => (&rest[1..e], e + usize::from(rest[e..].starts_with(';'))),
            None if rest.len() > 1 => (&rest[1..], rest.len()),
            _ => {
                out.push('&');
                rest = &rest[1..];
                continue;
            }
        };
        let ch = if let Some(num) = body.strip_prefix('#') {
            let n = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse().ok(),
            };
            n.and_then(char::from_u32).map(|c| c.to_string())
        } else {
            named(body).map(str::to_string)
        };
        match ch {
            Some(c) => {
                out.push_str(&c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" | "AMP" => "&",
        "lt" | "LT" => "<",
        "gt" | "GT" => ">",
        "quot" | "QUOT" => "\"",
        "apos" => "'",
        "nbsp" => "\u{a0}",
        "zwnj" => "\u{200c}",
        "zwj" => "\u{200d}",
        "shy" => "\u{ad}",
        "middot" => "·",
        "bull" => "•",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "euro" => "€",
        "pound" => "£",
        "yen" => "¥",
        "cent" => "¢",
        "deg" => "°",
        "times" => "×",
        "divide" => "÷",
        "rarr" => "→",
        "larr" => "←",
        "uarr" => "↑",
        "darr" => "↓",
        "check" => "✓",
        "eacute" => "é",
        "egrave" => "è",
        "aacute" => "á",
        "agrave" => "à",
        "ouml" => "ö",
        "uuml" => "ü",
        "auml" => "ä",
        "szlig" => "ß",
        "ntilde" => "ñ",
        "ccedil" => "ç",
        "iexcl" => "¡",
        "iquest" => "¿",
        "ensp" => "\u{2002}",
        "emsp" => "\u{2003}",
        "thinsp" => "\u{2009}",
        _ => return None,
    })
}

// ---------------------------------------------------------------- formatted

/// An HTML email made safe to show as HTML, and whether it asks for pictures
/// from the internet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Safe {
    pub html: String,
    /// True when it has `http(s)` images. They are not loaded unless the
    /// customer asks: loading one tells the sender the mail was opened, when,
    /// and from what IP address.
    pub remote_images: bool,
}

/// The largest picture carried inside a message that is shown inline, and the
/// most of them in total. Past these the picture is simply not shown.
const INLINE_ONE: usize = 1024 * 1024;
const INLINE_ALL: usize = 4 * 1024 * 1024;

/// An HTML email, cleaned.
///
/// This is the first of two locks. Everything that can run or submit goes —
/// scripts, event handlers, `javascript:` links, forms, frames, objects,
/// `<meta>` refreshes, `<base>` — using ammonia, a sanitiser built on a real
/// HTML parser, because a hand-written one is how the tricks get through.
/// Layout survives: tables and their old attributes, fonts, colours, and
/// `<style>` blocks, which is what makes a receipt look like a receipt. The
/// second lock is in the interface: the result is shown in a sandboxed frame
/// with scripts off and no way to reach the page around it.
///
/// `inline` maps a Content-ID to the picture carried in the message under it;
/// `cid:` references become `data:` URLs so those pictures show without any
/// request being made.
pub fn safe(html: &str, inline: &[(String, String, Vec<u8>)]) -> Safe {
    let mut html = html.to_string();
    let mut budget = INLINE_ALL;
    for (cid, mime, data) in inline {
        let image = matches!(
            mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        );
        let reference = format!("cid:{cid}");
        if !image || data.len() > INLINE_ONE || data.len() > budget || !html.contains(&reference) {
            continue;
        }
        budget -= data.len();
        let url = format!("data:{mime};base64,{}", crate::words::base64_encode(data));
        html = html.replace(&reference, &url);
    }
    let cleaned = cleaner().clean(&html).to_string();
    // Quotes and spaces vary with how the sanitiser writes a value back out.
    // This only decides whether to offer "Load images"; what actually stops a
    // remote picture loading is the frame's content security policy.
    let flat: String = cleaned
        .to_ascii_lowercase()
        .replace("&quot;", "")
        .replace("&#39;", "")
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | ' ' | '\t' | '\n'))
        .collect();
    let remote_images = ["src=http", "url(http", "background=http", "srcset=http"]
        .iter()
        .any(|p| flat.contains(p));
    Safe {
        html: cleaned,
        remote_images,
    }
}

fn cleaner() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_tags([
        "center", "font", "style", "big", "small", "u", "s", "strike", "tt",
    ])
    .rm_clean_content_tags(["style"])
    .add_clean_content_tags([
        "title", "noscript", "template", "xml", "textarea", "select", "option", "button",
    ])
    .add_generic_attributes([
        "style",
        "align",
        "valign",
        "bgcolor",
        "width",
        "height",
        "dir",
        "class",
        "border",
        "background",
    ])
    .add_tag_attributes(
        "table",
        [
            "cellpadding",
            "cellspacing",
            "border",
            "bgcolor",
            "width",
            "align",
        ],
    )
    .add_tag_attributes(
        "td",
        [
            "colspan", "rowspan", "nowrap", "bgcolor", "width", "height", "align", "valign",
        ],
    )
    .add_tag_attributes(
        "th",
        [
            "colspan", "rowspan", "nowrap", "bgcolor", "width", "height", "align", "valign",
        ],
    )
    .add_tag_attributes("img", ["src", "alt", "width", "height", "border"])
    .add_tag_attributes("font", ["color", "face", "size"])
    // A link keeps its address only if it is a web page or someone to write
    // to. Followed inside the frame it would go nowhere (the frame may not
    // navigate); clicked, it asks for a new window, which the app refuses and
    // turns into "open this in your browser?", naming where it really goes.
    .attribute_filter(|element, attribute, value| {
        if element == "a" && attribute == "href" {
            return link_target(value).then_some(value.into());
        }
        Some(value.into())
    })
    .rm_tags(["area", "map"])
    .link_rel(Some("noopener noreferrer"))
    .add_url_schemes(["data"])
    .url_relative(ammonia::UrlRelative::Deny)
    .filter_style_properties(STYLE_PROPERTIES.iter().copied().collect())
    .strip_comments(true);
    b
}

/// Whether a link's address may stay in the message: http, https and
/// mailto, nothing else. The app checks again before acting on a click.
fn link_target(href: &str) -> bool {
    let lower = href.trim_start().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// The CSS allowed in a `style` attribute: what lays out and colours a
/// message. Nothing here can run anything; the list exists so that nothing
/// new and unknown gets through by default.
const STYLE_PROPERTIES: &[&str] = &[
    "color",
    "background",
    "background-color",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-color",
    "border-style",
    "border-width",
    "border-radius",
    "border-collapse",
    "border-spacing",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "width",
    "min-width",
    "max-width",
    "height",
    "min-height",
    "max-height",
    "display",
    "vertical-align",
    "text-align",
    "text-decoration",
    "text-transform",
    "text-indent",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "line-height",
    "letter-spacing",
    "word-spacing",
    "white-space",
    "word-break",
    "overflow-wrap",
    "list-style",
    "list-style-type",
    "table-layout",
    "opacity",
    "float",
    "clear",
    "overflow",
    "direction",
    "box-sizing",
    "mso-line-height-rule",
];

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_headings_get_their_own_lines() {
        assert_eq!(
            to_text("<h1>Thanks, Sam!</h1><p>Your order has shipped.</p><p>See you soon.</p>"),
            "Thanks, Sam!\n\nYour order has shipped.\n\nSee you soon."
        );
        assert_eq!(to_text("one<br>two<br/>three"), "one\ntwo\nthree");
    }

    #[test]
    fn table_cells_stay_apart_and_rows_stay_on_their_own_lines() {
        let t = to_text(
            "<table><tr><td>Item</td><td>Qty</td></tr><tr><td>Blue mug</td><td>2</td></tr></table>",
        );
        assert_eq!(t, "Item  Qty\nBlue mug  2");
    }

    #[test]
    fn a_link_keeps_its_address() {
        assert_eq!(
            to_text(
                r#"<p>Please <a href="https://example.com/verify?t=abc&amp;u=1">verify your email</a>.</p>"#
            ),
            "Please verify your email <https://example.com/verify?t=abc&u=1>."
        );
        // Unless the text already is the address, or it goes nowhere a person can.
        assert_eq!(
            to_text(r#"<a href="https://example.com/">example.com</a>"#),
            "example.com"
        );
        assert_eq!(to_text(r#"<a href="javascript:x()">Click</a>"#), "Click");
        assert_eq!(
            to_text(r#"<a href="mailto:help@shop.example?subject=Hi">Contact us</a>"#),
            "Contact us <help@shop.example>"
        );
        assert_eq!(
            to_text(r#"<a href="https://t.example/p"><img src="logo.png" alt=""></a>"#),
            ""
        );
    }

    #[test]
    fn what_the_reader_never_sees_is_left_out() {
        let t = to_text(
            r#"<html><head><title>T</title><style>p{color:red}</style></head><body>
               <script>alert(1)</script>
               <div style="display: none; max-height:0">Preheader</div>
               <span hidden>also hidden</span>
               <p>Visible</p><img src="https://t.example/pixel.gif" width="1" height="1"></body></html>"#,
        );
        assert_eq!(t, "Visible");
    }

    #[test]
    fn a_hidden_element_ends_at_its_own_closing_tag() {
        assert_eq!(
            to_text(
                r#"<div style="display:none"><div>inner</div>still hidden</div><div>shown</div>"#
            ),
            "shown"
        );
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            to_text("Fish &amp; chips &mdash; &pound;9 &#8364;10 &#x2713; &nbsp;done"),
            "Fish & chips — £9 €10 ✓ \u{a0}done"
        );
        assert_eq!(
            decode_entities("AT&T &unknown; & alone"),
            "AT&T &unknown; & alone"
        );
    }

    #[test]
    fn lists_are_bulleted() {
        assert_eq!(
            to_text("<ul><li>One</li><li>Two</li></ul><p>After</p>"),
            "• One\n• Two\n\nAfter"
        );
    }

    #[test]
    fn a_stray_angle_bracket_is_text() {
        assert_eq!(to_text("if a < b and c > d"), "if a < b and c > d");
        assert_eq!(to_text("<3 you"), "<3 you");
    }

    #[test]
    fn broken_markup_does_not_lose_the_rest() {
        // A quote that never closes loses only its own tag, not the message.
        assert_eq!(to_text(r#"<p class="x>Unclosed quote"#), "Unclosed quote");
        assert_eq!(to_text("<p>ok</p><div"), "ok");
        assert_eq!(to_text("<!-- never closed"), "");
        assert_eq!(
            to_text("<!--[if mso]><p>outlook</p><![endif]--><p>everyone</p>"),
            "everyone"
        );
    }

    #[test]
    fn hostile_markup_is_read_in_linear_time() {
        // Stray quotes and unterminated tags, as much as one fetch can hold.
        // Rescanning from every '<' would be ~10^9 steps; this must stay near
        // the size of the input. Timed loosely so a slow machine cannot fail it.
        let image = format!(
            "<img src=\"data:image/png;base64,{}\">After",
            "A".repeat(60_000)
        );
        for bomb in [
            "<a x=\"".repeat(11_000),
            "<a\"".repeat(21_000) + ">",
            // An odd number of quotes leaves the only '>' inside one.
            "<a\"".repeat(21_001) + ">",
            image.clone(),
        ] {
            let started = std::time::Instant::now();
            let _ = to_text(&bomb);
            let took = started.elapsed();
            assert!(took < std::time::Duration::from_secs(2), "{took:?}");
        }
        assert_eq!(to_text(&image), "After");
    }

    /// Deterministic pseudo-random inputs built from the pieces that make
    /// markup hard: partial tags, quotes, entities, multi-byte characters.
    pub(crate) fn junk(seed: u64, pieces: &[&str], n: usize) -> String {
        let mut x = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut out = String::new();
        for _ in 0..n {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            out.push_str(pieces[(x % pieces.len() as u64) as usize]);
        }
        out
    }

    #[test]
    fn no_markup_makes_it_panic() {
        let pieces = [
            "<",
            ">",
            "</",
            "/>",
            "<a",
            " href=\"",
            "\"",
            "'",
            "=",
            "<p>",
            "</p>",
            "<div style=\"display:none\">",
            "</div>",
            "<td>",
            "<li>",
            "<br>",
            "<img alt=\"é\">",
            "</a>",
            "&",
            "&amp;",
            "&#",
            "&#x",
            ";",
            "é",
            "日本",
            "👍",
            " ",
            "\n",
            "<!--",
            "-->",
            "<!",
            "<script>",
            "</script>",
            "https://x.example/",
            "mailto:",
            "<h2>",
        ];
        for seed in 0..3000 {
            let _ = to_text(&junk(seed, &pieces, 60));
        }
    }

    #[test]
    fn alt_text_stands_in_for_a_picture() {
        assert_eq!(
            to_text(r#"<img src="x.png" alt="Shop the sale"><p>Ends Sunday</p>"#),
            "Shop the sale\nEnds Sunday"
        );
    }

    // ------------------------------------------------------------- formatted

    fn clean(html: &str) -> Safe {
        safe(html, &[])
    }

    #[test]
    fn nothing_that_can_run_survives() {
        for hostile in [
            "<script>alert(1)</script>",
            "<img src=x onerror=alert(1)>",
            "<a href=\"javascript:alert(1)\">x</a>",
            "<a href=\"JaVaScRiPt:alert(1)\">x</a>",
            "<svg><script>alert(1)</script></svg>",
            "<iframe src=\"https://evil.example\"></iframe>",
            "<object data=\"x\"></object><embed src=\"x\">",
            "<form action=\"https://evil.example\"><input name=p></form>",
            "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.example\">",
            "<base href=\"https://evil.example/\">",
            "<body onload=alert(1)>",
            "<div style=\"background:url(javascript:alert(1))\">x</div>",
            "<math><mtext><table><mglyph><style><img src=x onerror=alert(1)>",
            "<noscript><p title=\"</noscript><img src=x onerror=alert(1)>\">",
        ] {
            let out = clean(hostile).html.to_ascii_lowercase();
            for bad in [
                "<script",
                "onerror",
                "onload",
                "javascript:",
                "<iframe",
                "<object",
                "<embed",
                "<form",
                "<meta",
                "<base",
                "<input",
            ] {
                assert!(!out.contains(bad), "{hostile:?} left {bad:?} in {out:?}");
            }
        }
    }

    #[test]
    fn a_receipt_keeps_its_layout() {
        let out = clean(
            "<html><head><title>Receipt</title><style>.total{font-weight:bold}</style></head><body>\
             <table width=\"600\" cellpadding=\"4\" bgcolor=\"#ffffff\"><tr><td align=\"right\" style=\"color:#333;padding:8px\">\
             <font face=\"Arial\" color=\"#111\">Total</font></td><td class=\"total\">$18.00</td></tr></table></body></html>",
        )
        .html;
        for kept in [
            "<table",
            "width=\"600\"",
            "cellpadding=\"4\"",
            "bgcolor=\"#ffffff\"",
            "align=\"right\"",
            "color:#333",
            "<font",
            "face=\"Arial\"",
            "<style>",
            ".total{font-weight:bold}",
            "class=\"total\"",
            "$18.00",
        ] {
            assert!(out.contains(kept), "lost {kept:?}: {out}");
        }
        assert!(
            !out.contains("Receipt"),
            "the title is not body text: {out}"
        );
    }

    #[test]
    fn remote_pictures_are_noticed_and_embedded_ones_carried() {
        let tracked = clean("<p>Hi</p><img src=\"https://t.example/open.gif\" width=1 height=1>");
        assert!(tracked.remote_images);
        let styled = clean("<div style=\"background: url('https://t.example/bg.png')\">x</div>");
        assert!(styled.remote_images, "{}", styled.html);
        let plain = clean("<p>Just words</p>");
        assert!(!plain.remote_images);

        let logo = vec![137, 80, 78, 71];
        let out = safe(
            "<img src=\"cid:logo@x\" alt=\"Logo\"><img src=\"cid:page@x\">",
            &[
                ("logo@x".into(), "image/png".into(), logo),
                ("page@x".into(), "text/html".into(), b"<script>".to_vec()),
            ],
        );
        assert!(
            out.html.contains("src=\"data:image/png;base64,iVBORw==\""),
            "{}",
            out.html
        );
        assert!(
            !out.html.contains("text/html"),
            "only pictures are inlined: {}",
            out.html
        );
        assert!(!out.remote_images);
    }

    #[test]
    fn web_and_mail_links_keep_their_addresses_and_nothing_else_does() {
        let out = clean("<p>Please <a href=\"https://shop.example/order?id=1&amp;t=2\" target=\"_self\">see your order</a> or <a href=\"mailto:help@shop.example\">write</a>.</p>").html;
        assert!(
            out.contains("href=\"https://shop.example/order?id=1&amp;t=2\""),
            "{out}"
        );
        assert!(out.contains("href=\"mailto:help@shop.example\""), "{out}");
        // Its own target would make it try to replace the frame; it takes the
        // frame's instead, which asks for a new window.
        assert!(!out.contains("target"), "{out}");
        assert!(out.contains("rel=\"noopener noreferrer\""), "{out}");
        for other in [
            "<a href=\"file:///etc/passwd\">x</a>",
            "<a href=\"data:text/html,x\">x</a>",
            "<a href=\"tel:+15550100\">x</a>",
            "<a href=\"ftp://example.com/x\">x</a>",
            "<a href=\"ms-msdt:/id x\">x</a>",
            "<a href=\"  javascript:alert(1)\">x</a>",
            "<map><area href=\"https://x.example\"></map>",
        ] {
            let out = clean(other).html;
            assert!(!out.contains("href"), "{other} -> {out}");
        }
    }

    #[test]
    fn a_relative_link_goes_nowhere() {
        // Relative to the app itself, it could only ever point at the app.
        let out = clean("<a href=\"/index.html\">x</a><img src=\"icons/mark.png\">").html;
        assert!(
            !out.contains("index.html") && !out.contains("icons/"),
            "{out}"
        );
    }
}
