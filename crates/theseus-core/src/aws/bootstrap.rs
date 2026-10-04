//! `theseus aws bootstrap` (AWS design §5, C2 = 14b; `infra/aws/bootstrap.md`):
//! an account's first stacks, from the templates this binary carries.
//!
//! **The plan is read-only.** For each stack (`theseus-foundation` and
//! `theseus-posture` in the account's region, and `theseus-posture-relay` in
//! us-east-1 when that is another region) it asks CloudFormation whether the
//! stack exists. A new stack's plan is the template's resources under its
//! parameters, read statically (`theseus_aws_guard::planned_resources`): no
//! change set, since a create's change set makes a stack in
//! `REVIEW_IN_PROGRESS`, which is a write. An existing stack is compared with
//! its deployed template and parameters: equal is no change, which is what
//! makes a second plan after the apply show none. It also reads the
//! singletons that would collide (a GuardDuty detector, an account analyzer)
//! and other trails. Its digest covers every stack's template, parameters,
//! action, policy, and what the apply sets on it.
//!
//! **A stack's policy names only resources the stack has.** AWS refuses a
//! stack policy that names a logical id its stack lacks, so each stack's
//! policy file is cut to the resources its template makes under the plan's
//! parameters: the lean posture (`TrailKey=aws-managed`) has no `TrailKmsKey`.
//!
//! **A re-run finishes what a stopped run skipped.** For an existing stack the
//! plan also reads its policy and its termination protection, and the apply
//! sets whichever is missing. A policy that is set but is not this binary's
//! stays as it is, with a warning.
//!
//! **The apply** carries out the plan whose digest the operator approved,
//! after planning again: the foundation by a change set signed with the key
//! (no role exists yet); then, in a floor session of the owner role it made,
//! the posture and the relay by change sets the deployer applies, each
//! change set's resources checked against the plan first; then each stack's
//! policy and termination protection, where the plan says the stack lacks
//! them, with no change set.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_aws::Credentials;
use theseus_aws_guard::{glob, Context, Truth};
use theseus_protocol::{AwsBootstrapParams, AwsBootstrapResult, AwsBootstrapStack};

use super::session::{self, Kind};
use super::stack::{resource_changes, role_arn, Cfn, ChangeSetSpec, Param, Ready, DEPLOYER};
use super::tend::FOUNDATION;
use super::{Account, Failure, Request, Signer};

pub const FOUNDATION_TEMPLATE: &str = include_str!("../../../../infra/aws/theseus-foundation.yaml");
pub const POSTURE_TEMPLATE: &str = include_str!("../../../../infra/aws/theseus-posture.yaml");
pub const RELAY_TEMPLATE: &str = include_str!("../../../../infra/aws/theseus-posture-relay.yaml");
pub const FOUNDATION_POLICY: &str =
    include_str!("../../../../infra/aws/stack-policies/theseus-foundation.json");
pub const POSTURE_POLICY: &str =
    include_str!("../../../../infra/aws/stack-policies/theseus-posture.json");
pub const RELAY_POLICY: &str =
    include_str!("../../../../infra/aws/stack-policies/theseus-posture-relay.json");
/// What the apply sets on a stack besides its change set.
pub const STACK_POLICY: &str = "stack policy";
pub const TERMINATION_PROTECTION: &str = "termination protection";

pub const POSTURE: &str = "theseus-posture";
pub const RELAY: &str = "theseus-posture-relay";
/// The owner role the foundation makes.
pub const OWNER_ROLE: &str = "theseus-owner";
/// Where the global services record their events, and the relay runs.
pub const RELAY_REGION: &str = "us-east-1";
/// The foundation's budget when the config names none: the template's.
pub const DEFAULT_BUDGET_USD: u32 = 50;
/// How long a stack's creation may take.
const CREATE_WITHIN: Duration = Duration::from_secs(20 * 60);

/// One stack of the plan.
#[derive(Clone, Debug)]
pub struct Planned {
    pub stack: &'static str,
    pub region: String,
    pub template: &'static str,
    /// The stack's policy: its file's, naming only `logical_ids`.
    pub policy: String,
    pub parameters: BTreeMap<String, String>,
    /// `create`, `update`, or `none`.
    pub action: &'static str,
    /// A create's resources, `LogicalId (Type)`.
    pub resources: Vec<String>,
    /// The resources the template makes under the parameters, by logical id.
    pub logical_ids: Vec<String>,
    /// An update's changes.
    pub changes: Vec<String>,
    /// What the apply sets besides a change set (`STACK_POLICY`,
    /// `TERMINATION_PROTECTION`): a create, both; an existing stack, what it
    /// lacks.
    pub sets: Vec<&'static str>,
}

/// The whole plan.
#[derive(Debug)]
pub struct Plan {
    pub stacks: Vec<Planned>,
    pub warnings: Vec<String>,
    pub digest: String,
    pub owner_user: String,
}

/// The sha256 of a text, in hex.
fn sha(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn said(f: &Failure) -> String {
    f.to_string()
}

/// The parameters an existing stack holds.
fn held(stack: &Value) -> BTreeMap<String, String> {
    stack["Parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["ParameterKey"].as_str()?.to_string(),
                p["ParameterValue"].as_str().unwrap_or_default().to_string(),
            ))
        })
        .collect()
}

/// A stack's policy from its file, naming only the resources the stack
/// makes (`made`, by logical id): a `LogicalResourceId/<id>` that matches
/// none of them leaves its statement, and a statement left with no resource
/// goes. `"*"` and every other form stay. AWS refuses a policy that names a
/// logical id its stack lacks ("stack policies can only be applied to
/// logical ids referenced in the template"). With nothing cut, the file as
/// written.
pub fn stack_policy(file: &str, made: &[String]) -> Result<String, String> {
    let mut doc: Value =
        serde_json::from_str(file).map_err(|e| format!("a stack policy is not JSON: {e}"))?;
    let has = |r: &Value| match r
        .as_str()
        .and_then(|r| r.strip_prefix("LogicalResourceId/"))
    {
        Some(id) => made.iter().any(|m| glob(id, m)),
        None => true,
    };
    let Some(statements) = doc.get_mut("Statement").and_then(Value::as_array_mut) else {
        return Ok(file.to_string());
    };
    let before = statements.clone();
    statements.retain_mut(|st| match st.get_mut("Resource") {
        Some(Value::Array(rs)) => {
            rs.retain(has);
            !rs.is_empty()
        }
        Some(r) => has(r),
        None => true,
    });
    if *statements == before {
        return Ok(file.to_string());
    }
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

/// Two stack policies say the same, whatever their spacing.
fn same_policy(a: &str, b: &str) -> bool {
    match (
        serde_json::from_str::<Value>(a),
        serde_json::from_str::<Value>(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim() == b.trim(),
    }
}

/// What the plan reads with: a session of the owner role when it exists
/// already (a plan after the apply), so the key signs nothing but STS; else
/// the key, before any role exists.
async fn reader(account: &Arc<Account>) -> Option<Credentials> {
    let want = session::Want {
        kind: Kind::Work,
        name: session::session_name("theseus-bootstrap-plan"),
        execution: None,
        lasts: session::SHORTEST,
        inline: None,
    };
    account.session(&want, Some(OWNER_ROLE), None).await.ok()
}

/// Read-only: the singletons a new posture would collide with, and other
/// trails. Each is a warning, and a parameter.
async fn singletons(
    account: &Arc<Account>,
    signer: Signer<'_>,
    warnings: &mut Vec<String>,
) -> (bool, bool) {
    let region = account.cfg.region.as_str();
    let read = |service: &'static str, op: &'static str, input: Value| {
        let account = account.clone();
        async move {
            let r = Request {
                service,
                operation: op,
                input: &input,
                region,
                pages: 1,
                class: "read",
                signer,
            };
            account.request(None, &r).await
        }
    };
    let guardduty = match read("guardduty", "ListDetectors", json!({})).await {
        Ok(o) => o.body["DetectorIds"]
            .as_array()
            .is_some_and(|d| !d.is_empty()),
        Err(f) => {
            warnings.push(format!(
                "GuardDuty's detectors could not be read ({}); the plan assumes none",
                said(&f)
            ));
            false
        }
    };
    if guardduty {
        warnings.push(format!(
            "a GuardDuty detector exists in {region} already: the posture takes GuardDuty=disabled"
        ));
    }
    let analyzer = match read(
        "accessanalyzer",
        "ListAnalyzers",
        json!({"type": "ACCOUNT"}),
    )
    .await
    {
        Ok(o) => o.body["analyzers"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        Err(f) => {
            warnings.push(format!(
                "Access Analyzer's analyzers could not be read ({}); the plan assumes none",
                said(&f)
            ));
            false
        }
    };
    if analyzer {
        warnings.push(format!(
            "an account analyzer exists in {region} already: the posture takes AccessAnalyzer=disabled"
        ));
    }
    match read("cloudtrail", "DescribeTrails", json!({})).await {
        Ok(o) => {
            let others: Vec<String> = o.body["trailList"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["Name"].as_str())
                .filter(|n| *n != "theseus-trail")
                .map(String::from)
                .collect();
            if !others.is_empty() {
                warnings.push(format!(
                    "other trails exist ({}): theseus-trail's management events are free only as \
                     the account's first copy, $2 per 100,000 events otherwise",
                    others.join(", ")
                ));
            }
        }
        Err(f) => warnings.push(format!(
            "the account's trails could not be read ({})",
            said(&f)
        )),
    }
    (guardduty, analyzer)
}

/// The plan: read-only.
pub async fn plan(account: &Arc<Account>, p: &AwsBootstrapParams) -> Result<Plan, String> {
    if let Some(t) = p.trail_key.as_deref() {
        if !matches!(t, "aws-managed" | "customer") {
            return Err(format!("trail_key {t:?} is not aws-managed or customer"));
        }
    }
    account.root_key().await?;
    let owner_user = account.user_name().ok_or_else(|| {
        format!(
            "account {}'s key is not an IAM user's, so nothing can name the owner role's one \
             principal: the bootstrap needs the root-of-trust user's key",
            account.id
        )
    })?;
    let session = reader(account).await;
    let signer = session.as_ref().map_or(Signer::Key, Signer::With);
    let home = account.cfg.region.clone();
    let mut warnings = Vec::new();
    let cfn = Cfn {
        account,
        region: &home,
        binding: None,
        signer,
    };
    let found = cfn.stack(FOUNDATION).await.map_err(|f| said(&f))?;
    let params = foundation_params(account, p, &owner_user, found.as_ref());
    let mut stacks = vec![
        stack_plan(
            &cfn,
            FOUNDATION,
            FOUNDATION_TEMPLATE,
            FOUNDATION_POLICY,
            params,
            found,
            &mut warnings,
        )
        .await?,
    ];
    let posture = cfn.stack(POSTURE).await.map_err(|f| said(&f))?;
    let (guardduty, analyzer) = if posture.is_some() {
        (false, false)
    } else {
        singletons(account, signer, &mut warnings).await
    };
    let params = posture_params(p, &owner_user, posture.as_ref(), guardduty, analyzer);
    stacks.push(
        stack_plan(
            &cfn,
            POSTURE,
            POSTURE_TEMPLATE,
            POSTURE_POLICY,
            params,
            posture,
            &mut warnings,
        )
        .await?,
    );
    if home != RELAY_REGION {
        let east = Cfn {
            account,
            region: RELAY_REGION,
            binding: None,
            signer,
        };
        let relay = east.stack(RELAY).await.map_err(|f| said(&f))?;
        let params = BTreeMap::from([("HomeRegion".to_string(), home.clone())]);
        stacks.push(
            stack_plan(
                &east,
                RELAY,
                RELAY_TEMPLATE,
                RELAY_POLICY,
                params,
                relay,
                &mut warnings,
            )
            .await?,
        );
    }
    Ok(Plan {
        digest: digest(&stacks),
        stacks,
        warnings,
        owner_user,
    })
}

/// The foundation's parameters: the key's user, the config's budget (else
/// the stack's, else the template's), and the alert address (else the
/// stack's).
fn foundation_params(
    account: &Account,
    p: &AwsBootstrapParams,
    owner_user: &str,
    existing: Option<&Value>,
) -> BTreeMap<String, String> {
    let had = existing.map(held).unwrap_or_default();
    let budget = account
        .cfg
        .monthly_budget_usd
        .map(|n| n.to_string())
        .or_else(|| had.get("MonthlyBudgetUsd").cloned())
        .unwrap_or_else(|| DEFAULT_BUDGET_USD.to_string());
    let email = p
        .alert_email
        .clone()
        .or_else(|| had.get("AlertEmail").cloned())
        .unwrap_or_default();
    BTreeMap::from([
        ("OwnerUserName".to_string(), owner_user.to_string()),
        ("MonthlyBudgetUsd".to_string(), budget),
        ("AlertEmail".to_string(), email),
    ])
}

/// The posture's parameters: what the operator chose, else what the stack
/// holds, else the defaults, a singleton that exists already turned off.
fn posture_params(
    p: &AwsBootstrapParams,
    owner_user: &str,
    existing: Option<&Value>,
    guardduty: bool,
    analyzer: bool,
) -> BTreeMap<String, String> {
    let had = existing.map(held).unwrap_or_default();
    let keep = |k: &str, new: &str| had.get(k).cloned().unwrap_or_else(|| new.to_string());
    let off = |exists: bool| if exists { "disabled" } else { "enabled" };
    BTreeMap::from([
        ("FoundationStack".to_string(), FOUNDATION.to_string()),
        ("OwnerUserName".to_string(), owner_user.to_string()),
        (
            "TrailKey".to_string(),
            p.trail_key
                .clone()
                .unwrap_or_else(|| keep("TrailKey", "aws-managed")),
        ),
        ("GuardDuty".to_string(), keep("GuardDuty", off(guardduty))),
        (
            "AccessAnalyzer".to_string(),
            keep("AccessAnalyzer", off(analyzer)),
        ),
        (
            "SnapshotPublicSharing".to_string(),
            keep("SnapshotPublicSharing", "block-all-sharing"),
        ),
    ])
}

/// The plan's digest: every stack's name, region, action, template,
/// parameters, policy, and what the apply sets on it.
pub(super) fn digest(stacks: &[Planned]) -> String {
    let all: Vec<Value> = stacks
        .iter()
        .map(|s| {
            json!([
                s.stack,
                s.region,
                s.action,
                sha(s.template),
                s.parameters,
                sha(&s.policy),
                s.sets
            ])
        })
        .collect();
    sha(&json!(all).to_string())[..16].to_string()
}

/// One stack's plan: a create's resources, or an update's changes, or none;
/// its policy, cut to the resources the template makes under the
/// parameters (for an existing stack, the template and parameters the plan
/// applies); and what the apply sets besides a change set.
async fn stack_plan(
    cfn: &Cfn<'_>,
    stack: &'static str,
    template: &'static str,
    policy: &'static str,
    parameters: BTreeMap<String, String>,
    existing: Option<Value>,
    warnings: &mut Vec<String>,
) -> Result<Planned, String> {
    let parsed = theseus_aws_guard::parse_template(template).map_err(|e| e.to_string())?;
    let ctx = Context {
        account: cfn.account.id.clone(),
        region: cfn.region.to_string(),
    };
    let made = theseus_aws_guard::planned_resources(&parsed, &ctx, &parameters)
        .map_err(|e| e.to_string())?;
    let logical_ids: Vec<String> = made.iter().map(|r| r.logical_id.clone()).collect();
    let mut p = Planned {
        stack,
        region: cfn.region.to_string(),
        template,
        policy: stack_policy(policy, &logical_ids)?,
        parameters,
        action: "none",
        resources: Vec::new(),
        logical_ids,
        changes: Vec::new(),
        sets: Vec::new(),
    };
    let Some(existing) = existing else {
        p.resources = made
            .iter()
            .map(|r| {
                let maybe = if r.exists == Truth::Maybe {
                    ", if its condition holds"
                } else {
                    ""
                };
                format!("{} ({}{maybe})", r.logical_id, r.resource_type)
            })
            .collect();
        p.action = "create";
        p.sets = vec![STACK_POLICY, TERMINATION_PROTECTION];
        return Ok(p);
    };
    let status = existing["StackStatus"].as_str().unwrap_or("?");
    if !status.ends_with("_COMPLETE") || status.contains("ROLLBACK") || status.starts_with("DELETE")
    {
        return Err(format!(
            "stack {stack} in {} is {status}: the bootstrap changes only a settled stack; \
             aws.stack.status shows why it is so",
            cfn.region
        ));
    }
    let deployed = cfn.template(stack).await.map_err(|f| said(&f))?;
    if deployed.trim_end() != template.trim_end() {
        p.changes
            .push("its template differs from this binary's".into());
    }
    let had = held(&existing);
    for (k, v) in &p.parameters {
        match had.get(k) {
            Some(h) if h == v => {}
            Some(h) => p.changes.push(format!("{k}: {h} to {v}")),
            None => p.changes.push(format!("{k}: new, {v}")),
        }
    }
    if !p.changes.is_empty() {
        p.action = "update";
    }
    unprotected(cfn, &mut p, &existing, warnings).await?;
    Ok(p)
}

/// What an existing stack lacks, which a run stopped after its change set
/// left unset: its policy (`GetStackPolicy`) and its termination protection
/// (`DescribeStacks`). A policy that is set but is not this binary's stays as
/// it is, with a warning: a hand edit is the operator's, and `aws.stack.*`
/// governs the stack after the bootstrap.
async fn unprotected(
    cfn: &Cfn<'_>,
    p: &mut Planned,
    existing: &Value,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    match cfn.policy(p.stack).await.map_err(|f| said(&f))? {
        None => p.sets.push(STACK_POLICY),
        Some(held) if !same_policy(&held, &p.policy) => warnings.push(format!(
            "{} in {} has a stack policy that is not the one this binary would set: the bootstrap \
             leaves it as it is, and aws.stack.* governs the stack from here",
            p.stack, p.region
        )),
        Some(_) => {}
    }
    if existing["EnableTerminationProtection"].as_bool() != Some(true) {
        p.sets.push(TERMINATION_PROTECTION);
    }
    Ok(())
}

/// The plan as the wire carries it.
pub fn result(account: &Account, plan: &Plan, applied: bool) -> AwsBootstrapResult {
    let changes = plan
        .stacks
        .iter()
        .any(|s| s.action != "none" || !s.sets.is_empty());
    let mut next = Vec::new();
    if applied {
        if plan
            .stacks
            .iter()
            .any(|s| s.stack == FOUNDATION && s.action == "create")
            && !plan.stacks[0].parameters["AlertEmail"].is_empty()
        {
            next.push(
                "SNS mails the alert address a confirmation link: do not open it (opening it lets \
                 any alert's link unsubscribe the address). Copy the link's address, or its \
                 Token=… value, and run `theseus aws confirm-alerts <token>`, which confirms it \
                 so only the account can unsubscribe."
                    .into(),
            );
        }
        if account.cfg.owner_role.is_none() {
            next.push(format!(
                "Add owner_role = \"{OWNER_ROLE}\" to [aws.accounts.{}] in the config and restart: \
                 every call then signs in a role session named by its execution, and the key signs \
                 only STS.",
                account.id
            ));
        }
        next.push(
            "Account settings with no CloudFormation type (S3's account-level Block Public Access, \
             EBS encryption by default) are infra/aws/bootstrap.md's step 4, made once by hand."
                .into(),
        );
    } else if changes {
        next.push(format!(
            "Apply it with `theseus aws bootstrap --apply {}` and this plan's own --alert-email \
             and --trail-key (the digest covers them), or answer yes at the question.",
            plan.digest
        ));
    } else {
        next.push("Nothing to do: every stack is as this binary plans it.".into());
    }
    AwsBootstrapResult {
        account: account.id.clone(),
        region: account.cfg.region.clone(),
        stacks: plan
            .stacks
            .iter()
            .map(|s| AwsBootstrapStack {
                stack: s.stack.into(),
                region: s.region.clone(),
                action: s.action.into(),
                resources: s.resources.clone(),
                changes: s.changes.clone(),
                parameters: s.parameters.clone(),
                sets: s.sets.iter().map(|x| x.to_string()).collect(),
                policy: s.policy.clone(),
            })
            .collect(),
        warnings: plan.warnings.clone(),
        digest: plan.digest.clone(),
        changes,
        applied,
        next,
    }
}

/// The floor session the apply makes its guarded changes in (§3.6): the
/// owner role it made, allow-all, no guard. IAM may take a few seconds to
/// know a new role, so the first mint is tried for up to a minute.
async fn floor(account: &Arc<Account>, name: &str) -> Result<Credentials, String> {
    let want = session::Want {
        kind: Kind::Floor,
        name: session::session_name(name),
        execution: None,
        lasts: Duration::from_secs(3600),
        inline: None,
    };
    let mut last = String::new();
    for _ in 0..12 {
        match account.session(&want, Some(OWNER_ROLE), None).await {
            Ok(c) => return Ok(c),
            Err(e) => last = e,
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Err(last)
}

/// Apply the plan whose digest is `digest`, after planning again.
pub async fn apply(
    account: &Arc<Account>,
    p: &AwsBootstrapParams,
    digest: &str,
) -> Result<Plan, String> {
    let plan = plan(account, p).await?;
    if plan.digest != digest {
        return Err(format!(
            "the plan changed since {digest} (it is {} now): run theseus aws bootstrap again, and \
             apply what it shows",
            plan.digest
        ));
    }
    let name = format!("bootstrap-{}", theseus_protocol::now_unix_ms());
    let mut floor_creds: Option<Credentials> = None;
    for s in plan
        .stacks
        .iter()
        .filter(|s| s.action != "none" || !s.sets.is_empty())
    {
        let first = s.stack == FOUNDATION && s.action == "create";
        if !first && floor_creds.is_none() {
            floor_creds = Some(floor(account, &format!("{name}.floor")).await?);
        }
        if s.action != "none" {
            let signer = if first {
                Signer::Key
            } else {
                Signer::With(floor_creds.as_ref().expect("minted"))
            };
            apply_stack(account, s, signer, &name, !first).await?;
        }
        if first {
            floor_creds = Some(floor(account, &format!("{name}.floor")).await?);
        }
        set_what_it_lacks(account, s, floor_creds.as_ref().expect("minted")).await?;
    }
    Ok(plan)
}

/// What the stack's plan says the apply sets on it besides a change set,
/// in the floor session: its policy, its termination protection.
async fn set_what_it_lacks(
    account: &Arc<Account>,
    s: &Planned,
    floor: &Credentials,
) -> Result<(), String> {
    let cfn = Cfn {
        account,
        region: &s.region,
        binding: None,
        signer: Signer::With(floor),
    };
    if s.sets.contains(&STACK_POLICY) {
        cfn.set_policy(s.stack, &s.policy)
            .await
            .map_err(|f| said(&f))?;
    }
    if s.sets.contains(&TERMINATION_PROTECTION) {
        cfn.protect(s.stack).await.map_err(|f| said(&f))?;
    }
    Ok(())
}

/// One stack's change set: made, checked against the plan, executed, and
/// waited for.
async fn apply_stack(
    account: &Arc<Account>,
    s: &Planned,
    signer: Signer<'_>,
    execution: &str,
    deployer: bool,
) -> Result<(), String> {
    let cfn = Cfn {
        account,
        region: &s.region,
        binding: None,
        signer,
    };
    let spec = ChangeSetSpec {
        stack: s.stack,
        name: format!("theseus-{execution}"),
        kind: if s.action == "create" {
            "CREATE"
        } else {
            "UPDATE"
        },
        body: Some(s.template),
        url: None,
        parameters: s
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), Param::Value(v.clone())))
            .collect(),
        role: deployer.then(|| role_arn(&account.id, DEPLOYER)),
        tags: vec![
            ("theseus:owner".into(), "theseus".into()),
            ("theseus:stack".into(), s.stack.into()),
            ("theseus:deployment".into(), account.cfg.deployment().into()),
            ("theseus:execution".into(), execution.into()),
        ],
        description: format!("theseus aws bootstrap ({execution})"),
    };
    let id = cfn.create_change_set(&spec).await.map_err(|f| said(&f))?;
    let changes = match cfn.ready(s.stack, &id).await.map_err(|f| said(&f))? {
        Ready::Changes { changes, .. } => changes,
        Ready::Empty { .. } => {
            let _ = cfn.delete_change_set(s.stack, &id).await;
            return Ok(());
        }
        Ready::Failed { reason, .. } => {
            return Err(format!("{}'s change set failed: {reason}", s.stack))
        }
    };
    if let Some(why) = differs(s, &changes) {
        let _ = cfn.delete_change_set(s.stack, &id).await;
        return Err(format!(
            "{}'s change set is not the plan, so nothing was applied: {why}",
            s.stack
        ));
    }
    cfn.execute(s.stack, &id).await.map_err(|f| said(&f))?;
    let settled = cfn
        .settle(s.stack, CREATE_WITHIN)
        .await
        .map_err(|f| said(&f))?;
    let done = if s.action == "create" {
        "CREATE_COMPLETE"
    } else {
        "UPDATE_COMPLETE"
    };
    if settled.status != done {
        return Err(format!(
            "{} in {} ended {}{}{}",
            s.stack,
            s.region,
            settled.status,
            settled
                .reason
                .map(|r| format!(" ({r})"))
                .unwrap_or_default(),
            settled
                .failures
                .first()
                .map(|f| format!("; first failure {f}"))
                .unwrap_or_default()
        ));
    }
    Ok(())
}

/// Why a change set is not the stack's plan: a create adds other resources
/// than planned; an update replaces or removes what holds state, or touches a
/// guardrail (those go through aws.stack.plan and .apply instead).
fn differs(s: &Planned, changes: &[Value]) -> Option<String> {
    let rc = resource_changes(changes);
    if s.action == "create" {
        let mut added: Vec<&str> = rc.iter().map(|c| c.logical_id.as_str()).collect();
        let mut planned: Vec<&str> = s.logical_ids.iter().map(String::as_str).collect();
        added.sort_unstable();
        planned.sort_unstable();
        return (added != planned).then(|| {
            format!(
                "it adds {} where the plan made {}",
                added.join(", "),
                planned.join(", ")
            )
        });
    }
    let v = theseus_aws_guard::embedded().check_change_set(&rc);
    let lines: Vec<String> = v
        .floor
        .iter()
        .chain(&v.destructive)
        .map(|h| h.why.clone())
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "{} (plan and apply it with aws.stack.plan and aws.stack.apply, where the operator \
             approves each)",
            lines.join("; ")
        )
    })
}
