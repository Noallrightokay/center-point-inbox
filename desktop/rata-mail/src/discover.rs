//! Finding the mail server for an address.
//!
//! People know their email address; almost nobody knows their IMAP hostname.
//! Known domains resolve from a table, and everything else is settled by asking
//! the domain's own DNS — which is the case that matters, because most work
//! addresses are on a domain that *points at* a mail provider rather than
//! running one. `anthropic.com` has no `imap.anthropic.com`; its MX says
//! Google, and that is the answer.

use crate::key::domain_of;

/// What a domain resolves to, and where that provider hides its app passwords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailHost {
    pub host: &'static str,
    pub label: &'static str,
    pub help: &'static str,
}

/// How a candidate was arrived at. Worth carrying so the app can say "that
/// domain's mail is at Google Workspace" rather than showing a hostname the
/// customer has never seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Source {
    Table,
    Srv,
    Mx,
    Guess,
    Override,
    /// The domain's SPF record names the provider that sends its mail
    /// (L1), found when the MX is a filter or names nobody RATA knows.
    Spf,
    /// `autodiscover.<domain>` is a CNAME to Microsoft 365's own (L1).
    Autodiscover,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub host: String,
    pub port: u16,
    pub label: String,
    pub help: Option<String>,
    pub source: Source,
}

pub const IMAP_PORT: u16 = 993;

const GOOGLE_HELP: &str = "myaccount.google.com/apppasswords — needs 2-Step Verification on";
/// Apple moved its account pages from appleid.apple.com to
/// account.apple.com; BETA.md and the help page name the new one too.
const APPLE_HELP: &str = "account.apple.com → Sign-In and Security → App-Specific Passwords";
/// Microsoft's IMAP host, for Outlook.com, Hotmail, Live, MSN and Microsoft
/// 365 alike. Discovery finds it like any other; what differs is how it is
/// signed in to (see [`is_microsoft`]).
pub const MS_IMAP: &str = "outlook.office365.com";

/// What a Microsoft mailbox signs in with, in the place other providers say
/// where their app passwords are.
///
/// Microsoft turned off password sign-in to IMAP: Outlook.com in September
/// 2024, Exchange Online before that. Only OAuth 2.0 gets in now, app
/// passwords included. So a password is never sent to Microsoft (`verify`
/// answers `Verify::Microsoft` before dialling) and the app signs in through
/// Microsoft's own page instead.
pub const MS_SIGN_IN: &str = "Signs in through Microsoft's own page, in your browser.";

/// What to say in a build that has no Microsoft sign-in (no client id was
/// compiled in): the mailbox cannot be added, and why. C0's words.
pub const MS_HELP: &str = "Outlook.com, Hotmail and Live mailboxes cannot be added to RATA yet. Microsoft only lets other apps into them through its own sign-in page (OAuth), and no longer accepts passwords or app passwords for IMAP. RATA does not have that sign-in yet.";
pub const MS365_HELP: &str = "That address's mail is at Microsoft 365, which RATA cannot open yet. Microsoft only lets other apps into Microsoft 365 mailboxes through its own sign-in page (OAuth), and no longer accepts passwords for IMAP. RATA does not have that sign-in yet.";

/// Whether a server is Microsoft's, whose IMAP no longer takes a password.
/// Suffix matching, like [`mx_rule`]: this host, or anything under it.
pub fn is_microsoft(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    [
        "outlook.com",
        "office365.com",
        "office.com",
        "hotmail.com",
        "live.com",
    ]
    .iter()
    .any(|s| h == *s || h.ends_with(&format!(".{s}")))
}

/// Microsoft's own consumer domains: Outlook.com, Hotmail, Live and MSN,
/// regional ones included. The same list as `MS_DOMAINS` in `app.html`.
pub fn is_microsoft_consumer(domain: &str) -> bool {
    matches!(
        domain.trim().to_ascii_lowercase().as_str(),
        "outlook.com"
            | "hotmail.com"
            | "live.com"
            | "msn.com"
            | "passport.com"
            | "windowslive.com"
            | "hotmail.co.uk"
            | "hotmail.fr"
            | "hotmail.de"
            | "hotmail.it"
            | "hotmail.es"
            | "live.co.uk"
            | "live.fr"
            | "live.de"
            | "live.it"
            | "live.nl"
            | "live.ca"
            | "live.com.au"
            | "outlook.fr"
            | "outlook.de"
            | "outlook.es"
            | "outlook.it"
            | "outlook.jp"
    )
}
const YAHOO_HELP: &str = "login.yahoo.com → Account security → Generate app password";

/// Consumer domains, answered without a lookup.
pub fn table(domain: &str) -> Option<MailHost> {
    let h = |host, label, help| Some(MailHost { host, label, help });
    if is_microsoft_consumer(domain) {
        return h(MS_IMAP, "Outlook", MS_SIGN_IN);
    }
    match domain {
        "gmail.com" | "googlemail.com" => h("imap.gmail.com", "Gmail", GOOGLE_HELP),
        "icloud.com" | "me.com" | "mac.com" => h("imap.mail.me.com", "iCloud Mail", APPLE_HELP),
        "yahoo.com" | "ymail.com" => h("imap.mail.yahoo.com", "Yahoo Mail", YAHOO_HELP),
        "aol.com" => h(
            "imap.aol.com",
            "AOL Mail",
            "login.aol.com → Account security → Generate app password",
        ),
        "zoho.com" => h(
            "imap.zoho.com",
            "Zoho Mail",
            "accounts.zoho.com → Security → App passwords",
        ),
        "fastmail.com" | "fastmail.fm" => h(
            "imap.fastmail.com",
            "Fastmail",
            "fastmail.com → Settings → Privacy & Security → App passwords",
        ),
        "gmx.com" => h("imap.gmx.com", "GMX", "Enable IMAP in GMX settings"),
        "gmx.net" => h("imap.gmx.net", "GMX", "Enable IMAP in GMX settings"),
        "mail.com" => h(
            "imap.mail.com",
            "Mail.com",
            "Enable IMAP in your Mail.com settings",
        ),
        "web.de" => h(
            "imap.web.de",
            "WEB.DE",
            "Enable IMAP in your WEB.DE settings",
        ),
        "yandex.com" => h(
            "imap.yandex.com",
            "Yandex Mail",
            "id.yandex.com → Security → App passwords",
        ),
        "comcast.net" => h(
            "imap.comcast.net",
            "Comcast",
            "Use your Xfinity password, with third-party access enabled",
        ),
        "att.net" => h(
            "imap.mail.att.net",
            "AT&T Mail",
            "currently.att.net → Profile → Sign-in info → Manage secure mail key",
        ),
        "verizon.net" => h(
            "imap.aol.com",
            "Verizon Mail",
            "Verizon mail is served by AOL — generate an AOL app password",
        ),
        "rogers.com" => h(
            "imap.broadband.rogers.com",
            "Rogers",
            "Use your Rogers email password",
        ),
        "shaw.ca" => h("imap.shaw.ca", "Shaw", "Use your Shaw email password"),
        "bell.net" | "sympatico.ca" => h("imap.bell.net", "Bell", "Use your Bell email password"),
        _ => None,
    }
}

/// Providers that encrypt on the device and run no IMAP server anyone can
/// reach. Saying so beats three timeouts and a box asking for a server name.
pub fn no_imap(domain: &str) -> Option<&'static str> {
    match domain {
        "proton.me" | "protonmail.com" | "pm.me" => Some("Proton Mail"),
        "tuta.com" | "tutanota.com" => Some("Tuta"),
        _ => None,
    }
}

/// What an MX record tells us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MxRule {
    /// Whoever serves this domain, and where their app passwords live.
    Serves(MailHost),
    /// There is no mailbox to reach. The string is the reason, for the customer.
    Refuse(&'static str),
    /// A spam filter in front of the real mailbox. It says nothing about where
    /// that mailbox is, so connecting to it would fail confusingly.
    Filtered,
}

/// Map an MX target to what it means. Suffix matching rather than regex: every
/// rule here is "this host, or anything under it".
pub fn mx_rule(exchange: &str) -> Option<MxRule> {
    let mx = exchange.trim_end_matches('.').to_ascii_lowercase();
    let under = |suffix: &str| mx == suffix || mx.ends_with(&format!(".{suffix}"));
    let serves = |host, label, help| Some(MxRule::Serves(MailHost { host, label, help }));

    if under("google.com") || under("googlemail.com") {
        return serves(
            "imap.gmail.com",
            "Google Workspace",
            "myaccount.google.com/apppasswords, signed in with your work address — needs 2-Step Verification on",
        );
    }
    if under("outlook.com") || under("office365.com") {
        return serves(MS_IMAP, "Microsoft 365", MS_SIGN_IN);
    }
    // Zoho keeps each account in one region's data centre, and only that
    // region's servers know it: an EU or Indian domain signs in at
    // imap.zoho.eu or imap.zoho.in, never imap.zoho.com. The MX says which.
    for (region, imap, help) in ZOHO_REGIONS {
        if under(region) {
            return serves(imap, "Zoho Mail", help);
        }
    }
    if under("messagingengine.com") {
        return serves(
            "imap.fastmail.com",
            "Fastmail",
            "fastmail.com → Settings → Privacy & Security → App passwords",
        );
    }
    if under("icloud.com") {
        return serves("imap.mail.me.com", "iCloud Mail", APPLE_HELP);
    }
    if under("yahoodns.net") {
        return serves("imap.mail.yahoo.com", "Yahoo Mail", YAHOO_HELP);
    }
    if under("secureserver.net") {
        return serves(
            "imap.secureserver.net",
            "GoDaddy",
            "Use your mailbox password, or an app password if your plan issues them",
        );
    }
    if under("registrar-servers.com") {
        return serves(
            "mail.privateemail.com",
            "Namecheap Private Email",
            "Use your Private Email mailbox password",
        );
    }
    if under("titan.email") {
        return serves(
            "imap.titan.email",
            "Titan",
            "Use your Titan mailbox password",
        );
    }
    if under("ionos.com") || under("1and1.com") || under("kundenserver.de") || under("perfora.net")
    {
        return serves("imap.ionos.com", "IONOS", "Use your IONOS mailbox password");
    }
    if under("emailsrvr.com") {
        return serves(
            "secure.emailsrvr.com",
            "Rackspace Email",
            "Use your Rackspace mailbox password",
        );
    }
    if under("hostinger.com") {
        return serves(
            "imap.hostinger.com",
            "Hostinger Email",
            "hPanel → Emails → the mailbox password",
        );
    }
    if under("migadu.com") {
        return serves(
            "imap.migadu.com",
            "Migadu",
            "admin.migadu.com → the mailbox → its password",
        );
    }
    if under("mailbox.org") {
        return serves(
            "imap.mailbox.org",
            "mailbox.org",
            "Use your mailbox.org password",
        );
    }

    if under("protonmail.ch") || under("proton.me") || under("protonmail.com") {
        return Some(MxRule::Refuse(
            "That domain's mail is hosted by Proton, which encrypts it on the device and offers no IMAP server RATA can reach. Their bridge only runs on your own computer.",
        ));
    }
    if under("tutanota.de") || under("tuta.com") {
        return Some(MxRule::Refuse(
            "That domain's mail is hosted by Tuta, which encrypts it on the device and offers no IMAP server RATA can reach.",
        ));
    }
    if under("improvmx.com")
        || under("forwardemail.net")
        || under("mailgun.org")
        || under("sendgrid.net")
    {
        return Some(MxRule::Refuse(
            "That domain forwards its mail somewhere else rather than keeping a mailbox of its own. Link the address the mail is forwarded to — that is where it actually lands.",
        ));
    }

    if under("pphosted.com")
        || under("mimecast.com")
        || under("barracudanetworks.com")
        || under("messagelabs.com")
        || under("iphmx.com")
        || under("trendmicro.com")
    {
        return Some(MxRule::Filtered);
    }
    None
}

/// Zoho's data centres: the MX domain, the IMAP host there, and where that
/// region's app passwords live. Matched by whole labels, so `zoho.com` is
/// not the end of `mx.zoho.com.au`.
const ZOHO_REGIONS: [(&str, &str, &str); 8] = [
    (
        "zoho.eu",
        "imap.zoho.eu",
        "accounts.zoho.eu → Security → App passwords",
    ),
    (
        "zoho.in",
        "imap.zoho.in",
        "accounts.zoho.in → Security → App passwords",
    ),
    (
        "zoho.com.au",
        "imap.zoho.com.au",
        "accounts.zoho.com.au → Security → App passwords",
    ),
    (
        "zoho.jp",
        "imap.zoho.jp",
        "accounts.zoho.jp → Security → App passwords",
    ),
    (
        "zohocloud.ca",
        "imap.zohocloud.ca",
        "accounts.zohocloud.ca → Security → App passwords",
    ),
    (
        "zoho.sa",
        "imap.zoho.sa",
        "accounts.zoho.sa → Security → App passwords",
    ),
    (
        "zoho.com.cn",
        "imap.zoho.com.cn",
        "accounts.zoho.com.cn → Security → App passwords",
    ),
    (
        "zoho.com",
        "imap.zoho.com",
        "accounts.zoho.com → Security → App passwords",
    ),
];

/// The host Zoho documents for organisation accounts (a company's own
/// domain at Zoho) in the region whose personal host is `imap_host`:
/// `imappro.zoho.eu` for `imap.zoho.eu`. `None` for any host that is not one
/// of [`ZOHO_REGIONS`]' own.
///
/// Discovery reads the region from the MX and signs in at
/// `imap.zoho.<region>`; when that refuses the password of a mailbox found
/// that way, `verify` tries this host once before saying the password is
/// wrong.
pub fn zoho_pro(imap_host: &str) -> Option<String> {
    let h = imap_host.trim_end_matches('.').to_ascii_lowercase();
    ZOHO_REGIONS
        .iter()
        .find(|(_, imap, _)| *imap == h)
        .map(|(region, _, _)| format!("imappro.{region}"))
}

/// The submission host for a Zoho organisation account signed in to at
/// `imappro.<region>`: `smtppro.<region>`, as Zoho documents. `None` for
/// anything else.
fn zoho_pro_smtp(imap_host: &str) -> Option<String> {
    let h = imap_host.trim_end_matches('.').to_ascii_lowercase();
    ZOHO_REGIONS
        .iter()
        .find(|(region, _, _)| h == format!("imappro.{region}"))
        .map(|(region, _, _)| format!("smtppro.{region}"))
}

// ------------------------------------------------- evidence behind a filter

/// SPF `include:` targets that only a mailbox provider's own customers
/// publish, each with an MX name that [`mx_rule`] already reads as that
/// provider, so the host, label and help are the MX's own. Exact names
/// only: an include is evidence of one provider, never of a name that
/// merely ends the same way. Each is the value the provider's own
/// documentation tells a domain to publish:
///
/// - `spf.protection.outlook.com`: Microsoft, "Set up SPF to identify
///   valid email sources for your Microsoft 365 domain" (learn.microsoft.com).
/// - `_spf.google.com`: Google Workspace Admin Help, "Set up SPF"
///   (support.google.com/a/answer/10685031).
/// - `spf.messagingengine.com`: Fastmail, "Manual DNS configuration"
///   (fastmail.help).
/// - `_spf.mail.hostinger.com`: Hostinger Email, "How to set up SPF"
///   (support.hostinger.com).
/// - `icloud.com`: Apple, "Use iCloud Mail with a custom email domain",
///   `v=spf1 include:icloud.com ~all` (support.apple.com).
/// - `spf.titan.email`: Titan, "What are SPF records"
///   (support.titan.email).
/// - `emailsrvr.com`: Rackspace, "Create an SPF policy"
///   (docs.rackspace.com).
/// - `spf.migadu.com`: Migadu's DNS setup (migadu.com/guides).
/// - `spf.privateemail.com`: Namecheap, "Private Email records for domains
///   with third-party DNS" (namecheap.com/support).
/// - `one.zoho.com`: Zoho Mail admin help, "SPF configuration", for an
///   organisation using several Zoho services (zoho.com/mail/help).
///
/// Zoho's regional values are in [`spf_provider`]. Not here, on purpose:
/// `zcsend.net` is Zoho Campaigns, a mailing-list sender, which says
/// nothing about where a domain's mailboxes are; and the bulk senders
/// (SendGrid, Mailgun, Mailchimp, Amazon SES) for the same reason.
const SPF_INCLUDES: [(&str, &str); 10] = [
    ("spf.protection.outlook.com", "outlook.com"),
    ("_spf.google.com", "google.com"),
    ("spf.messagingengine.com", "messagingengine.com"),
    ("_spf.mail.hostinger.com", "hostinger.com"),
    ("icloud.com", "icloud.com"),
    ("spf.titan.email", "titan.email"),
    ("emailsrvr.com", "emailsrvr.com"),
    ("spf.migadu.com", "migadu.com"),
    ("spf.privateemail.com", "registrar-servers.com"),
    ("one.zoho.com", "zoho.com"),
];

/// The mailbox provider an SPF `include:` target names, if it names one.
///
/// Zoho, by region: Zoho Mail's current value is `zohomail.<region>`
/// (`include:zohomail.com`, zoho.com/mail/help/adminconsole/spf-configuration.html;
/// each region's `zohomail.` domain publishes that region's
/// `spf.zohomail.` record), and the value it documented before was the
/// region's own domain (`include:zoho.eu`). Both lead to [`ZOHO_REGIONS`]'
/// host for that region. Canada has no `zohomail.` domain of Zoho's own.
pub fn spf_provider(include: &str) -> Option<MailHost> {
    let inc = include.trim_end_matches('.').to_ascii_lowercase();
    let mx = SPF_INCLUDES
        .iter()
        .find(|(name, _)| *name == inc)
        .map(|(_, mx)| (*mx).to_string())
        .or_else(|| {
            ZOHO_REGIONS.iter().find_map(|(region, _, _)| {
                let current = region
                    .strip_prefix("zoho.")
                    .map(|rest| format!("zohomail.{rest}"));
                (inc == *region || current.as_deref() == Some(inc.as_str()))
                    .then(|| region.to_string())
            })
        })?;
    match mx_rule(&mx) {
        Some(MxRule::Serves(h)) => Some(h),
        _ => None,
    }
}

/// The provider an `autodiscover.<domain>` CNAME names: Microsoft 365
/// when it is `autodiscover.outlook.com`, the record Microsoft tells every
/// Microsoft 365 domain to publish ("Add DNS records to connect your
/// domain", learn.microsoft.com). Nothing else is read from it.
pub fn autodiscover_provider(target: &str) -> Option<MailHost> {
    let t = target.trim_end_matches('.').to_ascii_lowercase();
    if t != "autodiscover.outlook.com" {
        return None;
    }
    match mx_rule(&t) {
        Some(MxRule::Serves(h)) => Some(h),
        _ => None,
    }
}

/// The parts of an SPF record discovery reads (RFC 7208): the domains it
/// includes with a pass, and where it redirects.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Spf {
    /// `include:` targets qualified `+` (or not at all), lowercased, in
    /// the record's order. A `-`, `~` or `?` include says that sender is
    /// *not* to be trusted outright, so it is evidence of nothing.
    pub includes: Vec<String>,
    /// `redirect=`, only when the record has no `all`, since `all` makes
    /// a redirect meaningless (RFC 7208 §6.1).
    pub redirect: Option<String>,
}

/// The domain's one SPF record among its TXT records. None when it has
/// none, or more than one: two `v=spf1` records are an error (RFC 7208
/// §4.5), and an error is evidence of nothing.
pub fn spf_record(txt: &[String]) -> Option<&str> {
    let mut spf = txt.iter().filter(|t| {
        let b = t.as_bytes();
        b.len() >= 6 && b[..6].eq_ignore_ascii_case(b"v=spf1") && (b.len() == 6 || b[6] == b' ')
    });
    let one = spf.next()?;
    if spf.next().is_some() {
        return None;
    }
    Some(one.as_str())
}

/// Read an SPF record strictly. None for anything RFC 7208 calls a
/// permanent error that RATA can see without a lookup: a missing
/// version, an unknown mechanism, a mechanism or modifier with a bad
/// name, an `include` with no domain, or two `redirect`s.
pub fn parse_spf(record: &str) -> Option<Spf> {
    let mut terms = record.split_ascii_whitespace();
    if !terms.next()?.eq_ignore_ascii_case("v=spf1") {
        return None;
    }
    let mut spf = Spf::default();
    let mut all = false;
    for term in terms {
        let (qualifier, rest) = match term.as_bytes()[0] {
            q @ (b'+' | b'-' | b'~' | b'?') => (Some(q), &term[1..]),
            _ => (None, term),
        };
        let end = rest.find([':', '/', '=']).unwrap_or(rest.len());
        let (name, tail) = rest.split_at(end);
        let named = name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if !named {
            return None;
        }
        // A modifier: `name=value`, never qualified.
        if let Some(value) = tail.strip_prefix('=') {
            if qualifier.is_some() {
                return None;
            }
            if name.eq_ignore_ascii_case("redirect") {
                if value.is_empty() || spf.redirect.is_some() {
                    return None;
                }
                spf.redirect = Some(value.trim_end_matches('.').to_ascii_lowercase());
            }
            // `exp=` and unknown modifiers are ignored (RFC 7208 §6).
            continue;
        }
        match name.to_ascii_lowercase().as_str() {
            "all" => {
                if !tail.is_empty() {
                    return None;
                }
                all = true;
            }
            "include" => {
                let domain = tail.strip_prefix(':').filter(|d| !d.is_empty())?;
                // A macro (`%{d}`) expands per message; it names no one.
                if matches!(qualifier, None | Some(b'+')) && !domain.contains('%') {
                    spf.includes
                        .push(domain.trim_end_matches('.').to_ascii_lowercase());
                }
            }
            "a" | "mx" | "ptr" | "ip4" | "ip6" | "exists" => {}
            _ => return None,
        }
    }
    if all {
        spf.redirect = None;
    }
    Some(spf)
}

/// The conventional names, for a domain that really does run its own server.
/// Tried last, so a stale `imap.<domain>` cannot outrank what the MX says.
pub fn conventional(email: &str) -> Vec<String> {
    let d = domain_of(email);
    if d.is_empty() {
        return vec![];
    }
    vec![format!("imap.{d}"), format!("mail.{d}"), d]
}

/// Submission hosts to try for a mailbox, best first.
///
/// One name is not always enough: `outlook.office365.com` serves both a
/// consumer Outlook address and a company's Microsoft 365 mailbox, and the two
/// submit to different hosts.
pub fn smtp_candidates(imap_host: &str, email: &str) -> Vec<String> {
    fn add(out: &mut Vec<String>, h: String) {
        if !h.is_empty() && !out.contains(&h) {
            out.push(h);
        }
    }
    let mut out: Vec<String> = Vec::new();

    if imap_host == MS_IMAP {
        // Outlook.com's own submission host first for its own addresses: a
        // token refused by one Microsoft host stops the attempt (a second
        // host would say the same about a password), so the likelier one
        // goes first.
        if is_microsoft_consumer(&domain_of(email)) {
            add(&mut out, "smtp-mail.outlook.com".into());
        }
        add(&mut out, "smtp.office365.com".into());
        add(&mut out, "smtp-mail.outlook.com".into());
        add(&mut out, imap_host.into());
        return out;
    }
    // A Zoho organisation account submits at its region's `smtppro.` host,
    // the counterpart of the `imappro.` one it reads from.
    if let Some(pro) = zoho_pro_smtp(imap_host) {
        add(&mut out, pro);
        return out;
    }
    let known = match imap_host {
        "imap.gmail.com" => Some("smtp.gmail.com"),
        "imap.mail.me.com" => Some("smtp.mail.me.com"),
        "imap.mail.yahoo.com" => Some("smtp.mail.yahoo.com"),
        "imap.aol.com" => Some("smtp.aol.com"),
        "imap.gmx.com" => Some("mail.gmx.com"),
        "imap.gmx.net" => Some("mail.gmx.net"),
        _ => None,
    };
    if let Some(k) = known {
        add(&mut out, k.into());
    } else if let Some(rest) = imap_host.strip_prefix("imap.") {
        add(&mut out, format!("smtp.{rest}"));
    } else if !imap_host.is_empty() {
        // A host not named imap.<something> is usually the submission host too —
        // mail.privateemail.com and secure.emailsrvr.com both are.
        add(&mut out, imap_host.into());
    }
    if out.is_empty() {
        add(&mut out, format!("smtp.{}", domain_of(email)));
    }
    out
}

/// Implicit TLS first: 465 is encrypted from the first byte where 587 starts in
/// the clear and upgrades.
pub const SMTP_PORTS: [(u16, bool); 2] = [(465, true), (587, false)];

/// Submission hosts that document 587 (STARTTLS) as their port, and answer
/// nothing on 465 — so trying 465 first there costs a timeout per address
/// before the send even starts: Microsoft's two and iCloud's.
const SUBMISSION_587: &[&str] = &[
    "smtp-mail.outlook.com",
    "smtp.office365.com",
    "smtp.mail.me.com",
];

/// The ports to try on one submission host, best first: its documented port
/// where RATA knows it, else [`SMTP_PORTS`].
pub fn smtp_ports(host: &str) -> [(u16, bool); 2] {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    if SUBMISSION_587.contains(&h.as_str()) {
        [(587, false), (465, true)]
    } else {
        SMTP_PORTS
    }
}

/// "The server said no" and "there is no server" need different answers: the
/// first means the password is wrong and trying again only pushes the account
/// closer to being locked.
pub fn is_auth_failure(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    [
        "auth",
        "credential",
        "login",
        "password",
        "authenticationfailed",
        "535",
        "534",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_domains_answer_without_a_lookup() {
        for (addr, host) in [
            ("someone@gmail.com", "imap.gmail.com"),
            ("someone@icloud.com", "imap.mail.me.com"),
            ("someone@outlook.com", "outlook.office365.com"),
            ("someone@yahoo.com", "imap.mail.yahoo.com"),
            ("bookkeeper@sympatico.ca", "imap.bell.net"),
        ] {
            let d = domain_of(addr);
            assert_eq!(table(&d).expect(addr).host, host, "{addr}");
        }
        assert!(table("thesherwood.group").is_none());
        // Apple's app passwords are at account.apple.com now, by the table
        // and by the MX alike.
        assert!(
            table("icloud.com")
                .unwrap()
                .help
                .starts_with("account.apple.com → ")
        );
        match mx_rule("mx01.mail.icloud.com") {
            Some(MxRule::Serves(h)) => assert_eq!(h.help, APPLE_HELP),
            other => panic!("{other:?}"),
        }
        assert!(!APPLE_HELP.contains("appleid."));
    }

    #[test]
    fn a_business_domain_is_settled_by_its_mx() {
        // The case the whole module exists for: companies do not run mail
        // servers, they point a domain at one.
        for (mx, host, label) in [
            ("aspmx.l.google.com", "imap.gmail.com", "Google Workspace"),
            (
                "alt2.aspmx.l.google.com",
                "imap.gmail.com",
                "Google Workspace",
            ),
            (
                "acme-com.mail.protection.outlook.com",
                "outlook.office365.com",
                "Microsoft 365",
            ),
            ("mx.zoho.eu", "imap.zoho.eu", "Zoho Mail"),
            (
                "in1-smtp.messagingengine.com",
                "imap.fastmail.com",
                "Fastmail",
            ),
            ("mx1.titan.email", "imap.titan.email", "Titan"),
            (
                "mx.emailsrvr.com",
                "secure.emailsrvr.com",
                "Rackspace Email",
            ),
        ] {
            match mx_rule(mx) {
                Some(MxRule::Serves(h)) => {
                    assert_eq!(h.host, host, "{mx}");
                    assert_eq!(h.label, label, "{mx}");
                    assert!(
                        !h.help.is_empty(),
                        "{mx} should say where app passwords live, or why there are none"
                    );
                }
                other => panic!("{mx} -> {other:?}"),
            }
        }
    }

    #[test]
    fn a_zoho_domain_signs_in_at_its_own_region() {
        for (mx, host) in [
            ("mx.zoho.com", "imap.zoho.com"),
            ("mx2.zoho.com", "imap.zoho.com"),
            ("mx.zoho.eu", "imap.zoho.eu"),
            ("mx3.zoho.eu", "imap.zoho.eu"),
            ("mx.zoho.in", "imap.zoho.in"),
            ("mx.zoho.com.au", "imap.zoho.com.au"),
            ("mx.zoho.jp", "imap.zoho.jp"),
            ("mx.zohocloud.ca", "imap.zohocloud.ca"),
            ("mx.zoho.sa", "imap.zoho.sa"),
            ("mx.zoho.com.cn", "imap.zoho.com.cn"),
        ] {
            match mx_rule(mx) {
                Some(MxRule::Serves(h)) => {
                    assert_eq!(h.host, host, "{mx}");
                    assert_eq!(h.label, "Zoho Mail", "{mx}");
                    let region = host.trim_start_matches("imap.");
                    assert!(
                        h.help.starts_with(&format!("accounts.{region} ")),
                        "{mx}: {}",
                        h.help
                    );
                }
                other => panic!("{mx} -> {other:?}"),
            }
        }
        // And sending follows: the region's own submission host.
        assert_eq!(smtp_candidates("imap.zoho.eu", "a@b.de"), ["smtp.zoho.eu"]);
        assert_eq!(smtp_candidates("imap.zoho.in", "a@b.in"), ["smtp.zoho.in"]);
        // Not a region: a name that only ends the same way.
        assert_eq!(mx_rule("mx.notzoho.eu"), None);
    }

    #[test]
    fn a_zoho_organisation_host_is_the_same_regions_and_sends_from_its_twin() {
        for (imap, pro, smtp) in [
            ("imap.zoho.com", "imappro.zoho.com", "smtppro.zoho.com"),
            ("imap.zoho.eu", "imappro.zoho.eu", "smtppro.zoho.eu"),
            ("imap.zoho.in", "imappro.zoho.in", "smtppro.zoho.in"),
            (
                "imap.zoho.com.au",
                "imappro.zoho.com.au",
                "smtppro.zoho.com.au",
            ),
            ("imap.zoho.jp", "imappro.zoho.jp", "smtppro.zoho.jp"),
            (
                "imap.zohocloud.ca",
                "imappro.zohocloud.ca",
                "smtppro.zohocloud.ca",
            ),
            ("imap.zoho.sa", "imappro.zoho.sa", "smtppro.zoho.sa"),
            (
                "imap.zoho.com.cn",
                "imappro.zoho.com.cn",
                "smtppro.zoho.com.cn",
            ),
        ] {
            assert_eq!(zoho_pro(imap).as_deref(), Some(pro), "{imap}");
            assert_eq!(smtp_candidates(pro, "a@b.de"), [smtp], "{pro}");
            // The personal host still sends from its own counterpart.
            assert_eq!(
                smtp_candidates(imap, "a@b.de"),
                [imap.replacen("imap.", "smtp.", 1)],
                "{imap}"
            );
        }
        // Only Zoho's own personal hosts: never another provider's, never a
        // pro host again, never a name that only looks like one.
        for host in [
            "imap.gmail.com",
            "imappro.zoho.eu",
            "imap.zoho.example",
            "imap.notzoho.eu",
            "imap.fastmail.com",
            "",
        ] {
            assert_eq!(zoho_pro(host), None, "{host}");
        }
        assert_eq!(
            smtp_candidates("imappro.example.com", "a@b.de"),
            ["imappro.example.com"]
        );
    }

    #[test]
    fn microsoft_and_icloud_are_sent_to_on_587_first() {
        for host in [
            "smtp-mail.outlook.com",
            "smtp.office365.com",
            "smtp.mail.me.com",
            "SMTP.Office365.com.",
        ] {
            assert_eq!(smtp_ports(host), [(587, false), (465, true)], "{host}");
        }
        for host in ["smtp.gmail.com", "smtp.zoho.eu", "mail.example.com"] {
            assert_eq!(smtp_ports(host), SMTP_PORTS, "{host}");
        }
    }

    #[test]
    fn three_answers_end_the_search_instead_of_producing_a_hostname() {
        assert!(matches!(
            mx_rule("mail.protonmail.ch"),
            Some(MxRule::Refuse(_))
        ));
        match mx_rule("mx1.improvmx.com") {
            Some(MxRule::Refuse(why)) => assert!(why.contains("forwards"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            mx_rule("mx0a-000abc01.pphosted.com"),
            Some(MxRule::Filtered)
        );
    }

    #[test]
    fn an_mx_nobody_recognises_leaves_the_conventional_names_their_turn() {
        assert_eq!(mx_rule("mail.somecompany.example"), None);
        assert_eq!(
            conventional("owner@thesherwood.group"),
            [
                "imap.thesherwood.group",
                "mail.thesherwood.group",
                "thesherwood.group"
            ]
        );
    }

    #[test]
    fn microsoft_is_found_and_signs_in_through_microsoft() {
        for d in [
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "hotmail.co.uk",
            "outlook.jp",
            "live.com.au",
        ] {
            let known = table(d).expect(d);
            assert_eq!(known.host, MS_IMAP, "{d}");
            assert!(is_microsoft(known.host), "{d}");
            assert_eq!(known.label, "Outlook", "{d}");
            assert_eq!(known.help, MS_SIGN_IN, "{d}");
        }
        assert!(!is_microsoft_consumer("gmail.com"));
        assert!(!is_microsoft_consumer("notoutlook.com"));
        match mx_rule("acme-com.mail.protection.outlook.com") {
            Some(MxRule::Serves(h)) => {
                assert!(is_microsoft(h.host));
                assert_eq!(h.label, "Microsoft 365");
                assert_eq!(h.help, MS_SIGN_IN);
            }
            other => panic!("{other:?}"),
        }
        // C0's words stay, for a build without Microsoft sign-in.
        assert!(MS_HELP.contains("cannot be added") && MS_HELP.contains("OAuth"));
        assert!(MS365_HELP.contains("Microsoft 365") && MS365_HELP.contains("OAuth"));
        for host in [
            "outlook.office365.com",
            "OUTLOOK.OFFICE365.COM.",
            "imap-mail.outlook.com",
            "outlook.office.com",
            "hotmail-com.olc.protection.outlook.com",
        ] {
            assert!(is_microsoft(host), "{host}");
        }
        for host in [
            "imap.gmail.com",
            "notoutlook.com",
            "outlook.com.example",
            "",
        ] {
            assert!(!is_microsoft(host), "{host}");
        }
    }

    #[test]
    fn proton_is_refused_up_front() {
        assert_eq!(no_imap("proton.me"), Some("Proton Mail"));
        assert_eq!(no_imap("gmail.com"), None);
    }

    #[test]
    fn sending_derives_from_whatever_imap_host_was_found() {
        assert_eq!(
            smtp_candidates("imap.gmail.com", "a@b.com"),
            ["smtp.gmail.com"]
        );
        assert_eq!(
            smtp_candidates("imap.titan.email", "a@b.com"),
            ["smtp.titan.email"]
        );
        // Not named imap.* — it is the submission host too.
        assert_eq!(
            smtp_candidates("mail.privateemail.com", "a@b.com"),
            ["mail.privateemail.com"]
        );
        // Microsoft needs both: business and consumer submit to different hosts.
        let ms = smtp_candidates("outlook.office365.com", "a@b.com");
        assert_eq!(ms[0], "smtp.office365.com");
        assert!(ms.contains(&"smtp-mail.outlook.com".to_string()), "{ms:?}");
        // Outlook.com's own addresses try Outlook.com's host first, and every
        // host a Microsoft mailbox is given is Microsoft's: its token goes
        // nowhere else.
        let consumer = smtp_candidates("outlook.office365.com", "a@hotmail.co.uk");
        assert_eq!(consumer[0], "smtp-mail.outlook.com");
        assert!(consumer.contains(&"smtp.office365.com".to_string()));
        for h in ms.iter().chain(&consumer) {
            assert!(is_microsoft(h), "{h}");
        }
    }

    #[test]
    fn a_refusal_is_told_apart_from_an_unreachable_server() {
        assert!(is_auth_failure("Invalid credentials (Failure)"));
        assert!(is_auth_failure(
            "535 5.7.8 Username and Password not accepted"
        ));
        assert!(!is_auth_failure("getaddrinfo ENOTFOUND imap.example.com"));
        assert!(!is_auth_failure("Socket timeout"));
    }
}
