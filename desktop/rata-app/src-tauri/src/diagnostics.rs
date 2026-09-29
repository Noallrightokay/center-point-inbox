//! Settings → Copy diagnostics: one plain-text block a tester pastes into a
//! bug report (H8).
//!
//! The repository the report goes to is public, so the block says only what
//! a fix needs and nothing a stranger could use: the build, the system, the
//! licence's plan and day, and for each mailbox its servers, how it signs
//! in, whether it is parked, its last error and what RATA has seen of it this
//! session. **Never** an address (a mailbox is `Mailbox 1`), a password, a
//! token, the licence key, message text, or a path that could carry the
//! account's name. Every sentence that came from a server or an error goes
//! through [`clean`] first, and the whole block through [`no_at`] last, so
//! not even an `@` survives.
//!
//! Everything here comes from what the app already holds. Nothing signs in,
//! dials or asks a server anything to fill it.

use rata_mail::credential::said_within;

use crate::licence::on_day;

/// The most characters of one error sentence the block keeps: an app's
/// sentence with a server's 200 inside it, whole.
const LINE_MOST: usize = 600;

/// What the running build is. Filled by the command, which alone can see
/// the Tauri config the updater key is in.
pub struct Build {
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub release: bool,
    pub licence_key: bool,
    pub updater_key: bool,
    pub microsoft: bool,
}

impl Build {
    /// This build, but for the updater key, which lives in the Tauri config.
    pub fn this(updater_key: bool, licence_key: bool, microsoft: bool) -> Self {
        Build {
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            release: !cfg!(debug_assertions),
            licence_key,
            updater_key,
            microsoft,
        }
    }
}

/// The licence, as far as the block says anything about it: the plan and
/// the day, never the token or whose it is.
pub enum Licensed {
    Yes { plan: String, until: i64 },
    No { why: String, until: Option<i64> },
}

/// An error kept from the last time something went wrong with a mailbox.
#[derive(Debug, Clone)]
pub struct Trouble {
    pub at: u64,
    /// What was being done: `refresh`, `send`.
    pub doing: &'static str,
    /// The problem's kind (`net`, `auth`, `oauth`…).
    pub kind: String,
    pub said: String,
}

/// What the app has noticed about one mailbox since it started, kept in
/// memory only (`Rata::notes`).
#[derive(Debug, Clone, Default)]
pub struct Noted {
    pub last_error: Option<Trouble>,
    pub last_good: Option<u64>,
    /// The special folders a refresh brought mail, read/starred, a gap or a
    /// listing from: `sent`, `junk`, `archive`, `drafts`.
    pub folders: std::collections::BTreeSet<&'static str>,
    /// Archive is Gmail's All Mail, found by its `\All` attribute.
    pub all_mail: bool,
    /// `host:port` of the submission server the last message went through.
    pub smtp_used: Option<String>,
}

/// One mailbox, as the block needs it. `email` and `secrets` are only for
/// taking themselves out of what is shown; neither is ever written.
pub struct MailboxFacts {
    pub email: String,
    pub label: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub found_by: String,
    pub smtp_hosts: Vec<String>,
    pub oauth: bool,
    pub parked_since: Option<u64>,
    pub live: bool,
    pub noted: Noted,
    pub secrets: Vec<String>,
}

pub struct Facts {
    pub build: Build,
    pub licence: Licensed,
    pub mailboxes: Vec<MailboxFacts>,
    pub last_refresh: Option<u64>,
    pub schema: u32,
    pub on_disk: Option<u32>,
    pub now: u64,
}

/// The block, ready to paste.
pub fn render(f: &Facts) -> String {
    let b = &f.build;
    let yes = |v: bool| if v { "yes" } else { "no" };
    let mut out: Vec<String> = vec![
        "RATA diagnostics".into(),
        format!(
            "Version: {} ({} build)",
            b.version,
            if b.release { "release" } else { "debug" }
        ),
        format!("System: {} {}", b.os, b.arch),
        format!(
            "Build keys: licence key {}, updater key {}, Microsoft sign-in {}",
            yes(b.licence_key),
            yes(b.updater_key),
            yes(b.microsoft)
        ),
    ];
    out.push(match &f.licence {
        Licensed::Yes { plan, until } => {
            format!("Licence: {}, until {}", clean_plain(plan), on_day(*until))
        }
        Licensed::No { why, until } => format!(
            "Licence: not in use ({}){}",
            clean_plain(why),
            until
                .map(|u| format!(", dated until {}", on_day(u)))
                .unwrap_or_default()
        ),
    });
    out.push(format!(
        "Last refresh: {}",
        f.last_refresh
            .map(|t| when(t, f.now))
            .unwrap_or_else(|| "none since RATA started".into())
    ));
    out.push(format!(
        "Store: schema {}, file at start {}",
        f.schema,
        f.on_disk
            .map(|v| format!("version {v}"))
            .unwrap_or_else(|| "none".into())
    ));
    out.push(format!("Mailboxes: {}", f.mailboxes.len()));

    for (i, m) in f.mailboxes.iter().enumerate() {
        let name = format!("Mailbox {}", i + 1);
        let tidy = |s: &str| clean(s, &m.email, &name, &m.secrets);
        out.push(String::new());
        out.push(format!("{name}: {}", tidy(&m.label)));
        out.push(format!(
            "  IMAP: {}:{} (TLS from the start), found by {}",
            tidy(&m.imap_host),
            m.imap_port,
            if m.found_by.is_empty() {
                "an older RATA".into()
            } else {
                tidy(&m.found_by)
            }
        ));
        let smtp = m
            .smtp_hosts
            .iter()
            .map(|h| tidy(h))
            .collect::<Vec<_>>()
            .join(", then ");
        out.push(match &m.noted.smtp_used {
            Some(used) => format!(
                "  SMTP: last sent through {} ({})",
                tidy(used),
                smtp_mode(used)
            ),
            None => format!(
                "  SMTP: not used yet; tries {smtp} on 465 (TLS from the start), then 587 (STARTTLS)"
            ),
        });
        out.push(format!(
            "  Signs in with: {}",
            if m.oauth {
                "Microsoft (OAuth)"
            } else {
                "an app password"
            }
        ));
        out.push(match m.parked_since {
            None => "  Parked: no".into(),
            Some(t) if m.oauth => format!(
                "  Parked: yes, since {}: Microsoft's sign-in has to be renewed (Sign in to Microsoft again)",
                when(t, f.now)
            ),
            Some(t) => format!(
                "  Parked: yes, since {}: the server refused the app password",
                when(t, f.now)
            ),
        });
        out.push(match &m.noted.last_error {
            None => "  Last error: none since RATA started".into(),
            Some(e) => format!(
                "  Last error: {} ({}, {}): {}",
                when(e.at, f.now),
                e.doing,
                tidy(&e.kind),
                tidy(&e.said)
            ),
        });
        out.push(format!(
            "  Last good refresh: {}",
            m.noted
                .last_good
                .map(|t| when(t, f.now))
                .unwrap_or_else(|| "none since RATA started".into())
        ));
        out.push(format!("  Special folders seen: {}", folders(&m.noted)));
        out.push(format!(
            "  Live connection (new mail as it arrives): {}",
            if m.live { "up" } else { "down" }
        ));
    }
    no_at(&out.join("\n"))
}

/// Which special folders have shown up in a refresh, and how Archive was
/// found when that is known. The engine does not say whether the others
/// were found by attribute or by name.
fn folders(n: &Noted) -> String {
    let named = |f: &str| match f {
        "sent" => "Sent",
        "junk" => "Spam",
        "drafts" => "Drafts",
        "archive" if n.all_mail => "Archive (Gmail's All Mail, by its \\All attribute)",
        "archive" => "Archive",
        _ => "",
    };
    let seen: Vec<&str> = ["sent", "junk", "drafts", "archive"]
        .into_iter()
        .filter(|f| n.folders.contains(f))
        .map(named)
        .collect();
    if seen.is_empty() {
        "none yet".into()
    } else {
        seen.join(", ")
    }
}

/// How a submission server is spoken to, from the port it answered on.
fn smtp_mode(used: &str) -> &'static str {
    match used.rsplit_once(':').map(|(_, p)| p) {
        Some("465") => "TLS from the start",
        Some("587") => "STARTTLS",
        _ => "TLS mode unknown",
    }
}

/// A moment, in UTC to the minute, and how long ago.
fn when(t: u64, now: u64) -> String {
    let secs = t as i64;
    let in_day = secs.rem_euclid(86_400);
    let ago = now.saturating_sub(t);
    let ago = if ago < 60 {
        "just now".to_string()
    } else if ago < 3600 {
        format!("{} min ago", ago / 60)
    } else if ago < 172_800 {
        format!("{} h ago", ago / 3600)
    } else {
        format!("{} days ago", ago / 86_400)
    };
    format!(
        "{} {:02}:{:02} UTC, {ago}",
        on_day(secs),
        in_day / 3600,
        in_day % 3600 / 60
    )
}

/// An error sentence, or anything else a server or a person could have put
/// words into, fit for a public bug report:
///
/// 1. the mailbox's own address becomes `Mailbox N`;
/// 2. `said`, the engine's rule for a server's words, takes out every secret
///    (the password, the token and the forms they travel in), any start of
///    one, and every run of 16 or more base64 characters, and keeps one line
///    without control or bidi characters, here of at most `LINE_MOST`. The
///    address is one of the secrets too, so the base64 rule applies even when
///    the keychain gave nothing to look for;
/// 3. any other address, and every other `@`, becomes `[address]`;
/// 4. the home folder, which usually carries the account's name, becomes `~`.
pub fn clean(text: &str, email: &str, name: &str, secrets: &[String]) -> String {
    let text = replace_ci(text, email.trim(), name);
    let mut all: Vec<String> = secrets.iter().filter(|s| !s.is_empty()).cloned().collect();
    all.push(email.trim().to_string());
    let text = said_within(&text, &all, LINE_MOST);
    let text = addresses_out(&text);
    home_out(&text)
}

/// A sentence the app wrote itself (a plan's name, a licence's reason): one
/// line, no address, no home folder.
fn clean_plain(text: &str) -> String {
    home_out(&addresses_out(&said_within(text, &[], LINE_MOST)))
}

/// `text` with every `needle` replaced by `with`, ignoring ASCII case.
fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let (hay, low) = (text.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(at) = hay[from..].find(&low) {
        out.push_str(&text[from..from + at]);
        out.push_str(with);
        from += at + low.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Every run of characters around an `@` that could be part of an address,
/// replaced by `[address]`.
fn addresses_out(text: &str) -> String {
    let local = |c: char| c.is_alphanumeric() || "._%+-'!#$&*=?^`{|}~".contains(c);
    let domain = |c: char| c.is_alphanumeric() || ".-[]:".contains(c);
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Take back the local part already written.
        while out.chars().last().is_some_and(local) {
            out.pop();
        }
        out.push_str("[address]");
        i += 1;
        while i < chars.len() && (domain(chars[i]) || chars[i] == '@') {
            i += 1;
        }
        // A sentence's full stop after an address stays.
        if out.ends_with("[address]") && i > 0 && chars[i - 1] == '.' {
            out.push('.');
        }
    }
    out
}

/// The home folder as `~`: a path under it names the account.
fn home_out(text: &str) -> String {
    let mut out = text.to_string();
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(home) = std::env::var(var) {
            let home = home.trim_end_matches(['/', '\\']);
            // "/" or "C:" alone would take every path apart.
            if home.len() > 3 {
                out = replace_ci(&out, home, "~");
            }
        }
    }
    out
}

/// The last word: whatever else happened, no `@` leaves.
fn no_at(block: &str) -> String {
    block.replace('@', " at ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_becomes_the_mailbox_or_goes() {
        let s = clean(
            "Could not sign in to Me@Example.com; ask postmaster@example.com.",
            "me@example.com",
            "Mailbox 1",
            &[],
        );
        assert_eq!(s, "Could not sign in to Mailbox 1; ask [address].");
        assert!(!addresses_out("a @ b and x@y").contains('@'));
    }

    #[test]
    fn a_time_reads_as_a_day_and_a_minute() {
        // 29 September 2026 14:03:00 UTC.
        let t = 1_790_690_580;
        assert_eq!(when(t, t + 125), "29 September 2026 14:03 UTC, 2 min ago");
    }

    #[test]
    fn the_port_says_how_smtp_was_spoken_to() {
        assert_eq!(smtp_mode("smtp.example.com:465"), "TLS from the start");
        assert_eq!(smtp_mode("smtp.example.com:587"), "STARTTLS");
    }
}
