//! The credentials a call signs with (AWS design §3.5): a session's, minted
//! and cached by the core's `AwsCreds`, or the root key's for the few calls it
//! signs. The client only borrows them, one call at a time.

use std::time::SystemTime;

use zeroize::Zeroizing;

/// An access key, its secret, and a session token. The secret parts are
/// zeroized on drop, and neither `Debug` nor any error ever shows them.
#[derive(Clone)]
pub struct Credentials {
    access_key_id: String,
    secret_access_key: Zeroizing<String>,
    session_token: Option<Zeroizing<String>>,
    expires: Option<SystemTime>,
}

impl Credentials {
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
        session_token: Option<String>,
        expires: Option<SystemTime>,
    ) -> Credentials {
        Credentials {
            access_key_id: access_key_id.into(),
            secret_access_key: Zeroizing::new(secret_access_key.into()),
            session_token: session_token.map(Zeroizing::new),
            expires,
        }
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub fn has_session_token(&self) -> bool {
        self.session_token.is_some()
    }

    /// The secret key, for a job's environment at its launch only: a session's
    /// the broker hands a granted program (AWS design §3.5), never the root
    /// key, and never a record.
    pub fn expose_secret(&self) -> &str {
        &self.secret_access_key
    }

    /// The session token, as `expose_secret`.
    pub fn expose_token(&self) -> Option<&str> {
        self.session_token.as_ref().map(|t| t.as_str())
    }

    /// When a session's credentials stop working.
    pub fn expires(&self) -> Option<SystemTime> {
        self.expires
    }

    /// The signer's view of them.
    pub(crate) fn identity(&self) -> aws_smithy_runtime_api::client::identity::Identity {
        aws_credential_types::Credentials::new(
            self.access_key_id.as_str(),
            self.secret_access_key.as_str(),
            self.session_token.as_ref().map(|t| t.to_string()),
            self.expires,
            "theseus",
        )
        .into()
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let id = &self.access_key_id;
        let shown = if id.len() > 8 && id.is_ascii() {
            format!("{}…{}", &id[..4], &id[id.len() - 4..])
        } else {
            "…".to_owned()
        };
        f.debug_struct("Credentials")
            .field("access_key_id", &shown)
            .field("secret_access_key", &"(hidden)")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "(hidden)"),
            )
            .field("expires", &self.expires)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_secrets() {
        let c = Credentials::new(
            "AKIDEXAMPLE12345",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            Some("session-token-example".into()),
            None,
        );
        let shown = format!("{c:?}");
        assert!(!shown.contains("wJalr"), "{shown}");
        assert!(!shown.contains("session-token-example"), "{shown}");
        assert!(!shown.contains("AKIDEXAMPLE12345"), "{shown}");
        assert!(shown.contains("AKID…2345"), "{shown}");
    }
}
