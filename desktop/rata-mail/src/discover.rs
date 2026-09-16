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
const APPLE_HELP: &str = "appleid.apple.com → Sign-In and Security → App-Specific Passwords";
const MS_HELP: &str =
    "account.microsoft.com → Security → App passwords (needs two-step verification)";
const YAHOO_HELP: &str = "login.yahoo.com → Account security → Generate app password";

/// Consumer domains, answered without a lookup.
pub fn table(domain: &str) -> Option<MailHost> {
    let h = |host, label, help| Some(MailHost { host, label, help });
    match domain {
        "gmail.com" | "googlemail.com" => h("imap.gmail.com", "Gmail", GOOGLE_HELP),
        "icloud.com" | "me.com" | "mac.com" => h("imap.mail.me.com", "iCloud Mail", APPLE_HELP),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => {
            h("outlook.office365.com", "Outlook", MS_HELP)
        }
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
        return serves(
            "outlook.office365.com",
            "Microsoft 365",
            "Your Microsoft 365 account → Security → App passwords. Some organisations disable these — your IT administrator can tell you.",
        );
    }
    if under("zoho.com") || under("zoho.eu") || under("zoho.in") {
        return serves(
            "imap.zoho.com",
            "Zoho Mail",
            "accounts.zoho.com → Security → App passwords",
        );
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

    if imap_host == "outlook.office365.com" {
        add(&mut out, "smtp.office365.com".into());
        add(&mut out, "smtp-mail.outlook.com".into());
        add(&mut out, imap_host.into());
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
            ("mx.zoho.eu", "imap.zoho.com", "Zoho Mail"),
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
                        "{mx} should say where app passwords live"
                    );
                }
                other => panic!("{mx} -> {other:?}"),
            }
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
