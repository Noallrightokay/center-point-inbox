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
/// the url crate's IDNA, so nothing new is built for it. `None` for a name
/// that has no such spelling.
pub(crate) fn question(name: &str) -> Option<String> {
    if name.is_ascii() {
        return Some(name.to_string());
    }
    match ammonia::url::Host::parse(name) {
        Ok(ammonia::url::Host::Domain(ascii)) => Some(ascii),
        _ => None,
    }
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
}
