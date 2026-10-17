//! Where RATA is willing to open a socket.
//!
//! The mail server is chosen by the user: guessed from their domain, published
//! in their domain's MX or SRV records, or typed into the "server address"
//! box. All three mean an address supplied from outside decides where a socket
//! opens — and on the desktop that socket opens from *inside the customer's own
//! network*, behind their router, next to their NAS and their printer and
//! whatever else is on their LAN.
//!
//! That is a sharper problem than it was on the server. A hosted RATA reaching
//! 192.168.1.1 reaches its own datacentre; the desktop app reaching it reaches
//! the customer's home router. So every hostname is resolved first and refused
//! if any address it answers with is loopback, link-local, private, carrier-
//! grade NAT or otherwise not on the public internet.
//!
//! The JavaScript this replaces had a bug worth remembering: it matched IPv6
//! addresses as *text*, so `::ffff:127.0.0.1` was blocked while `::ffff:7f00:1`
//! — the identical address written in hex — was not. Here `Ipv6Addr` parses
//! every spelling to the same sixteen bytes before anything is decided, so that
//! class of bug cannot recur. Canonicalise, then judge.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// The loopback tests (`tests/loopback.rs`) need RATA to open a socket to a real
// mail server on this machine, which is exactly what this module exists to
// refuse. So the one exception is a feature that cannot reach a customer: a
// release build with it on does not compile, and a debug build without it is
// unchanged. See `loopback_test_server`. This gate is on debug assertions,
// which a release profile can switch on; `build.rs` closes that by refusing
// the feature whenever the profile is release (SEC-5, review row 5). Keep both.
#[cfg(all(feature = "loopback-tests", not(debug_assertions)))]
compile_error!(
    "the loopback-tests feature lets the outbound guard connect to 127.0.0.1 and is for debug test builds only; it must never be on in a release build"
);

/// Why a host was refused. `NotFound` is separate from the rest because it
/// means "try the next candidate", not "stop".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostVerdict {
    Allowed,
    NotPublic,
    Malformed,
    NotFound,
}

impl HostVerdict {
    pub fn is_allowed(&self) -> bool {
        matches!(self, HostVerdict::Allowed)
    }
    /// The sentence to show a customer. Written for someone who typed a server
    /// address and got it wrong, not for an attacker.
    pub fn explain(&self, host: &str) -> String {
        match self {
            HostVerdict::Allowed => String::new(),
            HostVerdict::NotPublic => format!(
                "{host} is not a public mail server — it points inside a private network, and RATA will not connect there."
            ),
            HostVerdict::Malformed => format!("\"{host}\" is not a valid mail server address."),
            HostVerdict::NotFound => format!("No mail server answers at {host}."),
        }
    }
}

/// Is this address somewhere a mail server could legitimately live?
///
/// A mail server the whole world has to reach is public by definition, so
/// nothing legitimate is lost by refusing everything else.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_public(v4),
        IpAddr::V6(v6) => v6_public(v6),
    }
}

fn v4_public(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_loopback()            // 127/8
        || ip.is_private()        // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local()     // 169.254/16, which carries cloud metadata
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || a == 0                 // "this network"
        || a >= 240               // reserved, and 255.255.255.255 with it
        || (a == 100 && (64..=127).contains(&b))   // carrier-grade NAT
        || (a == 192 && b == 0)                    // IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19))) // benchmarking
}

fn v6_public(ip: Ipv6Addr) -> bool {
    // An IPv4 address wearing an IPv6 coat is still that IPv4 address, and on a
    // dual-stack host connecting to it reaches the IPv4 one. Unwrap before
    // judging — every embedding below carries a v4 address inside it.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_public(v4);
    }
    let seg = ip.segments();

    // ::a.b.c.d — the deprecated IPv4-compatible form, still routable text.
    if seg[..6] == [0, 0, 0, 0, 0, 0] && !ip.is_unspecified() && !ip.is_loopback() {
        return v4_public(Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            seg[6] as u8,
            (seg[7] >> 8) as u8,
            seg[7] as u8,
        ));
    }
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        // 64:ff9b::/96 — NAT64's well-known prefix carries its v4 in the last
        // two groups. On an IPv6-only network with DNS64 this is the only
        // answer an IPv4-only mail host has, and the gateway connects to the
        // v4 inside, so that is what is judged.
        return v4_public(Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            seg[6] as u8,
            (seg[7] >> 8) as u8,
            seg[7] as u8,
        ));
    }
    if seg[0] == 0x64 && seg[1] == 0xff9b {
        // The rest of 64:ff9b::/32, the local-use 64:ff9b:1::/48 among it
        // (RFC 8215): where its v4 sits depends on the operator's prefix
        // length, so there is nothing to judge.
        return false;
    }
    if seg[0] == 0x2002 {
        // 2002::/16 — 6to4 carries its v4 in the next two groups
        return v4_public(Ipv4Addr::new(
            (seg[1] >> 8) as u8,
            seg[1] as u8,
            (seg[2] >> 8) as u8,
            seg[2] as u8,
        ));
    }
    if seg[0] & 0xfe00 == 0xfc00 {
        return false; // fc00::/7 unique local
    }
    if seg[0] & 0xffc0 == 0xfe80 {
        return false; // fe80::/10 link local
    }
    true
}

/// Names that never belong to a public mail server, whatever DNS says about
/// them. Checked before any lookup so a resolver that helpfully answers for
/// `localhost` cannot get a word in. The zones themselves count too, and so
/// does all of `.arpa` (the reverse zones and `home.arpa`): no mail server is
/// named there, and on Apple systems the system's resolver answers `local`
/// and the link-local reverse zones by asking the local network.
fn is_private_name(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    ["localhost", "local", "internal", "arpa"]
        .iter()
        .any(|zone| h == *zone || h.ends_with(&format!(".{zone}")))
}

/// Normalise a host as typed, and reject anything that is not a plausible
/// hostname or IP literal before it reaches a resolver.
pub fn normalise(host: &str) -> Option<String> {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() || h.len() > 253 {
        return None;
    }
    if !h
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
    {
        return None;
    }
    Some(h)
}

/// Judge a host without touching the network. Returns `None` when the host is
/// a name rather than a literal and therefore has to be resolved.
pub fn check_literal(host: &str) -> Option<HostVerdict> {
    let h = match normalise(host) {
        Some(h) => h,
        None => return Some(HostVerdict::Malformed),
    };
    if is_private_name(&h) {
        return Some(HostVerdict::NotPublic);
    }
    let bare = h.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<IpAddr>() {
        Ok(ip) if loopback_test_server(ip) => Some(HostVerdict::Allowed),
        Ok(ip) => Some(if is_public(ip) {
            HostVerdict::Allowed
        } else {
            HostVerdict::NotPublic
        }),
        // Not a literal — the caller has to resolve it.
        Err(_) => None,
    }
}

/// Whether `ip` is the loopback tests' own mail server: `127.0.0.1` written as
/// exactly that, and only in a debug build with the `loopback-tests` feature.
/// Always false otherwise — the release build cannot even be compiled with the
/// feature on (see the `compile_error!` at the top of this file, and
/// `build.rs`).
///
/// Deliberately narrow. Not the rest of 127/8, not `::1`, not
/// `::ffff:127.0.0.1` or any other spelling, not `localhost` (refused as a
/// name before this is asked), and never a name that *resolves* to 127.0.0.1:
/// only a literal typed as the server address, so a hostile DNS answer still
/// cannot reach the loopback interface even in a test build.
#[cfg(all(feature = "loopback-tests", debug_assertions))]
fn loopback_test_server(ip: IpAddr) -> bool {
    ip == IpAddr::V4(Ipv4Addr::LOCALHOST)
}

#[cfg(not(all(feature = "loopback-tests", debug_assertions)))]
fn loopback_test_server(_ip: IpAddr) -> bool {
    false
}

/// Judge a set of resolved addresses. Empty means nothing answered.
pub fn check_resolved(addrs: &[IpAddr]) -> HostVerdict {
    if addrs.is_empty() {
        return HostVerdict::NotFound;
    }
    // Every address, not the first: a name that answers with one public and one
    // private address is a name that can be made to resolve either way.
    if addrs.iter().copied().all(is_public) {
        HostVerdict::Allowed
    } else {
        HostVerdict::NotPublic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> HostVerdict {
        check_literal(s).expect("literal")
    }

    #[test]
    fn the_ipv6_spellings_that_defeated_the_javascript() {
        // Every one of these is 127.0.0.1, 169.254.169.254 or an RFC1918
        // address wearing a different hat. The JS guard matched text and let
        // most of them through; this parses them to the same bytes.
        for same in [
            "::ffff:127.0.0.1",
            "::ffff:7f00:1",
            "0:0:0:0:0:ffff:127.0.0.1",
            "::ffff:a9fe:a9fe",
            "::ffff:c0a8:1",
            "::ffff:0a00:0001",
            "64:ff9b::7f00:1",
            "2002:7f00:1::1",
            "::0.0.0.0",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert_eq!(v(same), HostVerdict::NotPublic, "{same} should be refused");
        }
    }

    #[test]
    fn real_public_addresses_are_allowed() {
        for ok in [
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "[2606:4700:4700::1111]",
        ] {
            assert_eq!(v(ok), HostVerdict::Allowed, "{ok} should be allowed");
        }
    }

    #[test]
    fn the_v4_ranges() {
        for bad in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.4.4",
            "172.31.255.255",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "240.0.0.1",
        ] {
            assert_eq!(v(bad), HostVerdict::NotPublic, "{bad} should be refused");
        }
        // 172.15 and 172.32 sit either side of the private block and are public.
        assert_eq!(v("172.15.0.1"), HostVerdict::Allowed);
        assert_eq!(v("172.32.0.1"), HostVerdict::Allowed);
        assert_eq!(v("100.63.0.1"), HostVerdict::Allowed);
        assert_eq!(v("100.128.0.1"), HostVerdict::Allowed);
    }

    /// `v4` as DNS64 synthesises it: the well-known NAT64 prefix,
    /// `64:ff9b::/96`, with the IPv4 address in the last 32 bits.
    fn nat64(v4: &str) -> IpAddr {
        let [a, b, c, d] = v4.parse::<Ipv4Addr>().unwrap().octets();
        IpAddr::V6(Ipv6Addr::new(
            0x64,
            0xff9b,
            0,
            0,
            0,
            0,
            u16::from_be_bytes([a, b]),
            u16::from_be_bytes([c, d]),
        ))
    }

    #[test]
    fn nat64_is_judged_by_the_ipv4_address_inside() {
        // On an IPv6-only network with DNS64 (Apple's app review network,
        // T-Mobile US) an IPv4-only mail host answers only like this, and the
        // gateway connects to the IPv4 address inside.
        for ok in ["64:ff9b::808:808", "64:ff9b::8.8.8.8", "[64:ff9b::101:101]"] {
            assert_eq!(v(ok), HostVerdict::Allowed, "{ok} should be allowed");
        }
        // 127.0.0.1, 10.0.0.1, 169.254.169.254, 100.64.0.1, 192.168.1.1.
        for bad in [
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b::6440:1",
            "64:ff9b::c0a8:101",
            "64:ff9b::127.0.0.1",
        ] {
            assert_eq!(v(bad), HostVerdict::NotPublic, "{bad} should be refused");
        }
        // Exactly as the IPv4 address itself is judged, every range the
        // guard refuses and the public addresses either side of them.
        for v4 in [
            "127.0.0.1",
            "127.255.255.254",
            "10.0.0.1",
            "172.16.4.4",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "198.18.0.1",
            "198.19.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "198.51.100.1",
            "203.0.113.1",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "8.8.8.8",
            "1.1.1.1",
            "172.15.0.1",
            "172.32.0.1",
            "100.63.0.1",
            "100.128.0.1",
            "192.1.0.1",
            "198.20.0.1",
        ] {
            let ip: IpAddr = v4.parse().unwrap();
            assert_eq!(is_public(nat64(v4)), is_public(ip), "{}", nat64(v4));
        }
    }

    #[test]
    fn the_rest_of_the_nat64_block_is_still_refused() {
        // 64:ff9b:1::/48 is for an operator's own NAT64 (RFC 8215), where the
        // IPv4 address sits wherever that operator's prefix length puts it,
        // so there is nothing to judge. The rest of 64:ff9b::/32 is not
        // assigned, and a /96 with anything but zeros before the IPv4 address
        // is not the well-known prefix.
        for bad in [
            "64:ff9b:1::808:808",
            "64:ff9b:1:808:808::",
            "64:ff9b:1:ffff:ffff:ffff:ffff:ffff",
            "64:ff9b:2::808:808",
            "64:ff9b::1:808:808",
            "64:ff9b:0:0:1::808:808",
        ] {
            assert_eq!(v(bad), HostVerdict::NotPublic, "{bad} should be refused");
        }
    }

    #[test]
    fn the_other_ipv4_embeddings_are_judged_as_before() {
        // IPv4-mapped (::ffff:0:0/96) and the deprecated IPv4-compatible
        // (::/96) forms are judged by the IPv4 address inside; `::` and `::1`
        // are themselves, never 0.0.0.0 and 0.0.0.1.
        for ok in ["::ffff:808:808", "::ffff:8.8.8.8", "::808:808"] {
            assert_eq!(v(ok), HostVerdict::Allowed, "{ok} should be allowed");
        }
        for bad in ["::ffff:a00:1", "::a00:1", "::7f00:1", "::a9fe:a9fe", "::1"] {
            assert_eq!(v(bad), HostVerdict::NotPublic, "{bad} should be refused");
        }
    }

    #[test]
    fn a_nat64_answer_is_judged_like_any_other() {
        // What a hostile resolver can say about a name: one wrapped private
        // address condemns it, as a plain one does.
        let public = nat64("8.8.8.8");
        assert_eq!(check_resolved(&[public]), HostVerdict::Allowed);
        for inside in ["127.0.0.1", "10.0.0.1", "169.254.169.254", "100.64.0.1"] {
            assert_eq!(
                check_resolved(&[public, nat64(inside)]),
                HostVerdict::NotPublic,
                "{inside}"
            );
            assert_eq!(check_resolved(&[nat64(inside)]), HostVerdict::NotPublic);
        }
    }

    #[test]
    fn names_that_are_never_public() {
        for bad in [
            "localhost",
            "LOCALHOST",
            "db.internal",
            "nas.local",
            "x.home.arpa",
            "a.localhost",
            "local",
            "internal",
            "home.arpa",
            "1.0.254.169.in-addr.arpa",
            "b.e.f.ip6.arpa",
            "arpa",
        ] {
            assert_eq!(v(bad), HostVerdict::NotPublic, "{bad} should be refused");
        }
    }

    #[test]
    fn a_hostname_is_not_judged_without_resolving_it() {
        assert!(check_literal("imap.gmail.com").is_none());
        assert!(check_literal("mail.example.co.uk").is_none());
    }

    #[test]
    fn rubbish_is_refused_rather_than_guessed_at() {
        for bad in [
            "not a host name",
            "",
            "   ",
            "x@y.z",
            "http://imap.gmail.com",
            &"a".repeat(300),
        ] {
            assert_eq!(
                v(bad),
                HostVerdict::Malformed,
                "{bad:?} should be malformed"
            );
        }
    }

    #[test]
    fn one_private_answer_condemns_the_whole_name() {
        let pubk: IpAddr = "8.8.8.8".parse().unwrap();
        let priv_: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(check_resolved(&[pubk]), HostVerdict::Allowed);
        assert_eq!(check_resolved(&[pubk, priv_]), HostVerdict::NotPublic);
        assert_eq!(check_resolved(&[]), HostVerdict::NotFound);
    }

    #[test]
    fn a_refusal_says_something_a_person_can_act_on() {
        let msg = HostVerdict::NotPublic.explain("192.168.1.1");
        assert!(msg.contains("private network"), "{msg}");
        assert!(!HostVerdict::NotFound.explain("x.invalid").is_empty());
    }
}
