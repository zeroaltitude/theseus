//! AWS's wire types beyond an account's health line (AWS design §3.7, §5's
//! C2): the budget and GuardDuty as their reads after serving found them,
//! `aws.bootstrap`'s plan, and `aws.confirm_alerts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// An account's monthly budget (`theseus-monthly`), from `DescribeBudget`,
/// which is free, read every six hours after serving. Amounts in cents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsBudgetStatus {
    pub limit_cents: u64,
    pub actual_cents: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub forecast_cents: Option<u64>,
    pub read_at_unix_ms: u64,
}

/// GuardDuty's usage over its last 30 days (`GetUsageStatistics`, free),
/// read weekly after serving. Health warns past `warn_cents`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsGuardDutyStatus {
    pub cents_30_days: u64,
    pub warn_cents: u64,
    pub read_at_unix_ms: u64,
}

/// `aws.bootstrap`'s params: the plan of an account's foundation, posture,
/// and relay stacks, read-only; or, with `apply`, that plan carried out, if
/// its digest is still `apply`'s.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsBootstrapParams {
    /// The account, when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub account: Option<String>,
    /// The operator's address on the alerts topic; for a new foundation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub alert_email: Option<String>,
    /// The trail's encryption: `aws-managed` (SSE-S3, the default) or
    /// `customer` (its own KMS key, about $1 a month).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub trail_key: Option<String>,
    /// Apply the plan whose digest this is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub apply: Option<String>,
}

/// One stack of the bootstrap's plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsBootstrapStack {
    pub stack: String,
    pub region: String,
    /// `create`, `update`, or `none` (it is as planned).
    pub action: String,
    /// Each resource a create makes, as `LogicalId (AWS::Type)`.
    pub resources: Vec<String>,
    /// What an update changes: the template, a parameter, a tag.
    pub changes: Vec<String>,
    pub parameters: BTreeMap<String, String>,
    /// What the apply sets on the stack besides a change set: `stack policy`,
    /// `termination protection`. A create gets both; an existing stack, what
    /// it lacks, as when a run stopped before setting them.
    #[serde(default)]
    pub sets: Vec<String>,
    /// The stack policy the apply sets: its file's, naming only the resources
    /// the stack makes under these parameters. The digest covers it.
    #[serde(default)]
    pub policy: String,
}

/// `aws.bootstrap`'s result: the plan, and whether it was applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsBootstrapResult {
    pub account: String,
    pub region: String,
    pub stacks: Vec<AwsBootstrapStack>,
    /// What the read-only checks found that the operator should know.
    pub warnings: Vec<String>,
    /// The plan's digest, which `apply` names.
    pub digest: String,
    /// Some stack is created or changed, or gets a policy or termination
    /// protection it lacks.
    pub changes: bool,
    pub applied: bool,
    /// What the operator does next.
    pub next: Vec<String>,
}

/// `aws.confirm_alerts`'s params (theseus-9p40): the token from SNS's
/// confirmation email, which the core confirms with
/// `AuthenticateOnUnsubscribe`. A secret: never printed, logged, or kept.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsConfirmAlertsParams {
    /// The account, when several are bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub account: Option<String>,
    /// The token, or the whole confirmation link (copied, never opened).
    pub token: String,
}

impl std::fmt::Debug for AwsConfirmAlertsParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsConfirmAlertsParams")
            .field("account", &self.account)
            .field("token", &"<withheld>")
            .finish()
    }
}

/// `aws.confirm_alerts`'s result: the subscription, as SNS read it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsConfirmAlertsResult {
    pub account: String,
    /// The alerts topic's ARN.
    pub topic: String,
    /// The subscription's ARN.
    pub subscription: String,
    /// SNS's `ConfirmationWasAuthenticated`: only the account can
    /// unsubscribe it.
    pub authenticated: bool,
    /// SNS's `PendingConfirmation`.
    pub pending: bool,
    /// The subscribed address, its local part masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub endpoint: Option<String>,
    /// The confirmation's request id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub request_id: Option<String>,
}
