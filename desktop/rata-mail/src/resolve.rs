//! Asking DNS where a domain's mail actually lives.
//!
//! This is the half of discovery that needs the network, kept apart from the
//! tables and rules in [`crate::discover`] so those stay testable without one.
//!
//! Order matters and is deliberate: a domain's own SRV record, then its MX,
//! then, only when neither named a provider, the evidence a domain leaves
//! of where its mailboxes are (its SPF record and its `autodiscover` name,
//! L1), then the conventional names. Conventional names go *last* so a
//! stale `imap.<domain>` left over from an old setup cannot outrank what
//! the DNS actually says today.

use std::net::IpAddr;

use std::future::Future;

#[cfg(any(system_dns, test))]
use std::time::Duration;

#[cfg(not(system_dns))]
use hickory_resolver::{
    Resolver as HickoryResolver, TokioResolver,
    proto::rr::{RData, RecordType},
};

#[cfg(system_dns)]
use crate::{dns_sd, records};

use crate::discover::{
    Candidate, IMAP_PORT, MS_SIGN_IN, MailHost, MxRule, Source, autodiscover_provider,
    conventional, is_microsoft, is_microsoft_consumer, mx_rule, no_imap, parse_spf, spf_provider,
    spf_record, table,
};
use crate::guard::{HostVerdict, check_literal, check_resolved, normalise};
use crate::key::domain_of;

/// A DNS resolver. Built once and shared — each one carries its own cache, so
/// making a fresh one per lookup would throw that away.
///
/// On iOS it is the system's own resolver instead, and holds nothing: there
/// are no DNS settings an app can read there, and the system keeps the cache
/// (`crate::dns_sd`).
pub struct Resolver {
    #[cfg(not(system_dns))]
    inner: TokioResolver,
}

/// One SRV answer, from either resolver.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Srv {
    pub(crate) priority: u16,
    pub(crate) weight: u16,
    pub(crate) port: u16,
    /// With its final dot; the root alone, `.`, says there is no service.
    pub(crate) target: String,
}

/// How long the system's resolver is given for one question on iOS: about
/// what hickory takes at most on the desktop (two tries of five seconds). A
/// question with no answer by then is a lookup that failed, and discovery
/// goes on as after any other.
#[cfg(any(system_dns, test))]
pub(crate) const SYSTEM_WAIT: Duration = Duration::from_secs(10);

#[cfg(not(system_dns))]
impl Resolver {
    /// Use the machine's own DNS settings, which is what a desktop app should
    /// do: the customer's resolver may be the only one that can see their
    /// company's internal names, and it is the one they chose.
    pub fn system() -> Result<Self, String> {
        let builder = HickoryResolver::builder_tokio()
            .map_err(|e| format!("could not read this machine's DNS settings: {e}"))?;
        let inner = builder
            .build()
            .map_err(|e| format!("could not start the DNS resolver: {e}"))?;
        Ok(Self { inner })
    }

    /// Every address a name answers with, A and AAAA together.
    pub async fn addresses(&self, host: &str) -> Vec<IpAddr> {
        match self.inner.lookup_ip(host).await {
            Ok(r) => r.iter().collect::<Vec<IpAddr>>(),
            Err(_) => vec![],
        }
    }

    /// Every MX answer, as (preference, exchange).
    async fn mx_answers(&self, domain: &str) -> Vec<(u16, String)> {
        // 0.26 returns a plain Lookup, and the rdata fields are public rather
        // than accessors.
        match self.inner.mx_lookup(domain).await {
            Ok(r) => r
                .answers()
                .iter()
                .filter_map(|rec| match &rec.data {
                    RData::MX(mx) => Some((mx.preference, mx.exchange.to_utf8())),
                    _ => None,
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Every SRV answer at `name`.
    async fn srv_answers(&self, name: &str) -> Vec<Srv> {
        let Ok(lookup) = self.inner.srv_lookup(name).await else {
            return vec![];
        };
        lookup
            .answers()
            .iter()
            .filter_map(|rec| match &rec.data {
                RData::SRV(srv) => Some(Srv {
                    priority: srv.priority,
                    weight: srv.weight,
                    port: srv.port,
                    target: srv.target.to_utf8(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Every TXT answer, each one's strings joined.
    async fn txt_answers(&self, name: &str) -> Vec<String> {
        match self.inner.txt_lookup(name).await {
            Ok(r) => r
                .answers()
                .iter()
                .filter_map(|rec| match &rec.data {
                    RData::TXT(txt) => Some(
                        txt.txt_data
                            .iter()
                            .map(|part| String::from_utf8_lossy(part))
                            .collect::<String>(),
                    ),
                    _ => None,
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    /// The first CNAME answer, which is the name's own record.
    async fn cname_answer(&self, name: &str) -> Option<String> {
        let lookup = self.inner.lookup(name, RecordType::CNAME).await.ok()?;
        lookup.answers().iter().find_map(|rec| match &rec.data {
            RData::CNAME(c) => Some(c.0.to_utf8()),
            _ => None,
        })
    }
}

/// iOS: the system's own resolver answers, records through dns_sd and
/// addresses through getaddrinfo (`crate::dns_sd`). On a Mac only the
/// `system-dns` feature's tests use it.
#[cfg(system_dns)]
impl Resolver {
    /// There are no settings to read first, so this cannot fail.
    pub fn system() -> Result<Self, String> {
        Ok(Self {})
    }

    /// Every address a name answers with, A and AAAA together, from
    /// getaddrinfo ([`system_addresses`]).
    pub async fn addresses(&self, host: &str) -> Vec<IpAddr> {
        system_addresses(host).await
    }

    async fn mx_answers(&self, domain: &str) -> Vec<(u16, String)> {
        let found = dns_sd::ask(domain, dns_sd::MX).await;
        found
            .iter()
            .map(Vec::as_slice)
            .filter_map(records::mx)
            .collect()
    }

    async fn srv_answers(&self, name: &str) -> Vec<Srv> {
        let found = dns_sd::ask(name, dns_sd::SRV).await;
        found
            .iter()
            .map(Vec::as_slice)
            .filter_map(records::srv)
            .collect()
    }

    async fn txt_answers(&self, name: &str) -> Vec<String> {
        let found = dns_sd::ask(name, dns_sd::TXT).await;
        found
            .iter()
            .map(Vec::as_slice)
            .filter_map(records::txt)
            .collect()
    }

    async fn cname_answer(&self, name: &str) -> Option<String> {
        let found = dns_sd::ask(name, dns_sd::CNAME).await;
        found.iter().map(Vec::as_slice).find_map(records::cname)
    }
}

/// What both resolvers answer, read the same way.
impl Resolver {
    /// MX records, lowest priority first — which is the order a sender would
    /// try them, and therefore the order most likely to name the real provider.
    pub async fn mx(&self, domain: &str) -> Vec<String> {
        by_preference(self.mx_answers(domain).await)
    }

    /// RFC 6186: a domain may publish where its IMAP is, and its port with it.
    /// Few do, but the ones that do are telling us the answer outright.
    pub async fn srv_imaps(&self, domain: &str) -> Option<(String, u16)> {
        let name = format!("_imaps._tcp.{domain}");
        srv_choice(self.srv_answers(&name).await)
    }

    /// TXT records, each one's strings joined as RFC 7208 §3.3 says (a
    /// long SPF record is published as several strings of at most 255
    /// bytes). A failed lookup is no records.
    pub async fn txt(&self, name: &str) -> Vec<String> {
        self.txt_answers(name).await
    }

    /// Where a name is an alias for, if it is one: the first CNAME answer,
    /// which is the name's own record. A failed lookup is no alias.
    pub async fn cname(&self, name: &str) -> Option<String> {
        let target = self.cname_answer(name).await?;
        Some(target.trim_end_matches('.').to_string())
    }
}

/// MX exchanges, lowest preference first; equals keep the order they came in.
fn by_preference(mut records: Vec<(u16, String)>) -> Vec<String> {
    records.sort_by_key(|(p, _)| *p);
    records.into_iter().map(|(_, e)| e).collect()
}

/// The SRV answer to use, as host and port.
fn srv_choice(mut best: Vec<Srv>) -> Option<(String, u16)> {
    // Lowest priority wins; among equals, the heaviest. Only the winner is
    // used — a second SRV record is a fallback for a mail client that
    // cannot find the first, and RATA has the MX and the conventional names
    // behind it for that.
    best.sort_by_key(|s| (s.priority, std::cmp::Reverse(s.weight)));
    let first = best.first()?;
    let target = first.target.trim_end_matches('.').to_string();
    // A root target means "explicitly no service here", which is an answer.
    if target.is_empty() {
        return None;
    }
    Some((target, first.port))
}

/// Every address the system's resolver gives a name, A and AAAA alike, in
/// the order it gives them, each once (iOS). This is getaddrinfo, by way of
/// tokio's `lookup_host`: the only way to the network's own DNS on iOS, and
/// on an IPv6-only network what makes an IPv6 address (NAT64) for a name
/// that has only IPv4 ones. [`resolve_public`] still judges every address it
/// hands back, exactly as it judges hickory's. Nothing when the name does not
/// resolve, or not within [`SYSTEM_WAIT`].
#[cfg(any(system_dns, test))]
async fn system_addresses(host: &str) -> Vec<IpAddr> {
    let asked = tokio::net::lookup_host((host, 0));
    let Ok(Ok(found)) = tokio::time::timeout(SYSTEM_WAIT, asked).await else {
        return vec![];
    };
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in found.map(|a| a.ip()) {
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

/// What discovery asks DNS. The machine's resolver answers in the app, a
/// script in the tests, so every rule of the order below is tested without
/// the network. Each answers "nothing" when the lookup fails.
pub(crate) trait Lookup {
    fn mx(&self, domain: &str) -> impl Future<Output = Vec<String>> + Send;
    fn srv_imaps(&self, domain: &str) -> impl Future<Output = Option<(String, u16)>> + Send;
    fn txt(&self, name: &str) -> impl Future<Output = Vec<String>> + Send;
    fn cname(&self, name: &str) -> impl Future<Output = Option<String>> + Send;
}

impl Lookup for Resolver {
    fn mx(&self, domain: &str) -> impl Future<Output = Vec<String>> + Send {
        Resolver::mx(self, domain)
    }
    fn srv_imaps(&self, domain: &str) -> impl Future<Output = Option<(String, u16)>> + Send {
        Resolver::srv_imaps(self, domain)
    }
    fn txt(&self, name: &str) -> impl Future<Output = Vec<String>> + Send {
        Resolver::txt(self, name)
    }
    fn cname(&self, name: &str) -> impl Future<Output = Option<String>> + Send {
        Resolver::cname(self, name)
    }
}

/// Judge a host and, if it passes, hand back the very addresses that were
/// judged.
///
/// Returning them is the point. Checking a name and then handing the *name* to
/// `TcpStream::connect` resolves it a second time, and a resolver is free to
/// answer differently on the second ask — so a name that answered `93.184.x.x`
/// for the check can answer `192.168.1.1` for the connection, and the guard has
/// judged one address while the socket opens to another. Connecting to the
/// vetted addresses closes that door: there is only ever one lookup.
///
/// `Err` carries why, and `HostVerdict::NotFound` in particular means "try the
/// next candidate" rather than "stop".
pub async fn resolve_public(
    resolver: &Resolver,
    host: &str,
) -> Result<(String, Vec<IpAddr>), HostVerdict> {
    let h = match normalise(host) {
        Some(h) => h,
        None => return Err(HostVerdict::Malformed),
    };
    if let Some(verdict) = check_literal(&h) {
        // A literal, or a name that is refused whatever DNS says about it.
        if !verdict.is_allowed() {
            return Err(verdict);
        }
        let bare = h.trim_start_matches('[').trim_end_matches(']');
        return match bare.parse::<IpAddr>() {
            Ok(ip) => Ok((h, vec![ip])),
            Err(_) => Err(HostVerdict::Malformed),
        };
    }
    let addrs = resolver.addresses(&h).await;
    match check_resolved(&addrs) {
        HostVerdict::Allowed => Ok((h, addrs)),
        other => Err(other),
    }
}

/// Judge a host, resolving it when it is a name rather than a literal.
pub async fn check_host(resolver: &Resolver, host: &str) -> HostVerdict {
    match resolve_public(resolver, host).await {
        Ok(_) => HostVerdict::Allowed,
        Err(v) => v,
    }
}

/// The outcome of looking for a domain's mail server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery {
    /// Servers to try, best first.
    Candidates {
        hosts: Vec<Candidate>,
        /// A filtering service sits in front of the real mailbox, so the MX
        /// cannot say where that mailbox is. Named so the app can explain the
        /// server box rather than just showing it.
        filtered_by: Option<String>,
    },
    /// There is nothing to connect to, and why. Saying so beats three timeouts.
    Refuse(String),
}

/// Forgive the ways people paste a server address: a scheme in front, a port
/// or a path after. `imaps://imap.example.com:993/` means `imap.example.com`.
/// Bracketed IPv6 literals keep their colons.
fn tidy_typed_host(raw: &str) -> &str {
    let mut h = raw.trim();
    if let Some(i) = h.find("://") {
        h = &h[i + 3..];
    }
    if let Some(i) = h.find('/') {
        h = &h[..i];
    }
    if !h.starts_with('[')
        && let Some((name, port)) = h.rsplit_once(':')
        && !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && !name.contains(':')
    {
        h = name;
    }
    h
}

/// Everything worth trying for an address, best first.
pub async fn discover(resolver: &Resolver, email: &str, host_override: Option<&str>) -> Discovery {
    discover_with(resolver, email, host_override).await
}

/// [`discover`], asking `dns`.
pub(crate) async fn discover_with<D: Lookup + Sync>(
    dns: &D,
    email: &str,
    host_override: Option<&str>,
) -> Discovery {
    let domain = domain_of(email);

    // A server address the customer typed is honoured or refused out loud —
    // never quietly replaced by discovery, which would link whatever the
    // domain's DNS suggests instead of the server they asked for.
    let typed = host_override.map(str::trim).filter(|h| !h.is_empty());
    if let Some(raw) = typed {
        let Some(given) = normalise(tidy_typed_host(raw)) else {
            return Discovery::Refuse(format!(
                "\"{raw}\" is not a server address RATA can use. Enter just the name, like imap.example.com."
            ));
        };
        // A Microsoft server is honoured like any other. What differs is how
        // it is signed in to, and `verify` sees to that: a password is never
        // sent to Microsoft (see `discover::MS_SIGN_IN`). It is named for
        // Microsoft, not for the address's own domain.
        let microsoft = is_microsoft(&given);
        let label = match (microsoft, is_microsoft_consumer(&domain)) {
            (true, true) => "Outlook".to_string(),
            (true, false) => "Microsoft 365".to_string(),
            (false, _) => domain,
        };
        return Discovery::Candidates {
            hosts: vec![Candidate {
                host: given,
                port: IMAP_PORT,
                label,
                help: microsoft.then(|| MS_SIGN_IN.to_string()),
                source: Source::Override,
            }],
            filtered_by: None,
        };
    }

    if domain.is_empty() {
        return Discovery::Refuse("That does not look like an email address.".into());
    }
    if let Some(who) = no_imap(&domain) {
        return Discovery::Refuse(format!(
            "{who} encrypts mail on your device and offers no IMAP server RATA can reach. Their bridge only runs on your own computer."
        ));
    }
    if let Some(known) = table(&domain) {
        return Discovery::Candidates {
            hosts: vec![Candidate {
                host: known.host.into(),
                port: IMAP_PORT,
                label: known.label.into(),
                help: Some(known.help.into()),
                source: Source::Table,
            }],
            filtered_by: None,
        };
    }

    let mut hosts: Vec<Candidate> = Vec::new();
    let mut filtered_by: Option<String> = None;

    // A domain at Microsoft 365 is found here like any other: its SRV or MX
    // names Microsoft's host, and `verify` then keeps a password away from it.
    if let Some((target, port)) = dns.srv_imaps(&domain).await {
        let microsoft = is_microsoft(&target);
        hosts.push(Candidate {
            host: target,
            port,
            label: if microsoft {
                "Microsoft 365".into()
            } else {
                domain.clone()
            },
            help: microsoft.then(|| MS_SIGN_IN.to_string()),
            source: Source::Srv,
        });
    }

    let exchanges = dns.mx(&domain).await;
    for exchange in &exchanges {
        match mx_rule(exchange) {
            Some(MxRule::Refuse(why)) => return Discovery::Refuse(why.into()),
            Some(MxRule::Filtered) => {
                filtered_by.get_or_insert_with(|| exchange.trim_end_matches('.').to_string());
            }
            Some(MxRule::Serves(h)) => {
                if !hosts.iter().any(|c| c.host == h.host) {
                    hosts.push(Candidate {
                        host: h.host.into(),
                        port: IMAP_PORT,
                        label: h.label.into(),
                        help: Some(h.help.into()),
                        source: Source::Mx,
                    });
                }
            }
            None => {}
        }
    }

    // Neither the SRV nor the MX named a provider: the MX is a filter, a
    // name RATA does not know, or missing. What the domain publishes for
    // its own sending and for Outlook's setup still says where its
    // mailboxes are, Microsoft 365 or Google Workspace behind a Mimecast
    // most often.
    let mut evidence = if hosts.is_empty() {
        found_elsewhere(dns, &domain).await
    } else {
        vec![]
    };
    // An unrecognised MX under the domain itself (`mail.acme.com` for
    // acme.com) is usually the domain's own server, which the conventional
    // names find, as they did before L1; an on-premises Exchange that
    // sends through Microsoft 365 has exactly this SPF. So its evidence
    // goes after them. Behind a filter, or a provider RATA does not know,
    // the evidence goes first.
    let own_mx = filtered_by.is_none()
        && exchanges.iter().any(|mx| {
            let mx = mx.trim_end_matches('.').to_ascii_lowercase();
            mx == domain || mx.ends_with(&format!(".{domain}"))
        });
    if !own_mx {
        add_new(&mut hosts, std::mem::take(&mut evidence));
    }

    for guess in conventional(email) {
        if !hosts.iter().any(|c| c.host == guess) {
            hosts.push(Candidate {
                host: guess,
                port: IMAP_PORT,
                label: domain.clone(),
                help: None,
                source: Source::Guess,
            });
        }
    }
    add_new(&mut hosts, evidence);

    Discovery::Candidates { hosts, filtered_by }
}

/// Append each candidate whose host is not already listed.
fn add_new(hosts: &mut Vec<Candidate>, more: Vec<Candidate>) {
    for c in more {
        if !hosts.iter().any(|h| h.host == c.host) {
            hosts.push(c);
        }
    }
}

/// A provider found as evidence rather than by the MX.
fn evidenced(h: MailHost, source: Source) -> Candidate {
    Candidate {
        host: h.host.into(),
        port: IMAP_PORT,
        label: h.label.into(),
        help: Some(h.help.into()),
        source,
    }
}

/// Where a domain's mailboxes are, by what it publishes besides its MX
/// (L1). DNS only, and at most three lookups: the domain's TXT and
/// `autodiscover.<domain>`'s CNAME, side by side, then one `redirect=` of
/// its SPF record, never a second. `include:`s are read by name, never
/// looked up. A lookup that fails is no evidence.
///
/// The autodiscover CNAME comes first: only a Microsoft 365 mailbox needs
/// it. The SPF record counts only when every provider it includes is the
/// same one; a domain whose SPF names both Google and Microsoft (half way
/// through a move, or one sending through the other) says nothing about
/// which holds the mailboxes.
async fn found_elsewhere<D: Lookup + Sync>(dns: &D, domain: &str) -> Vec<Candidate> {
    let auto = format!("autodiscover.{domain}");
    let (txt, alias) = tokio::join!(dns.txt(domain), dns.cname(&auto));

    let mut out: Vec<Candidate> = Vec::new();
    if let Some(h) = alias.as_deref().and_then(autodiscover_provider) {
        out.push(evidenced(h, Source::Autodiscover));
    }

    let Some(spf) = spf_record(&txt).and_then(parse_spf) else {
        return out;
    };
    let mut includes = spf.includes;
    if let Some(next) = spf.redirect {
        // One hop. A redirect that leads to no record, two records or a
        // broken one makes the whole record an error (RFC 7208 §6.1),
        // which is evidence of nothing. Its own redirect is not followed.
        if next.contains('%') {
            return out;
        }
        let more = dns.txt(&next).await;
        let Some(more) = spf_record(&more).and_then(parse_spf) else {
            return out;
        };
        includes.extend(more.includes);
    }
    let mut named: Vec<MailHost> = Vec::new();
    for h in includes.iter().filter_map(|i| spf_provider(i)) {
        if !named.iter().any(|n| n.host == h.host) {
            named.push(h);
        }
    }
    if let [only] = named.as_slice() {
        add_new(&mut out, vec![evidenced(only.clone(), Source::Spf)]);
    }
    out
}

/// A DNS that answers from a script, and remembers what it was asked.
#[cfg(test)]
pub(crate) mod scripted {
    use std::sync::Mutex;

    use super::Lookup;

    #[derive(Default)]
    pub(crate) struct Dns {
        srv: Option<(String, u16)>,
        mx: Vec<String>,
        txt: Vec<(String, String)>,
        cname: Vec<(String, String)>,
        asked: Mutex<Vec<String>>,
    }

    impl Dns {
        pub(crate) fn new() -> Self {
            Self::default()
        }
        pub(crate) fn srv(mut self, host: &str, port: u16) -> Self {
            self.srv = Some((host.into(), port));
            self
        }
        pub(crate) fn mx(mut self, exchange: &str) -> Self {
            self.mx.push(exchange.into());
            self
        }
        /// One TXT record at `name`; call again for a second.
        pub(crate) fn txt(mut self, name: &str, record: &str) -> Self {
            self.txt.push((name.into(), record.into()));
            self
        }
        pub(crate) fn cname(mut self, name: &str, target: &str) -> Self {
            self.cname.push((name.into(), target.into()));
            self
        }
        /// Every lookup made, in order, as `"TXT acme.com"`.
        pub(crate) fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
        fn note(&self, what: &str, name: &str) {
            self.asked.lock().unwrap().push(format!("{what} {name}"));
        }
    }

    impl Lookup for Dns {
        async fn mx(&self, domain: &str) -> Vec<String> {
            self.note("MX", domain);
            self.mx.clone()
        }
        async fn srv_imaps(&self, domain: &str) -> Option<(String, u16)> {
            self.note("SRV", domain);
            self.srv.clone()
        }
        async fn txt(&self, name: &str) -> Vec<String> {
            self.note("TXT", name);
            self.txt
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, r)| r.clone())
                .collect()
        }
        async fn cname(&self, name: &str) -> Option<String> {
            self.note("CNAME", name);
            self.cname
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These reach real DNS. They use names that have been stable for years and
    // assert on the shape of the answer rather than a specific record, so a
    // provider reshuffling its MX does not turn into a red build.
    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn a_consumer_address_needs_no_lookup_at_all() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match discover(&r, "someone@gmail.com", None).await {
                Discovery::Candidates { hosts, .. } => {
                    assert_eq!(hosts.len(), 1);
                    assert_eq!(hosts[0].host, "imap.gmail.com");
                    assert_eq!(hosts[0].source, Source::Table);
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn proton_is_refused_before_anything_is_tried() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match discover(&r, "someone@proton.me", None).await {
                Discovery::Refuse(why) => assert!(why.contains("no IMAP server"), "{why}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn microsoft_is_found_like_any_other_provider() {
        // C0 refused these here. They are found now, and `verify` keeps a
        // password away from them (imap.rs:
        // `a_password_is_never_sent_to_microsoft`).
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            for addr in [
                "someone@outlook.com",
                "someone@hotmail.com",
                "someone@live.com",
                "someone@hotmail.co.uk",
            ] {
                match discover(&r, addr, None).await {
                    Discovery::Candidates { hosts, .. } => {
                        assert_eq!(hosts.len(), 1, "{addr}: {hosts:?}");
                        assert_eq!(hosts[0].host, "outlook.office365.com", "{addr}");
                        assert_eq!(hosts[0].label, "Outlook", "{addr}");
                        assert_eq!(hosts[0].source, Source::Table, "{addr}");
                    }
                    other => panic!("{addr}: {other:?}"),
                }
            }
            // Typing Microsoft's server by hand is honoured like any other.
            match discover(&r, "me@example.com", Some("outlook.office365.com")).await {
                Discovery::Candidates { hosts, .. } => {
                    assert_eq!(hosts.len(), 1);
                    assert_eq!(hosts[0].host, "outlook.office365.com");
                    assert_eq!(hosts[0].source, Source::Override);
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_domain_at_microsoft_365_is_found_by_its_mx() {
        // Real DNS, like the Anthropic test below: microsoft.com's MX is
        // Microsoft 365's (microsoft-com.mail.protection.outlook.com).
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match discover(&r, "someone@microsoft.com", None).await {
                Discovery::Candidates { hosts, .. } => {
                    let first = &hosts[0];
                    assert!(is_microsoft(&first.host), "{hosts:?}");
                    assert_eq!(first.label, "Microsoft 365", "{hosts:?}");
                    assert!(
                        hosts
                            .iter()
                            .any(|c| c.host == "outlook.office365.com" && c.source == Source::Mx),
                        "{hosts:?}"
                    );
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_typed_server_address_is_the_only_one_tried() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            match discover(&r, "me@example.com", Some("mail.example.org")).await {
                Discovery::Candidates { hosts, .. } => {
                    assert_eq!(hosts.len(), 1);
                    assert_eq!(hosts[0].source, Source::Override);
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_pasted_server_address_is_tidied_rather_than_dropped() {
        assert_eq!(tidy_typed_host("imap.example.com"), "imap.example.com");
        assert_eq!(tidy_typed_host("imap.example.com:993"), "imap.example.com");
        assert_eq!(
            tidy_typed_host("imaps://imap.example.com:993/"),
            "imap.example.com"
        );
        assert_eq!(
            tidy_typed_host("https://mail.example.com/login"),
            "mail.example.com"
        );
        assert_eq!(tidy_typed_host("[2001:db8::1]"), "[2001:db8::1]");
    }

    #[test]
    fn a_server_address_that_cannot_be_used_is_refused_not_ignored() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            // Used to fall through to discovery and quietly link a server the
            // customer never typed.
            match discover(&r, "me@example.com", Some("imap example com")).await {
                Discovery::Refuse(why) => assert!(why.contains("not a server address"), "{why}"),
                other => panic!("a bad override was not refused: {other:?}"),
            }
            // Blank means "no override", not "a bad one".
            assert!(!matches!(
                discover(&r, "me@gmail.com", Some("   ")).await,
                Discovery::Refuse(_)
            ));
        });
    }

    #[test]
    fn a_domain_with_no_dns_falls_back_to_the_conventional_names() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            let addr = format!("me@nx-{}.invalid", std::process::id());
            match discover(&r, &addr, None).await {
                Discovery::Candidates { hosts, .. } => {
                    assert!(!hosts.is_empty());
                    assert!(hosts.iter().all(|c| c.source == Source::Guess), "{hosts:?}");
                }
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn the_guard_still_refuses_a_private_name_after_resolving_it() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            assert_eq!(check_host(&r, "localhost").await, HostVerdict::NotPublic);
            assert_eq!(check_host(&r, "127.0.0.1").await, HostVerdict::NotPublic);
            assert_eq!(
                check_host(&r, "::ffff:7f00:1").await,
                HostVerdict::NotPublic
            );
            let missing = format!("nx-{}.invalid", std::process::id());
            assert_eq!(check_host(&r, &missing).await, HostVerdict::NotFound);
        });
    }

    #[test]
    fn a_real_business_domain_resolves_through_its_mx() {
        rt().block_on(async {
            let r = Resolver::system().expect("resolver");
            // anthropic.com runs no mail server of its own; its MX says Google.
            match discover(&r, "someone@anthropic.com", None).await {
                Discovery::Candidates { hosts, .. } => {
                    let first = &hosts[0];
                    assert_eq!(
                        first.source,
                        Source::Mx,
                        "expected the MX to answer: {hosts:?}"
                    );
                    assert!(first.help.is_some(), "and to carry its app-password help");
                    // The conventional guesses must still be behind it.
                    assert!(hosts.iter().any(|c| c.source == Source::Guess));
                }
                other => panic!("{other:?}"),
            }
        });
    }

    // ------------------------------------------ L1: evidence behind a filter

    use super::scripted::Dns;
    use crate::discover::{MS_IMAP, spf_provider};

    const MIMECAST: &str = "eu-smtp-inbound-1.mimecast.com";

    fn found(dns: &Dns, email: &str) -> (Vec<Candidate>, Option<String>) {
        match rt().block_on(discover_with(dns, email, None)) {
            Discovery::Candidates { hosts, filtered_by } => (hosts, filtered_by),
            other => panic!("{email}: {other:?}"),
        }
    }

    fn guesses(hosts: &[Candidate]) -> Vec<&str> {
        hosts
            .iter()
            .filter(|c| c.source == Source::Guess)
            .map(|c| c.host.as_str())
            .collect()
    }

    #[test]
    fn behind_a_filter_each_providers_spf_names_its_host() {
        for (include, host, label) in [
            ("spf.protection.outlook.com", MS_IMAP, "Microsoft 365"),
            ("_spf.google.com", "imap.gmail.com", "Google Workspace"),
            ("spf.messagingengine.com", "imap.fastmail.com", "Fastmail"),
            (
                "_spf.mail.hostinger.com",
                "imap.hostinger.com",
                "Hostinger Email",
            ),
            ("icloud.com", "imap.mail.me.com", "iCloud Mail"),
            ("spf.titan.email", "imap.titan.email", "Titan"),
            ("emailsrvr.com", "secure.emailsrvr.com", "Rackspace Email"),
            ("spf.migadu.com", "imap.migadu.com", "Migadu"),
            (
                "spf.privateemail.com",
                "mail.privateemail.com",
                "Namecheap Private Email",
            ),
            ("zohomail.com", "imap.zoho.com", "Zoho Mail"),
            ("one.zoho.com", "imap.zoho.com", "Zoho Mail"),
            ("zoho.com", "imap.zoho.com", "Zoho Mail"),
            ("zohomail.eu", "imap.zoho.eu", "Zoho Mail"),
            ("zoho.eu", "imap.zoho.eu", "Zoho Mail"),
            ("zohomail.in", "imap.zoho.in", "Zoho Mail"),
            ("zoho.in", "imap.zoho.in", "Zoho Mail"),
            ("zohomail.com.au", "imap.zoho.com.au", "Zoho Mail"),
            ("zohomail.jp", "imap.zoho.jp", "Zoho Mail"),
            ("zohocloud.ca", "imap.zohocloud.ca", "Zoho Mail"),
            ("zohomail.sa", "imap.zoho.sa", "Zoho Mail"),
            ("zohomail.com.cn", "imap.zoho.com.cn", "Zoho Mail"),
            ("ZOHO.COM.", "imap.zoho.com", "Zoho Mail"),
        ] {
            let dns = Dns::new()
                .mx(MIMECAST)
                .txt("acme.com", "google-site-verification=abc")
                .txt(
                    "acme.com",
                    &format!("v=spf1 ip4:192.0.2.1 include:{include} ~all"),
                );
            let (hosts, filtered_by) = found(&dns, "ann@acme.com");
            let first = &hosts[0];
            assert_eq!(first.host, host, "{include}: {hosts:?}");
            assert_eq!(first.label, label, "{include}");
            assert_eq!(first.source, Source::Spf, "{include}");
            assert_eq!(first.port, IMAP_PORT, "{include}");
            // The provider's own help, the one its MX gives.
            let help = spf_provider(include).expect(include).help;
            assert_eq!(first.help.as_deref(), Some(help), "{include}");
            // The filter is still named, and the guesses are still behind.
            assert_eq!(filtered_by.as_deref(), Some(MIMECAST), "{include}");
            assert_eq!(
                guesses(&hosts),
                ["imap.acme.com", "mail.acme.com", "acme.com"],
                "{include}"
            );
        }
    }

    #[test]
    fn a_microsoft_365_domain_behind_a_filter_is_offered_microsofts_sign_in() {
        let dns = Dns::new()
            .mx(MIMECAST)
            .mx("eu-smtp-inbound-2.mimecast.com")
            .txt(
                "acme.com",
                "v=spf1 include:spf.protection.outlook.com include:_netblocks.mimecast.com -all",
            );
        let (hosts, filtered_by) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].host, MS_IMAP, "{hosts:?}");
        assert!(is_microsoft(&hosts[0].host));
        assert_eq!(hosts[0].label, "Microsoft 365");
        assert_eq!(hosts[0].help.as_deref(), Some(MS_SIGN_IN));
        assert_eq!(hosts[0].source, Source::Spf);
        assert_eq!(filtered_by.as_deref(), Some(MIMECAST));
    }

    #[test]
    fn an_autodiscover_alias_to_outlook_names_microsoft_365() {
        // No SPF at all: the alias alone is enough.
        let dns = Dns::new()
            .mx("mx0a-001.pphosted.com")
            .cname("autodiscover.acme.com", "autodiscover.outlook.com.");
        let (hosts, filtered_by) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].host, MS_IMAP, "{hosts:?}");
        assert_eq!(hosts[0].label, "Microsoft 365");
        assert_eq!(hosts[0].help.as_deref(), Some(MS_SIGN_IN));
        assert_eq!(hosts[0].source, Source::Autodiscover);
        assert_eq!(filtered_by.as_deref(), Some("mx0a-001.pphosted.com"));
        assert_eq!(guesses(&hosts).len(), 3);

        // With a Microsoft SPF too, Microsoft is listed once, by its alias.
        let both = Dns::new()
            .mx(MIMECAST)
            .cname("autodiscover.acme.com", "autodiscover.outlook.com")
            .txt("acme.com", "v=spf1 include:spf.protection.outlook.com -all");
        let (hosts, _) = found(&both, "ann@acme.com");
        assert_eq!(hosts.iter().filter(|c| c.host == MS_IMAP).count(), 1);
        assert_eq!(hosts[0].source, Source::Autodiscover);

        // An alias anywhere else names nobody.
        for target in [
            "autodiscover.acme.com",
            "autodiscover.outlook.com.evil.example",
            "outlook.office365.com",
            "autodiscover.secureserver.net",
        ] {
            let dns = Dns::new()
                .mx(MIMECAST)
                .cname("autodiscover.acme.com", target);
            let (hosts, _) = found(&dns, "ann@acme.com");
            assert!(
                hosts.iter().all(|c| c.source == Source::Guess),
                "{target}: {hosts:?}"
            );
        }
    }

    #[test]
    fn an_mx_nobody_recognises_is_read_with_the_spf_too() {
        let dns = Dns::new()
            .mx("mx01.hornetsecurity.com")
            .txt("acme.de", "v=spf1 include:_spf.google.com ~all");
        let (hosts, filtered_by) = found(&dns, "ann@acme.de");
        assert_eq!(hosts[0].host, "imap.gmail.com", "{hosts:?}");
        assert_eq!(hosts[0].label, "Google Workspace");
        assert_eq!(hosts[0].source, Source::Spf);
        // Not a filter RATA knows, so none is named.
        assert_eq!(filtered_by, None);
        assert_eq!(guesses(&hosts), ["imap.acme.de", "mail.acme.de", "acme.de"]);

        // No MX at all is read the same way.
        let dns = Dns::new().txt("acme.de", "v=spf1 include:_spf.google.com ~all");
        let (hosts, _) = found(&dns, "ann@acme.de");
        assert_eq!(hosts[0].source, Source::Spf);
    }

    #[test]
    fn a_domains_own_mx_keeps_its_conventional_names_first() {
        // An on-premises server (or Exchange sending through Microsoft
        // 365): the names it always had come first, as before L1, and the
        // evidence after them.
        let dns = Dns::new().mx("mail.acme.com.").txt(
            "acme.com",
            "v=spf1 mx include:spf.protection.outlook.com -all",
        );
        let (hosts, _) = found(&dns, "ann@acme.com");
        let order: Vec<_> = hosts.iter().map(|c| (c.host.as_str(), c.source)).collect();
        assert_eq!(
            order,
            [
                ("imap.acme.com", Source::Guess),
                ("mail.acme.com", Source::Guess),
                ("acme.com", Source::Guess),
                (MS_IMAP, Source::Spf),
            ]
        );
    }

    #[test]
    fn a_provider_the_mx_names_needs_no_more_lookups() {
        let dns = Dns::new()
            .mx("aspmx.l.google.com")
            .txt("acme.com", "v=spf1 include:spf.protection.outlook.com -all")
            .cname("autodiscover.acme.com", "autodiscover.outlook.com");
        let (hosts, _) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].host, "imap.gmail.com");
        assert_eq!(hosts[0].source, Source::Mx);
        assert!(hosts.iter().all(|c| c.host != MS_IMAP), "{hosts:?}");
        assert_eq!(dns.asked(), ["SRV acme.com", "MX acme.com"]);

        // Nor an SRV answer.
        let dns = Dns::new()
            .srv("imap.acme.com", 993)
            .mx(MIMECAST)
            .txt("acme.com", "v=spf1 include:_spf.google.com -all");
        let (hosts, _) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].source, Source::Srv);
        assert!(hosts.iter().all(|c| c.source != Source::Spf), "{hosts:?}");
        assert_eq!(dns.asked(), ["SRV acme.com", "MX acme.com"]);

        // Nor a typed server, nor the table.
        let dns = Dns::new().mx(MIMECAST);
        let _ = rt().block_on(discover_with(&dns, "ann@acme.com", Some("mail.acme.com")));
        let _ = rt().block_on(discover_with(&dns, "ann@gmail.com", None));
        assert!(dns.asked().is_empty(), "{:?}", dns.asked());
    }

    #[test]
    fn only_a_passing_include_is_evidence() {
        for (record, evidence) in [
            ("v=spf1 include:_spf.google.com -all", true),
            ("v=spf1 +include:_spf.google.com -all", true),
            ("V=SPF1 INCLUDE:_SPF.GOOGLE.COM -ALL", true),
            ("v=spf1 include:_spf.google.com. -all", true),
            ("v=spf1   include:_spf.google.com\t-all", true),
            ("v=spf1 -include:_spf.google.com -all", false),
            ("v=spf1 ~include:_spf.google.com -all", false),
            ("v=spf1 ?include:_spf.google.com -all", false),
            ("v=spf1 include:%{d}._spf.google.com -all", false),
            ("v=spf1 include:mail._spf.google.com -all", false),
            ("v=spf1 include:google.com -all", false),
            ("v=spf1 include:zcsend.net -all", false),
            (
                "v=spf1 include:sendgrid.net include:mailgun.org -all",
                false,
            ),
            ("v=spf1 include:zohomail.ca -all", false),
        ] {
            let dns = Dns::new().mx(MIMECAST).txt("acme.com", record);
            let (hosts, _) = found(&dns, "ann@acme.com");
            assert_eq!(
                hosts.iter().any(|c| c.source == Source::Spf),
                evidence,
                "{record}: {hosts:?}"
            );
            // Whatever the record says, nothing is refused because of it.
            assert_eq!(guesses(&hosts).len(), 3, "{record}");
        }
    }

    #[test]
    fn a_redirect_is_followed_one_hop_and_no_further() {
        let dns = Dns::new()
            .mx(MIMECAST)
            .txt("acme.com", "v=spf1 redirect=_spf.acme.com")
            .txt(
                "_spf.acme.com",
                "v=spf1 include:spf.protection.outlook.com redirect=_spf2.acme.com",
            )
            .txt("_spf2.acme.com", "v=spf1 include:_spf.google.com -all");
        let (hosts, _) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].host, MS_IMAP, "{hosts:?}");
        assert_eq!(hosts[0].source, Source::Spf);
        assert!(
            hosts.iter().all(|c| c.host != "imap.gmail.com"),
            "{hosts:?}"
        );
        let asked = dns.asked();
        assert!(
            asked.contains(&"TXT _spf.acme.com".to_string()),
            "{asked:?}"
        );
        assert!(
            !asked.contains(&"TXT _spf2.acme.com".to_string()),
            "{asked:?}"
        );
        // At most three lookups beyond the SRV and MX: TXT, CNAME, one hop.
        assert_eq!(asked.len(), 5, "{asked:?}");

        // A record with `all` has no redirect to follow.
        let dns = Dns::new()
            .mx(MIMECAST)
            .txt("acme.com", "v=spf1 redirect=_spf.acme.com ~all")
            .txt("_spf.acme.com", "v=spf1 include:_spf.google.com -all");
        let (hosts, _) = found(&dns, "ann@acme.com");
        assert!(hosts.iter().all(|c| c.source != Source::Spf), "{hosts:?}");
        assert!(!dns.asked().contains(&"TXT _spf.acme.com".to_string()));

        // A redirect to nothing, to two records or to a broken one makes
        // the whole record an error: the base's own include is not kept.
        for target in [
            vec![],
            vec![
                "v=spf1 include:_spf.google.com -all",
                "v=spf1 include:_spf.google.com -all",
            ],
            vec!["v=spf1 nonsense:x -all"],
        ] {
            let mut dns = Dns::new().mx(MIMECAST).txt(
                "acme.com",
                "v=spf1 include:_spf.google.com redirect=_spf.acme.com",
            );
            for r in &target {
                dns = dns.txt("_spf.acme.com", r);
            }
            let (hosts, _) = found(&dns, "ann@acme.com");
            assert!(
                hosts.iter().all(|c| c.source != Source::Spf),
                "{target:?}: {hosts:?}"
            );
        }
    }

    #[test]
    fn a_broken_or_doubled_spf_record_is_no_evidence() {
        for records in [
            // Two SPF records: an error, whichever is meant.
            vec![
                "v=spf1 include:_spf.google.com -all",
                "v=spf1 include:_spf.google.com -all",
            ],
            vec!["v=spf1 include: -all"],
            vec!["v=spf1 include -all"],
            vec!["v=spf1 frobnicate:_spf.google.com include:_spf.google.com"],
            vec!["v=spf1 include:_spf.google.com redirect=a.example redirect=b.example"],
            vec!["v=spf1 include:_spf.google.com -redirect=a.example"],
            vec!["v=spf1 include:_spf.google.com all:x"],
            vec!["v=spf1 include:_spf.google.com +"],
            vec!["v=spf10 include:_spf.google.com -all"],
            vec!["spf1 include:_spf.google.com -all"],
            vec![" v=spf1 include:_spf.google.com -all"],
            vec!["include:_spf.google.com"],
            // Two providers: a move half done says nothing about either.
            vec!["v=spf1 include:_spf.google.com include:spf.protection.outlook.com -all"],
        ] {
            let mut dns = Dns::new().mx(MIMECAST);
            for r in &records {
                dns = dns.txt("acme.com", r);
            }
            let (hosts, _) = found(&dns, "ann@acme.com");
            assert!(
                hosts.iter().all(|c| c.source == Source::Guess),
                "{records:?}: {hosts:?}"
            );
        }
        // The same provider twice is still one, among other terms.
        let dns = Dns::new().mx(MIMECAST).txt(
            "acme.com",
            "v=spf1 include:zoho.eu include:zohomail.eu exp=why.acme.com ip6:2001:db8::/32 a/24 -all",
        );
        let (hosts, _) = found(&dns, "ann@acme.com");
        assert_eq!(hosts[0].host, "imap.zoho.eu", "{hosts:?}");
        assert_eq!(hosts.iter().filter(|c| c.source == Source::Spf).count(), 1);
    }

    #[test]
    fn with_no_evidence_the_conventional_names_are_tried_as_before() {
        let dns = Dns::new()
            .mx(MIMECAST)
            .txt("acme.com", "v=spf1 include:_netblocks.mimecast.com -all");
        let (hosts, filtered_by) = found(&dns, "ann@acme.com");
        let all: Vec<_> = hosts.iter().map(|c| (c.host.as_str(), c.source)).collect();
        assert_eq!(
            all,
            [
                ("imap.acme.com", Source::Guess),
                ("mail.acme.com", Source::Guess),
                ("acme.com", Source::Guess),
            ]
        );
        assert_eq!(filtered_by.as_deref(), Some(MIMECAST));

        // Nothing answering at all, the same.
        let (hosts, filtered_by) = found(&Dns::new(), "ann@acme.com");
        assert!(hosts.iter().all(|c| c.source == Source::Guess));
        assert_eq!(hosts.len(), 3);
        assert_eq!(filtered_by, None);
    }

    #[test]
    fn a_refusing_mx_still_refuses_whatever_the_spf_says() {
        let dns = Dns::new()
            .mx("mail.protonmail.ch")
            .txt("acme.com", "v=spf1 include:_spf.google.com -all");
        assert!(matches!(
            rt().block_on(discover_with(&dns, "ann@acme.com", None)),
            Discovery::Refuse(_)
        ));
    }

    // ------------------------------- what hickory and the system's share

    fn srv(priority: u16, weight: u16, port: u16, target: &str) -> Srv {
        Srv {
            priority,
            weight,
            port,
            target: target.into(),
        }
    }

    #[test]
    fn mx_answers_are_tried_lowest_preference_first() {
        let answers = vec![
            (20, "alt2.example.".to_string()),
            (5, "first.example.".to_string()),
            (20, "alt3.example.".to_string()),
            (10, "alt1.example.".to_string()),
        ];
        assert_eq!(
            by_preference(answers),
            [
                "first.example.",
                "alt1.example.",
                "alt2.example.",
                "alt3.example."
            ]
        );
        assert!(by_preference(vec![]).is_empty());
    }

    #[test]
    fn the_srv_answer_used_is_the_lowest_priority_then_the_heaviest() {
        let answers = vec![
            srv(10, 100, 993, "backup.example.com."),
            srv(5, 1, 993, "light.example.com."),
            srv(5, 9, 9993, "heavy.example.com."),
        ];
        assert_eq!(
            srv_choice(answers),
            Some(("heavy.example.com".to_string(), 9993))
        );
        // The root alone says there is no IMAP here, and that is the answer
        // even with another record behind it.
        assert_eq!(srv_choice(vec![srv(0, 0, 0, ".")]), None);
        assert_eq!(
            srv_choice(vec![srv(0, 0, 0, "."), srv(1, 0, 993, "imap.example.com.")]),
            None
        );
        assert_eq!(srv_choice(vec![]), None);
    }

    #[test]
    fn the_systems_addresses_are_each_given_once_and_judged_like_any_others() {
        // getaddrinfo, as iOS uses it; it answers on every system, so this
        // runs everywhere.
        rt().block_on(async {
            let local = system_addresses("localhost").await;
            assert!(!local.is_empty(), "localhost has no address");
            assert!(local.iter().all(IpAddr::is_loopback), "{local:?}");
            let mut once = local.clone();
            once.sort();
            once.dedup();
            assert_eq!(once.len(), local.len(), "{local:?}");
            assert_eq!(check_resolved(&local), HostVerdict::NotPublic);

            let missing = format!("nx-{}.invalid", std::process::id());
            assert!(system_addresses(&missing).await.is_empty());
        });
    }

    // ----------------------------- iOS: the system's resolver, real DNS
    //
    // These ask real DNS the way an iOS build does, through dns_sd and
    // getaddrinfo, so they need an Apple system and the network. They are
    // ignored by default; .github/workflows/mail-apple.yml runs them on
    // macOS with the `system-dns` feature, under which every other test in
    // this crate that looks a name up goes the same way too.
    #[cfg(system_dns)]
    mod through_the_system {
        use std::time::{Duration, Instant};

        use tokio::io::AsyncReadExt;

        use super::*;
        use crate::discover::MS_IMAP;
        use crate::guard::is_public;

        /// Well inside `SYSTEM_WAIT`: an answer that takes longer was waited
        /// out, not given.
        const QUICK: Duration = Duration::from_secs(5);

        fn system() -> Resolver {
            Resolver::system().expect("the system's resolver")
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn mx_records_come_from_the_system_lowest_preference_first() {
            rt().block_on(async {
                let r = system();
                for (domain, imap) in [
                    ("gmail.com", "imap.gmail.com"),
                    ("fastmail.com", "imap.fastmail.com"),
                ] {
                    let answers = r.mx_answers(domain).await;
                    assert!(answers.len() >= 2, "{domain}: {answers:?}");
                    let exchanges = r.mx(domain).await;
                    assert_eq!(exchanges.len(), answers.len(), "{domain}");
                    let preference = |e: &String| {
                        answers
                            .iter()
                            .find(|(_, x)| x == e)
                            .map(|(p, _)| *p)
                            .unwrap()
                    };
                    assert!(
                        exchanges
                            .windows(2)
                            .all(|w| preference(&w[0]) <= preference(&w[1])),
                        "{domain}: {answers:?}"
                    );
                    for e in &exchanges {
                        assert!(e.ends_with('.'), "{e}");
                        match mx_rule(e) {
                            Some(MxRule::Serves(h)) => assert_eq!(h.host, imap, "{e}"),
                            other => panic!("{domain}: {e}: {other:?}"),
                        }
                    }
                }
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn srv_records_come_from_the_system() {
            rt().block_on(async {
                let r = system();
                assert_eq!(
                    r.srv_imaps("gmail.com").await,
                    Some(("imap.gmail.com".to_string(), 993))
                );
                assert_eq!(
                    r.srv_imaps("fastmail.com").await,
                    Some(("imap.fastmail.com".to_string(), 993))
                );
                // fastmail.com's `_submission._tcp` names the root alone, RFC
                // 6186's way of saying there is no such service: read, and
                // taken as no service.
                let none = r.srv_answers("_submission._tcp.fastmail.com").await;
                assert!(none.iter().any(|s| s.target == "."), "{}", none.len());
                assert_eq!(srv_choice(none), None);
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn txt_records_come_from_the_system_whole() {
            rt().block_on(async {
                let r = system();
                // gmail.com's SPF record, and the one it redirects to.
                let txt = r.txt("gmail.com").await;
                let spf = spf_record(&txt)
                    .and_then(parse_spf)
                    .expect("gmail.com's SPF");
                let next = spf.redirect.expect("gmail.com's SPF redirects");
                assert!(spf_record(&r.txt(&next).await).is_some(), "{next}");
                // gmail.com publishes several, each its own reply: all of
                // them are gathered, not just the first.
                let (raw, ended) = dns_sd::asked("gmail.com", dns_sd::TXT).await;
                let all: Vec<String> = raw.iter().filter_map(|r| records::txt(r)).collect();
                assert!(
                    all.len() >= 2 && all.len() == raw.len() && ended == dns_sd::Ended::Answered,
                    "{} records ({} read), {ended:?}",
                    raw.len(),
                    all.len()
                );
                assert_eq!(spf_record(&all), spf_record(&txt));
                // A 2048-bit DKIM key is longer than a string's 255 bytes, so
                // it is published as several; they arrive as one record.
                let dkim = r.txt("google._domainkey.anthropic.com").await;
                assert!(
                    dkim.iter()
                        .any(|t| t.starts_with("v=DKIM1") && t.len() > 255 && !t.contains('"')),
                    "{dkim:?}"
                );
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn a_cname_comes_from_the_system() {
            rt().block_on(async {
                let r = system();
                // microsoft.com's autodiscover name is an alias for
                // Outlook's: L1's evidence of Microsoft 365.
                let alias = r.cname("autodiscover.microsoft.com").await;
                assert_eq!(alias.as_deref(), Some("autodiscover.outlook.com"));
                assert!(alias.as_deref().and_then(autodiscover_provider).is_some());
                // A name that exists but is no alias: nothing, at once.
                let started = Instant::now();
                assert_eq!(r.cname("gmail.com").await, None);
                assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn an_alias_on_the_way_is_followed_and_not_taken_for_the_answer() {
            rt().block_on(async {
                let r = system();
                // Fastmail's DKIM name is an alias for where the key is
                // kept: the TXT record at the end of it is the answer.
                let dkim = r.txt("fm1._domainkey.fastmail.com").await;
                assert!(dkim.iter().any(|t| t.starts_with("v=DKIM1")), "{dkim:?}");
                // autodiscover.microsoft.com is an alias of an alias of an
                // alias, with no TXT record at the end: the system hands back
                // each alias, then says there is none. Nothing, at once.
                let started = Instant::now();
                assert!(r.txt("autodiscover.microsoft.com").await.is_empty());
                assert!(started.elapsed() < QUICK, "{:?}", started.elapsed());
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn a_name_that_does_not_exist_answers_nothing_and_at_once() {
            rt().block_on(async {
                let r = system();
                let pid = std::process::id();
                for name in [format!("nx-{pid}.gmail.com"), format!("nx-{pid}.invalid")] {
                    // Each record type is answered "no such name", not
                    // waited out: fresh from the network the first time,
                    // from the system's cache the second, which is believed
                    // once nothing fresher comes (`dns_sd::SETTLE`).
                    for (what, rrtype, asked) in [
                        ("MX", dns_sd::MX, name.clone()),
                        ("SRV", dns_sd::SRV, format!("_imaps._tcp.{name}")),
                        ("TXT", dns_sd::TXT, name.clone()),
                        ("CNAME", dns_sd::CNAME, format!("autodiscover.{name}")),
                    ] {
                        for time in ["first", "second"] {
                            let t = Instant::now();
                            let (found, ended) = dns_sd::asked(&asked, rrtype).await;
                            let took = t.elapsed();
                            assert!(
                                found.is_empty()
                                    && ended == dns_sd::Ended::Answered
                                    && took < QUICK,
                                "{what} {asked}, {time} time: {} records, {ended:?} after {took:?}",
                                found.len()
                            );
                        }
                    }
                    assert!(r.mx(&name).await.is_empty(), "{name}");
                    assert_eq!(r.srv_imaps(&name).await, None, "{name}");
                    assert!(r.txt(&name).await.is_empty(), "{name}");
                    assert_eq!(r.cname(&name).await, None, "{name}");
                    let t = Instant::now();
                    assert_eq!(check_host(&r, &name).await, HostVerdict::NotFound);
                    let took = t.elapsed();
                    assert!(took < QUICK, "addresses of {name}: {took:?}");
                }
            });
        }

        #[test]
        #[ignore = "real DNS through dns_sd: run on macOS by mail-apple.yml"]
        fn discovery_runs_through_the_system_end_to_end() {
            rt().block_on(async {
                let r = system();
                let first = |found: Discovery| match found {
                    Discovery::Candidates { hosts, .. } => {
                        hosts.into_iter().next().expect("a candidate")
                    }
                    other => panic!("{other:?}"),
                };
                // Gmail and Fastmail addresses: the table knows them.
                for (addr, host) in [
                    ("someone@gmail.com", "imap.gmail.com"),
                    ("someone@fastmail.com", "imap.fastmail.com"),
                ] {
                    let c = first(discover(&r, addr, None).await);
                    assert_eq!((c.host.as_str(), c.port), (host, IMAP_PORT), "{addr}");
                }
                // Domains of theirs the table does not know, found by what
                // their DNS says: pobox.com's SRV names Fastmail's IMAP, and
                // anthropic.com's MX is Google's.
                let c = first(discover(&r, "someone@pobox.com", None).await);
                assert_eq!(
                    (c.host.as_str(), c.port, c.source),
                    ("imap.fastmail.com", 993, Source::Srv)
                );
                let c = first(discover(&r, "someone@anthropic.com", None).await);
                assert_eq!((c.host.as_str(), c.source), ("imap.gmail.com", Source::Mx));
                // L1's evidence, asked side by side: microsoft.com's
                // autodiscover alias names Microsoft 365.
                let found = found_elsewhere(&r, "microsoft.com").await;
                assert!(
                    found
                        .iter()
                        .any(|c| c.host == MS_IMAP && c.source == Source::Autodiscover),
                    "{found:?}"
                );
            });
        }

        #[test]
        #[ignore = "real DNS and IMAP: run on macOS by mail-apple.yml"]
        fn a_mail_server_is_reached_at_the_addresses_the_system_gives() {
            // The guarded dial from start to finish, with no password: the
            // system's addresses, each judged by the guard, a socket to one
            // of them, TLS checked against the name, and the server's
            // greeting.
            rt().block_on(async {
                let r = system();
                for host in ["imap.gmail.com", "imap.fastmail.com"] {
                    let (name, addrs) = resolve_public(&r, host).await.expect(host);
                    assert!(addrs.iter().all(|ip| is_public(*ip)), "{addrs:?}");
                    let tcp = crate::imap::dial(&addrs, IMAP_PORT, host)
                        .await
                        .expect(host);
                    let mut tls = crate::imap::wrap_tls(tcp, &name, host).await.expect(host);
                    let mut greeting = [0u8; 64];
                    let n = tokio::time::timeout(Duration::from_secs(20), tls.read(&mut greeting))
                        .await
                        .expect("a greeting in time")
                        .expect("a greeting");
                    assert!(
                        greeting[..n].starts_with(b"* OK"),
                        "{host}: {}",
                        String::from_utf8_lossy(&greeting[..n])
                    );
                }
            });
        }

        /// TEMPORARY, removed once read: how long getaddrinfo takes on the
        /// runner, in each of the ways the tests above reach it. It prints
        /// and never fails.
        #[test]
        #[ignore = "a probe for mail-apple.yml's diagnostics step"]
        fn probe_getaddrinfo_timings() {
            use std::net::ToSocketAddrs;
            let pid = std::process::id();
            let fresh = |n: u16| format!("nx-{pid}-{n}.gmail.com");
            let t = Instant::now();
            let got = (fresh(1).as_str(), 0)
                .to_socket_addrs()
                .map(Iterator::count);
            eprintln!(
                "PROBE std, no runtime, {}: {:?} {got:?}",
                fresh(1),
                t.elapsed()
            );
            rt().block_on(async {
                for name in [
                    fresh(2),
                    format!("{}.", fresh(3)),
                    "imap.gmail.com".to_string(),
                ] {
                    let t = Instant::now();
                    let got = system_addresses(&name).await;
                    eprintln!("PROBE system_addresses {name}: {:?} {got:?}", t.elapsed());
                }
                let name = fresh(4);
                let t = Instant::now();
                let got = tokio::net::lookup_host((name.as_str(), 0))
                    .await
                    .map(Iterator::count);
                eprintln!(
                    "PROBE lookup_host, no timeout, {name}: {:?} {got:?}",
                    t.elapsed()
                );
                for rrtype in [dns_sd::MX, dns_sd::TXT, dns_sd::CNAME] {
                    let name = fresh(100 + rrtype);
                    let _ = dns_sd::asked(&name, rrtype).await;
                    let t = Instant::now();
                    let got = system_addresses(&name).await;
                    eprintln!(
                        "PROBE system_addresses after dns_sd type {rrtype}, {name}: {:?} {got:?}",
                        t.elapsed()
                    );
                }
            });
        }
    }
}
