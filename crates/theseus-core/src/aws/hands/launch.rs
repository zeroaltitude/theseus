//! `aws.hands.run`'s request, its backend, and each hand's launch (AWS design
//! §3.3): Lambda's asynchronous `Invoke` of the hand function, or ECS's
//! `RunTask` on Fargate in the hands VPC's private subnets.
//!
//! - **Where hands run** ([`HandsEnv`]) is read once per account from the
//!   stacks' outputs (`theseus-foundation`, `theseus-hands`,
//!   `theseus-hands-network`) at the first call, and kept: no config key.
//! - **The backend** ([`choose`]): the model's when it names one; else, until
//!   Jev chooses (`shell.v1`), Lambda for a hand that fits 10 minutes and 10
//!   GB on the hand image under the basic profile, and Fargate for the rest.
//!   Batch arrays are not built: a group of more than ten goes to the same
//!   two.
//! - **The NAT stays off.** A Fargate hand needs egress (its image, its
//!   logs, the queue), so it launches only while the network stack's
//!   `NatGateway` reads `enabled`; otherwise the call fails with the reason
//!   and the stack update that turns it on (about $36 a month while on).
//!   Nothing here turns it on: that is the operator's, through the stack
//!   path. Lambda hands need no VPC, and are the default. In an existing
//!   VPC (`NatGateway` reads `existing`), its own NAT is the way out:
//!   [`super::network::egress`] reads its routes, and the refusal names the
//!   subnet that has none, never a NAT to turn on.
//! - **Tags** on every task and task definition: `theseus:owner`,
//!   `theseus:execution`, `theseus:session`, `theseus:group`,
//!   `theseus:correlation`, `theseus:ttl`, and `theseus:deployment`;
//!   `startedBy` is the group. A Lambda invocation takes no tags: its
//!   function's timeout is its TTL's second layer.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_tools::AwsBinding;

use super::envelope::{HandSpec, VERSION};
use crate::aws::session::Kind;
use crate::aws::{Account, Request, Signer};

/// The tool's name.
pub const RUN: &str = "aws.hands.run";

/// A hand's own action's tool name: one per hand, beside the group's call.
pub const HAND: &str = "aws.hand";

/// The most hands one call starts.
pub const MAX_HANDS: usize = 100;

/// Lambda's limits for a hand (§3.3's table): 15 minutes is its timeout;
/// the tool keeps to 10 for a default choice.
const LAMBDA_FITS_SECS: u64 = 600;
const LAMBDA_MAX_SECS: u64 = 900;
const LAMBDA_MAX_MB: u64 = 10_240;

/// A hand's TTL unless the call names one.
pub const DEFAULT_TTL_SECS: u64 = 600;

/// The longest TTL a call may name: a Fargate hand's, 12 hours.
const MAX_TTL_SECS: u64 = 12 * 3600;

/// The wrapper's deadline is its TTL less this, so it reports before its
/// backend's own limit ends it.
const REPORT_MARGIN_SECS: u64 = 30;

/// When to finish a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    All,
    FirstSuccess,
    Quorum(u32),
}

impl Until {
    pub fn words(self) -> String {
        match self {
            Until::All => "all".into(),
            Until::FirstSuccess => "first_success".into(),
            Until::Quorum(n) => format!("a quorum of {n}"),
        }
    }
}

/// `"all"`, `"first_success"`, or `{"quorum": n}`.
fn until(v: &Value) -> Result<Until, String> {
    match v {
        Value::Null => Ok(Until::All),
        Value::String(s) if s == "all" => Ok(Until::All),
        Value::String(s) if s == "first_success" => Ok(Until::FirstSuccess),
        Value::Object(o) if o.len() == 1 && o.get("quorum").is_some_and(Value::is_u64) => {
            Ok(Until::Quorum(o["quorum"].as_u64().unwrap_or(1) as u32))
        }
        other => Err(format!(
            "until is \"all\", \"first_success\", or {{\"quorum\": n}}, not {other}"
        )),
    }
}

/// Where a hand runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Lambda,
    Fargate,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Lambda => "lambda",
            Backend::Fargate => "fargate",
        }
    }
}

/// The call's input, as the model writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    argv: Vec<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    inputs: Option<Vec<Value>>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    vcpu: Option<f64>,
    #[serde(default)]
    memory_mb: Option<u64>,
    #[serde(default)]
    ttl_secs: Option<u64>,
    #[serde(default)]
    max_usd: Option<f64>,
    #[serde(default)]
    until: Value,
    #[serde(default)]
    concurrency: Option<u32>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    backend: Option<Backend>,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    region: Option<String>,
}

/// A call's request, checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandsRequest {
    pub argv: Vec<String>,
    /// One per hand: an item of `inputs`, or null for each of `count`.
    pub inputs: Vec<Value>,
    pub image: Option<String>,
    pub vcpu: f64,
    pub memory_mb: u64,
    pub ttl_secs: u64,
    pub max_usd: Option<f64>,
    pub until: Until,
    /// The most hands running at once; the rest wait for a place.
    pub concurrency: u32,
    /// The role profile: `theseus-hand-<profile>`.
    pub profile: String,
    /// The backend the model named, if it did.
    pub backend: Option<Backend>,
    pub account: Option<String>,
    pub region: Option<String>,
}

/// The call's input, checked, or why it is invalid.
pub fn parse(input: &Value) -> Result<HandsRequest, String> {
    let a: Args = theseus_tools::parse(input)?;
    if a.argv.is_empty() || a.argv[0].is_empty() {
        return Err("argv names the program and its arguments; it is empty".into());
    }
    let inputs = match (a.count, a.inputs) {
        (Some(_), Some(_)) => return Err("give count or inputs, not both".into()),
        (None, Some(i)) => i,
        (Some(n), None) => vec![Value::Null; n as usize],
        (None, None) => vec![Value::Null],
    };
    if inputs.is_empty() || inputs.len() > MAX_HANDS {
        return Err(format!(
            "a call starts 1 to {MAX_HANDS} hands, not {}",
            inputs.len()
        ));
    }
    let ttl_secs = a.ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
    if !(60..=MAX_TTL_SECS).contains(&ttl_secs) {
        return Err(format!("ttl_secs is 60 to {MAX_TTL_SECS}, not {ttl_secs}"));
    }
    let until = until(&a.until)?;
    if let Until::Quorum(q) = until {
        if q == 0 || q as usize > inputs.len() {
            return Err(format!("a quorum is 1 to {}, not {q}", inputs.len()));
        }
    }
    if let Some(m) = a.max_usd {
        if m.is_nan() || m <= 0.0 {
            return Err(format!("max_usd is more than 0, not {m}"));
        }
    }
    let profile = a.profile.unwrap_or_else(|| "basic".into());
    if !["basic", "read", "owner"].contains(&profile.as_str()) {
        return Err(format!(
            "profile is basic, read, or owner (the hands stack's theseus-hand-*), not {profile}"
        ));
    }
    let vcpu = a.vcpu.unwrap_or(1.0);
    let memory_mb = a.memory_mb.unwrap_or(2048);
    let n = inputs.len() as u32;
    Ok(HandsRequest {
        argv: a.argv,
        inputs,
        image: a.image,
        vcpu,
        memory_mb,
        ttl_secs,
        max_usd: a.max_usd,
        until,
        concurrency: a.concurrency.unwrap_or(n).clamp(1, n),
        profile,
        backend: a.backend,
        account: a.account,
        region: a.region,
    })
}

/// Fargate's CPU units and memory for a size, when it has that size.
pub fn fargate_size(vcpu: f64, memory_mb: u64) -> Option<(u32, u64)> {
    let gb = |lo: u64, hi: u64, step: u64| {
        (memory_mb >= lo * 1024 && memory_mb <= hi * 1024 && memory_mb.is_multiple_of(step * 1024))
            .then_some(memory_mb)
    };
    let units = (vcpu * 1024.0).round() as u32;
    let mem = match units {
        256 => [512, 1024, 2048].contains(&memory_mb).then_some(memory_mb),
        512 => gb(1, 4, 1),
        1024 => gb(2, 8, 1),
        2048 => gb(4, 16, 1),
        4096 => gb(8, 30, 1),
        8192 => gb(16, 60, 4),
        16384 => gb(32, 120, 8),
        _ => None,
    }?;
    Some((units, mem))
}

/// The backend a request runs on (§3.3, until Jev chooses), or why it has
/// none.
pub fn choose(r: &HandsRequest) -> Result<Backend, String> {
    let lambda_fits = r.image.is_none()
        && r.profile == "basic"
        && r.memory_mb <= LAMBDA_MAX_MB
        && r.ttl_secs <= LAMBDA_MAX_SECS;
    match r.backend {
        Some(Backend::Lambda) if !lambda_fits => Err(format!(
            "a Lambda hand runs the hand image under the basic profile for at most {} minutes \
             and {} GB; this one asks for {}{}{}",
            LAMBDA_MAX_SECS / 60,
            LAMBDA_MAX_MB / 1024,
            if r.image.is_some() {
                "its own image, "
            } else {
                ""
            },
            if r.profile != "basic" {
                format!("the {} profile, ", r.profile)
            } else {
                String::new()
            },
            format_args!("{} s and {} MB", r.ttl_secs, r.memory_mb)
        )),
        Some(b) => Ok(b),
        None if lambda_fits && r.ttl_secs <= LAMBDA_FITS_SECS => Ok(Backend::Lambda),
        None => Ok(Backend::Fargate),
    }
    .and_then(|b| match b {
        Backend::Fargate if fargate_size(r.vcpu, r.memory_mb).is_none() => Err(format!(
            "Fargate has no size of {} vCPU with {} MB (0.25 to 16 vCPU, each with its own \
             range of memory)",
            r.vcpu, r.memory_mb
        )),
        b => Ok(b),
    })
}

/// Rates for the worst-case estimate (us-west-2, x86, on demand, as of
/// 2026-10): a price table the weekly updater refreshes is part 2's.
const LAMBDA_USD_PER_GB_SECOND: f64 = 0.000_016_666_7;
const LAMBDA_USD_PER_REQUEST: f64 = 0.000_000_2;
const FARGATE_USD_PER_VCPU_HOUR: f64 = 0.040_48;
const FARGATE_USD_PER_GB_HOUR: f64 = 0.004_445;

/// What one hand costs at most, or for `secs`: its backend's rate for its
/// size. A Lambda hand is priced at its function's memory.
pub fn cost_usd(backend: Backend, r: &HandsRequest, secs: f64, lambda_mb: u64) -> f64 {
    match backend {
        Backend::Lambda => {
            LAMBDA_USD_PER_REQUEST + secs * (lambda_mb as f64 / 1024.0) * LAMBDA_USD_PER_GB_SECOND
        }
        Backend::Fargate => {
            let hours = secs.max(60.0) / 3600.0;
            hours
                * (r.vcpu * FARGATE_USD_PER_VCPU_HOUR
                    + (r.memory_mb as f64 / 1024.0) * FARGATE_USD_PER_GB_HOUR)
        }
    }
}

/// Where the account's hands run: its stacks' outputs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct HandsEnv {
    pub bucket: String,
    pub queue_url: String,
    pub cluster_arn: String,
    pub log_group: String,
    pub subnets: Vec<String>,
    pub security_group: String,
    /// A way out through a NAT: the network stack's own, as its parameter
    /// last set it, or an existing VPC's, its routes read at discovery.
    pub nat: bool,
    /// An existing VPC's routes, as discovery read them (theseus-mgw.9):
    /// `None` in the stack's own VPC; else every subnet routes out through a
    /// NAT, or why not. Never stored: a group's record keeps `nat`.
    #[serde(skip)]
    pub existing: Option<Result<(), String>>,
    /// The Lambda hand, once the hand image exists.
    pub lambda_function: Option<String>,
    /// The hand image the stack names (`HandImageUri`).
    pub hand_image: Option<String>,
    pub execution_role_arn: String,
    /// `basic`, `read`, `owner` → the role's ARN.
    pub roles: BTreeMap<String, String>,
    /// The Lambda hand's memory, for its cost.
    pub lambda_memory_mb: u64,
}

/// One stack's outputs and parameters, by key.
async fn stack(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
    name: &str,
) -> Result<BTreeMap<String, String>, String> {
    let input = json!({"StackName": name});
    let out = account
        .request(
            binding,
            &Request {
                service: "cloudformation",
                operation: "DescribeStacks",
                input: &input,
                region,
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| format!("the {name} stack: {e}"))?;
    let s = &out.body["Stacks"][0];
    // Parameters, then outputs, so an output wins where a key is both: the
    // network stack's `NatGateway` output reads `existing` in an existing
    // VPC while its parameter stays `disabled` (theseus-mgw.9).
    let mut m = BTreeMap::new();
    for (list, k, v) in [
        ("Parameters", "ParameterKey", "ParameterValue"),
        ("Outputs", "OutputKey", "OutputValue"),
    ] {
        for o in s[list].as_array().into_iter().flatten() {
            if let (Some(k), Some(v)) = (o[k].as_str(), o[v].as_str()) {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    if m.is_empty() {
        return Err(format!(
            "the {name} stack has no outputs: is it deployed in {region}? (infra/aws/README.md)"
        ));
    }
    Ok(m)
}

/// Read where hands run from the three stacks.
pub async fn discover(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
) -> Result<HandsEnv, String> {
    let f = stack(account, binding, region, "theseus-foundation").await?;
    let h = stack(account, binding, region, "theseus-hands").await?;
    let n = stack(account, binding, region, "theseus-hands-network").await;
    let need = |m: &BTreeMap<String, String>, stack: &str, k: &str| {
        m.get(k)
            .cloned()
            .ok_or_else(|| format!("the {stack} stack has no output {k}"))
    };
    let mut roles = BTreeMap::new();
    for (p, k) in [
        ("basic", "HandBasicRoleArn"),
        ("read", "HandReadRoleArn"),
        ("owner", "HandOwnerRoleArn"),
    ] {
        if let Some(v) = h.get(k) {
            roles.insert(p.to_string(), v.clone());
        }
    }
    // The network stack is Fargate's alone: without it, Lambda hands run.
    let n = n.unwrap_or_default();
    let subnets: Vec<String> = n
        .get("PrivateSubnetIds")
        .map(|s| s.split(',').map(String::from).collect())
        .unwrap_or_default();
    // An existing VPC's subnets route out through its own NAT, or Fargate
    // hands do not run there (theseus-mgw.9).
    let existing = match n.get("NatGateway").map(String::as_str) {
        Some("existing") => {
            let vpc = n.get("VpcId").map_or("?", String::as_str);
            Some(super::network::egress(account, binding, region, vpc, &subnets).await)
        }
        _ => None,
    };
    Ok(HandsEnv {
        bucket: need(&f, "theseus-foundation", "BucketName")?,
        queue_url: need(&f, "theseus-foundation", "CompletionQueueUrl")?,
        cluster_arn: h.get("ClusterArn").cloned().unwrap_or_default(),
        log_group: h
            .get("HandsLogGroupName")
            .cloned()
            .unwrap_or_else(|| "/theseus/hands".into()),
        subnets,
        security_group: n.get("HandsSecurityGroupId").cloned().unwrap_or_default(),
        nat: n.get("NatGateway").is_some_and(|v| v == "enabled")
            || matches!(existing, Some(Ok(()))),
        existing,
        lambda_function: h.get("LambdaHandArn").cloned(),
        hand_image: h.get("HandImageUri").filter(|s| !s.is_empty()).cloned(),
        execution_role_arn: h.get("HandExecutionRoleArn").cloned().unwrap_or_default(),
        roles,
        lambda_memory_mb: h
            .get("LambdaHandMemoryMb")
            .and_then(|m| m.parse().ok())
            .unwrap_or(2048),
    })
}

/// Why `env` cannot run this backend, if it cannot.
pub fn unready(env: &HandsEnv, backend: Backend, r: &HandsRequest) -> Option<String> {
    match backend {
        Backend::Lambda if env.lambda_function.is_none() => Some(
            "the Lambda hand does not exist yet: the theseus-hands stack makes it once its \
             HandImageUri parameter names the pushed hand image (infra/aws/hand/README.md)"
                .into(),
        ),
        Backend::Fargate if env.subnets.is_empty() || env.cluster_arn.is_empty() => Some(
            "Fargate hands need the theseus-hands-network and theseus-hands stacks, deployed"
                .into(),
        ),
        Backend::Fargate if matches!(env.existing, Some(Err(_))) => {
            env.existing.clone().and_then(Result::err)
        }
        Backend::Fargate if !env.nat => Some(
            "Fargate hands need the hands VPC's NAT, which is off (it costs about $36 a month \
             while on): the operator turns it on with aws.stack.plan and aws.stack.apply of \
             theseus-hands-network with NatGateway=enabled, or the call runs on Lambda"
                .into(),
        ),
        Backend::Fargate if r.image.is_none() && env.hand_image.is_none() => Some(
            "the hand image is not pushed yet (the theseus-hands stack's HandImageUri is empty)"
                .into(),
        ),
        Backend::Fargate if !env.roles.contains_key(&r.profile) => Some(format!(
            "the theseus-hands stack has no theseus-hand-{} role",
            r.profile
        )),
        _ => None,
    }
}

/// What a group's every hand carries.
pub struct GroupIds<'a> {
    pub group: &'a str,
    pub execution_id: &'a str,
    pub session_id: &'a str,
    pub deployment: &'a str,
    /// `theseus:ttl`: when the group's hands are past their TTL, in UTC.
    pub ttl_at: String,
}

/// The tags of a hand's task (§3.7), in ECS's form.
pub fn tags(ids: &GroupIds<'_>, correlation_id: Option<&str>) -> Vec<Value> {
    let mut t = vec![
        ("theseus:owner", "theseus"),
        ("theseus:execution", ids.execution_id),
        ("theseus:session", ids.session_id),
        ("theseus:group", ids.group),
        ("theseus:ttl", ids.ttl_at.as_str()),
        ("theseus:deployment", ids.deployment),
    ];
    if let Some(c) = correlation_id {
        t.push(("theseus:correlation", c));
    }
    t.into_iter()
        .map(|(k, v)| json!({"key": k, "value": v}))
        .collect()
}

/// A hand's spec, for its launch.
#[expect(clippy::too_many_arguments, reason = "each is one of the spec's own")]
pub fn spec(
    env: &HandsEnv,
    r: &HandsRequest,
    backend: Backend,
    group: &str,
    index: u32,
    correlation_id: &str,
    key: &[u8],
    region: &str,
    endpoint: Option<&str>,
) -> HandSpec {
    HandSpec {
        v: VERSION,
        correlation_id: correlation_id.into(),
        group: group.into(),
        index,
        input: r.inputs.get(index as usize).cloned().unwrap_or(Value::Null),
        argv: r.argv.clone(),
        deadline_secs: r.ttl_secs.saturating_sub(REPORT_MARGIN_SECS).max(30),
        region: region.into(),
        bucket: env.bucket.clone(),
        queue_url: env.queue_url.clone(),
        log_group: env.log_group.clone(),
        backend: backend.as_str().into(),
        key: hex::encode(key),
        endpoint: endpoint.map(String::from),
    }
}

/// The group's Fargate task definition: the hand image's `theseusd hand`;
/// or, for another image, an init container that copies the binary into a
/// shared volume and the image running it (ECS container dependencies).
/// Its ARN.
pub async fn register_task_definition(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
    env: &HandsEnv,
    r: &HandsRequest,
    ids: &GroupIds<'_>,
) -> Result<String, String> {
    let (cpu, mem) = fargate_size(r.vcpu, r.memory_mb).ok_or("no Fargate size")?;
    let hand_image = env.hand_image.clone().unwrap_or_default();
    let logs = |prefix: &str| {
        json!({"logDriver": "awslogs", "options": {
            "awslogs-group": env.log_group, "awslogs-region": region,
            "awslogs-stream-prefix": prefix}})
    };
    let containers = match &r.image {
        None => vec![json!({
            "name": "hand", "image": hand_image, "essential": true,
            "command": ["hand"], "logConfiguration": logs("ecs"),
        })],
        Some(image) => vec![
            json!({
                "name": "theseus", "image": hand_image, "essential": false,
                "entryPoint": ["/bin/cp"],
                "command": ["/usr/local/bin/theseusd", "/hand/theseusd"],
                "mountPoints": [{"sourceVolume": "hand", "containerPath": "/hand"}],
                "logConfiguration": logs("init"),
            }),
            json!({
                "name": "hand", "image": image, "essential": true,
                "entryPoint": ["/hand/theseusd"], "command": ["hand"],
                "dependsOn": [{"containerName": "theseus", "condition": "SUCCESS"}],
                "mountPoints": [{"sourceVolume": "hand", "containerPath": "/hand", "readOnly": true}],
                "logConfiguration": logs("ecs"),
            }),
        ],
    };
    let family: String = format!("theseus-hand-{}", ids.group)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(255)
        .collect();
    let input = json!({
        "family": family,
        "requiresCompatibilities": ["FARGATE"],
        "networkMode": "awsvpc",
        "cpu": cpu.to_string(),
        "memory": mem.to_string(),
        "taskRoleArn": env.roles.get(&r.profile).cloned().unwrap_or_default(),
        "executionRoleArn": env.execution_role_arn,
        "containerDefinitions": containers,
        "volumes": if r.image.is_some() { json!([{"name": "hand"}]) } else { json!([]) },
        "tags": tags(ids, None),
    });
    let out = account
        .request(
            binding,
            &Request {
                service: "ecs",
                operation: "RegisterTaskDefinition",
                input: &input,
                region,
                pages: 1,
                class: "write",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    out.body["taskDefinition"]["taskDefinitionArn"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "RegisterTaskDefinition named no task definition".into())
}

/// Launch one hand. Its external id (Lambda's request id, or the task's
/// ARN), or why it did not start.
#[expect(clippy::too_many_arguments, reason = "a launch's whole context")]
pub async fn launch(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
    env: &HandsEnv,
    backend: Backend,
    task_definition: Option<&str>,
    spec: &HandSpec,
    ids: &GroupIds<'_>,
) -> Result<String, String> {
    let payload = serde_json::to_string(spec).map_err(|e| e.to_string())?;
    let (service, operation, input) = match backend {
        Backend::Lambda => (
            "lambda",
            "Invoke",
            json!({
                "FunctionName": env.lambda_function.clone().unwrap_or_default(),
                "InvocationType": "Event",
                "Payload": payload,
            }),
        ),
        Backend::Fargate => (
            "ecs",
            "RunTask",
            json!({
                "cluster": env.cluster_arn,
                "taskDefinition": task_definition.unwrap_or_default(),
                "launchType": "FARGATE",
                "count": 1,
                "startedBy": ids.group,
                "networkConfiguration": {"awsvpcConfiguration": {
                    "subnets": env.subnets,
                    "securityGroups": [env.security_group],
                    "assignPublicIp": "DISABLED",
                }},
                "overrides": {"containerOverrides": [{
                    "name": "hand",
                    "environment": [{"name": super::hand::SPEC_ENV, "value": payload}],
                }]},
                "tags": tags(ids, Some(&spec.correlation_id)),
                "propagateTags": "TASK_DEFINITION",
                "enableECSManagedTags": true,
            }),
        ),
    };
    let out = account
        .request(
            binding,
            &Request {
                service,
                operation,
                input: &input,
                region,
                pages: 1,
                class: "run",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    match backend {
        Backend::Lambda => {
            let status = out.body["StatusCode"]
                .as_u64()
                .unwrap_or(u64::from(out.status));
            if status != 202 {
                return Err(format!("Lambda's Invoke answered {status}, not 202"));
            }
            Ok(out.request_id.unwrap_or_default())
        }
        Backend::Fargate => {
            if let Some(f) = out.body["failures"].as_array().and_then(|f| f.first()) {
                return Err(format!(
                    "RunTask failed: {} {}",
                    f["reason"].as_str().unwrap_or_default(),
                    f["detail"].as_str().unwrap_or_default()
                ));
            }
            out.body["tasks"][0]["taskArn"]
                .as_str()
                .map(String::from)
                .ok_or_else(|| "RunTask started no task".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(v: Value) -> HandsRequest {
        parse(&v).unwrap()
    }

    /// The choice until Jev's: Lambda for what fits 10 minutes and 10 GB
    /// on the hand image under the basic profile; Fargate for the rest; the
    /// model's when it names one that can run it.
    #[test]
    fn the_backend_is_chosen_as_the_design_says() {
        let lambda = req(json!({"argv": ["true"], "count": 3}));
        assert_eq!(choose(&lambda), Ok(Backend::Lambda));
        for v in [
            json!({"argv": ["true"], "ttl_secs": 1200}),
            json!({"argv": ["true"], "image": "example.invalid/tools:1"}),
            json!({"argv": ["true"], "profile": "read"}),
            json!({"argv": ["true"], "memory_mb": 16384, "vcpu": 2}),
        ] {
            assert_eq!(choose(&req(v.clone())), Ok(Backend::Fargate), "{v}");
        }
        let named = req(json!({"argv": ["true"], "backend": "fargate"}));
        assert_eq!(choose(&named), Ok(Backend::Fargate));
        let cannot = req(json!({"argv": ["true"], "backend": "lambda", "image": "x:1"}));
        assert!(choose(&cannot).unwrap_err().contains("its own image"));
        let odd = req(json!({"argv": ["true"], "vcpu": 3, "ttl_secs": 3600}));
        assert!(choose(&odd).unwrap_err().contains("no size"));
    }

    /// What is invalid says so, and a request's defaults are the design's.
    #[test]
    fn a_request_is_checked_and_filled() {
        let r =
            req(json!({"argv": ["sh", "-c", "x"], "inputs": ["a", "b"], "until": {"quorum": 2}}));
        assert_eq!(r.inputs, [json!("a"), json!("b")]);
        assert_eq!(
            (r.until, r.concurrency, r.ttl_secs),
            (Until::Quorum(2), 2, 600)
        );
        assert_eq!(r.profile, "basic");
        for (v, why) in [
            (json!({"argv": []}), "empty"),
            (
                json!({"argv": ["x"], "count": 2, "inputs": [1]}),
                "not both",
            ),
            (json!({"argv": ["x"], "count": 101}), "1 to 100"),
            (json!({"argv": ["x"], "count": 0}), "1 to 100"),
            (json!({"argv": ["x"], "until": "most"}), "until is"),
            (
                json!({"argv": ["x"], "count": 2, "until": {"quorum": 3}}),
                "a quorum is 1 to 2",
            ),
            (json!({"argv": ["x"], "ttl_secs": 5}), "ttl_secs"),
            (json!({"argv": ["x"], "max_usd": 0}), "max_usd"),
            (json!({"argv": ["x"], "profile": "admin"}), "profile"),
            (json!({"argv": ["x"], "colour": "red"}), "colour"),
        ] {
            let e = parse(&v).unwrap_err();
            assert!(e.contains(why), "{v}: {e}");
        }
        let c = req(json!({"argv": ["x"], "count": 5, "concurrency": 9}));
        assert_eq!(c.concurrency, 5);
    }

    /// Fargate's sizes, as ECS takes them.
    #[test]
    fn fargate_sizes_are_ecss() {
        assert_eq!(fargate_size(0.25, 512), Some((256, 512)));
        assert_eq!(fargate_size(1.0, 2048), Some((1024, 2048)));
        assert_eq!(fargate_size(1.0, 1024), None);
        assert_eq!(fargate_size(16.0, 122_880), Some((16384, 122_880)));
        assert_eq!(fargate_size(3.0, 8192), None);
    }

    /// The estimate: a Lambda hand by its function's memory, a Fargate one
    /// by its size, a minute at least.
    #[test]
    fn a_hands_cost_is_its_backends_rate() {
        let r = req(json!({"argv": ["x"]}));
        let l = cost_usd(Backend::Lambda, &r, 600.0, 2048);
        assert!(
            (l - (0.000_000_2 + 600.0 * 2.0 * 0.000_016_666_7)).abs() < 1e-9,
            "{l}"
        );
        let f = cost_usd(Backend::Fargate, &r, 3600.0, 0);
        assert!((f - (0.040_48 + 2.0 * 0.004_445)).abs() < 1e-9, "{f}");
        assert_eq!(
            cost_usd(Backend::Fargate, &r, 1.0, 0),
            cost_usd(Backend::Fargate, &r, 60.0, 0)
        );
    }
}
