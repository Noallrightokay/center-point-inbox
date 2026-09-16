//! Asking DNS where a domain's mail actually lives.
//!
//! This is the half of discovery that needs the network, kept apart from the
//! tables and rules in [`crate::discover`] so those stay testable without one.
//!
//! Order matters and is deliberate: a domain's own SRV record, then its MX,
//! then the conventional names. Conventional names go *last* so a stale
//! `imap.<domain>` left over from an old setup cannot outrank what the MX
//! actually says today.

use std::net::IpAddr;

use hickory_resolver::{Resolver as HickoryResolver, TokioResolver, proto::rr::RData};

use crate::discover::{
    Candidate, IMAP_PORT, MxRule, Source, conventional, mx_rule, no_imap, table,
};
use crate::guard::{HostVerdict, check_literal, check_resolved, normalise};
use crate::key::domain_of;

/// A DNS resolver. Built once and shared — each one carries its own cache, so
/// making a fresh one per lookup would throw that away.
pub struct Resolver {
    inner: TokioResolver,
}

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

    /// MX records, lowest priority first — which is the order a sender would
    /// try them, and therefore the order most likely to name the real provider.
    pub async fn mx(&self, domain: &str) -> Vec<String> {
        // 0.26 returns a plain Lookup, and the rdata fields are public rather
        // than accessors.
        let mut records: Vec<(u16, String)> = match self.inner.mx_lookup(domain).await {
            Ok(r) => r
                .answers()
                .iter()
                .filter_map(|rec| match &rec.data {
                    RData::MX(mx) => Some((mx.preference, mx.exchange.to_utf8())),
                    _ => None,
                })
                .collect(),
            Err(_) => vec![],
        };
        records.sort_by_key(|(p, _)| *p);
        records.into_iter().map(|(_, e)| e).collect()
    }

    /// RFC 6186: a domain may publish where its IMAP is, and its port with it.
    /// Few do, but the ones that do are telling us the answer outright.
    pub async fn srv_imaps(&self, domain: &str) -> Option<(String, u16)> {
        let name = format!("_imaps._tcp.{domain}");
        let lookup = self.inner.srv_lookup(&name).await.ok()?;
        let mut best: Vec<_> = lookup
            .answers()
            .iter()
            .filter_map(|rec| match &rec.data {
                RData::SRV(srv) => Some(srv),
                _ => None,
            })
            .collect();
        // Lowest priority wins; among equals, the heaviest. Only the winner is
        // used — a second SRV record is a fallback for a mail client that
        // cannot find the first, and RATA has the MX and the conventional names
        // behind it for that.
        best.sort_by_key(|s| (s.priority, std::cmp::Reverse(s.weight)));
        let first = best.first()?;
        let target = first.target.to_utf8();
        let target = target.trim_end_matches('.').to_string();
        // A root target means "explicitly no service here", which is an answer.
        if target.is_empty() {
            return None;
        }
        Some((target, first.port))
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

/// Everything worth trying for an address, best first.
pub async fn discover(resolver: &Resolver, email: &str, host_override: Option<&str>) -> Discovery {
    let domain = domain_of(email);

    if let Some(given) = host_override.and_then(normalise) {
        return Discovery::Candidates {
            hosts: vec![Candidate {
                host: given,
                port: IMAP_PORT,
                label: domain,
                help: None,
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

    if let Some((target, port)) = resolver.srv_imaps(&domain).await {
        hosts.push(Candidate {
            host: target,
            port,
            label: domain.clone(),
            help: None,
            source: Source::Srv,
        });
    }

    for exchange in resolver.mx(&domain).await {
        match mx_rule(&exchange) {
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

    Discovery::Candidates { hosts, filtered_by }
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
}
