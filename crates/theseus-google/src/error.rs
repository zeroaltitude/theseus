//! Google's errors, in words that say what to do (Google design §2.2, §2.3,
//! §4.3). Nothing here holds a token, a code or a client secret.

use serde_json::Value;

use crate::scopes::Service;

/// What an error asks of the owner or the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// `invalid_grant`, or a 401 the refresh cannot cure: `theseus google auth`.
    NeedsAuth,
    /// 403 `SERVICE_DISABLED` / `accessNotConfigured`: the API is off in the project.
    ServiceDisabled(Box<Disabled>),
    /// The account is enrolled in Advanced Protection, which blocks unverified apps.
    AdvancedProtection,
    /// A Workspace admin's API controls refuse the app.
    AdminPolicy,
    /// 429, or a rate-limit reason: back off and repeat.
    Throttle,
    /// 5xx: Google's own trouble.
    Server,
    /// The connection failed before an answer.
    Network,
    /// Anything else Google refused: 400, 403, 404 and the rest.
    Refused,
}

/// An API that is off in the project, and where to turn it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disabled {
    pub api: String,
    pub project: Option<String>,
    pub activation_url: Option<String>,
}

/// A refusal or a failure, with Google's own words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.words())]
pub struct GoogleError {
    pub status: u16,
    /// Google's `reason` (`notFound`, `rateLimitExceeded`) or OAuth `error`.
    pub reason: String,
    pub message: String,
    pub kind: Kind,
}

/// The sentence `NeedsAuth` carries everywhere.
pub const LAPSED: &str = "Google access lapsed: run `theseus google auth`";

impl GoogleError {
    pub fn needs_auth() -> Self {
        Self {
            status: 400,
            reason: "invalid_grant".into(),
            message: String::new(),
            kind: Kind::NeedsAuth,
        }
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self {
            status: 0,
            reason: "network".into(),
            message: message.into(),
            kind: Kind::Network,
        }
    }

    /// The sentence the model and the owner read.
    pub fn words(&self) -> String {
        match &self.kind {
            Kind::NeedsAuth => LAPSED.to_string(),
            Kind::ServiceDisabled(d) => {
                let Disabled { api, project, activation_url } = &**d;
                let mut s = format!("enable the {api} in project {}", project.as_deref().unwrap_or("(unknown)"));
                if let Some(u) = activation_url {
                    s.push_str(&format!(": {u}"));
                }
                s
            }
            Kind::AdvancedProtection => "Google's Advanced Protection Program blocks this app \
                (an unverified one): the account is enrolled, so this route needs a different plan"
                .to_string(),
            Kind::AdminPolicy => "the Workspace admin's API controls refuse this app \
                (admin_policy_enforced): an admin can trust the client id, or tick \"Trust internal, \
                domain-owned apps\""
                .to_string(),
            Kind::Network => format!("the request did not reach Google: {}", self.message),
            _ => {
                let m = if self.message.is_empty() { &self.reason } else { &self.message };
                format!("Google answered {}: {m}", self.status)
            }
        }
    }

    /// Whether the request may have been sent and run (the retry table's
    /// "after send"): a server error. A throttle was refused, so it did not run.
    pub fn may_have_run(&self) -> bool {
        matches!(self.kind, Kind::Server)
    }
}

/// A token endpoint's error body (`{"error": "invalid_grant", ...}`).
pub fn of_token_error(status: u16, body: &Value) -> GoogleError {
    let reason = body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let message = body
        .get("error_description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    GoogleError {
        status,
        kind: oauth_kind(&reason, &message, status),
        reason,
        message,
    }
}

/// What an OAuth `error` (and its description) means for the owner.
pub fn oauth_kind(reason: &str, message: &str, status: u16) -> Kind {
    let both = format!("{reason} {message}").to_ascii_lowercase();
    if both.contains("advanced protection") {
        Kind::AdvancedProtection
    } else if both.contains("admin_policy_enforced") {
        Kind::AdminPolicy
    } else if reason == "invalid_grant" {
        Kind::NeedsAuth
    } else if status >= 500 {
        Kind::Server
    } else {
        Kind::Refused
    }
}

fn reasons(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let e = v.get("error").unwrap_or(v);
    for x in e
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(r) = x.get("reason").and_then(Value::as_str) {
            out.push(r.to_string());
        }
    }
    for d in e
        .get("details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(r) = d.get("reason").and_then(Value::as_str) {
            out.push(r.to_string());
        }
    }
    out
}

/// An API error body (`{"error": {"code": 403, "message": ..., "errors": [..], "details": [..]}}`).
pub fn of_api_error(status: u16, body: &Value) -> GoogleError {
    let e = body.get("error").unwrap_or(body);
    let message = e
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let rs = reasons(body);
    let has = |r: &str| rs.iter().any(|x| x == r);
    let mut reason = rs.first().cloned().unwrap_or_default();
    if reason.is_empty() {
        reason = e
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
    }
    let kind = if has("SERVICE_DISABLED") || has("accessNotConfigured") {
        disabled(e, &message)
    } else if status == 429 || has("rateLimitExceeded") || has("userRateLimitExceeded") {
        Kind::Throttle
    } else if status >= 500 {
        Kind::Server
    } else if status == 401 {
        Kind::NeedsAuth
    } else if message.to_ascii_lowercase().contains("advanced protection") {
        Kind::AdvancedProtection
    } else if message.contains("admin_policy_enforced") {
        Kind::AdminPolicy
    } else {
        Kind::Refused
    };
    GoogleError {
        status,
        reason,
        message,
        kind,
    }
}

/// The API a `SERVICE_DISABLED` names, and the project and link Google gives.
fn disabled(e: &Value, message: &str) -> Kind {
    let meta = e
        .get("details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|d| d.get("metadata"));
    let field = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let api = field("service")
        .and_then(|h| {
            Service::of_api_host(&h)
                .map(|s| s.api_label().to_string())
                .or(Some(h))
        })
        .unwrap_or_else(|| "API".to_string());
    let project = field("containerInfo")
        .or_else(|| field("consumer").map(|c| c.trim_start_matches("projects/").to_string()))
        .or_else(|| {
            // `accessNotConfigured`'s message: "... has not been used in project 123456 before ..."
            let at = message.find("project ")? + "project ".len();
            let id: String = message[at..]
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            (!id.is_empty()).then_some(id)
        });
    let activation_url = field("activationUrl").or_else(|| {
        message
            .split_whitespace()
            .find(|w| w.starts_with("https://console.developers.google.com"))
            .map(|w| w.trim_end_matches(['.', ',']).to_string())
    });
    Kind::ServiceDisabled(Box::new(Disabled {
        api,
        project,
        activation_url,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_disabled_api_names_the_api_to_enable() {
        let body = json!({"error": {"code": 403, "message": "Gmail API has not been used in project 123456 before or it is disabled.",
            "errors": [{"reason": "accessNotConfigured", "domain": "usageLimits"}],
            "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "SERVICE_DISABLED",
                "metadata": {"service": "gmail.googleapis.com", "consumer": "projects/123456",
                    "activationUrl": "https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=123456"}}]}});
        let e = of_api_error(403, &body);
        assert_eq!(
            e.words(),
            "enable the Gmail API in project 123456: https://console.developers.google.com/apis/api/gmail.googleapis.com/overview?project=123456"
        );
        // The older shape: only `accessNotConfigured` and the message's own words.
        let old = json!({"error": {"code": 403, "message": "Google Drive API has not been used in project 987 before or it is disabled. Enable it by visiting https://console.developers.google.com/apis/api/drive.googleapis.com/overview?project=987 then retry.",
            "errors": [{"reason": "accessNotConfigured"}]}});
        let w = of_api_error(403, &old).words();
        assert!(w.starts_with("enable the API in project 987: https://console.developers.google.com/apis/api/drive"), "{w}");
    }

    #[test]
    fn oauth_errors_map_to_what_to_do() {
        let e = of_token_error(
            400,
            &json!({"error": "invalid_grant", "error_description": "Token has been expired or revoked."}),
        );
        assert_eq!(e.kind, Kind::NeedsAuth);
        assert_eq!(e.words(), LAPSED);
        let e = of_token_error(
            400,
            &json!({"error": "access_denied", "error_description": "Account is enrolled in the Advanced Protection Program"}),
        );
        assert_eq!(e.kind, Kind::AdvancedProtection);
        let e = of_token_error(400, &json!({"error": "admin_policy_enforced"}));
        assert_eq!(e.kind, Kind::AdminPolicy);
        assert!(of_api_error(429, &json!({"error": {"message": "slow"}})).kind == Kind::Throttle);
        assert!(of_api_error(503, &json!({})).kind == Kind::Server);
    }
}
