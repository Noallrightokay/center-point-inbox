//! Settings → Copy diagnostics: one plain-text block a tester pastes into a
//! bug report (H8).
//!
//! The repository the report goes to is public, so the block says only what
//! a fix needs and nothing a stranger could use: the build, the system, the
//! licence's plan and day, and for each mailbox its servers, how it signs
//! in, whether it is parked, its last error, the latest failure of each other
//! kind of operation (J1), how its special folders were found and what RATA
//! has seen of it this session. **Never** an address (a mailbox is `Mailbox 1`), a password, a
//! token, the licence key, message text, a path that could carry the
//! account's name, or a name a person chose: a folder of the customer's, an
//! attachment's file name, a subject (SEC-8). An error is kept without the
//! names its operation touched ([`without_names`], when it is kept), every
//! sentence that came from a server or an error goes through [`clean`]
//! first, and the whole block through [`no_at`] last, so not even an `@`
//! survives.
//!
//! Everything here comes from what the app already holds. Nothing signs in,
//! dials or asks a server anything to fill it.

use std::collections::BTreeMap;

use rata_mail::credential::said_within;
use rata_mail::{FoundBy, Placed};

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
    /// What was being done: `refresh`, `send`; for an [`Op`], what of it
    /// (`archive`, `save attachment`…), or nothing when the op says it all.
    pub doing: &'static str,
    /// The problem's kind (`net`, `auth`, `oauth`…).
    pub kind: String,
    pub said: String,
}

/// The operations besides a refresh and a send whose latest failure is kept
/// for each mailbox (J1), in the order the block lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    /// Read, unread, star, unstar, delete, archive, move (`change`).
    Action,
    /// Load older mail, the gaps a refresh left, and re-reading by number.
    Older,
    /// Fetching one message whole: opening it in full, saving or converting
    /// an attachment, its pictures.
    Open,
    /// Searching on the server.
    Search,
    /// Saving the composer's draft to Drafts.
    Draft,
    /// Listing the customer's own folders, or reading one.
    Folders,
    /// The connection waiting for new mail (IMAP IDLE).
    Watch,
}

impl Op {
    fn label(self) -> &'static str {
        match self {
            Op::Action => "action",
            Op::Older => "older mail",
            Op::Open => "message fetch",
            Op::Search => "server search",
            Op::Draft => "draft save",
            Op::Folders => "folders",
            Op::Watch => "new-mail watch",
        }
    }
}

/// The latest failure of one [`Op`], and when it first worked after it.
#[derive(Debug, Clone)]
pub struct Failed {
    pub trouble: Trouble,
    pub worked_since: Option<u64>,
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
    /// How the latest refresh that listed the folders found each special
    /// one.
    pub places: Option<Placed>,
    /// `host:port` of the submission server the last message went through.
    pub smtp_used: Option<String>,
    /// The latest failure of each other kind of operation.
    pub failed: BTreeMap<Op, Failed>,
    /// The server said it has no IDLE, so new mail waits for the timer.
    pub no_idle: bool,
}

impl Noted {
    /// `op` failed: this replaces whatever failure of it was kept.
    pub fn failed(&mut self, op: Op, trouble: Trouble) {
        self.failed.insert(
            op,
            Failed {
                trouble,
                worked_since: None,
            },
        );
    }

    /// `op` worked. A failure kept for it stays, since what went wrong is
    /// still worth knowing, but now says when it first worked again. For an
    /// action that takes the same action: a star that works says nothing
    /// about an archive that did not.
    pub fn worked(&mut self, op: Op, doing: &str, at: u64) {
        if let Some(f) = self.failed.get_mut(&op)
            && f.worked_since.is_none()
            && (op != Op::Action || f.trouble.doing == doing)
        {
            f.worked_since = Some(at);
        }
    }

    /// Whether anything kept came from an error, so may hold a secret to
    /// take out.
    pub fn has_errors(&self) -> bool {
        self.last_error.is_some() || !self.failed.is_empty()
    }
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
        if m.noted.failed.is_empty() {
            out.push("  Other failures: none since RATA started".into());
        }
        for (op, failed) in &m.noted.failed {
            let t = &failed.trouble;
            out.push(format!(
                "  Last failed {}{}: {} ({}): {}{}",
                op.label(),
                if t.doing.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", t.doing)
                },
                when(t.at, f.now),
                tidy(&t.kind),
                tidy(&t.said),
                failed
                    .worked_since
                    .map(|w| format!(" — worked again {}", when(w, f.now)))
                    .unwrap_or_default()
            ));
        }
        out.push(format!("  Special folders seen: {}", folders(&m.noted)));
        out.push(format!(
            "  Special folders found: {}",
            found(m.noted.places.as_ref())
        ));
        out.push(format!(
            "  Live connection (new mail as it arrives): {}",
            if m.live {
                "up"
            } else if m.noted.no_idle {
                "down, the server does not offer IDLE (new mail comes with the five-minute refresh)"
            } else {
                "down"
            }
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

/// How the latest refresh that listed the folders found each special one:
/// by the attribute the server declares, by one of RATA's own fixed names
/// (that list's spelling, never the server's, which could be a label the
/// customer made), or as Gmail's All Mail.
fn found(places: Option<&Placed>) -> String {
    let Some(p) = places else {
        return "not listed yet since RATA started".into();
    };
    let how = |by: Option<FoundBy>| match by {
        None => "none on this server".to_string(),
        Some(FoundBy::Attribute) => "by attribute".into(),
        Some(FoundBy::Name(name)) => format!("by name (\"{name}\")"),
        Some(FoundBy::AllMail) => "Gmail's All Mail (by its \\All attribute)".into(),
    };
    [
        ("Sent", p.sent),
        ("Spam", p.junk),
        ("Drafts", p.drafts),
        ("Archive", p.archive),
    ]
    .into_iter()
    .map(|(folder, by)| format!("{folder}: {}", how(by)))
    .collect::<Vec<_>>()
    .join("; ")
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

/// A name a person chose, which a public bug report must never carry
/// (SEC-8): one of the customer's own folders (a label such as "Therapy
/// notes"), an attachment's file name (chosen by whoever sent it), what a
/// message is about. Each operation knows which of these it touched; the
/// error it keeps for the block is cleaned of every one of them as it is
/// kept ([`without_names`]), so no name is held even in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    /// A folder, as the server's LIST gave it (modified UTF-7).
    Folder(String),
    /// A file's name, as the message or the composer gave it.
    File(String),
    /// A message's subject.
    Subject(String),
}

/// A name standing where it was: what a reader of the block sees instead.
const FOLDER: &str = "[folder]";
const FILE: &str = "[file]";
const SUBJECT: &str = "[subject]";
/// A quoted span in an error that touched names of more than one kind, or
/// only a subject.
const NAMED: &str = "[name]";

/// A spelling this short is taken out only where it stands as a word of its
/// own: a folder called "To" takes out the word "to", never the "To" of
/// "Tomorrow".
const WORD_ONLY: usize = 4;

/// The start of a spelling at least this long is taken out too, where it
/// stands alone: a server's line cut short mid-name.
const CUT_FROM: usize = 6;

impl Name {
    fn label(&self) -> &'static str {
        match self {
            Name::Folder(_) => FOLDER,
            Name::File(_) => FILE,
            Name::Subject(_) => SUBJECT,
        }
    }

    /// Every spelling the name can come back in: from RATA's own sentence,
    /// or from a server echoing it in its own words.
    fn spellings(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        match self {
            Name::Folder(raw) => {
                // As LIST gave it, as the engine writes it (decoded), and
                // the other way: a server that took the name in UTF-8 can
                // still answer in modified UTF-7.
                let decoded = rata_mail::imap::utf7_imap(raw);
                let wholes = [
                    raw.clone(),
                    decoded.clone(),
                    utf7_encode(raw),
                    utf7_encode(&decoded),
                ];
                for whole in &wholes {
                    out.push(whole.clone());
                    // Each level alone, and the levels as the folder list
                    // shows them ("Personal / Therapy notes").
                    let parts: Vec<&str> = whole
                        .split(['/', '.', '\\'])
                        .map(str::trim)
                        .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("inbox"))
                        .collect();
                    out.extend(parts.iter().map(|p| p.to_string()));
                    out.extend(parts.iter().map(|p| rata_mail::imap::utf7_imap(p)));
                    out.push(parts.join(" / "));
                }
            }
            Name::File(raw) => {
                for whole in [raw.clone(), rata_mail::safe_file_name(raw)] {
                    // "name (2).pdf" is how a second copy is saved.
                    if let Some((stem, _)) = whole.rsplit_once('.') {
                        out.push(stem.to_string());
                    }
                    out.push(whole);
                }
            }
            Name::Subject(s) => out.push(s.clone()),
        }
        // As an IMAP quoted string carries it.
        let quoted: Vec<String> = out
            .iter()
            .map(|s| s.replace('\\', "\\\\").replace('"', "\\\""))
            .collect();
        out.extend(quoted);
        let mut out: Vec<String> = out
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("inbox"))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// `text` with every name in `names` taken out, in every spelling it could
/// come back in, ignoring case (Unicode's, not only ASCII's): each becomes
/// `[folder]`, `[file]` or `[subject]`. Then, since a server can echo a name
/// in a form none of those spellings foresaw, everything it put in quotes
/// goes too. Nothing changes when there are no names.
pub fn without_names(text: &str, names: &[Name]) -> String {
    if names.is_empty() {
        return text.to_string();
    }
    let mut spelled: Vec<(Vec<char>, &'static str)> = names
        .iter()
        .flat_map(|n| {
            n.spellings()
                .into_iter()
                .map(move |s| (fold(&s), n.label()))
        })
        .filter(|(s, _)| !s.is_empty())
        .collect();
    // The longest first, so a whole name goes before any level of it.
    spelled.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));

    let chars: Vec<char> = text.chars().collect();
    // Each character folded, and which character each folded one came from.
    let mut folded: Vec<char> = Vec::with_capacity(chars.len());
    let mut from: Vec<usize> = Vec::with_capacity(chars.len());
    for (i, c) in chars.iter().enumerate() {
        for l in c.to_lowercase() {
            folded.push(l);
            from.push(i);
        }
    }
    let word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    let mut out = String::with_capacity(text.len());
    let mut p = 0;
    while p < folded.len() {
        let at = from[p];
        // A match starts where a character of the text starts.
        let starts = p == 0 || from[p - 1] != at;
        let after_word = at > 0 && word(at - 1);
        let mut taken: Option<(usize, &str)> = None;
        if starts {
            for (needle, label) in &spelled {
                let n = needle.len();
                let same = folded[p..]
                    .iter()
                    .zip(needle)
                    .take_while(|(a, b)| a == b)
                    .count();
                if same == 0 {
                    continue;
                }
                // Where the match ends: at the end of a character, and
                // whether a letter or digit follows it.
                let end = p + same;
                let whole_char = end == folded.len() || from[end] != from[end - 1];
                let word_after = word(from[end - 1] + 1);
                let len = if same == n {
                    // The whole spelling; a short one only as a word of
                    // its own.
                    if !whole_char || (n < WORD_ONLY && (after_word || word_after)) {
                        continue;
                    }
                    n
                } else if n >= CUT_FROM
                    && same >= CUT_FROM
                    && whole_char
                    && !after_word
                    && !word_after
                {
                    // The start of one, where a line was cut: a whole word
                    // or more of it, standing alone.
                    same
                } else {
                    continue;
                };
                taken = Some((len, label));
                break;
            }
        }
        match taken {
            Some((len, label)) => {
                out.push_str(label);
                // On to the next character of the text after the match.
                let last = from[p + len - 1];
                while p < folded.len() && from[p] <= last {
                    p += 1;
                }
            }
            None => {
                if starts {
                    out.push(chars[at]);
                }
                p += 1;
            }
        }
    }
    unquoted(&out, quote_label(names))
}

/// What a quoted span becomes: the kind of name the operation touched, or
/// `[name]` when it touched more than one kind.
fn quote_label(names: &[Name]) -> &'static str {
    let first = names.first().map(Name::label).unwrap_or(NAMED);
    if first == SUBJECT {
        return NAMED;
    }
    if names.iter().all(|n| n.label() == first) {
        first
    } else {
        NAMED
    }
}

/// `text` with whatever stands in quotes, "straight", “curly”, «angled» or
/// 'single', replaced by `label`, the quotes kept. A single quote opens only
/// where no letter stands before it and closes only where none follows, so
/// "doesn't" is not one. A straight quote escaped with a backslash, as IMAP
/// writes one inside a quoted string, does not close it. A span holding
/// only a name already taken out stays as it is.
fn unquoted(text: &str, label: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    let close_of = |i: usize| -> Option<char> {
        match chars[i] {
            '"' => Some('"'),
            '\u{201c}' => Some('\u{201d}'),
            '\u{ab}' => Some('\u{bb}'),
            '\'' if (i == 0 || !word(i - 1))
                && chars.get(i + 1).is_some_and(|c| !c.is_whitespace()) =>
            {
                Some('\'')
            }
            _ => None,
        }
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let Some(close) = close_of(i) else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let mut j = i + 1;
        let end = loop {
            match chars.get(j) {
                None => break None,
                Some('\\') if close == '"' => j += 2,
                Some(&c) if c == close && (close != '\'' || !word(j + 1)) => break Some(j),
                Some(_) => j += 1,
            }
        };
        let Some(end) = end else {
            out.push(chars[i]);
            i += 1;
            continue;
        };
        let inside: String = chars[i + 1..end].iter().collect();
        out.push(chars[i]);
        if [FOLDER, FILE, SUBJECT, NAMED].contains(&inside.as_str()) {
            out.push_str(&inside);
        } else {
            out.push_str(label);
        }
        out.push(close);
        i = end + 1;
    }
    out
}

/// `s` folded for comparing without case.
fn fold(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// IMAP's modified UTF-7 (RFC 3501 §5.1.3), the other way from
/// `rata_mail::imap::utf7_imap`: how a server may spell "Entwürfe" back.
fn utf7_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run: Vec<u16> = Vec::new();
    let flush = |run: &mut Vec<u16>, out: &mut String| {
        if run.is_empty() {
            return;
        }
        let bytes: Vec<u8> = run.iter().flat_map(|u| u.to_be_bytes()).collect();
        let b64 = rata_mail::words::base64_encode(&bytes);
        out.push('&');
        out.push_str(&b64.trim_end_matches('=').replace('/', ","));
        out.push('-');
        run.clear();
    };
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut run, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut units = [0u16; 2];
            run.extend_from_slice(c.encode_utf16(&mut units));
        }
    }
    flush(&mut run, &mut out);
    out
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

    /// J1: how each special folder was found, in RATA's own words.
    #[test]
    fn a_special_folder_says_how_it_was_found() {
        assert_eq!(found(None), "not listed yet since RATA started");
        let p = Placed {
            sent: Some(FoundBy::Name("Sent Items")),
            junk: Some(FoundBy::Attribute),
            drafts: None,
            archive: Some(FoundBy::AllMail),
        };
        assert_eq!(
            found(Some(&p)),
            "Sent: by name (\"Sent Items\"); Spam: by attribute; Drafts: none on this server; Archive: Gmail's All Mail (by its \\All attribute)"
        );
    }

    /// J1: a failure is kept until the same operation fails again; working
    /// again is said beside it, and for an action only by the same action.
    #[test]
    fn a_failure_stays_and_says_when_it_worked_again() {
        let trouble = |doing| Trouble {
            at: 10,
            doing,
            kind: "no-place".into(),
            said: "No Archive.".into(),
        };
        let mut n = Noted::default();
        assert!(!n.has_errors());
        n.failed(Op::Action, trouble("archive"));
        n.failed(Op::Open, trouble("save attachment"));
        assert!(n.has_errors());
        n.worked(Op::Action, "star", 20);
        assert_eq!(n.failed[&Op::Action].worked_since, None);
        n.worked(Op::Action, "archive", 30);
        n.worked(Op::Action, "archive", 40);
        assert_eq!(n.failed[&Op::Action].worked_since, Some(30));
        // Any fetch of a whole message working says the fetching works.
        n.worked(Op::Open, "open in full", 50);
        assert_eq!(n.failed[&Op::Open].worked_since, Some(50));
        // Another operation's success touches nothing else.
        n.worked(Op::Search, "", 60);
        assert!(!n.failed.contains_key(&Op::Search));
        // Failing again starts over.
        n.failed(Op::Action, trouble("archive"));
        assert_eq!(n.failed[&Op::Action].worked_since, None);
        assert_eq!(n.failed.len(), 2);
    }

    /// SEC-8: a name goes in every spelling it comes back in, and a short
    /// one only where it stands alone.
    #[test]
    fn a_name_goes_in_every_spelling() {
        let folder = |raw: &str| vec![Name::Folder(raw.into())];
        let gone = |text: &str, names: &[Name]| without_names(text, names);
        assert_eq!(utf7_encode("Entwürfe"), "Entw&APw-rfe");
        assert_eq!(utf7_encode("日本語"), "&ZeVnLIqe-");
        assert_eq!(utf7_encode("Tom & Jerry"), "Tom &- Jerry");

        let entw = folder("INBOX.Entw&APw-rfe.Akte 7");
        for said in [
            "NO Mailbox \"INBOX.Entw&APw-rfe.Akte 7\" doesn't exist",
            "NO Mailbox \"INBOX.Entwürfe.Akte 7\" doesn't exist",
            "NO Mailbox \"inbox.ENTWÜRFE.akte 7\" doesn't exist",
            "NO Mailbox \"Entwürfe / Akte 7\" doesn't exist",
        ] {
            assert_eq!(
                gone(said, &entw),
                "NO Mailbox \"[folder]\" doesn't exist",
                "{said}"
            );
        }
        // A level alone, unquoted, keeps the words around it.
        assert_eq!(
            gone("NO Akte 7 is busy, try later", &entw),
            "NO [folder] is busy, try later"
        );
        // Quoted by the server in a form no spelling foresaw.
        assert_eq!(
            gone("NO [NONEXISTENT] 'Entwu\u{308}rfe' gone", &entw),
            "NO [NONEXISTENT] '[folder]' gone"
        );
        // Cut short: a start of it standing alone, never a longer word.
        let therapy = folder("Therapy notes");
        assert_eq!(
            gone("NO cannot open Therapy no", &therapy),
            "NO cannot open [folder]"
        );
        assert_eq!(gone("NO Therapist", &therapy), "NO Therapist");
        // A short name only as a word of its own.
        let to = folder("To");
        assert_eq!(
            gone("Tomorrow: to the folder To, then to", &to),
            "Tomorrow: [folder] the folder [folder], then [folder]"
        );
        // A file as sent, as saved, and as a second copy is saved.
        let file = vec![Name::File("Smith v. Smith\u{202e}fdp.exe".into())];
        let safe = rata_mail::safe_file_name("Smith v. Smith\u{202e}fdp.exe");
        assert!(!gone(&format!("{safe} could not be saved"), &file).contains("Smith"));
        assert!(!gone("Smith v. Smith\u{202e}fdp (2).exe", &file).contains("Smith"));
        // Nothing to take out, nothing changed; quotes too.
        assert_eq!(gone("NO \"x\" 'y'", &[]), "NO \"x\" 'y'");
        // Kinds mixed: a quoted span says only that it was a name.
        let mixed = vec![Name::File("a.pdf".into()), Name::Subject("Hi".into())];
        assert_eq!(gone("552 \"b.pdf\" no", &mixed), "552 \"[name]\" no");
        assert_eq!(gone("552 a.pdf: Hi", &mixed), "552 [file]: [subject]");
    }
}
