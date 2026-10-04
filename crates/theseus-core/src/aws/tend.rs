//! The account's tenders after serving (AWS design §3.7, §3.10; C2 = 14b),
//! never on the start path, and only once the config names the owner role:
//!
//! - **The budget's reconcile**, once a start: `monthly_budget_usd` against
//!   the foundation stack's `MonthlyBudgetUsd`. When they differ it makes a
//!   change set of the old template with the new amount, and applies it only
//!   if it changes the budget and nothing else; any other change stops it,
//!   and health says what. Ledgered as `aws.budget.reconciled`.
//! - **The budget's line**, every six hours: `DescribeBudget`, which is free.
//! - **GuardDuty's usage**, weekly: `GetUsageStatistics`, which is free.
//!   Health warns when the month's projection passes $1.
//! - **The CloudTrail cross-check** ([`super::crosscheck`]), daily, from an
//!   hour after serving: the events of Theseus's identities that nothing in
//!   the ledger accounts for. Ledgered as `aws.trail.checked`.
//!
//! They sign in the tender session `theseus-tender`, whose inline policy
//! ([`policy`]) allows exactly these calls.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{AwsBudgetStatus, AwsGuardDutyStatus, LedgerKind};

use crate::ledger::LedgerRow;

use super::session::Kind;
use super::stack::{role_arn, Cfn, ChangeSetSpec, Param, Ready, DEPLOYER};
use super::tools::to_cents;
use super::{Account, Failure, Request, Signer};

/// The tender's name: its session is `theseus-tender`.
pub const TENDER: &str = "tender";
/// The foundation stack, whose budget the reconcile keeps.
pub const FOUNDATION: &str = "theseus-foundation";
/// The budget the foundation makes.
pub const BUDGET: &str = "theseus-monthly";
/// How often the budget's line is read.
pub const BUDGET_EVERY: Duration = Duration::from_secs(6 * 3600);
/// How often GuardDuty's usage is read.
pub const GUARDDUTY_EVERY: Duration = Duration::from_secs(7 * 24 * 3600);
/// The month's GuardDuty projection health warns past.
pub const GUARDDUTY_WARN_CENTS: u64 = 100;

/// A tender's inline policy: its one allow (§3.5).
pub fn policy(tender: &str, account: &str) -> Option<Value> {
    (tender == TENDER).then(|| {
        json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Sid": "TheFoundationsBudget",
                    "Effect": "Allow",
                    "Action": [
                        "cloudformation:DescribeStacks", "cloudformation:CreateChangeSet",
                        "cloudformation:DescribeChangeSet", "cloudformation:ExecuteChangeSet",
                        "cloudformation:DeleteChangeSet",
                    ],
                    "Resource": format!("arn:aws:cloudformation:*:{account}:stack/{FOUNDATION}/*"),
                },
                {
                    "Sid": "TheDeployerToCloudFormation",
                    "Effect": "Allow",
                    "Action": "iam:PassRole",
                    "Resource": role_arn(account, DEPLOYER),
                    "Condition": {"StringEquals": {"iam:PassedToService": "cloudformation.amazonaws.com"}},
                },
                {
                    "Sid": "ReadTheBudget",
                    "Effect": "Allow",
                    "Action": "budgets:ViewBudget",
                    "Resource": format!("arn:aws:budgets::{account}:budget/{BUDGET}"),
                },
                {
                    "Sid": "ReadTheTrail",
                    "Effect": "Allow",
                    "Action": "cloudtrail:LookupEvents",
                    "Resource": "*",
                },
                {
                    "Sid": "ReadGuardDutysUsage",
                    "Effect": "Allow",
                    "Action": ["guardduty:ListDetectors", "guardduty:GetDetector", "guardduty:GetUsageStatistics"],
                    "Resource": "*",
                },
            ],
        })
    })
}

fn cfn(account: &Arc<Account>) -> Cfn<'_> {
    Cfn {
        account,
        region: &account.cfg.region,
        binding: None,
        signer: Signer::As(Kind::Tender(TENDER)),
    }
}

fn said(f: &Failure) -> String {
    f.to_string()
}

/// What the reconcile did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciled {
    /// The config names no amount: the stack's stands.
    Unset,
    /// The stack holds the config's amount already.
    Equal(u32),
    Changed {
        from: u32,
        to: u32,
    },
    /// It stopped, and why: no foundation, a diff beyond the budget, a failure.
    Stopped(String),
}

impl Reconciled {
    /// Health's words for it.
    pub fn line(&self) -> String {
        match self {
            Reconciled::Unset => {
                "the config names no monthly_budget_usd; the stack's stands".into()
            }
            Reconciled::Equal(n) => format!("the stack holds the config's ${n} a month"),
            Reconciled::Changed { from, to } => {
                format!("changed the budget from ${from} to ${to} a month")
            }
            Reconciled::Stopped(why) => format!("STOPPED: {why}"),
        }
    }
}

/// The budget's reconcile: config to stack, idempotent.
pub async fn reconcile(account: &Arc<Account>) -> Reconciled {
    let Some(want) = account.cfg.monthly_budget_usd else {
        return Reconciled::Unset;
    };
    let cfn = cfn(account);
    let stack = match cfn.stack(FOUNDATION).await {
        Ok(Some(s)) => s,
        Ok(None) => return Reconciled::Stopped(format!("there is no {FOUNDATION} stack")),
        Err(f) => return Reconciled::Stopped(format!("DescribeStacks failed: {}", said(&f))),
    };
    let params: Vec<(String, String)> = stack["Parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["ParameterKey"].as_str()?.to_string(),
                p["ParameterValue"].as_str().unwrap_or_default().to_string(),
            ))
        })
        .collect();
    let Some(had) = params
        .iter()
        .find(|(k, _)| k == "MonthlyBudgetUsd")
        .and_then(|(_, v)| v.trim().parse::<f64>().ok())
        .map(|v| v as u32)
    else {
        return Reconciled::Stopped(format!("{FOUNDATION} has no MonthlyBudgetUsd parameter"));
    };
    if had == want {
        return Reconciled::Equal(want);
    }
    change_budget(&cfn, &params, had, want).await
}

/// The change set of the old template with the new amount, applied only if
/// it changes the budget and nothing else.
async fn change_budget(
    cfn: &Cfn<'_>,
    params: &[(String, String)],
    had: u32,
    want: u32,
) -> Reconciled {
    let spec = ChangeSetSpec {
        stack: FOUNDATION,
        name: format!("theseus-budget-{want}-{}", theseus_protocol::now_unix_ms()),
        kind: "UPDATE",
        body: None,
        url: None,
        parameters: params
            .iter()
            .map(|(k, _)| {
                let v = if k == "MonthlyBudgetUsd" {
                    Param::Value(want.to_string())
                } else {
                    Param::Previous
                };
                (k.clone(), v)
            })
            .collect(),
        role: Some(role_arn(&cfn.account.id, DEPLOYER)),
        tags: Vec::new(),
        description: format!("theseus: the budget's reconcile, ${had} to ${want}"),
    };
    let id = match cfn.create_change_set(&spec).await {
        Ok(id) => id,
        Err(f) => return Reconciled::Stopped(format!("CreateChangeSet failed: {}", said(&f))),
    };
    let changes = match cfn.ready(FOUNDATION, &id).await {
        Ok(Ready::Changes { changes, .. }) => changes,
        Ok(Ready::Empty { .. }) => {
            let _ = cfn.delete_change_set(FOUNDATION, &id).await;
            return Reconciled::Equal(want);
        }
        Ok(Ready::Failed { reason, .. }) => {
            return Reconciled::Stopped(format!("the change set failed: {reason}"))
        }
        Err(f) => return Reconciled::Stopped(format!("DescribeChangeSet failed: {}", said(&f))),
    };
    let beyond: Vec<String> = changes
        .iter()
        .filter_map(|c| {
            let r = &c["ResourceChange"];
            let id = r["LogicalResourceId"].as_str().unwrap_or("?");
            let only_budget = id == "MonthlyBudget"
                && r["Action"].as_str() == Some("Modify")
                && r["Replacement"].as_str() == Some("False");
            (!only_budget).then(|| format!("{} {id}", r["Action"].as_str().unwrap_or("?")))
        })
        .collect();
    if !beyond.is_empty() || changes.is_empty() {
        let _ = cfn.delete_change_set(FOUNDATION, &id).await;
        return Reconciled::Stopped(format!(
            "the change set for ${want} touches more than the budget ({}); nothing was applied",
            if beyond.is_empty() {
                "nothing at all".to_string()
            } else {
                beyond.join(", ")
            }
        ));
    }
    if let Err(f) = cfn.execute(FOUNDATION, &id).await {
        return Reconciled::Stopped(format!("ExecuteChangeSet failed: {}", said(&f)));
    }
    match cfn.settle(FOUNDATION, Duration::from_secs(10 * 60)).await {
        Ok(s) if s.status == "UPDATE_COMPLETE" => Reconciled::Changed {
            from: had,
            to: want,
        },
        Ok(s) => Reconciled::Stopped(format!(
            "{FOUNDATION} ended {}{}",
            s.status,
            s.failures
                .first()
                .map(|f| format!(" ({f})"))
                .unwrap_or_default()
        )),
        Err(f) => Reconciled::Stopped(format!("waiting for {FOUNDATION} failed: {}", said(&f))),
    }
}

/// The budget's line: `DescribeBudget`, which is free.
pub async fn read_budget(account: &Arc<Account>) -> Result<AwsBudgetStatus, String> {
    let input = json!({"AccountId": account.id, "BudgetName": BUDGET});
    let r = Request {
        service: "budgets",
        operation: "DescribeBudget",
        input: &input,
        region: "us-east-1",
        pages: 1,
        class: "read",
        signer: Signer::As(Kind::Tender(TENDER)),
    };
    let out = account.request(None, &r).await.map_err(|f| said(&f))?;
    let b = &out.body["Budget"];
    let amount = |v: &Value| v["Amount"].as_str().and_then(to_cents);
    Ok(AwsBudgetStatus {
        limit_cents: amount(&b["BudgetLimit"]).unwrap_or(0),
        actual_cents: amount(&b["CalculatedSpend"]["ActualSpend"]).unwrap_or(0),
        forecast_cents: amount(&b["CalculatedSpend"]["ForecastedSpend"]),
        read_at_unix_ms: theseus_protocol::now_unix_ms(),
    })
}

/// Every GuardDuty feature the usage read sums.
const FEATURES: [&str; 13] = [
    "FLOW_LOGS",
    "CLOUD_TRAIL",
    "DNS_LOGS",
    "S3_DATA_EVENTS",
    "EKS_AUDIT_LOGS",
    "EBS_MALWARE_PROTECTION",
    "RDS_LOGIN_EVENTS",
    "LAMBDA_NETWORK_LOGS",
    "EKS_RUNTIME_MONITORING",
    "FARGATE_RUNTIME_MONITORING",
    "EC2_RUNTIME_MONITORING",
    "RDS_DBI_PROTECTION_PROVISIONED",
    "RDS_DBI_PROTECTION_SERVERLESS",
];

/// GuardDuty's usage over its last 30 days, projected to a month when the
/// detector is younger: `GetUsageStatistics`, which is free. None when the
/// region has no detector.
pub async fn read_guardduty(account: &Arc<Account>) -> Result<Option<AwsGuardDutyStatus>, String> {
    let region = account.cfg.region.as_str();
    let call = |op: &'static str, input: Value| {
        let account = account.clone();
        async move {
            let r = Request {
                service: "guardduty",
                operation: op,
                input: &input,
                region,
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Tender(TENDER)),
            };
            account.request(None, &r).await.map_err(|f| said(&f))
        }
    };
    let detectors = call("ListDetectors", json!({})).await?;
    let Some(id) = detectors.body["DetectorIds"][0].as_str().map(String::from) else {
        return Ok(None);
    };
    let detector = call("GetDetector", json!({"DetectorId": id})).await?;
    let usage = call(
        "GetUsageStatistics",
        json!({
            "DetectorId": id,
            "UsageStatisticType": "SUM_BY_ACCOUNT",
            "UsageCriteria": {"Features": FEATURES},
        }),
    )
    .await?;
    let total: u64 = usage.body["UsageStatistics"]["SumByAccount"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["Total"]["Amount"].as_str().and_then(to_cents))
        .sum();
    let now = theseus_protocol::now_unix_ms();
    let days = detector.body["CreatedAt"]
        .as_str()
        .and_then(age_days(now))
        .unwrap_or(30)
        .clamp(1, 30);
    Ok(Some(AwsGuardDutyStatus {
        cents_30_days: total * 30 / days,
        warn_cents: GUARDDUTY_WARN_CENTS,
        read_at_unix_ms: now,
    }))
}

/// Whole days from an RFC 3339 time (`2026-10-03T…`) to `now_ms`.
fn age_days(now_ms: u64) -> impl Fn(&str) -> Option<u64> {
    move |t: &str| {
        let (y, rest) = t.get(..10)?.split_once('-')?;
        let (m, d) = rest.split_once('-')?;
        let day = crate::wake::days_from_civil(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        let today = (now_ms / 86_400_000) as i64;
        u64::try_from(today - day).ok()
    }
}

impl crate::Core {
    /// The AWS tenders after serving (§3.10): each account with an owner
    /// role, once its check has passed, reconciles its budget once, then
    /// reads the budget every six hours and GuardDuty's usage weekly. Between
    /// reads it holds the core only weakly.
    pub fn tend_aws_after_serving(self: &Arc<Self>) {
        let Some(aws) = self.tools.aws.clone() else {
            return;
        };
        for account in aws.accounts().filter(|a| a.cfg.owner_role.is_some()) {
            self.crosscheck_daily(account.clone());
            let core = Arc::downgrade(self);
            let account = account.clone();
            tokio::spawn(async move {
                if !account.settled().await {
                    return;
                }
                let r = reconcile(&account).await;
                tracing::info!(account = %account.id, reconcile = %r.line(), "aws: the budget's reconcile");
                if let (Reconciled::Changed { from, to }, Some(c)) = (&r, core.upgrade()) {
                    let row = json!({"account": account.id, "from_usd": from, "to_usd": to});
                    let ledger = LedgerRow::new(LedgerKind::AwsBudgetReconciled, None, None, row);
                    if let Err(e) = c.store.append_ledger(&ledger) {
                        tracing::warn!(error = %e, "ledger append failed");
                    }
                }
                account.tended.lock().unwrap().reconcile = Some(r.line());
                let mut guardduty_due = std::time::Instant::now();
                loop {
                    if core.upgrade().is_none() {
                        return;
                    }
                    match read_budget(&account).await {
                        Ok(b) => account.tended.lock().unwrap().budget = Some(b),
                        Err(e) => {
                            tracing::warn!(account = %account.id, error = %e, "aws: the budget's read failed")
                        }
                    }
                    if std::time::Instant::now() >= guardduty_due {
                        match read_guardduty(&account).await {
                            Ok(g) => account.tended.lock().unwrap().guardduty = g,
                            Err(e) => {
                                tracing::warn!(account = %account.id, error = %e, "aws: GuardDuty's usage read failed")
                            }
                        }
                        guardduty_due += GUARDDUTY_EVERY;
                    }
                    tokio::time::sleep(BUDGET_EVERY).await;
                }
            });
        }
    }

    /// The CloudTrail cross-check (§3.8), from an hour after serving, then
    /// daily: each run one `aws.trail.checked` row, and a warning in the log
    /// for each event of Theseus's that nothing in the ledger accounts for.
    fn crosscheck_daily(self: &Arc<Self>, account: Arc<Account>) {
        use super::crosscheck::{self, Ledgered};
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            if !account.settled().await {
                return;
            }
            tokio::time::sleep(crosscheck::LAG).await;
            loop {
                let Some(c) = core.upgrade() else { return };
                let rows = theseus_store::blocking(|| c.store.ledger_tail::<LedgerRow>(100_000));
                drop(c);
                let checked = match rows {
                    Err(e) => Err(format!("the ledger's read failed: {e}")),
                    Ok(rows) => {
                        let ledgered =
                            Ledgered::of(rows.iter().map(|(_, r)| (r.kind.as_str(), &r.data)));
                        crosscheck::events(&account, theseus_protocol::now_unix_ms())
                            .await
                            .map_err(|f| f.to_string())
                            .map(|events| {
                                let role = account.cfg.owner_role.clone().unwrap_or_default();
                                let key = account.identity().map(|(arn, _)| arn);
                                let (ours, found) = crosscheck::unaccounted(
                                    &events,
                                    &ledgered,
                                    &role,
                                    key.as_deref(),
                                );
                                (events.len(), ours, found)
                            })
                    }
                };
                match checked {
                    Err(e) => {
                        tracing::warn!(account = %account.id, error = %e, "aws: the CloudTrail cross-check failed")
                    }
                    Ok((read, ours, found)) => {
                        for u in &found {
                            tracing::warn!(account = %account.id, event = %u.event, session = %u.session, request = ?u.request_id, "aws: CloudTrail shows a call of Theseus's that the ledger does not account for");
                        }
                        let row = crosscheck::row(&account.id, read, ours, &found);
                        let ledger = LedgerRow::new(LedgerKind::AwsTrailChecked, None, None, row);
                        if let Some(c) = core.upgrade() {
                            if let Err(e) = c.store.append_ledger(&ledger) {
                                tracing::warn!(error = %e, "ledger append failed");
                            }
                        }
                    }
                }
                tokio::time::sleep(crosscheck::EVERY).await;
            }
        });
    }
}
