//! Credentials (theseus-gh7): what a job may be handed, and what stays the
//! harness's own, as `theseusd check` and health say it. A job gets a secret
//! only by its program's grant at its launch, at L0 and in L1 alike
//! (theseus-w5op).

use serde::{Deserialize, Serialize};

/// What a job may be handed, and what stays the harness's own (theseus-gh7):
/// the secrets the operator's `[broker]` names, which a job may get (a
/// program's grant at its start), and the AWS keys and the providers' keys,
/// which only Theseus's own tools read. Names only, never a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HarnessOnly {
    /// Each secret a job may be handed: every one `[broker]` names.
    pub handed: Vec<String>,
    /// The AWS keys' `[secrets]` names.
    pub aws: Vec<String>,
    /// The providers' keys' `[secrets]` names.
    pub providers: Vec<String>,
    /// Those of `aws` and `providers` that `[broker]` names after all, so a
    /// job may be handed them: the operator's own choice, said aloud.
    pub exposed: Vec<String>,
    /// The programs `[broker.programs]` gives an AWS job session at launch
    /// (`aws_account`, AWS design §3.5): short-lived, under the guards, and
    /// never the key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aws_sessions: Vec<String>,
}

impl HarnessOnly {
    /// `broker: a job may be handed 1 secret (github_token); harness-only:
    /// the AWS keys (aws_access_key_id, aws_secret_access_key) and the
    /// providers' keys (anthropic_api_key)`.
    pub fn line(&self) -> String {
        let mut line = match self.handed.as_slice() {
            [] => "broker: a job may be handed no secret".to_string(),
            [one] => format!("broker: a job may be handed 1 secret ({one})"),
            all => format!(
                "broker: a job may be handed {} secrets ({})",
                all.len(),
                all.join(", ")
            ),
        };
        let kept = |names: &[String]| -> String {
            names
                .iter()
                .filter(|n| !self.exposed.contains(n))
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let kinds: Vec<String> = [
            ("the AWS keys", kept(&self.aws)),
            ("the providers' keys", kept(&self.providers)),
        ]
        .into_iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(what, names)| format!("{what} ({names})"))
        .collect();
        if !kinds.is_empty() {
            line.push_str(&format!("; harness-only: {}", kinds.join(" and ")));
        }
        if !self.aws_sessions.is_empty() {
            line.push_str(&format!(
                "; jobs get short-lived AWS sessions, never the key ({})",
                self.aws_sessions.join(", ")
            ));
        }
        if !self.exposed.is_empty() {
            line.push_str(&format!(
                "; not harness-only, since [broker] names them: {}",
                self.exposed.join(", ")
            ));
        }
        line
    }
}
