//! What the system's resolver is asked, and reading what it answers (iOS;
//! see `crate::dns_sd`).
//!
//! dns_sd hands each answer's rdata exactly as it came off the wire, except
//! that a name inside it is written out in full rather than compressed. The
//! readers here are plain Rust over those bytes, so they are tested on every
//! platform. Each refuses rdata that is cut short, runs on past its record,
//! or names a host no mail server could have, rather than guessing at it,
//! and none reads past the end of what it was given.
//!
//! Each gives what hickory's records give on the desktop, so `resolve` treats
//! both alike: a name in text with its final dot (`imap.gmail.com.`, the root
//! as `.`), and a TXT record's strings joined as RFC 7208 §3.3 says.

use crate::guard::{HostVerdict, check_literal};
use crate::resolve::Srv;

/// The longest name DNS carries, in wire form with its final zero byte
/// (RFC 1035 §3.1).
const NAME_MAX: usize = 255;

/// The longest label (RFC 1035 §2.3.4). A length byte above this is either a
/// pointer back into a message (compression) or a label type nobody uses,
/// and dns_sd hands neither.
const LABEL_MAX: usize = 63;

/// A name as the system's resolver should be asked it. dns_sd sends a name's
/// bytes as they are, so an international domain is spelt the way DNS holds
/// it (`xn--…`), as hickory spells it on the desktop. ammonia already carries
/// the url crate's IDNA, so nothing new is built for it.
///
/// `None` for a name that has no such spelling, and for one that is not to
/// be asked at all (`unasked`).
pub(crate) fn question(name: &str) -> Option<String> {
    let ascii = if name.is_ascii() {
        name.to_string()
    } else {
        match ammonia::url::Host::parse(name) {
            Ok(ammonia::url::Host::Domain(ascii)) => ascii,
            _ => return None,
        }
    };
    (!unasked(&ascii)).then_some(ascii)
}

/// The domains that are never asked about, each with everything under it:
/// the outbound guard's private names (`crate::guard::check_literal`), and
/// every domain Apple's resolver answers by multicast DNS on the local
/// network rather than by asking DNS (`local`, and the link-local reverse
/// zones 169.254/16 and fe80::/10).
const UNASKED: [&str; 9] = [
    "local",
    "localhost",
    "internal",
    "home.arpa",
    "254.169.in-addr.arpa",
    "8.e.f.ip6.arpa",
    "9.e.f.ip6.arpa",
    "a.e.f.ip6.arpa",
    "b.e.f.ip6.arpa",
];

/// Whether `name` is not to be asked at all.
///
/// No public mail server is under any of `UNASKED`, so no answer about one
/// could be used, and every server discovery finds there is refused by the
/// guard anyway. And on Apple's systems a question there goes out by
/// multicast DNS to the local network, which iOS asks the customer's leave
/// for, and which never says a name does not exist, so each question would
/// wait out all of `SYSTEM_WAIT`. Nor is a name with anything in it but
/// letters, digits, `.`, `-` and `_`: dns_sd reads a `\` in a name as an
/// escape (`loca\l` is `local`, and so is `loca\108`), and no mail server's
/// name has anything else. The desktop, which asks hickory, is unchanged.
///
/// The whole name is judged, so a name that is only a service label over a
/// suffix (`_spf.local`) is caught. Then the guard judges it as a host name
/// too, and the domain under its service labels (`_imaps._tcp.`), which
/// catches a private address written as a name (`10.0.0.1`,
/// `_imaps._tcp.192.168.1.1`). The guard reads host names, which have no
/// `_`, so for it a label holding one (`_imaps`, `a_b`) is a plain label.
fn unasked(name: &str) -> bool {
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return true;
    }
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let under = |domain: &str| {
        name == domain
            || name
                .strip_suffix(domain)
                .is_some_and(|start| start.ends_with('.'))
    };
    if UNASKED.iter().any(|d| under(d)) {
        return true;
    }
    // A name to look up (`None`), or a public address written as one.
    let refused = |host: &str| !matches!(check_literal(host), None | Some(HostVerdict::Allowed));
    let labels: Vec<&str> = name.split('.').collect();
    let host: Vec<&str> = labels
        .iter()
        .map(|label| if label.contains('_') { "x" } else { label })
        .collect();
    if refused(&host.join(".")) {
        return true;
    }
    let service = labels.iter().rposition(|l| l.contains('_'));
    service.is_some_and(|last| last + 1 < labels.len() && refused(&labels[last + 1..].join(".")))
}

/// An MX record (RFC 1035 §3.3.9): its preference and its exchange.
pub(crate) fn mx(rdata: &[u8]) -> Option<(u16, String)> {
    let preference = u16_at(rdata, 0)?;
    let (exchange, used) = name(rdata.get(2..)?)?;
    (2 + used == rdata.len()).then_some((preference, exchange))
}

/// An SRV record (RFC 2782).
pub(crate) fn srv(rdata: &[u8]) -> Option<Srv> {
    let priority = u16_at(rdata, 0)?;
    let weight = u16_at(rdata, 2)?;
    let port = u16_at(rdata, 4)?;
    let (target, used) = name(rdata.get(6..)?)?;
    (6 + used == rdata.len()).then_some(Srv {
        priority,
        weight,
        port,
        target,
    })
}

/// A TXT record (RFC 1035 §3.3.14): one or more strings, each a length byte
/// and that many bytes, joined into one as hickory's are on the desktop.
pub(crate) fn txt(rdata: &[u8]) -> Option<String> {
    if rdata.is_empty() {
        return None;
    }
    let mut text = String::new();
    let mut rest = rdata;
    while let Some((&len, after)) = rest.split_first() {
        let (part, next) = after.split_at_checked(usize::from(len))?;
        text.push_str(&String::from_utf8_lossy(part));
        rest = next;
    }
    Some(text)
}

/// A CNAME record: the name this one is an alias for.
pub(crate) fn cname(rdata: &[u8]) -> Option<String> {
    let (target, used) = name(rdata)?;
    (used == rdata.len()).then_some(target)
}

/// A big-endian `u16` at `at`, if there are two bytes there.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes(pair.try_into().ok()?))
}

/// A name written out in full at the start of `bytes`: its text, ending in a
/// dot, and how many bytes it took.
///
/// Only letters, digits, `-` and `_` are read in a label. Every name RATA
/// asks about is a host name (`_` for the `_imaps._tcp` kind); a label with
/// anything else in it, a dot or a space or a byte that is not ASCII, could
/// only be turned into text by escaping it, and no mail server is named that
/// way. So the record is refused instead, which leaves discovery to its next
/// answer.
fn name(bytes: &[u8]) -> Option<(String, usize)> {
    let mut text = String::new();
    let mut at = 0;
    loop {
        let len = usize::from(*bytes.get(at)?);
        at += 1;
        if len == 0 {
            break;
        }
        if len > LABEL_MAX {
            return None;
        }
        let label = bytes.get(at..at + len)?;
        if !label
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return None;
        }
        at += len;
        // Room is still needed for the final zero byte.
        if at + 1 > NAME_MAX {
            return None;
        }
        text.extend(label.iter().map(|&b| char::from(b)));
        text.push('.');
    }
    if text.is_empty() {
        text.push('.');
    }
    Some((text, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name in wire form, from its labels.
    fn wire(labels: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for l in labels {
            out.push(u8::try_from(l.len()).unwrap());
            out.extend_from_slice(l.as_bytes());
        }
        out.push(0);
        out
    }

    fn srv_rdata(priority: u16, weight: u16, port: u16, target: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&priority.to_be_bytes());
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&port.to_be_bytes());
        out.extend(wire(target));
        out
    }

    fn mx_rdata(preference: u16, exchange: &[&str]) -> Vec<u8> {
        let mut out = preference.to_be_bytes().to_vec();
        out.extend(wire(exchange));
        out
    }

    fn txt_rdata(strings: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for s in strings {
            out.push(u8::try_from(s.len()).unwrap());
            out.extend_from_slice(s);
        }
        out
    }

    #[test]
    fn an_srv_record_is_read_as_dns_sd_hands_it() {
        // _imaps._tcp.gmail.com, byte for byte: 5 0 993 imap.gmail.com.
        let rdata = [
            0, 5, 0, 0, 0x03, 0xe1, 4, b'i', b'm', b'a', b'p', 5, b'g', b'm', b'a', b'i', b'l', 3,
            b'c', b'o', b'm', 0,
        ];
        let s = srv(&rdata).expect("srv");
        assert_eq!(
            (s.priority, s.weight, s.port, s.target.as_str()),
            (5, 0, 993, "imap.gmail.com.")
        );

        let s = srv(&srv_rdata(0, 1, 993, &["imap", "fastmail", "com"])).unwrap();
        assert_eq!(
            (s.priority, s.weight, s.port, s.target.as_str()),
            (0, 1, 993, "imap.fastmail.com.")
        );
        // "No service here" is a target of the root alone, and it is read,
        // not refused: it is an answer (`resolve::srv_choice`).
        let s = srv(&srv_rdata(0, 0, 0, &[])).unwrap();
        assert_eq!(s.target, ".");
        // The highest numbers there are.
        let s = srv(&srv_rdata(65535, 65535, 65535, &["a"])).unwrap();
        assert_eq!((s.priority, s.weight, s.port), (65535, 65535, 65535));
    }

    #[test]
    fn an_mx_record_is_read_as_dns_sd_hands_it() {
        let rdata = mx_rdata(10, &["alt1", "gmail-smtp-in", "l", "google", "com"]);
        assert_eq!(
            mx(&rdata),
            Some((10, "alt1.gmail-smtp-in.l.google.com.".to_string()))
        );
        // Case is kept, as hickory keeps it; `discover` lowercases.
        assert_eq!(
            mx(&mx_rdata(0, &["IN1-SMTP", "MessagingEngine", "com"])),
            Some((0, "IN1-SMTP.MessagingEngine.com.".to_string()))
        );
        // A null MX (RFC 7505) names the root.
        assert_eq!(mx(&mx_rdata(0, &[])), Some((0, ".".to_string())));
    }

    #[test]
    fn a_txt_records_strings_are_joined_into_one() {
        assert_eq!(
            txt(&txt_rdata(&[b"v=spf1 include:_spf.google.com ~all"])).as_deref(),
            Some("v=spf1 include:_spf.google.com ~all")
        );
        // A record longer than one string's 255 bytes is published as
        // several, and they are one record (RFC 7208 §3.3).
        let long = "a".repeat(255);
        let rdata = txt_rdata(&[long.as_bytes(), b" include:_spf.google.com -all"]);
        assert_eq!(
            txt(&rdata),
            Some(format!("{long} include:_spf.google.com -all"))
        );
        assert_eq!(
            txt(&txt_rdata(&[b"v=spf1 ", b"", b"-all"])).as_deref(),
            Some("v=spf1 -all")
        );
        // One empty string is a record with nothing in it.
        assert_eq!(txt(&[0]).as_deref(), Some(""));
        // Bytes that are not UTF-8 are replaced, string by string, as
        // hickory's are on the desktop.
        assert_eq!(
            txt(&txt_rdata(&[b"v=spf1 \xff", b"-all"])).as_deref(),
            Some("v=spf1 \u{fffd}-all")
        );
    }

    #[test]
    fn a_cname_record_is_read_as_dns_sd_hands_it() {
        assert_eq!(
            cname(&wire(&["autodiscover", "outlook", "com"])).as_deref(),
            Some("autodiscover.outlook.com.")
        );
    }

    #[test]
    fn rdata_that_is_cut_short_is_refused() {
        // Every proper prefix of a record that holds a name is too short:
        // a number or a label cut, or the final zero byte missing.
        let whole = [
            srv_rdata(5, 0, 993, &["imap", "gmail", "com"]),
            mx_rdata(10, &["aspmx", "l", "google", "com"]),
            wire(&["autodiscover", "outlook", "com"]),
        ];
        for cut in 0..whole[0].len() {
            assert!(srv(&whole[0][..cut]).is_none(), "srv cut at {cut}");
        }
        for cut in 0..whole[1].len() {
            assert!(mx(&whole[1][..cut]).is_none(), "mx cut at {cut}");
        }
        for cut in 0..whole[2].len() {
            assert!(cname(&whole[2][..cut]).is_none(), "cname cut at {cut}");
        }
        // A TXT string whose length runs past the end, and no strings at all.
        assert_eq!(txt(&[]), None);
        assert_eq!(txt(&[5, b'v', b'=']), None);
        assert_eq!(txt(&[3, b'a', b'b', b'c', 1]), None);
        assert_eq!(txt(&[255]), None);
    }

    #[test]
    fn rdata_that_runs_on_past_its_record_is_refused() {
        let mut s = srv_rdata(5, 0, 993, &["imap", "gmail", "com"]);
        s.push(0);
        assert!(srv(&s).is_none());
        let mut m = mx_rdata(10, &["aspmx", "l", "google", "com"]);
        m.extend_from_slice(b"\x03com\x00");
        assert!(mx(&m).is_none());
        let mut c = wire(&["autodiscover", "outlook", "com"]);
        c.push(7);
        assert!(cname(&c).is_none());
    }

    #[test]
    fn a_name_dns_sd_would_never_hand_over_is_refused() {
        // A compression pointer: dns_sd writes names out in full, and one
        // pointing back into a message RATA does not have means nothing.
        assert_eq!(mx(&[0, 10, 0xc0, 0x0c]), None);
        assert_eq!(mx(&[0, 10, 3, b'c', b'o', b'm', 0xc0, 0x0c]), None);
        assert_eq!(cname(&[0xc0, 0x0c]), None);
        // The label types nobody uses, and a label longer than 63 bytes.
        assert_eq!(cname(&[0x40, 0]), None);
        assert_eq!(cname(&[0x80, 0]), None);
        let mut long = vec![64];
        long.extend(std::iter::repeat_n(b'a', 64));
        long.push(0);
        assert_eq!(cname(&long), None);
        // 63 is allowed.
        let mut most = vec![63];
        most.extend(std::iter::repeat_n(b'a', 63));
        most.push(0);
        assert_eq!(cname(&most).map(|n| n.len()), Some(64));
    }

    #[test]
    fn a_name_longer_than_dns_allows_is_refused() {
        let l63 = "a".repeat(63);
        let l61 = "b".repeat(61);
        // 3 × (1 + 63) + (1 + 61) + 1 = 255 bytes: the longest name there is.
        let longest = wire(&[&l63, &l63, &l63, &l61]);
        assert_eq!(longest.len(), 255);
        assert_eq!(cname(&longest).map(|n| n.len()), Some(253 + 1));
        // One byte more is not a name.
        let l62 = "b".repeat(62);
        let over = wire(&[&l63, &l63, &l63, &l62]);
        assert_eq!(over.len(), 256);
        assert_eq!(cname(&over), None);
        assert_eq!(srv(&srv_rdata(0, 0, 993, &[&l63, &l63, &l63, &l62])), None);
        // And far more, in labels that are each allowed.
        let many: Vec<&str> = std::iter::repeat_n("x", 300).collect();
        assert_eq!(cname(&wire(&many)), None);
    }

    #[test]
    fn a_label_no_host_could_have_is_refused() {
        for bad in [
            &b"a b"[..],
            b"a.b",
            b"a\0b",
            b"caf\xc3\xa9",
            b"*",
            b"a/b",
            b"a\"b",
            b"a\\b",
            b"\x7f",
        ] {
            let mut rdata = vec![u8::try_from(bad.len()).unwrap()];
            rdata.extend_from_slice(bad);
            rdata.extend_from_slice(b"\x03com\x00");
            assert_eq!(cname(&rdata), None, "{bad:?}");
            let mut m = vec![0, 10];
            m.extend_from_slice(&rdata);
            assert_eq!(mx(&m), None, "{bad:?}");
        }
        // Underscores and hyphens are how service and host names are spelt.
        assert_eq!(
            cname(&wire(&["_imaps", "_tcp", "mail-1", "example", "com"])).as_deref(),
            Some("_imaps._tcp.mail-1.example.com.")
        );
    }

    #[test]
    fn no_rdata_whatever_its_bytes_makes_a_reader_panic() {
        // Every reader over a few hundred thousand byte strings, most of them
        // nonsense: each answers or refuses, and none panics or reads past
        // the end (which in Rust would be a panic too).
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..200_000 {
            let len = (next() % 64) as usize;
            let mut bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            // Length bytes that are small often enough to make labels.
            if round % 2 == 0 {
                for b in bytes.iter_mut().step_by(3) {
                    *b %= 12;
                }
            }
            let _ = srv(&bytes);
            let _ = mx(&bytes);
            let _ = txt(&bytes);
            let _ = cname(&bytes);
        }
    }

    #[test]
    fn an_international_domain_is_asked_as_dns_spells_it() {
        assert_eq!(question("gmail.com").as_deref(), Some("gmail.com"));
        assert_eq!(
            question("_imaps._tcp.example.com").as_deref(),
            Some("_imaps._tcp.example.com")
        );
        assert_eq!(question("bücher.de").as_deref(), Some("xn--bcher-kva.de"));
        assert_eq!(
            question("_imaps._tcp.bücher.de").as_deref(),
            Some("_imaps._tcp.xn--bcher-kva.de")
        );
        assert_eq!(
            question("autodiscover.münchen.de").as_deref(),
            Some("autodiscover.xn--mnchen-3ya.de")
        );
        // A name with no such spelling is not asked at all.
        assert_eq!(question("bü cher.de"), None);
        assert_eq!(question("bücher/de"), None);
    }

    #[test]
    fn a_name_the_guard_refuses_is_never_asked() {
        // Every question discovery could make about an address at a
        // `.local` domain, which Apple's resolver would send by multicast
        // DNS, and the guard's other private names.
        for name in [
            "acme.local",
            "ACME.Local.",
            "_imaps._tcp.acme.local",
            "autodiscover.acme.local",
            "google._domainkey.acme.local",
            "_spf.mail.acme.local",
            "printer.local",
            "bücher.local",
            "_imaps._tcp.bücher.local",
            "localhost",
            "mail.localhost",
            "corp.internal",
            "_imaps._tcp.corp.internal",
            "nas.home.arpa",
            "10.0.0.1",
            "192.168.1.1",
            // A service label straight over the suffix, and a host label
            // with `_` in it: the whole name is judged, not what is under
            // the last `_`.
            "_spf.local",
            "_dmarc.internal",
            "_dmarc.localhost",
            "_x.home.arpa",
            "a_b.local",
            "acme_corp.local",
            "_imaps._tcp.a_b.local",
            "a_b.home.arpa",
            "corp_x.internal",
            "_spf.10.0.0.1",
            "_imaps._tcp.192.168.1.1",
            // The suffixes alone, in any case, with or without a final dot.
            "local",
            "LOCAL.",
            "internal",
            "home.arpa",
            "localhost.",
            // The link-local reverse zones, answered by multicast DNS too.
            "1.0.254.169.in-addr.arpa",
            "254.169.in-addr.arpa",
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.e.f.ip6.arpa",
            "b.e.f.ip6.arpa",
            // What dns_sd would read as an escape: `\l` is `l`, `\108` is
            // `l`, `\046` is a dot inside a label.
            "acme.loca\\l",
            "_spf.acme.loca\\108",
            "acme\\046local",
            "acme.local\\",
            // Nothing a host name could have.
            "*.local",
            "*.example.com",
            "x+y.local",
            "x+y.example.com",
            "acme.local:993",
            "[::1]",
            "a b.example.com",
            "a\u{0}b.example.com",
            "a/b.example.com",
            // Nothing at all.
            "",
            ".",
        ] {
            assert_eq!(question(name), None, "{name}");
        }
        // A public name is asked as before, `.local` or `_` anywhere in it
        // or not.
        for name in [
            "gmail.com",
            "_imaps._tcp.gmail.com",
            "google._domainkey.example.com",
            "local.example.com",
            "_imaps._tcp.local.example.com",
            "mail.localdomain.example",
            "example.localhost.com",
            "notlocal",
            "mylocal.com",
            "acme.nolocal",
            "autodiscover.example.com",
            "_dmarc.example.com",
            "_spf.example.com",
            "a_b.example.com",
            "Autodiscover.Example.COM.",
            "254.169.in-addr.arpa.example.com",
            "1.2.0.192.in-addr.arpa",
            "_",
        ] {
            assert_eq!(question(name).as_deref(), Some(name), "{name}");
        }
    }
}
