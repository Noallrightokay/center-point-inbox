//! Naming a mailbox.

use sha2::{Digest, Sha256};

/// The domain half of an address, lowercased. Empty when there is not one.
pub fn domain_of(email: &str) -> String {
    email
        .trim()
        .to_ascii_lowercase()
        .split_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default()
}

/// A stable identifier for one mailbox: a readable slug of the address, plus a
/// digest of the address as written.
///
/// The slug alone is not enough, and the way it fails is silent. Folding
/// everything outside `[a-z0-9]` to `_` maps `a.b@x.com` and `a-b@x.com` onto
/// the same key — and where that key is a primary key, linking the second
/// overwrites the first: one mailbox quietly replaced by another, and a later
/// unlink removing the wrong one. That bug shipped in the JavaScript and is
/// why the digest is here.
pub fn mail_key(email: &str) -> String {
    let addr = email.trim().to_ascii_lowercase();
    let slug: String = addr
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .chars()
        .take(120)
        .collect();
    let digest = Sha256::digest(addr.as_bytes());
    let tag: String = digest.iter().take(5).map(|b| format!("{b:02x}")).collect();
    format!("mail:{slug}.{tag}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_domain_comes_out_lowercased() {
        assert_eq!(domain_of("Mixed.Case@Example.COM"), "example.com");
        assert_eq!(domain_of("  a@b.co  "), "b.co");
        assert_eq!(domain_of("not-an-address"), "");
    }

    #[test]
    fn punctuation_does_not_collide_two_mailboxes_into_one() {
        // The exact pair that collided in the JavaScript.
        assert_ne!(mail_key("a.b@x.com"), mail_key("a-b@x.com"));
        assert_ne!(mail_key("a.b@x.com"), mail_key("ab@x.com"));
        assert_ne!(mail_key("a+b@x.com"), mail_key("a_b@x.com"));
    }

    #[test]
    fn the_same_address_always_lands_on_the_same_key() {
        assert_eq!(mail_key("Owner@Example.com"), mail_key("  owner@example.com "));
    }

    #[test]
    fn a_long_address_is_not_truncated_onto_its_neighbour() {
        let a = format!("{}@example.com", "x".repeat(200));
        let b = format!("{}@example.com", "x".repeat(201));
        assert_ne!(mail_key(&a), mail_key(&b));
    }

    #[test]
    fn it_stays_one_readable_token() {
        let k = mail_key("owner@example.com");
        assert!(k.starts_with("mail:owner_example_com."), "{k}");
        assert_eq!(k.rsplit('.').next().unwrap().len(), 10, "{k}");
    }
}
