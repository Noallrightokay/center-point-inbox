//! How a mailbox is signed in to: a password, or an OAuth 2.0 access token.
//!
//! Microsoft turned password sign-in off for Outlook.com and Microsoft 365
//! IMAP, so a Microsoft mailbox can only be opened with a token its owner got
//! by signing in to Microsoft. The token is presented with the `XOAUTH2` SASL
//! mechanism, the same one string on IMAP (`AUTHENTICATE XOAUTH2`) and SMTP
//! (`AUTH XOAUTH2`) — see [`xoauth2`].
//!
//! A refused token is its own kind of failure, never a refused password.
//! Access tokens last about an hour: when one is refused the app asks for a
//! fresh one, which it can do without the customer, whereas a refused
//! password needs them to type a new one. Mixing the two up either parks a
//! mailbox that only needed a refresh, or keeps presenting a password that
//! was refused — which is how providers decide to lock an account.
//!
//! **A token is never written into anything that is shown or logged.** The
//! `Debug` of a [`Credential`] hides the secret, and every sentence a
//! sign-in failure produces passes through `redact` first, in case a server
//! echoes what it was sent.

use std::fmt;

/// What a mailbox signs in with. Held only for the length of a call.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// An app password, in almost every case — `LOGIN` on IMAP, `AUTH PLAIN`
    /// or `AUTH LOGIN` on SMTP. The account's address is the user name.
    Password(String),
    /// An OAuth 2.0 access token, presented with `XOAUTH2`. `user` is who
    /// the token is for — the mailbox's address in practice.
    OAuth { user: String, access_token: String },
}

impl Credential {
    /// An OAuth credential.
    pub fn oauth(user: impl Into<String>, access_token: impl Into<String>) -> Self {
        Credential::OAuth {
            user: user.into(),
            access_token: access_token.into(),
        }
    }

    pub fn is_oauth(&self) -> bool {
        matches!(self, Credential::OAuth { .. })
    }
}

/// Written by hand so that no `{:?}` of an account — in a panic, a test
/// failure or a future log line — ever carries the secret.
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credential::Password(_) => f.write_str("Password(<hidden>)"),
            Credential::OAuth { user, .. } => f
                .debug_struct("OAuth")
                .field("user", user)
                .field("access_token", &"<hidden>")
                .finish(),
        }
    }
}

/// The `XOAUTH2` initial client response, before base64:
/// `user=<user>` 0x01 `auth=Bearer <token>` 0x01 0x01.
///
/// Nothing when either part is empty or holds a control character: 0x01
/// separates the fields, so one inside the user name could write a field of
/// its own, and a line break has no business in either. A token that fails
/// this is not one Microsoft issued, so the caller treats it as refused.
pub fn xoauth2(user: &str, access_token: &str) -> Option<Vec<u8>> {
    let bad = |s: &str| s.is_empty() || s.chars().any(|c| c.is_control());
    if bad(user) || bad(access_token) || access_token.contains(' ') {
        return None;
    }
    let mut out = Vec::with_capacity(user.len() + access_token.len() + 20);
    out.extend_from_slice(b"user=");
    out.extend_from_slice(user.as_bytes());
    out.push(0x01);
    out.extend_from_slice(b"auth=Bearer ");
    out.extend_from_slice(access_token.as_bytes());
    out.extend_from_slice(&[0x01, 0x01]);
    Some(out)
}

/// What a failure message says in place of a secret.
pub(crate) const HIDDEN: &str = "[token hidden]";

/// `text` with the token, and the base64 it travelled as, taken out.
///
/// A server has no reason to echo either back, but the sentence this goes into
/// is shown to the customer and may be pasted into a bug report, so it is not
/// left to the server's manners. Passwords are left alone: the existing
/// messages never carried one, and a short password would blank out ordinary
/// words.
pub fn redact(text: &str, credential: &Credential) -> String {
    let Credential::OAuth { user, access_token } = credential else {
        return text.to_string();
    };
    let mut out = text.to_string();
    if let Some(sasl) = xoauth2(user, access_token) {
        let wire = crate::words::base64_encode(&sasl);
        out = out.replace(&wire, HIDDEN);
    }
    // Anything shorter is not a real token, and replacing it would blank out
    // ordinary words in the message.
    if access_token.len() >= 8 {
        out = out.replace(access_token.as_str(), HIDDEN);
    }
    out
}

/// The `status` a server put in its `XOAUTH2` error challenge — the base64
/// JSON Google and others send before the final refusal, such as
/// `{"status":"401","schemes":"bearer","scope":"…"}`. Only a short run of
/// letters and digits is kept: it is a stranger's text.
pub(crate) fn challenge_status(decoded: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(decoded);
    let at = text.find("\"status\"")?;
    let rest = text[at + "\"status\"".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let status: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    (!status.is_empty()).then_some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_xoauth2_string_is_exactly_what_the_mechanism_asks_for() {
        let got = xoauth2("someone@outlook.com", "EwBAbc.def-123").unwrap();
        assert_eq!(
            got,
            b"user=someone@outlook.com\x01auth=Bearer EwBAbc.def-123\x01\x01".to_vec()
        );
        // And on the wire: the example from Google's XOAUTH2 documentation,
        // byte for byte.
        let doc = xoauth2(
            "someuser@example.com",
            "ya29.vF9dft4qmTc2Nvb3RlckBhdHRhdmlzdGEuY29tCg",
        )
        .unwrap();
        assert_eq!(
            crate::words::base64_encode(&doc),
            "dXNlcj1zb21ldXNlckBleGFtcGxlLmNvbQFhdXRoPUJlYXJlciB5YTI5LnZGOWRmdDRxbVRjMk52YjNSbGNrQmhkSFJoZG1semRHRXVZMjl0Q2cBAQ=="
        );
    }

    #[test]
    fn a_user_or_token_that_could_forge_a_field_is_refused() {
        assert!(xoauth2("a@b.com\x01auth=Bearer x", "tok").is_none());
        assert!(xoauth2("a@b.com", "tok\x01\x01").is_none());
        assert!(xoauth2("a@b.com", "tok\r\nA2 LOGOUT").is_none());
        assert!(xoauth2("a@b.com", "two words").is_none());
        assert!(xoauth2("", "tok").is_none());
        assert!(xoauth2("a@b.com", "").is_none());
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let pw = format!("{:?}", Credential::Password("hunter2-secret".into()));
        assert!(!pw.contains("hunter2"), "{pw}");
        let tok = format!(
            "{:?}",
            Credential::oauth("me@example.com", "EwB-very-secret")
        );
        assert!(!tok.contains("very-secret"), "{tok}");
        assert!(tok.contains("me@example.com"), "{tok}");
    }

    #[test]
    fn an_echoed_token_is_taken_out_of_the_message() {
        let cred = Credential::oauth("me@example.com", "EwB-very-secret-token");
        let wire = crate::words::base64_encode(
            &xoauth2("me@example.com", "EwB-very-secret-token").unwrap(),
        );
        let said = format!("NO bad token EwB-very-secret-token in {wire}");
        let shown = redact(&said, &cred);
        assert!(!shown.contains("very-secret"), "{shown}");
        assert!(!shown.contains(&wire), "{shown}");
        assert!(shown.starts_with("NO bad token [token hidden]"), "{shown}");
        // A password is never searched for, so it cannot blank out words.
        let pw = Credential::Password("the".into());
        assert_eq!(redact("the server said no", &pw), "the server said no");
    }

    #[test]
    fn the_status_is_read_from_the_error_challenge_and_nothing_else() {
        let json = br#"{"status":"401","schemes":"bearer mac","scope":"https://mail.google.com/"}"#;
        assert_eq!(challenge_status(json).as_deref(), Some("401"));
        assert_eq!(
            challenge_status(br#"{ "status" : 400 }"#).as_deref(),
            Some("400")
        );
        assert_eq!(challenge_status(b"not json"), None);
        // A stranger's text is cut to a short word.
        assert_eq!(
            challenge_status(br#"{"status":"4<script>"}"#).as_deref(),
            Some("4")
        );
    }
}
