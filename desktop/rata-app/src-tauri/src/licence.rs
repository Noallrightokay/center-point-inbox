//! Whether this copy of RATA is paid for.
//!
//! The app reads the customer's mail directly from their mail server, so it
//! cannot ask us whether they have paid — not without being useless on a train,
//! and not without making our uptime a condition of them reading their own
//! mail. So the licence is **signed, not looked up**: the server holds an
//! Ed25519 private key and issues a short token saying who paid and for what,
//! and this checks it with no network at all.
//!
//! This is the other half of `rata-next/lib/licence.js`. The token format is
//! that file's, and there is a test here that verifies a token produced by it —
//! two implementations of a signature check that disagree is exactly the bug
//! that locks paying customers out, and it would not show up in either
//! codebase's own tests.
//!
//! The public key is compiled in. A build without one refuses every licence
//! rather than accepting any, because the alternative — treat a missing key as
//! "no checking needed" — turns a build mistake into a free product.

use serde::Serialize;

const PREFIX: &str = "v1";

/// The verifying key, set at build time:
///
/// ```sh
/// RATA_LICENCE_PUBLIC_KEY="$(cat licence.pub)" cargo build --release
/// ```
///
/// `option_env!` rather than `env!` so a developer build still compiles; it
/// then refuses every licence, which is the safe direction to fail in.
pub const PUBLIC_KEY: Option<&str> = option_env!("RATA_LICENCE_PUBLIC_KEY");

/// What was in the token. Readable on purpose: there is nothing secret in
/// "this person bought Pro", and a customer being able to see what they were
/// issued is a feature the first time something goes wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Licence {
    pub sub: String,
    pub plan: String,
    pub iat: i64,
    pub exp: i64,
}

/// Why a licence could not be used. Separate variants because the customer's
/// next step differs: an expired licence renews itself, a forged one does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Missing,
    Malformed,
    BadSignature,
    Expired,
    NoPublicKey,
}

impl Reason {
    /// What to tell the customer. The same sentences as `licence.js`, so the
    /// website and the app do not say different things about one licence.
    pub fn explain(self) -> &'static str {
        match self {
            Reason::Missing => {
                "Enter your licence key to use RATA on this computer. Sign in at mailrata.org to find it."
            }
            Reason::Expired => {
                "This licence needs refreshing. Open RATA while online and it will renew itself."
            }
            Reason::BadSignature | Reason::Malformed => {
                "This licence could not be read. Sign in at mailrata.org to get a new one."
            }
            Reason::NoPublicKey => {
                "This copy of RATA was built without a licence key and cannot check licences. Reinstall it from mailrata.org."
            }
        }
    }
}

/// A rejected licence, with whatever could still be read out of it.
#[derive(Debug, Clone)]
pub struct Rejected {
    pub reason: Reason,
    /// Present for an expired licence: it is genuine, so the app knows whose
    /// it is and what it is renewing.
    pub licence: Option<Licence>,
}

/// Check a licence, offline.
pub fn check(token: &str, public_pem: Option<&str>, now_secs: i64) -> Result<Licence, Rejected> {
    let bad = |reason| Rejected {
        reason,
        licence: None,
    };

    if token.trim().is_empty() {
        return Err(bad(Reason::Missing));
    }
    let key = match public_pem.and_then(verifying_key) {
        Some(k) => k,
        None => return Err(bad(Reason::NoPublicKey)),
    };

    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 || parts[0] != PREFIX {
        return Err(bad(Reason::Malformed));
    }

    // The signature covers the payload *as written*, so it is verified against
    // the base64 text rather than the decoded JSON — re-encoding could differ
    // by a byte and would fail a genuine licence.
    let sig_bytes = b64url(parts[2]);
    let signature: [u8; 64] = match sig_bytes.try_into() {
        Ok(s) => s,
        Err(_) => return Err(bad(Reason::Malformed)),
    };
    if key
        .verify_strict(
            parts[1].as_bytes(),
            &ed25519_dalek::Signature::from_bytes(&signature),
        )
        .is_err()
    {
        return Err(bad(Reason::BadSignature));
    }

    let payload: serde_json::Value = match serde_json::from_slice(&b64url(parts[1])) {
        Ok(v) => v,
        Err(_) => return Err(bad(Reason::Malformed)),
    };
    let licence = Licence {
        sub: payload
            .get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        plan: payload
            .get("plan")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        iat: payload.get("iat").and_then(|v| v.as_i64()).unwrap_or(0),
        exp: payload.get("exp").and_then(|v| v.as_i64()).unwrap_or(0),
    };
    if payload.get("v").and_then(|v| v.as_i64()) != Some(1)
        || licence.sub.is_empty()
        || licence.plan.is_empty()
        || licence.exp == 0
    {
        return Err(bad(Reason::Malformed));
    }

    // The clock is checked last, so a forged token is never reported as merely
    // expired — the app would offer to renew something nobody issued.
    if licence.exp < now_secs {
        return Err(Rejected {
            reason: Reason::Expired,
            licence: Some(licence),
        });
    }
    Ok(licence)
}

/// Pull the 32-byte Ed25519 key out of an SPKI PEM, which is what Node's
/// `export({ type: 'spki', format: 'pem' })` produces.
///
/// The whole structure is 44 bytes: a fixed 12-byte header naming the algorithm,
/// then the key. Matching the header rather than skipping 12 bytes means an RSA
/// key pasted in by mistake is refused instead of being read as a nonsense
/// Ed25519 one.
fn verifying_key(pem: &str) -> Option<ed25519_dalek::VerifyingKey> {
    const SPKI_ED25519: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    // Hosting panels mangle multi-line values, so a PEM whose newlines were
    // written as "\n" is accepted too.
    let body: String = pem
        .replace("\\n", "\n")
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    let der = rata_mail::words::base64(body.trim().as_bytes());

    let raw: &[u8] = if der.len() == 44 && der[..12] == SPKI_ED25519 {
        &der[12..]
    } else if der.len() == 32 {
        // A bare 32-byte key, for anyone who exported it that way.
        &der
    } else {
        return None;
    };
    let bytes: [u8; 32] = raw.try_into().ok()?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes).ok()
}

/// base64url, which is what the token uses — `-` and `_` rather than `+` and
/// `/`, and no padding.
fn b64url(s: &str) -> Vec<u8> {
    let standard: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    rata_mail::words::base64(standard.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real key and real tokens, produced by `rata-next/lib/licence.js` — the
    /// implementation that will actually be signing them in production.
    const JS_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA0ENRgo1K8QYK0W19M+57n3C21uDjHt4qX/o+S+BE51g=\n-----END PUBLIC KEY-----";
    const JS_TOKEN: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1MTY4MDAsImV4cCI6MTc5MjEwODgwMH0.d8B8bwGISFtqh3wODGdMd6CCNqoa99GnHDzmY1tvNqR2O8vhuQSoe9BezQrry4bMIQXuFwfmfeEj4GALgwV5CA";
    const JS_EXPIRED: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNTc3ODM2ODAwLCJleHAiOjE1ODA0Mjg4MDB9.plxEJBq9Rfprc0ZiXKRJ5bMZmd5KTHHIfM4r4oGtUYc84d6cX9NdqGRivL_jEUCQlS-7zqQ3vju8B4AD1rvMBA";

    /// 16 September 2026, the day the token above was issued.
    const ISSUED: i64 = 1_789_516_800;

    #[test]
    fn a_licence_signed_by_the_javascript_verifies_here() {
        // The test that matters most in this file. Two implementations of one
        // signature check that disagree is how paying customers get locked out,
        // and neither codebase's own tests would catch it.
        let l = check(JS_TOKEN, Some(JS_PUBLIC_KEY), ISSUED).expect("the real thing must verify");
        assert_eq!(l.sub, "buyer@example.com");
        assert_eq!(l.plan, "pro");
        assert_eq!(l.exp - l.iat, 30 * 86400, "thirty days offline");
    }

    #[test]
    fn a_pem_mangled_by_a_hosting_panel_still_works() {
        let flattened = JS_PUBLIC_KEY.replace('\n', "\\n");
        assert!(check(JS_TOKEN, Some(&flattened), ISSUED).is_ok());
    }

    #[test]
    fn expired_is_reported_as_expired_and_not_as_forgery() {
        // Telling a paying customer their licence is fake is a support call you
        // do not recover from.
        let e = check(JS_EXPIRED, Some(JS_PUBLIC_KEY), ISSUED).unwrap_err();
        assert_eq!(e.reason, Reason::Expired);
        // Still readable, so the app knows whose licence it is renewing.
        assert_eq!(e.licence.unwrap().sub, "buyer@example.com");
        assert!(e.reason.explain().contains("renew itself"));
    }

    #[test]
    fn the_edit_a_customer_would_actually_try() {
        // Take a real Base licence and change the word to "pro".
        let parts: Vec<&str> = JS_EXPIRED.split('.').collect();
        let payload = String::from_utf8(b64url(parts[1])).unwrap();
        assert!(payload.contains("\"base\""), "{payload}");
        let upgraded = payload.replace("\"base\"", "\"pro\"");
        let re = rata_mail::words::base64_encode(upgraded.as_bytes())
            .replace('+', "-")
            .replace('/', "_")
            .replace('=', "");
        let forged = format!("v1.{re}.{}", parts[2]);

        let e = check(&forged, Some(JS_PUBLIC_KEY), 0).unwrap_err();
        // Signature before clock: it must read as forged, not as expired.
        assert_eq!(e.reason, Reason::BadSignature);
    }

    #[test]
    fn a_licence_signed_by_somebody_elses_key_is_refused() {
        // A different, valid Ed25519 public key.
        let other = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAGb9ECWmEzf6FQbrBZ9w7lshQhqowtrbLDFw4rXAxZuE=\n-----END PUBLIC KEY-----";
        assert_eq!(
            check(JS_TOKEN, Some(other), ISSUED).unwrap_err().reason,
            Reason::BadSignature
        );
    }

    #[test]
    fn a_build_with_no_key_refuses_everything_rather_than_allowing_it() {
        // The direction this fails in is the whole point: a build mistake must
        // not turn into a free product.
        assert_eq!(
            check(JS_TOKEN, None, ISSUED).unwrap_err().reason,
            Reason::NoPublicKey
        );
        // And a key that is not an Ed25519 one is the same as no key, rather
        // than being read as a nonsense one.
        for junk in [
            "",
            "-----BEGIN PUBLIC KEY-----\nbm90IGEga2V5\n-----END PUBLIC KEY-----",
            "hello",
        ] {
            assert_eq!(
                check(JS_TOKEN, Some(junk), ISSUED).unwrap_err().reason,
                Reason::NoPublicKey,
                "{junk:?}"
            );
        }
    }

    #[test]
    fn rubbish_is_refused_without_panicking() {
        for junk in [
            "",
            "   ",
            "v1",
            "v1.a",
            "v1.a.b.c",
            "v2.a.b",
            "not a licence at all",
            "v1.!!!.???",
            "v1..",
            "v1.eyJ2IjoxfQ.AAAA",
        ] {
            let e = check(junk, Some(JS_PUBLIC_KEY), ISSUED).unwrap_err();
            assert!(
                matches!(
                    e.reason,
                    Reason::Malformed | Reason::Missing | Reason::BadSignature
                ),
                "{junk:?} gave {:?}",
                e.reason
            );
            assert!(!e.reason.explain().is_empty());
        }
    }

    #[test]
    fn a_licence_a_second_before_expiry_still_works() {
        let l = check(JS_TOKEN, Some(JS_PUBLIC_KEY), ISSUED).unwrap();
        assert!(
            check(JS_TOKEN, Some(JS_PUBLIC_KEY), l.exp).is_ok(),
            "on the boundary"
        );
        assert_eq!(
            check(JS_TOKEN, Some(JS_PUBLIC_KEY), l.exp + 1)
                .unwrap_err()
                .reason,
            Reason::Expired
        );
    }
}

/// What a plan allows.
///
/// These numbers are `rata-next/lib/plan.js` and must stay equal to it. They
/// are the same numbers the website quotes, and a cap the customer only meets
/// as a failing button is a cap that feels like a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub key: &'static str,
    pub label: &'static str,
    /// `None` is no limit.
    pub mail: Option<u32>,
    pub chat: Option<u32>,
    /// Viewing mailboxes side by side rather than in one stream.
    pub split: bool,
    pub ai: bool,
}

pub fn plan_def(key: &str) -> Plan {
    match key.trim().to_ascii_lowercase().as_str() {
        "base" => Plan {
            key: "base",
            label: "RATA Base",
            mail: Some(2),
            chat: Some(0),
            split: false,
            ai: false,
        },
        "pro" => Plan {
            key: "pro",
            label: "RATA Pro",
            mail: None,
            chat: Some(3),
            split: true,
            ai: true,
        },
        "enterprise" => Plan {
            key: "enterprise",
            label: "RATA Enterprise",
            mail: None,
            chat: None,
            split: true,
            ai: true,
        },
        // A signed licence naming a plan this build has never heard of means
        // the app is older than the price list, not that anything is wrong:
        // issuing one needs the private key, so nobody can reach this by
        // forging. Being generous costs nothing and being strict would lock a
        // paying customer out for the crime of not having updated.
        _ => Plan {
            key: "unknown",
            label: "RATA",
            mail: None,
            chat: None,
            split: true,
            ai: true,
        },
    }
}

/// A date a person can read, from a Unix timestamp. Only the day: the hour a
/// licence lapses is not something anybody needs to plan around.
pub fn on_day(unix_secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let days = unix_secs.div_euclid(86_400);
    // The same civil-from-days arithmetic as the Date header in rata-mail.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{d} {} {year}", MONTHS[(m - 1) as usize])
}
