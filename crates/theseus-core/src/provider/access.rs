//! What the owner is told when the provider answers a call with "not found"
//! or "permission denied".
//!
//! Anthropic's published errors (the API's errors page, read 2026-11-18): a 404
//! is `not_found_error`, "the requested resource was not found", and a 403 is
//! `permission_error`, "your API key does not have permission to use the
//! specified resource". A call here asks for one model, so for the owner the
//! first is "that model, by that name, is not found" and the second is "this
//! account may not use it". The provider's 404 does not say which of a wrong
//! name and an account without access it means, so the words say that and in
//! which order to check; its 403 is about access and says so. Either way the
//! provider's own words follow, so nothing it said is lost.

use serde_json::Value;

/// The plain words for a 404 or 403 whose body is Anthropic's error shape;
/// none for any other status or body, whose message stays the provider's own.
pub(super) fn words(status: u16, body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let e = v.get("error")?;
    let kind = e.get("type")?.as_str()?;
    let said = e.get("message").and_then(Value::as_str).unwrap_or("");
    let theirs = format!("{kind}: {said}");
    match (status, kind) {
        (404, "not_found_error") => Some(format!(
            "the provider says it does not find this model (\"not found\", 404). Its answer does \
             not say whether the name is wrong or this account may not use the model, so check \
             the model's name in the profile first; if the name is right, ask the provider for \
             access to that model. The provider's words: {theirs}"
        )),
        (403, "permission_error") => Some(format!(
            "the provider says this account may not use this model (\"permission denied\", 403): \
             that is about access, not the spelling of the name. Ask the provider for access to \
             the model, or run a profile of another model. The provider's words: {theirs}"
        )),
        _ => None,
    }
}

/// What a 401 or 403 is called at the head of the error: a 401 is a key the
/// provider does not accept, a 403 an account that may not do the thing.
pub(super) fn auth_head(status: &u16) -> &'static str {
    if *status == 403 {
        "permission denied"
    } else {
        "authentication failed"
    }
}

#[cfg(test)]
mod tests {
    use super::super::ProviderError;

    fn from(status: u16, body: &str) -> ProviderError {
        ProviderError::from_status(status, body, &reqwest::header::HeaderMap::new())
    }

    const NOT_FOUND: &str = r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-example-9"},"request_id":"req_1"}"#;
    const DENIED: &str = r#"{"type":"error","error":{"type":"permission_error","message":"Your API key does not have permission to use the specified resource."},"request_id":"req_2"}"#;

    /// "Model not found" (a 404): the owner reads that the provider did not
    /// find the model, that its answer does not say whether the name or the
    /// account is the cause, what to check first, and the provider's own words.
    /// The class and the retry rule are what they were.
    #[test]
    fn a_not_found_answer_says_so_and_what_to_check() {
        let e = from(404, NOT_FOUND);
        assert_eq!(e.class(), "invalid_request");
        assert!(!e.is_transient());
        let s = e.to_string();
        assert!(s.contains("does not find this model"), "{s}");
        assert!(s.contains("\"not found\", 404"), "{s}");
        assert!(
            s.contains("whether the name is wrong or this account may not use"),
            "{s}"
        );
        assert!(
            s.contains("check the model's name in the profile first"),
            "{s}"
        );
        assert!(s.contains("ask the provider for access"), "{s}");
        assert!(
            s.contains("not_found_error: model: claude-example-9"),
            "{s}"
        );
        assert!(!s.contains("permission denied"), "{s}");
    }

    /// "This account may not use it" (a 403): the owner reads that it is the
    /// access that was refused, not the name, and to ask for access; it is not
    /// headed "authentication failed", which would send them to their key.
    #[test]
    fn a_permission_answer_says_the_account_may_not_and_what_to_do() {
        let e = from(403, DENIED);
        assert_eq!(e.class(), "auth");
        assert!(!e.is_transient());
        let s = e.to_string();
        assert!(s.starts_with("permission denied (403): "), "{s}");
        assert!(s.contains("this account may not use this model"), "{s}");
        assert!(s.contains("not the spelling of the name"), "{s}");
        assert!(
            s.contains("ask the provider for access") || s.contains("Ask the provider for access"),
            "{s}"
        );
        assert!(
            s.contains("permission_error: Your API key does not have permission"),
            "{s}"
        );
        assert!(!s.contains("authentication failed"), "{s}");
    }

    /// The two answers are told apart, and nothing else is reworded: a 401 is
    /// still "authentication failed", a 404 or a 403 that is not Anthropic's
    /// shape keeps the provider's own message, and a 400 is untouched.
    #[test]
    fn only_those_two_answers_are_reworded() {
        let k = from(
            401,
            r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
        );
        assert_eq!(
            k.to_string(),
            "authentication failed (401): authentication_error: invalid x-api-key"
        );
        assert_eq!(
            from(404, "<html>nothing here</html>").to_string(),
            "invalid request (404): <html>nothing here</html>"
        );
        assert_eq!(
            from(403, r#"{"error":{"type":"other","message":"no"}}"#).to_string(),
            "permission denied (403): other: no"
        );
        // A 404 whose type is not "not found" and a 403 of another type.
        assert_eq!(
            from(404, r#"{"error":{"type":"other","message":"x"}}"#).to_string(),
            "invalid request (404): other: x"
        );
        let bad = from(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages: roles must alternate"}}"#,
        );
        assert_eq!(
            bad.to_string(),
            "invalid request (400): invalid_request_error: messages: roles must alternate"
        );
    }
}
