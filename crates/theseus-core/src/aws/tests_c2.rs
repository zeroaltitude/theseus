//! C2 (14b): the floor for each guardrail group, IaC-only as invalid input,
//! how session policies compose, the budget's reconcile and the bootstrap
//! (each idempotent, against a fake CloudFormation), the lean templates, and
//! a program's AWS job session.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_aws_guard::{glob, Context, Truth};
use theseus_protocol::AwsBootstrapParams;
use theseus_tools::{Plan, Tool};

use super::session::{policy_arns, policy_names, Kind};
use super::tests::{board, plain_ctx, sts, Fake, Reply, Seen, ACCOUNT, KEY_ID, SECRET};
use super::{bootstrap, tend, Aws};
use crate::config::{AwsAccountConfig, AwsConfig};
use crate::policy::{Posture, ToolPolicy};

pub(super) const SESSION_KEY: &str = "ASIATESTSESSION0001";
pub(super) const SESSION_SECRET: &str = "test-session-secret-0001";

// ------------------------------------------------------------------ the rig

fn config(endpoint: &str, owner: bool, budget: Option<u32>) -> AwsConfig {
    AwsConfig {
        accounts: BTreeMap::from([(
            ACCOUNT.to_string(),
            AwsAccountConfig {
                credentials: Default::default(),
                region: "us-west-2".into(),
                regions: vec!["us-west-2".into(), "us-east-1".into()],
                endpoint: Some(endpoint.into()),
                owner_role: owner.then(|| "theseus-owner".into()),
                deployment: None,
                monthly_budget_usd: budget,
                daily_budget_usd: None,
                hourly_alert_usd: crate::config::default_hourly_alert_usd(),
                runaway_factor: crate::config::default_runaway_factor(),
                durability: false,
                hands_network: None,
            },
        )]),
    }
}

fn layer(fake: &Fake, owner: bool, budget: Option<u32>) -> Arc<Aws> {
    Aws::from_config(&config(&fake.url, owner, budget), board()).expect("an account")
}

/// An owner's account whose config names the day's budget.
fn layer_daily(fake: &Fake, daily: u32) -> Arc<Aws> {
    let mut c = config(&fake.url, true, None);
    for a in c.accounts.values_mut() {
        a.daily_budget_usd = Some(daily);
    }
    Aws::from_config(&c, board()).expect("an account")
}

fn tool(aws: &Arc<Aws>, name: &str) -> Arc<dyn Tool> {
    aws.tools()
        .into_iter()
        .find(|t| t.name() == name)
        .unwrap_or_else(|| panic!("no tool {name}"))
}

fn policy(aws: &[(&str, Posture)]) -> ToolPolicy {
    ToolPolicy {
        roots: vec![],
        approve_paths: vec![],
        allow_argv: vec![],
        approve_argv: vec![],
        enforcement: Posture::Notify,
        tools: BTreeMap::new(),
        mcp: BTreeMap::new(),
        aws: aws.iter().map(|(k, p)| (k.to_string(), *p)).collect(),
        confirmer: "operator".into(),
        floor_paths: vec![],
        floor_argv: crate::policy::floor_argv(),
        private_addresses: Default::default(),
    }
}

fn plan(t: &Arc<dyn Tool>, service: &str, operation: &str, input: Value) -> Result<Plan, String> {
    t.plan(
        &json!({"service": service, "operation": operation, "input": input}),
        &plain_ctx(),
    )
}

/// `a=b&c=d`, decoded: a query protocol's request.
pub(super) fn form(body: &str) -> BTreeMap<String, String> {
    let dec = |s: &str| {
        let b = s.replace('+', " ");
        let bytes = b.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(v) = u8::from_str_radix(&b[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    body.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (dec(k), dec(v)))
        .collect()
}

/// The access key a request was signed with.
fn signed_by(s: &Seen) -> String {
    s.header("authorization")
        .and_then(|a| a.split("Credential=").nth(1))
        .and_then(|c| c.split('/').next())
        .unwrap_or_default()
        .to_string()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn xml(n: usize, body: String) -> Reply {
    (
        200,
        vec![
            ("x-amzn-requestid", format!("req-{n}")),
            ("content-type", "text/xml".into()),
        ],
        body,
    )
}

fn json_reply(n: usize, body: Value) -> Reply {
    (
        200,
        vec![
            ("x-amzn-requestid", format!("req-{n}")),
            ("content-type", "application/x-amz-json-1.1".into()),
        ],
        body.to_string(),
    )
}

fn error(n: usize, status: u16, code: &str, message: &str) -> Reply {
    (
        status,
        vec![("x-amzn-requestid", format!("req-{n}"))],
        format!(
            "<ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code><Message>{}</Message>\
             </Error><RequestId>req-{n}</RequestId></ErrorResponse>",
            xml_escape(message)
        ),
    )
}

pub(super) fn assumed(n: usize, name: &str) -> Reply {
    xml(
        n,
        format!(
            "<AssumeRoleResponse><AssumeRoleResult><Credentials><AccessKeyId>{SESSION_KEY}</AccessKeyId>\
             <SecretAccessKey>{SESSION_SECRET}</SecretAccessKey><SessionToken>test-session-token</SessionToken>\
             <Expiration>2026-10-04T00:00:00Z</Expiration></Credentials><AssumedRoleUser>\
             <Arn>arn:aws:sts::{ACCOUNT}:assumed-role/theseus-owner/{name}</Arn>\
             <AssumedRoleId>AROATEST:{name}</AssumedRoleId></AssumedRoleUser></AssumeRoleResult>\
             <ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata></AssumeRoleResponse>"
        ),
    )
}

/// A stack in the fake: its policy and termination protection too.
#[derive(Clone, Debug, Default)]
struct Stack {
    status: String,
    params: BTreeMap<String, String>,
    template: String,
    policy: Option<String>,
    protected: bool,
}

/// A stack the fake holds already, made from `template` under `params`.
fn made(template: &str, params: &[(&str, &str)]) -> Stack {
    Stack {
        status: "CREATE_COMPLETE".into(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        template: template.into(),
        ..Default::default()
    }
}

/// The lean posture's parameters, as the plan makes them for the fake's
/// account.
const LEAN_POSTURE: [(&str, &str); 6] = [
    ("FoundationStack", "theseus-foundation"),
    ("OwnerUserName", "example"),
    ("TrailKey", "aws-managed"),
    ("GuardDuty", "enabled"),
    ("AccessAnalyzer", "enabled"),
    ("SnapshotPublicSharing", "block-all-sharing"),
];

/// A change set in the fake: its stack, its kind, its parameters and
/// template, and its changes (action, logical id, type).
#[derive(Clone, Debug, Default)]
struct ChangeSet {
    stack: String,
    kind: String,
    params: BTreeMap<String, String>,
    template: Option<String>,
    changes: Vec<(String, String, String)>,
}

/// CloudFormation, STS, and the singletons' reads, as the reconcile and the
/// bootstrap call them, with state.
#[derive(Default)]
struct Cloud {
    stacks: BTreeMap<String, Stack>,
    change_sets: BTreeMap<String, ChangeSet>,
    owner_role: bool,
    /// The reconcile's change set touches more than the budget.
    beyond_budget: bool,
}

/// A query request's parameters (`Parameters.member.N.*`).
fn parameters(
    f: &BTreeMap<String, String>,
    previous: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for n in 1.. {
        let Some(k) = f.get(&format!("Parameters.member.{n}.ParameterKey")) else {
            break;
        };
        let v = match f.get(&format!("Parameters.member.{n}.UsePreviousValue")) {
            Some(t) if t == "true" => previous.get(k).cloned().unwrap_or_default(),
            _ => f
                .get(&format!("Parameters.member.{n}.ParameterValue"))
                .cloned()
                .unwrap_or_default(),
        };
        out.insert(k.clone(), v);
    }
    out
}

fn stack_xml(name: &str, s: &Stack) -> String {
    let params: String = s
        .params
        .iter()
        .map(|(k, v)| {
            format!(
                "<member><ParameterKey>{k}</ParameterKey><ParameterValue>{}</ParameterValue></member>",
                xml_escape(v)
            )
        })
        .collect();
    format!(
        "<member><StackName>{name}</StackName><StackId>arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/{name}/1</StackId>\
         <CreationTime>2026-10-03T12:00:00Z</CreationTime><StackStatus>{}</StackStatus>\
         <EnableTerminationProtection>{}</EnableTerminationProtection>\
         <Parameters>{params}</Parameters></member>",
        s.status, s.protected
    )
}

fn ok(n: usize, op: &str, inner: &str) -> Reply {
    xml(
        n,
        format!(
            "<{op}Response>{inner}<ResponseMetadata><RequestId>req-{n}</RequestId></ResponseMetadata></{op}Response>"
        ),
    )
}

impl Cloud {
    fn answer(&mut self, s: &Seen, n: usize) -> Reply {
        let target = s.header("x-amz-target").unwrap_or_default().to_string();
        if target.ends_with("DescribeTrails") {
            return json_reply(n, json!({"trailList": []}));
        }
        if target.ends_with("DescribeBudget") {
            return json_reply(
                n,
                json!({"Budget": {"BudgetName": "theseus-monthly", "BudgetLimit": {"Amount": "50.0", "Unit": "USD"},
                       "CalculatedSpend": {"ActualSpend": {"Amount": "3.2117", "Unit": "USD"},
                                           "ForecastedSpend": {"Amount": "4.1", "Unit": "USD"}},
                       "TimeUnit": "MONTHLY", "BudgetType": "COST"}}),
            );
        }
        if s.method == "GET" && s.target.starts_with("/detector") {
            return json_reply(n, json!({"detectorIds": []}));
        }
        if s.method == "GET" && s.target.starts_with("/analyzer") {
            return json_reply(n, json!({"analyzers": []}));
        }
        let f = form(&s.body);
        let name = f.get("StackName").cloned().unwrap_or_default();
        match f.get("Action").map(String::as_str).unwrap_or_default() {
            "GetCallerIdentity" => sts(ACCOUNT, n),
            "AssumeRole" if self.owner_role => {
                assumed(n, f.get("RoleSessionName").map_or("?", |s| s))
            }
            "AssumeRole" => error(
                n,
                403,
                "AccessDenied",
                "not authorized to perform: sts:AssumeRole",
            ),
            "DescribeStacks" => match self.stacks.get(&name) {
                Some(st) => ok(
                    n,
                    "DescribeStacks",
                    &format!(
                        "<DescribeStacksResult><Stacks>{}</Stacks></DescribeStacksResult>",
                        stack_xml(&name, st)
                    ),
                ),
                None => error(
                    n,
                    400,
                    "ValidationError",
                    &format!("Stack with id {name} does not exist"),
                ),
            },
            "GetTemplate" => {
                let body = self
                    .stacks
                    .get(&name)
                    .map(|s| s.template.clone())
                    .unwrap_or_default();
                ok(
                    n,
                    "GetTemplate",
                    &format!(
                        "<GetTemplateResult><TemplateBody>{}</TemplateBody></GetTemplateResult>",
                        xml_escape(&body)
                    ),
                )
            }
            "CreateChangeSet" => self.create_change_set(&f, &name, s, n),
            "DescribeChangeSet" => self.describe_change_set(&f, n),
            "ExecuteChangeSet" => {
                self.execute(&f);
                ok(n, "ExecuteChangeSet", "<ExecuteChangeSetResult/>")
            }
            "DeleteChangeSet" => ok(n, "DeleteChangeSet", "<DeleteChangeSetResult/>"),
            "SetStackPolicy" => self.set_policy(&f, &name, s, n),
            "GetStackPolicy" => self.get_policy(&name, n),
            "UpdateTerminationProtection" => self.protect(&f, &name, n),
            other => error(
                n,
                400,
                "InvalidAction",
                &format!("the fake does not know {other:?}"),
            ),
        }
    }

    /// `GetStackPolicy`: no body when no policy is set.
    fn get_policy(&self, name: &str, n: usize) -> Reply {
        let inner = match self.stacks.get(name).and_then(|s| s.policy.as_deref()) {
            Some(p) => format!(
                "<GetStackPolicyResult><StackPolicyBody>{}</StackPolicyBody></GetStackPolicyResult>",
                xml_escape(p)
            ),
            None => "<GetStackPolicyResult/>".into(),
        };
        ok(n, "GetStackPolicy", &inner)
    }

    fn protect(&mut self, f: &BTreeMap<String, String>, name: &str, n: usize) -> Reply {
        if let Some(st) = self.stacks.get_mut(name) {
            st.protected = f
                .get("EnableTerminationProtection")
                .is_some_and(|v| v == "true");
        }
        let inner = format!(
            "<UpdateTerminationProtectionResult><StackId>{name}</StackId></UpdateTerminationProtectionResult>"
        );
        ok(n, "UpdateTerminationProtection", &inner)
    }

    /// `SetStackPolicy`, refused as AWS refuses it: a policy naming a logical
    /// id the stack doesn't have (the bootstrap's second live run, 2026-10-03).
    fn set_policy(
        &mut self,
        f: &BTreeMap<String, String>,
        name: &str,
        s: &Seen,
        n: usize,
    ) -> Reply {
        let Some(stack) = self.stacks.get_mut(name) else {
            let missing = format!("Stack with id {name} does not exist");
            return error(n, 400, "ValidationError", &missing);
        };
        let ctx = Context {
            account: ACCOUNT.into(),
            region: s.region().unwrap_or("us-west-2").into(),
        };
        let has: Vec<String> = theseus_aws_guard::parse_template(&stack.template)
            .ok()
            .and_then(|t| theseus_aws_guard::planned_resources(&t, &ctx, &stack.params).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.logical_id)
            .collect();
        let body = f.get("StackPolicyBody").cloned().unwrap_or_default();
        let policy: Value = serde_json::from_str(&body).unwrap_or_default();
        for st in policy["Statement"].as_array().into_iter().flatten() {
            let named = match &st["Resource"] {
                Value::Array(a) => a.clone(),
                r => vec![r.clone()],
            };
            for r in named.iter().filter_map(Value::as_str) {
                let unknown = r
                    .strip_prefix("LogicalResourceId/")
                    .is_some_and(|id| !has.iter().any(|h| h == id));
                if unknown {
                    let why = format!(
                        "Error validating stack policy: Unknown logical id '{r}' in statement {{}} - \
                         stack policies can only be applied to logical ids referenced in the template"
                    );
                    return error(n, 400, "ValidationError", &why);
                }
            }
        }
        stack.policy = Some(body);
        ok(n, "SetStackPolicy", "")
    }

    fn execute(&mut self, f: &BTreeMap<String, String>) {
        let id = f.get("ChangeSetName").cloned().unwrap_or_default();
        let cs = self.change_sets.get(&id).cloned().unwrap_or_default();
        let stack = self.stacks.entry(cs.stack.clone()).or_default();
        stack.params = cs.params.clone();
        if let Some(t) = cs.template {
            stack.template = t;
        }
        stack.status = if cs.kind == "CREATE" {
            "CREATE_COMPLETE"
        } else {
            "UPDATE_COMPLETE"
        }
        .into();
        if cs.stack == "theseus-foundation" {
            self.owner_role = true;
        }
    }

    fn create_change_set(
        &mut self,
        f: &BTreeMap<String, String>,
        stack: &str,
        s: &Seen,
        n: usize,
    ) -> Reply {
        let id = f.get("ChangeSetName").cloned().unwrap_or_default();
        let kind = f
            .get("ChangeSetType")
            .cloned()
            .unwrap_or_else(|| "UPDATE".into());
        let previous = self
            .stacks
            .get(stack)
            .map(|s| s.params.clone())
            .unwrap_or_default();
        let params = parameters(f, &previous);
        let template = f.get("TemplateBody").cloned();
        let changes = if kind == "CREATE" {
            let t =
                theseus_aws_guard::parse_template(template.as_deref().unwrap_or_default()).unwrap();
            let ctx = Context {
                account: ACCOUNT.into(),
                region: s.region().unwrap_or("us-west-2").into(),
            };
            theseus_aws_guard::planned_resources(&t, &ctx, &params)
                .unwrap()
                .into_iter()
                .map(|r| ("Add".to_string(), r.logical_id, r.resource_type))
                .collect()
        } else {
            let mut c = Vec::new();
            // The day's budget (step 40 part 2): made at its first amount,
            // removed at 0, else modified.
            let (was, now) = (previous.get("DailyBudgetUsd"), params.get("DailyBudgetUsd"));
            if was != now {
                let zero = |v: Option<&String>| v.is_none_or(|v| v == "0");
                let action = match (zero(was), zero(now)) {
                    (true, false) => "Add",
                    (false, true) => "Remove",
                    _ => "Modify",
                };
                c.push((
                    action.to_string(),
                    "DailyBudget".to_string(),
                    "AWS::Budgets::Budget".to_string(),
                ));
            }
            if previous.get("MonthlyBudgetUsd") != params.get("MonthlyBudgetUsd") {
                c.push((
                    "Modify".to_string(),
                    "MonthlyBudget".to_string(),
                    "AWS::Budgets::Budget".to_string(),
                ));
                if self.beyond_budget {
                    c.push((
                        "Modify".to_string(),
                        "OwnerRole".to_string(),
                        "AWS::IAM::Role".to_string(),
                    ));
                }
            }
            c
        };
        self.change_sets.insert(
            id.clone(),
            ChangeSet {
                stack: stack.into(),
                kind,
                params,
                template,
                changes,
            },
        );
        ok(
            n,
            "CreateChangeSet",
            &format!(
                "<CreateChangeSetResult><Id>{id}</Id><StackId>{stack}</StackId></CreateChangeSetResult>"
            ),
        )
    }

    fn describe_change_set(&self, f: &BTreeMap<String, String>, n: usize) -> Reply {
        let id = f.get("ChangeSetName").cloned().unwrap_or_default();
        let cs = self.change_sets.get(&id).cloned().unwrap_or_default();
        let (status, reason) = if cs.changes.is_empty() {
            (
                "FAILED",
                "The submitted information didn't contain changes.",
            )
        } else {
            ("CREATE_COMPLETE", "")
        };
        let changes: String = cs
            .changes
            .iter()
            .map(|(a, id, ty)| {
                format!(
                    "<member><Type>Resource</Type><ResourceChange><Action>{a}</Action>\
                     <LogicalResourceId>{id}</LogicalResourceId><ResourceType>{ty}</ResourceType>\
                     <Replacement>False</Replacement></ResourceChange></member>"
                )
            })
            .collect();
        ok(
            n,
            "DescribeChangeSet",
            &format!(
                "<DescribeChangeSetResult><ChangeSetName>{id}</ChangeSetName><ChangeSetId>{id}</ChangeSetId>\
                 <StackName>{}</StackName><Status>{status}</Status><StatusReason>{reason}</StatusReason>\
                 <ExecutionStatus>AVAILABLE</ExecutionStatus><Changes>{changes}</Changes></DescribeChangeSetResult>",
                cs.stack
            ),
        )
    }
}

fn cloud(state: Cloud) -> (Fake, Arc<Mutex<Cloud>>) {
    let c = Arc::new(Mutex::new(state));
    let held = c.clone();
    let fake = Fake::start(move |s, n| held.lock().unwrap().answer(s, n));
    (fake, c)
}

/// The query actions the fake saw, in order.
fn actions(fake: &Fake) -> Vec<String> {
    fake.seen()
        .iter()
        .filter_map(|s| form(&s.body).get("Action").cloned())
        .collect()
}

/// The writes among them: what a read-only plan never sends.
fn writes(fake: &Fake) -> Vec<String> {
    actions(fake)
        .into_iter()
        .filter(|a| {
            !matches!(
                a.as_str(),
                "GetCallerIdentity"
                    | "AssumeRole"
                    | "DescribeStacks"
                    | "GetTemplate"
                    | "GetStackPolicy"
                    | "DescribeChangeSet"
            )
        })
        .collect()
}

/// The first `AssumeRole` among `seen`, decoded.
pub(super) fn first_mint(seen: &[Seen]) -> BTreeMap<String, String> {
    seen.iter()
        .map(|s| form(&s.body))
        .find(|f| f.get("Action").map(String::as_str) == Some("AssumeRole"))
        .expect("an AssumeRole")
}

/// A mint's managed session policies, in order.
pub(super) fn minted_arns(mint: &BTreeMap<String, String>) -> Vec<String> {
    (1..)
        .map_while(|i| mint.get(&format!("PolicyArns.member.{i}.arn")).cloned())
        .collect()
}

// ------------------------------------------------------------------ the floor

/// The floor (AWS design §3.6), for each guardrail group: a direct call that
/// hits a guardrail asks at every posture, even with `[policy.aws]` open, and
/// its reason names the guardrail; the same call that misses runs at its
/// posture.
#[test]
fn the_floor_asks_for_each_guardrail_group_at_every_posture() {
    let fake = Fake::start(|_, n| sts(ACCOUNT, n));
    let aws = layer(&fake, false, None);
    let call = tool(&aws, "aws.call");
    let open = policy(&[
        ("read", Posture::Open),
        ("write", Posture::Open),
        ("run", Posture::Open),
    ]);
    let net = |ip: &str| {
        json!({"taskDefinition": "td", "networkConfiguration": {"awsvpcConfiguration":
            {"subnets": ["subnet-1"], "assignPublicIp": ip}}})
    };
    let deny_spend = format!("arn:aws:iam::{ACCOUNT}:policy/theseus-deny-spend");
    let guard = format!("arn:aws:iam::{ACCOUNT}:policy/theseus-guard-limits");
    let scratch = format!("arn:aws:iam::{ACCOUNT}:policy/scratch");
    let action = json!({"AccountId": ACCOUNT, "BudgetName": "theseus-monthly", "ActionId": "a-1",
                        "ExecutionType": "REVERSE_BUDGET_ACTION"});
    let read_action =
        json!({"AccountId": ACCOUNT, "BudgetName": "theseus-monthly", "ActionId": "a-1"});
    for (group, hit, miss) in [
        (
            "public ingress",
            ("ecs", "RunTask", net("ENABLED")),
            ("ecs", "RunTask", net("DISABLED")),
        ),
        (
            "the audit trail",
            (
                "cloudtrail",
                "StopLogging",
                json!({"Name": "theseus-trail"}),
            ),
            (
                "cloudtrail",
                "StartLogging",
                json!({"Name": "theseus-trail"}),
            ),
        ),
        (
            "encryption at rest",
            ("ec2", "DisableEbsEncryptionByDefault", json!({})),
            ("ec2", "EnableEbsEncryptionByDefault", json!({})),
        ),
        (
            "long-lived credentials",
            (
                "iam",
                "DeleteAccessKey",
                json!({"UserName": "example", "AccessKeyId": "AKIAEXAMPLE"}),
            ),
            ("iam", "ListAccessKeys", json!({"UserName": "example"})),
        ),
        (
            "the budget",
            (
                "iam",
                "DetachRolePolicy",
                json!({"RoleName": "theseus-owner", "PolicyArn": deny_spend}),
            ),
            (
                "budgets",
                "DescribeBudget",
                json!({"AccountId": ACCOUNT, "BudgetName": "theseus-monthly"}),
            ),
        ),
        (
            "the budget",
            ("budgets", "ExecuteBudgetAction", action),
            ("budgets", "DescribeBudgetAction", read_action),
        ),
        (
            "the guards",
            ("iam", "DeletePolicy", json!({"PolicyArn": guard})),
            ("iam", "DeletePolicy", json!({"PolicyArn": scratch})),
        ),
    ] {
        let p = plan(&call, hit.0, hit.1, hit.2.clone()).unwrap_or_else(|e| panic!("{hit:?}: {e}"));
        let line = p
            .aws
            .as_ref()
            .and_then(|a| a.guardrail.clone())
            .unwrap_or_default();
        assert!(line.starts_with(group), "{hit:?}: {line}");
        let d = open.decide(call.as_ref(), &p);
        assert!(
            d.floor && d.posture == Posture::Approve && d.reason.contains("floor: "),
            "{hit:?}: {d:?}"
        );
        let p =
            plan(&call, miss.0, miss.1, miss.2.clone()).unwrap_or_else(|e| panic!("{miss:?}: {e}"));
        assert_eq!(
            p.aws.as_ref().and_then(|a| a.guardrail.clone()),
            None,
            "{miss:?}"
        );
        let d = open.decide(call.as_ref(), &p);
        assert!(!d.floor && d.posture == Posture::Open, "{miss:?}: {d:?}");
    }
}

/// IaC-only (§3.4): durable infrastructure, and a stack's own writes, are
/// invalid input that points to the stack tools, whatever the guardrails say
/// of it, and nothing is sent.
#[tokio::test]
async fn iac_only_and_stack_writes_are_invalid_input_and_nothing_is_sent() {
    let fake = Fake::start(|_, n| sts(ACCOUNT, n));
    let aws = layer(&fake, false, None);
    let call = tool(&aws, "aws.call");
    let open_to_all = json!({"GroupId": "sg-1", "IpPermissions": [{"IpProtocol": "tcp", "FromPort": 22,
                             "ToPort": 22, "IpRanges": [{"CidrIp": "0.0.0.0/0"}]}]});
    for (service, op, input, says) in [
        (
            "s3",
            "CreateBucket",
            json!({"Bucket": "theseus-scratch"}),
            "s3:CreateBucket makes or changes durable infrastructure; use aws.stack.plan (IaC is \
             an operator limit). Nothing was sent.",
        ),
        (
            "iam",
            "CreateRole",
            json!({"RoleName": "x", "AssumeRolePolicyDocument": "{}"}),
            "use aws.stack.plan",
        ),
        // A guardrail's operation that is IaC-only is checked when its stack applies.
        (
            "ec2",
            "AuthorizeSecurityGroupIngress",
            open_to_all,
            "use aws.stack.plan",
        ),
        (
            "events",
            "PutRule",
            json!({"Name": "theseus-posture-root-usage", "EventPattern": "{}"}),
            "use aws.stack.plan",
        ),
        (
            "cloudformation",
            "ExecuteChangeSet",
            json!({"ChangeSetName": "c", "StackName": "theseus-foundation"}),
            "writes a stack outside the review; use aws.stack.plan, then aws.stack.apply",
        ),
    ] {
        let e = plan(&call, service, op, input.clone()).unwrap_err();
        assert!(e.contains(says), "{service}:{op}: {e}");
        let f = call
            .run_async(
                &json!({"service": service, "operation": op, "input": input}),
                &plain_ctx(),
            )
            .await
            .unwrap_err();
        assert!(f.message.contains("use aws.stack.plan"), "{}", f.message);
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(fake.seen().is_empty(), "nothing was sent");
}

/// A deletion of what holds state waits (§3.9's approve list) at every
/// posture; a plain write takes `[policy.aws] write`, a run `run`, a read
/// `read`; and the tools' own postures follow their classes.
#[test]
fn deletions_wait_and_each_class_takes_its_line() {
    let fake = Fake::start(|_, n| sts(ACCOUNT, n));
    let aws = layer(&fake, false, None);
    let call = tool(&aws, "aws.call");
    let p = policy(&[
        ("read", Posture::Open),
        ("write", Posture::Notify),
        ("run", Posture::Notify),
        ("sqs:PurgeQueue", Posture::Open),
    ]);
    let del = plan(
        &call,
        "s3",
        "DeleteBucket",
        json!({"Bucket": "theseus-scratch"}),
    )
    .unwrap();
    assert!(del.aws.as_ref().unwrap().destructive);
    let d = p.decide(call.as_ref(), &del);
    assert!(
        d.posture == Posture::Approve && d.reason.contains("destructive"),
        "{d:?}"
    );
    // The approve list wins over the operation's own line.
    let queue = "https://sqs.us-west-2.amazonaws.com/111122223333/q";
    let purge = plan(&call, "sqs", "PurgeQueue", json!({"QueueUrl": queue})).unwrap();
    assert_eq!(p.decide(call.as_ref(), &purge).posture, Posture::Approve);
    for (service, op, input, posture, setting) in [
        (
            "sqs",
            "SendMessage",
            json!({"QueueUrl": queue, "MessageBody": "m"}),
            Posture::Notify,
            "[policy.aws] write = notify",
        ),
        (
            "lambda",
            "Invoke",
            json!({"FunctionName": "f"}),
            Posture::Notify,
            "[policy.aws] run = notify",
        ),
        (
            "sqs",
            "ListQueues",
            json!({}),
            Posture::Open,
            "[policy.aws] read = open",
        ),
    ] {
        let pl = plan(&call, service, op, input).unwrap();
        let d = p.decide(call.as_ref(), &pl);
        assert_eq!(d.posture, posture, "{service}:{op}");
        assert!(d.reason.contains(setting), "{}", d.reason);
    }
    assert_eq!(
        p.posture("aws.stack.apply"),
        (Posture::Notify, "[policy.aws] write = notify".into())
    );
    assert_eq!(
        p.posture("aws.call"),
        (Posture::Open, "[policy.aws] read = open".into())
    );
}

// ------------------------------------------------------------------ sessions

/// Whether a session's policies allow `action` on `resource`: an explicit
/// deny in any wins, else any allow (IAM's evaluation, over the
/// unconditional statements, which is where these cases look).
fn allowed(names: &[String], inline: Option<&Value>, action: &str, resource: &str) -> bool {
    let l = theseus_aws_guard::embedded();
    let docs: Vec<Value> = names
        .iter()
        .map(|n| {
            if n == super::ALLOW_ALL {
                json!({"Statement": [{"Effect": "Allow", "Action": "*", "Resource": "*"}]})
            } else {
                let p = l
                    .policies()
                    .into_iter()
                    .find(|p| &p.name == n)
                    .expect("a generated guard");
                serde_json::from_str(&p.minified()).unwrap()
            }
        })
        .chain(inline.cloned())
        .collect();
    let list = |v: &Value| -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    };
    let hits = |effect: &str| {
        docs.iter()
            .flat_map(|d| d["Statement"].as_array().cloned().unwrap_or_default())
            .filter(|s| s["Effect"] == effect && s.get("Condition").is_none())
            .any(|s| {
                list(&s["Action"]).iter().any(|a| glob(a, action))
                    && list(&s["Resource"]).iter().any(|r| glob(r, resource))
            })
    };
    hits("Allow") && !hits("Deny")
}

/// How session policies compose (§3.5): a work session carries the guards
/// and allow-all, so the guards' denies hold and the rest is allowed, the
/// stack path included; a job session adds the stack path's guard; a floor
/// session carries allow-all alone; a tender's inline policy is its one
/// allow. Each fits STS's limits.
#[test]
fn session_policies_compose_as_their_kind_says() {
    let work = policy_names(Kind::Work);
    let job = policy_names(Kind::Job);
    let floor = policy_names(Kind::Floor);
    assert_eq!(work.last().map(String::as_str), Some(super::ALLOW_ALL));
    assert!(work.iter().any(|n| n == "theseus-guard-limits"));
    assert!(work.iter().any(|n| n == "theseus-guard-iac"));
    assert!(job.iter().any(|n| n == "theseus-guard-stacks"));
    assert!(!work.iter().any(|n| n == "theseus-guard-stacks"));
    assert_eq!(floor, vec![super::ALLOW_ALL.to_string()]);
    assert!(policy_names(Kind::Tender(tend::TENDER)).is_empty());
    let deployer = format!("arn:aws:iam::{ACCOUNT}:role/theseus-cfn-deployer");
    let foundation =
        format!("arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/theseus-foundation/1");
    let object = "arn:aws:s3:::theseus-x/k";
    for (names, action, resource, want) in [
        (&work, "s3:PutObject", object, true),
        (&work, "s3:CreateBucket", "*", false),
        (&work, "cloudtrail:StopLogging", "*", false),
        (&work, "iam:CreateUser", "*", false),
        (
            &work,
            "cloudformation:ExecuteChangeSet",
            foundation.as_str(),
            true,
        ),
        (
            &job,
            "cloudformation:ExecuteChangeSet",
            foundation.as_str(),
            false,
        ),
        (&job, "iam:PassRole", deployer.as_str(), false),
        (&job, "s3:PutObject", object, true),
        (&floor, "cloudtrail:StopLogging", "*", true),
    ] {
        assert_eq!(
            allowed(names, None, action, resource),
            want,
            "{action} on {resource} in {names:?}"
        );
    }
    let t = tend::policy(tend::TENDER, ACCOUNT).unwrap();
    let none: Vec<String> = Vec::new();
    let budget = format!("arn:aws:budgets::{ACCOUNT}:budget/theseus-monthly");
    assert!(allowed(
        &none,
        Some(&t),
        "cloudformation:ExecuteChangeSet",
        &foundation
    ));
    assert!(allowed(&none, Some(&t), "budgets:ViewBudget", &budget));
    assert!(!allowed(&none, Some(&t), "s3:PutObject", object));
    // STS's limits: ten managed policies, and the ARNs and inline text together.
    let arns = policy_arns(ACCOUNT, Kind::Job);
    assert!(arns.len() <= theseus_aws_guard::SESSION_POLICY_ARNS);
    let text: usize = arns.iter().map(String::len).sum::<usize>() + t.to_string().len();
    assert!(
        text <= theseus_aws_guard::SESSION_POLICY_PLAINTEXT,
        "{text}"
    );
}

/// Once the config names the owner role, the key signs only STS (§3.5): an
/// `aws.call` write mints a work session named by its execution, with the
/// guards and allow-all, the deployment as its source identity, and its
/// tags, and the write signs with the session; an approved floor call that
/// the guards refuse mints a floor session with allow-all alone. Each mint
/// is an `aws.session.minted` row on the call's binding; no credential is.
#[tokio::test]
async fn the_key_signs_only_sts_and_each_call_its_session() {
    let fake = Fake::start(|s, n| {
        let f = form(&s.body);
        match f.get("Action").map(String::as_str) {
            Some("GetCallerIdentity") => sts(ACCOUNT, n),
            Some("AssumeRole") => assumed(n, f.get("RoleSessionName").map_or("?", |s| s)),
            _ => json_reply(n, json!({"MessageId": "m-1", "MD5OfMessageBody": "x"})),
        }
    });
    let aws = layer(&fake, true, None);
    let call = tool(&aws, "aws.call");
    let queue = "https://sqs.us-west-2.amazonaws.com/111122223333/q";
    for (service, op, input, kind, name) in [
        (
            "sqs",
            "SendMessage",
            json!({"QueueUrl": queue, "MessageBody": "m"}),
            Kind::Work,
            "exe_test",
        ),
        (
            "cloudtrail",
            "StopLogging",
            json!({"Name": "theseus-trail"}),
            Kind::Floor,
            "exe_test.floor",
        ),
    ] {
        let b = aws.bind("exe_test", "act_test_1", "toolu_1");
        let ctx = theseus_tools::ToolCtx {
            aws: Some(b.clone()),
            ..plain_ctx()
        };
        let input = json!({"service": service, "operation": op, "input": input});
        call.plan(&input, &ctx).unwrap();
        let before = fake.seen().len();
        call.run_async(&input, &ctx)
            .await
            .unwrap_or_else(|f| panic!("{op}: {}", f.message));
        let seen: Vec<Seen> = fake.seen()[before..].to_vec();
        let mint = first_mint(&seen);
        assert_eq!(
            mint["RoleArn"],
            format!("arn:aws:iam::{ACCOUNT}:role/theseus-owner")
        );
        assert_eq!(mint["RoleSessionName"], name);
        assert_eq!(mint["SourceIdentity"], "theseus");
        assert_eq!(minted_arns(&mint), policy_arns(ACCOUNT, kind), "{op}");
        assert!(mint.values().any(|v| v == "theseus:session"));
        assert!(mint.values().any(|v| v == kind.as_str()));
        for s in &seen {
            let by = signed_by(s);
            let sts_call = matches!(
                form(&s.body).get("Action").map(String::as_str),
                Some("AssumeRole" | "GetCallerIdentity")
            );
            assert_eq!(by == KEY_ID, sts_call, "{op}: {} signed by {by}", s.target);
        }
        let rows = b.sessions();
        assert_eq!(rows.len(), 1, "{op}: one mint");
        assert_eq!(rows[0]["kind"], kind.as_str());
        let text = rows[0].to_string();
        assert!(
            !text.contains(SESSION_SECRET) && !text.contains(SECRET),
            "{text}"
        );
    }
}

// ------------------------------------------------------------------ the reconcile

fn foundation(budget: &str) -> Stack {
    made(
        bootstrap::FOUNDATION_TEMPLATE,
        &[
            ("OwnerUserName", "example"),
            ("MonthlyBudgetUsd", budget),
            ("AlertEmail", ""),
        ],
    )
}

/// The budget's reconcile (§3.7) against a fake: config 60 and stack 50 is a
/// change set of the old template with 60, the rest kept, under the
/// deployer, signed in the tender's session, applied; run again, it finds
/// them equal and makes no change set. A change set that touches more than
/// the budget is deleted, never applied. No amount in the config: nothing.
#[tokio::test]
async fn the_reconcile_is_idempotent_and_touches_only_the_budget() {
    let state = Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), foundation("50"))]),
        owner_role: true,
        ..Default::default()
    };
    let (fake, c) = cloud(state);
    let aws = layer(&fake, true, Some(60));
    let account = aws.account(None).unwrap().clone();
    assert_eq!(
        tend::reconcile(&account).await,
        tend::Reconciled::Changed { from: 50, to: 60 }
    );
    let sets: Vec<BTreeMap<String, String>> = fake
        .seen()
        .iter()
        .map(|s| form(&s.body))
        .filter(|f| f.get("Action").map(String::as_str) == Some("CreateChangeSet"))
        .collect();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0]["UsePreviousTemplate"], "true");
    assert_eq!(
        sets[0]["RoleARN"],
        format!("arn:aws:iam::{ACCOUNT}:role/theseus-cfn-deployer")
    );
    {
        let held = &c.lock().unwrap().stacks["theseus-foundation"].params;
        assert_eq!(
            (
                held["MonthlyBudgetUsd"].as_str(),
                held["OwnerUserName"].as_str()
            ),
            ("60", "example")
        );
    }
    let mint = first_mint(&fake.seen());
    assert_eq!(mint["RoleSessionName"], "theseus-tender");
    assert!(mint.contains_key("Policy") && minted_arns(&mint).is_empty());
    // Again: equal, and no second change set.
    assert_eq!(tend::reconcile(&account).await, tend::Reconciled::Equal(60));
    let a = actions(&fake);
    assert_eq!(a.iter().filter(|x| *x == "CreateChangeSet").count(), 1);
    assert_eq!(a.iter().filter(|x| *x == "ExecuteChangeSet").count(), 1);

    // A change set that touches more than the budget stops it.
    let state = Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), foundation("50"))]),
        owner_role: true,
        beyond_budget: true,
        ..Default::default()
    };
    let (fake, c) = cloud(state);
    let aws = layer(&fake, true, Some(70));
    let account = aws.account(None).unwrap().clone();
    match tend::reconcile(&account).await {
        tend::Reconciled::Stopped(why) => assert!(
            why.contains("touches more than the budget (Modify OwnerRole)"),
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
    let a = actions(&fake);
    assert!(
        a.contains(&"DeleteChangeSet".to_string()) && !a.contains(&"ExecuteChangeSet".to_string()),
        "{a:?}"
    );
    assert_eq!(
        c.lock().unwrap().stacks["theseus-foundation"].params["MonthlyBudgetUsd"],
        "50"
    );

    // No amount in the config: the stack's stands, and nothing is asked.
    let (fake, _) = cloud(Cloud::default());
    let aws = layer(&fake, true, None);
    assert_eq!(
        tend::reconcile(aws.account(None).unwrap()).await,
        tend::Reconciled::Unset
    );
    assert!(actions(&fake).is_empty());
}

/// The day's budget (step 40 part 2) reconciles as the month's does: config
/// 5 against a stack at 0 is a change set of the old template with
/// `DailyBudgetUsd` 5 that makes the daily budget and changes nothing else,
/// applied; again, equal. A stack whose template predates the parameter
/// stops, saying the template comes first; no amount in the config, nothing.
#[tokio::test]
async fn the_daily_budget_reconciles_as_the_months_does() {
    let mut f = foundation("50");
    f.params.insert("DailyBudgetUsd".into(), "0".into());
    let state = Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), f)]),
        owner_role: true,
        ..Default::default()
    };
    let (fake, c) = cloud(state);
    let aws = layer_daily(&fake, 5);
    let account = aws.account(None).unwrap().clone();
    let daily = tend::Which::Daily;
    assert_eq!(
        tend::reconcile_budget(&account, daily).await,
        tend::Reconciled::Changed { from: 0, to: 5 }
    );
    {
        let held = &c.lock().unwrap().stacks["theseus-foundation"].params;
        assert_eq!(
            (
                held["DailyBudgetUsd"].as_str(),
                held["MonthlyBudgetUsd"].as_str()
            ),
            ("5", "50")
        );
    }
    assert_eq!(
        tend::reconcile_budget(&account, daily).await,
        tend::Reconciled::Equal(5)
    );
    assert_eq!(
        tend::Reconciled::Equal(5).line_for(daily),
        "the stack holds the config's $5 a day"
    );
    // The month's is not in this config: its reconcile asks nothing.
    assert_eq!(tend::reconcile(&account).await, tend::Reconciled::Unset);

    // A foundation from before the parameter.
    let state = Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), foundation("50"))]),
        owner_role: true,
        ..Default::default()
    };
    let (fake, _) = cloud(state);
    let aws = layer_daily(&fake, 5);
    match tend::reconcile_budget(aws.account(None).unwrap(), daily).await {
        tend::Reconciled::Stopped(why) => {
            assert!(why.contains("predates the daily budget"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!actions(&fake).contains(&"CreateChangeSet".to_string()));
}

/// The budget's line, from `DescribeBudget`, in cents.
#[tokio::test]
async fn the_budgets_line_reads_in_cents() {
    let (fake, _) = cloud(Cloud {
        owner_role: true,
        ..Default::default()
    });
    let aws = layer(&fake, true, None);
    let b = tend::read_budget(aws.account(None).unwrap()).await.unwrap();
    assert_eq!(
        (b.limit_cents, b.actual_cents, b.forecast_cents),
        (5000, 321, Some(410))
    );
}

// ------------------------------------------------------------------ the bootstrap

/// The bootstrap against a fake (§5, C2): the plan is read-only (no change
/// set, nothing written) and makes three stacks, their resources as the
/// templates make them; the apply makes them in order, the foundation's
/// change set signed with the key and the others in a floor session of the
/// owner role through the deployer, each with its policy and termination
/// protection; and a second plan finds every stack as planned. A stale
/// digest applies nothing.
#[tokio::test]
async fn the_bootstrap_plans_read_only_applies_once_and_plans_no_diff_after() {
    let (fake, c) = cloud(Cloud::default());
    let aws = layer(&fake, false, Some(50));
    let account = aws.account(None).unwrap().clone();
    let p = AwsBootstrapParams {
        alert_email: Some("alerts@example.com".into()),
        ..Default::default()
    };
    let plan = bootstrap::plan(&account, &p).await.unwrap();
    let r = bootstrap::result(&account, &plan, false);
    assert!(r.changes && !r.applied);
    let names: Vec<(&str, &str, &str)> = r
        .stacks
        .iter()
        .map(|s| (s.stack.as_str(), s.region.as_str(), s.action.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("theseus-foundation", "us-west-2", "create"),
            ("theseus-posture", "us-west-2", "create"),
            ("theseus-posture-relay", "us-east-1", "create"),
        ]
    );
    assert_eq!(r.stacks[0].parameters["OwnerUserName"], "example");
    assert_eq!(r.stacks[0].parameters["MonthlyBudgetUsd"], "50");
    assert_eq!(r.stacks[1].parameters["TrailKey"], "aws-managed");
    assert!(!r
        .stacks
        .iter()
        .flat_map(|s| &s.resources)
        .any(|x| x.contains("AWS::KMS::Key")));
    assert!(
        writes(&fake).is_empty(),
        "a plan writes nothing: {:?}",
        writes(&fake)
    );

    // A stale digest applies nothing.
    let e = bootstrap::apply(&account, &p, "0000000000000000")
        .await
        .unwrap_err();
    assert!(e.contains("the plan changed since"), "{e}");
    assert!(writes(&fake).is_empty());

    let applied = bootstrap::apply(&account, &p, &plan.digest).await.unwrap();
    assert!(bootstrap::result(&account, &applied, true).applied);
    let stacks = c.lock().unwrap().stacks.clone();
    assert_eq!(stacks.len(), 3);
    assert!(stacks.values().all(|s| s.status == "CREATE_COMPLETE"));
    // Each with its policy, which names only what the stack has (the fake
    // refuses another, as AWS does), and its termination protection.
    assert!(
        stacks.values().all(|s| s.policy.is_some() && s.protected),
        "{stacks:?}"
    );
    let posture = stacks["theseus-posture"].policy.clone().unwrap();
    assert!(
        !posture.contains("TrailKmsKey") && posture.contains("LogicalResourceId/Trail\""),
        "{posture}"
    );
    let sets: Vec<(String, String, String)> = fake
        .seen()
        .iter()
        .filter_map(|s| {
            let f = form(&s.body);
            (f.get("Action").map(String::as_str) == Some("CreateChangeSet")).then(|| {
                (
                    f["StackName"].clone(),
                    signed_by(s),
                    f.get("RoleARN").cloned().unwrap_or_default(),
                )
            })
        })
        .collect();
    assert_eq!(sets.len(), 3);
    assert_eq!(
        (sets[0].0.as_str(), sets[0].1.as_str(), sets[0].2.as_str()),
        ("theseus-foundation", KEY_ID, "")
    );
    for s in &sets[1..] {
        assert_eq!(s.1, SESSION_KEY, "{s:?}");
        assert!(s.2.ends_with(":role/theseus-cfn-deployer"), "{s:?}");
    }
    let a = actions(&fake);
    assert_eq!(a.iter().filter(|x| *x == "SetStackPolicy").count(), 3);
    assert_eq!(
        a.iter()
            .filter(|x| *x == "UpdateTerminationProtection")
            .count(),
        3
    );

    // A second plan: every stack as planned, nothing written, and no read
    // signed with the key, since the owner role exists now.
    let before = fake.seen().len();
    let again = bootstrap::plan(&account, &p).await.unwrap();
    let r = bootstrap::result(&account, &again, false);
    assert!(!r.changes, "{:?}", r.stacks);
    assert!(r
        .stacks
        .iter()
        .all(|s| s.action == "none" && s.sets.is_empty()));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    nothing_written_and_no_key(&fake.seen()[before..]);
}

/// A plan after the owner role exists: only reads, and none signed with the
/// key but STS's.
fn nothing_written_and_no_key(seen: &[Seen]) {
    assert!(seen.iter().all(|s| {
        let a = form(&s.body).get("Action").cloned().unwrap_or_default();
        matches!(a.as_str(), "AssumeRole" | "GetCallerIdentity") || signed_by(s) == SESSION_KEY
    }));
    let actions: Vec<String> = seen
        .iter()
        .filter_map(|s| form(&s.body).get("Action").cloned())
        .collect();
    assert!(
        actions.iter().all(|a| matches!(
            a.as_str(),
            "AssumeRole" | "DescribeStacks" | "GetTemplate" | "GetStackPolicy"
        )),
        "{actions:?}"
    );
}

/// The resumed apply's requests: 3 policies and 3 protections, set in the
/// floor session minted before the first of them, and one stack made.
fn three_guards_in_the_floor_session_and_one_create(seen: &[Seen]) {
    let a: Vec<String> = seen
        .iter()
        .filter_map(|s| form(&s.body).get("Action").cloned())
        .collect();
    let count = |op: &str| a.iter().filter(|x| *x == op).count();
    assert_eq!(
        (
            count("SetStackPolicy"),
            count("UpdateTerminationProtection"),
            count("CreateChangeSet"),
            count("ExecuteChangeSet")
        ),
        (3, 3, 1, 1),
        "{a:?}"
    );
    let floor = seen.iter().position(|s| {
        let f = form(&s.body);
        f.get("Action").map(String::as_str) == Some("AssumeRole")
            && f.get("RoleSessionName")
                .is_some_and(|n| n.ends_with(".floor"))
    });
    let first = seen
        .iter()
        .position(|s| form(&s.body).get("Action").map(String::as_str) == Some("SetStackPolicy"));
    assert!(floor.is_some() && floor < first, "{a:?}");
    for s in seen {
        let f = form(&s.body);
        if matches!(
            f.get("Action").map(String::as_str),
            Some("SetStackPolicy" | "UpdateTerminationProtection")
        ) {
            assert_eq!(signed_by(s), SESSION_KEY);
        }
    }
}

/// A stopped bootstrap resumes (theseus-oszz). The account as the second
/// live run left it: the foundation and the posture made, with no policy and
/// no termination protection, and no relay. The plan says what each lacks
/// and sends no write. The apply sets the two stacks' policies and
/// protections in the floor session, with no change set, and creates the
/// relay with its own. A second plan has nothing to do.
#[tokio::test]
async fn a_stopped_bootstrap_resumes_and_sets_what_it_skipped() {
    let owner = [
        ("OwnerUserName", "example"),
        ("MonthlyBudgetUsd", "50"),
        ("DailyBudgetUsd", "0"),
        ("AlertEmail", "alerts@example.com"),
    ];
    let (fake, c) = cloud(Cloud {
        stacks: BTreeMap::from([
            (
                "theseus-foundation".to_string(),
                made(bootstrap::FOUNDATION_TEMPLATE, &owner),
            ),
            (
                "theseus-posture".to_string(),
                made(bootstrap::POSTURE_TEMPLATE, &LEAN_POSTURE),
            ),
        ]),
        owner_role: true,
        ..Default::default()
    });
    let aws = layer(&fake, false, Some(50));
    let account = aws.account(None).unwrap().clone();
    let p = AwsBootstrapParams {
        alert_email: Some("alerts@example.com".into()),
        ..Default::default()
    };
    let plan = bootstrap::plan(&account, &p).await.unwrap();
    let r = bootstrap::result(&account, &plan, false);
    let both = ["stack policy", "termination protection"];
    let shape: Vec<(&str, &str, Vec<&str>)> = r
        .stacks
        .iter()
        .map(|s| {
            (
                s.stack.as_str(),
                s.action.as_str(),
                s.sets.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("theseus-foundation", "none", both.to_vec()),
            ("theseus-posture", "none", both.to_vec()),
            ("theseus-posture-relay", "create", both.to_vec()),
        ]
    );
    assert!(r.changes && r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(!r.stacks[1].policy.contains("TrailKmsKey"));
    assert!(writes(&fake).is_empty(), "{:?}", writes(&fake));
    // The digest names the exact policies, and what the apply sets.
    assert_eq!(bootstrap::digest(&plan.stacks), plan.digest);
    let mut other = plan.stacks.clone();
    other[1].policy.push('\n');
    assert_ne!(bootstrap::digest(&other), plan.digest, "the policy");
    let mut other = plan.stacks.clone();
    other[0].sets.pop();
    assert_ne!(bootstrap::digest(&other), plan.digest, "what it sets");

    let before = fake.seen().len();
    bootstrap::apply(&account, &p, &plan.digest).await.unwrap();
    three_guards_in_the_floor_session_and_one_create(&fake.seen()[before..]);
    let stacks = c.lock().unwrap().stacks.clone();
    assert_eq!(stacks["theseus-posture-relay"].status, "CREATE_COMPLETE");
    assert!(
        stacks.values().all(|s| s.policy.is_some() && s.protected),
        "{stacks:?}"
    );
    let posture = stacks["theseus-posture"].policy.clone().unwrap();
    assert!(!posture.contains("TrailKmsKey"), "{posture}");

    // A second plan: everything as planned, nothing to set, nothing written.
    let before = fake.seen().len();
    let again = bootstrap::plan(&account, &p).await.unwrap();
    let r = bootstrap::result(&account, &again, false);
    assert!(!r.changes, "{:?}", r.stacks);
    assert!(r
        .stacks
        .iter()
        .all(|s| s.action == "none" && s.sets.is_empty()));
    nothing_written_and_no_key(&fake.seen()[before..]);
}

/// A policy that is set but is not this binary's stays, with a warning: the
/// plan sets nothing on it, and its apply sends no write (theseus-oszz).
#[tokio::test]
async fn a_stack_policy_set_by_hand_stays_and_is_a_warning() {
    let by_hand =
        r#"{"Statement":[{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"}]}"#;
    let mut posture = made(bootstrap::POSTURE_TEMPLATE, &LEAN_POSTURE);
    posture.policy = Some(by_hand.into());
    posture.protected = true;
    let mut foundation = made(
        bootstrap::FOUNDATION_TEMPLATE,
        &[
            ("OwnerUserName", "example"),
            ("MonthlyBudgetUsd", "50"),
            ("DailyBudgetUsd", "0"),
            ("AlertEmail", ""),
        ],
    );
    foundation.policy = Some(bootstrap::FOUNDATION_POLICY.into());
    foundation.protected = true;
    let (fake, c) = cloud(Cloud {
        stacks: BTreeMap::from([
            ("theseus-foundation".to_string(), foundation),
            ("theseus-posture".to_string(), posture),
        ]),
        owner_role: true,
        ..Default::default()
    });
    let aws = layer(&fake, false, Some(50));
    let account = aws.account(None).unwrap().clone();
    let p = AwsBootstrapParams::default();
    let plan = bootstrap::plan(&account, &p).await.unwrap();
    let r = bootstrap::result(&account, &plan, false);
    assert!(r.stacks[..2]
        .iter()
        .all(|s| s.action == "none" && s.sets.is_empty()));
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(
        r.warnings[0].starts_with("theseus-posture in us-west-2 has a stack policy that is not"),
        "{:?}",
        r.warnings
    );
    // The apply makes the relay, and leaves the two stacks alone.
    bootstrap::apply(&account, &p, &plan.digest).await.unwrap();
    assert_eq!(
        c.lock().unwrap().stacks["theseus-posture"]
            .policy
            .as_deref(),
        Some(by_hand)
    );
    let guarded: Vec<String> = fake
        .seen()
        .iter()
        .map(|s| form(&s.body))
        .filter(|f| {
            matches!(
                f.get("Action").map(String::as_str),
                Some("SetStackPolicy" | "UpdateTerminationProtection")
            )
        })
        .map(|f| f["StackName"].clone())
        .collect();
    assert_eq!(guarded, ["theseus-posture-relay", "theseus-posture-relay"]);
}

/// The plan's ids for a template under parameters.
fn made_ids(template: &str, params: &[(&str, &str)]) -> Vec<String> {
    let t = theseus_aws_guard::parse_template(template).unwrap();
    let ctx = Context {
        account: ACCOUNT.into(),
        region: "us-west-2".into(),
    };
    let params: BTreeMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    theseus_aws_guard::planned_resources(&t, &ctx, &params)
        .unwrap()
        .into_iter()
        .map(|r| r.logical_id)
        .collect()
}

/// Each stack's policy names only the resources its stack makes
/// (theseus-oszz). AWS refused the lean posture's, which named the
/// `TrailKmsKey` that `TrailKey=aws-managed` does not make; the customer
/// posture's still protects the key; the foundation's and the relay's are
/// their files as written. A statement left with no resource goes, and `*`
/// stays. The fake refuses the uncut file as AWS did.
#[tokio::test]
async fn a_stack_policy_names_only_the_resources_its_stack_makes() {
    let owner = [("OwnerUserName", "example")];
    let lean = bootstrap::stack_policy(
        bootstrap::POSTURE_POLICY,
        &made_ids(bootstrap::POSTURE_TEMPLATE, &LEAN_POSTURE),
    )
    .unwrap();
    assert!(!lean.contains("TrailKmsKey"), "{lean}");
    for id in ["Trail", "TrailBucket", "TrailBucketPolicy"] {
        assert!(
            lean.contains(&format!("\"LogicalResourceId/{id}\"")),
            "{lean}"
        );
    }
    let customer = bootstrap::stack_policy(
        bootstrap::POSTURE_POLICY,
        &made_ids(
            bootstrap::POSTURE_TEMPLATE,
            &[("OwnerUserName", "example"), ("TrailKey", "customer")],
        ),
    )
    .unwrap();
    assert!(customer.contains("\"LogicalResourceId/TrailKmsKey\""));
    assert_eq!(customer, bootstrap::POSTURE_POLICY);
    let foundation = made_ids(bootstrap::FOUNDATION_TEMPLATE, &owner);
    assert_eq!(
        bootstrap::stack_policy(bootstrap::FOUNDATION_POLICY, &foundation).unwrap(),
        bootstrap::FOUNDATION_POLICY
    );
    let relay = made_ids(bootstrap::RELAY_TEMPLATE, &[]);
    assert_eq!(
        bootstrap::stack_policy(bootstrap::RELAY_POLICY, &relay).unwrap(),
        bootstrap::RELAY_POLICY
    );

    // The rule: a statement left with no resource goes, and "*" stays.
    let cut = bootstrap::stack_policy(
        r#"{"Statement":[
            {"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"},
            {"Effect":"Deny","Action":"Update:Delete","Principal":"*","Resource":"LogicalResourceId/Gone"},
            {"Effect":"Deny","Action":"Update:Replace","Principal":"*",
             "Resource":["LogicalResourceId/Gone","LogicalResourceId/Kept"]}]}"#,
        &["Kept".to_string()],
    )
    .unwrap();
    let v: Value = serde_json::from_str(&cut).unwrap();
    assert_eq!(v["Statement"].as_array().map(Vec::len), Some(2), "{cut}");
    assert_eq!(v["Statement"][0]["Resource"], "*");
    assert_eq!(
        v["Statement"][1]["Resource"],
        json!(["LogicalResourceId/Kept"])
    );

    // The fake refuses the uncut file as AWS refused it, and takes the cut.
    let (fake, c) = cloud(Cloud {
        stacks: BTreeMap::from([(
            "theseus-posture".to_string(),
            made(bootstrap::POSTURE_TEMPLATE, &LEAN_POSTURE),
        )]),
        ..Default::default()
    });
    let aws = layer(&fake, false, None);
    let account = aws.account(None).unwrap().clone();
    let cfn = super::stack::Cfn {
        account: &account,
        region: "us-west-2",
        binding: None,
        signer: super::Signer::Key,
    };
    let e = cfn
        .set_policy("theseus-posture", bootstrap::POSTURE_POLICY)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(
            "Error validating stack policy: Unknown logical id 'LogicalResourceId/TrailKmsKey'"
        ),
        "{e}"
    );
    assert!(c.lock().unwrap().stacks["theseus-posture"].policy.is_none());
    cfn.set_policy("theseus-posture", &lean).await.unwrap();
    assert_eq!(
        c.lock().unwrap().stacks["theseus-posture"]
            .policy
            .as_deref(),
        Some(lean.as_str())
    );
}

// ------------------------------------------------------------------ the templates

fn resources_of(template: &str, params: &[(&str, &str)]) -> Vec<(String, Truth)> {
    let t = theseus_aws_guard::parse_template(template).unwrap();
    let ctx = Context {
        account: ACCOUNT.into(),
        region: "us-west-2".into(),
    };
    let params: BTreeMap<String, String> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    theseus_aws_guard::planned_resources(&t, &ctx, &params)
        .unwrap()
        .into_iter()
        .map(|r| (r.resource_type, r.exists))
        .collect()
}

/// The lean posture (C2, theseus-nyzn): by default the stacks make no KMS
/// key, no CloudWatch alarm or metric filter, and no log group;
/// `TrailKey=customer` makes exactly one key, the trail's. The checks are
/// EventBridge rules: the 17, and the two findings rules. Each template
/// scans clean against the guard list, and fits a change set's body.
#[test]
fn the_lean_templates_make_no_key_alarm_or_log_group_by_default() {
    let owner = [("OwnerUserName", "example")];
    let count = |rs: &[(String, Truth)], ty: &str| rs.iter().filter(|(t, _)| t == ty).count();
    for (template, params) in [
        (bootstrap::FOUNDATION_TEMPLATE, &owner[..]),
        (bootstrap::POSTURE_TEMPLATE, &owner[..]),
        (bootstrap::RELAY_TEMPLATE, &[][..]),
    ] {
        let rs = resources_of(template, params);
        for ty in [
            "AWS::KMS::Key",
            "AWS::CloudWatch::Alarm",
            "AWS::Logs::LogGroup",
            "AWS::Logs::MetricFilter",
        ] {
            assert_eq!(count(&rs, ty), 0, "{ty}");
        }
        assert!(
            rs.iter().all(|(_, e)| *e == Truth::Yes),
            "every condition decided"
        );
        assert!(
            template.len() <= super::stack::BODY_LIMIT,
            "{} bytes",
            template.len()
        );
        let parsed = theseus_aws_guard::parse_template(template).unwrap();
        let ctx = Context {
            account: ACCOUNT.into(),
            region: "us-west-2".into(),
        };
        let given: BTreeMap<String, String> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let scan = theseus_aws_guard::embedded()
            .scan(&parsed, &ctx, &given)
            .unwrap();
        let hits: Vec<String> = scan.hits.iter().map(|h| h.confirm()).collect();
        assert!(
            hits.is_empty() && scan.notes.is_empty(),
            "{hits:?} {:?}",
            scan.notes
        );
    }
    let posture = resources_of(bootstrap::POSTURE_TEMPLATE, &owner);
    assert_eq!(count(&posture, "AWS::Events::Rule"), 19);
    let customer = resources_of(
        bootstrap::POSTURE_TEMPLATE,
        &[("OwnerUserName", "example"), ("TrailKey", "customer")],
    );
    assert_eq!(count(&customer, "AWS::KMS::Key"), 1);
    assert_eq!(count(&customer, "AWS::KMS::Alias"), 1);
    let quiet = resources_of(
        bootstrap::POSTURE_TEMPLATE,
        &[("OwnerUserName", "example"), ("GuardDuty", "disabled")],
    );
    assert_eq!(count(&quiet, "AWS::GuardDuty::Detector"), 0);
    assert_eq!(count(&quiet, "AWS::Events::Rule"), 18);
}

// ------------------------------------------------------------------ a program's session

/// `[broker.programs.aws]` (§3.5): before the owner role exists the program
/// gets nothing, and the result says why; after, it gets a job session named
/// by the job's correlation id, under the guards and the stack path's, for
/// its deadline, with the region and no `~/.aws` profile. Never the key.
#[tokio::test]
async fn a_programs_aws_grant_is_a_job_session_never_the_key() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let aws_cli = bin.path().join("aws");
    std::fs::write(&aws_cli, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&aws_cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.path().display());
    let mut cfg = crate::config::BrokerConfig::default();
    cfg.programs.insert(
        "aws".into(),
        crate::config::ProgramGrant {
            env: BTreeMap::new(),
            aws_account: Some(ACCOUNT.into()),
        },
    );
    let job = crate::broker::JobAws {
        correlation_id: "act_job_1",
        lasts: Duration::from_secs(600),
    };
    let argv = vec!["aws".to_string(), "s3".into(), "ls".into()];
    for owner in [false, true] {
        let fake = Fake::start(|s, n| {
            let f = form(&s.body);
            match f.get("Action").map(String::as_str) {
                Some("AssumeRole") => assumed(n, f.get("RoleSessionName").map_or("?", |s| s)),
                _ => sts(ACCOUNT, n),
            }
        });
        let broker = crate::broker::Broker::new(&cfg, board(), Some(path.clone()));
        broker.set_aws(layer(&fake, owner, None));
        let gate = broker.at_gate("proc.run", Some(&argv), &[], bin.path(), Some(&path));
        assert_eq!(gate.len(), 1);
        assert_eq!(gate[0].secret, crate::broker::aws_session_label(ACCOUNT));
        let j = broker
            .for_job_of(
                &argv,
                &[],
                bin.path(),
                Some(&path),
                Posture::Notify,
                Some(job),
            )
            .await;
        let env: BTreeMap<&str, &str> = j
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.expose()))
            .collect();
        assert!(
            !env.values().any(|v| *v == SECRET || *v == KEY_ID),
            "never the key"
        );
        if !owner {
            assert!(env.is_empty() && j.granted.is_empty());
            assert!(
                j.withheld[0]
                    .1
                    .contains("no session before its owner role exists"),
                "{:?}",
                j.withheld
            );
            continue;
        }
        assert_eq!(env["AWS_ACCESS_KEY_ID"], SESSION_KEY);
        assert_eq!(env["AWS_SECRET_ACCESS_KEY"], SESSION_SECRET);
        assert_eq!(env["AWS_SESSION_TOKEN"], "test-session-token");
        assert_eq!(
            (
                env["AWS_REGION"],
                env["AWS_CONFIG_FILE"],
                env["AWS_SHARED_CREDENTIALS_FILE"]
            ),
            ("us-west-2", "/dev/null", "/dev/null")
        );
        let mint = first_mint(&fake.seen());
        assert_eq!(mint["RoleSessionName"], "act_job_1");
        assert_eq!(
            mint["DurationSeconds"], "900",
            "the job's 600 s, raised to STS's shortest"
        );
        assert_eq!(minted_arns(&mint), policy_arns(ACCOUNT, Kind::Job));
        assert_eq!(
            crate::broker::got(&j.granted).as_deref(),
            Some("aws got AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY and AWS_SESSION_TOKEN")
        );
    }
}

/// The harness-only line says what jobs get instead of the key.
#[test]
fn the_harness_only_line_says_jobs_get_sessions_never_the_key() {
    let h = theseus_protocol::cred::HarnessOnly {
        aws: vec!["aws_access_key_id".into(), "aws_secret_access_key".into()],
        aws_sessions: vec!["aws".into()],
        ..Default::default()
    };
    assert_eq!(
        h.line(),
        "broker: a job may be handed no secret; harness-only: the AWS keys (aws_access_key_id, \
         aws_secret_access_key); jobs get short-lived AWS sessions, never the key (aws)"
    );
}

/// The owner role's trust lets the root of trust tag its sessions (found at
/// the first bootstrap, 2026-10-03). AWS authorizes `sts:TagSession` as an
/// action of its own, whose request context carries no `sts:SourceIdentity`,
/// so a statement allowing it under that key's `Null` condition refused every
/// tagged session, and `mint` tags every session. TagSession must sit in a
/// statement with no `sts:SourceIdentity` condition, while `sts:AssumeRole`'s
/// statement keeps requiring the source identity.
#[test]
fn the_owner_role_lets_its_root_of_trust_tag_sessions() {
    let t = theseus_aws_guard::parse_template(bootstrap::FOUNDATION_TEMPLATE)
        .unwrap()
        .to_json();
    let statements = t["Resources"]["OwnerRole"]["Properties"]["AssumeRolePolicyDocument"]
        ["Statement"]
        .as_array()
        .unwrap()
        .clone();
    let actions = |s: &Value| -> Vec<String> {
        match &s["Action"] {
            Value::String(a) => vec![a.clone()],
            Value::Array(v) => v
                .iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    };
    let names_source = |s: &Value| s["Condition"].to_string().contains("sts:SourceIdentity");
    let tagging: Vec<&Value> = statements
        .iter()
        .filter(|s| actions(s).iter().any(|a| a == "sts:TagSession"))
        .collect();
    assert!(!tagging.is_empty(), "a statement allows sts:TagSession");
    assert!(
        tagging.iter().all(|s| !names_source(s)),
        "sts:TagSession is never under an sts:SourceIdentity condition: {tagging:?}"
    );
    let assume = statements
        .iter()
        .find(|s| actions(s).iter().any(|a| a == "sts:AssumeRole"))
        .unwrap();
    assert!(names_source(assume), "the source identity stays required");
}

/// The hands network's plan takes the config's existing network
/// (theseus-mgw.9): a new stack's change set adds the hands' security group
/// alone, no VPC, subnet, route, endpoint, or NAT, and its parameters are
/// the config's; a plan that names another VPC, or the NAT beside it, is
/// refused before anything is sent. With no network in the config, the
/// plan's parameters say the stack's own VPC.
#[tokio::test]
async fn the_hands_network_plan_takes_the_configs_existing_network() {
    let (fake, _c) = cloud(Cloud {
        owner_role: true,
        ..Default::default()
    });
    let mut cfg = config(&fake.url, true, None);
    for a in cfg.accounts.values_mut() {
        a.hands_network = Some(crate::config::HandsNetwork {
            vpc: "vpc-0a1b2c3d4e5f60718".into(),
            subnets: vec![
                "subnet-0a1b2c3d4e5f60711".into(),
                "subnet-0a1b2c3d4e5f60722".into(),
            ],
            security_group: None,
        });
    }
    let aws = Aws::from_config(&cfg, board()).expect("an account");
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("theseus-hands-network.yaml");
    std::fs::write(&template, super::tests_network::NETWORK_TEMPLATE).unwrap();
    let path = template.display().to_string();
    let (r, _) = super::tests::call(
        &aws,
        "aws.stack.plan",
        json!({"stack": "theseus-hands-network", "template": path}),
    )
    .await;
    let text = r.expect("the plan").0.text;
    assert!(
        text.contains("a new stack): 1 change")
            && text.contains("+ HandsSecurityGroup (AWS::EC2::SecurityGroup)"),
        "{text}"
    );
    for part in [
        "VPC",
        "Subnet",
        "Route",
        "VPCEndpoint",
        "NatGateway",
        "FlowLog",
        "EIP",
    ] {
        assert!(
            !text.contains(&format!("(AWS::EC2::{part})")),
            "{part}: {text}"
        );
    }
    assert!(!text.contains("At the floor"), "{text}");
    let sets: Vec<BTreeMap<String, String>> = fake
        .seen()
        .iter()
        .map(|s| form(&s.body))
        .filter(|f| f.get("Action").map(String::as_str) == Some("CreateChangeSet"))
        .collect();
    assert_eq!(sets.len(), 1);
    let params = parameters(&sets[0], &BTreeMap::new());
    assert_eq!(params["ExistingVpcId"], "vpc-0a1b2c3d4e5f60718");
    assert_eq!(
        params["ExistingSubnetIds"],
        "subnet-0a1b2c3d4e5f60711,subnet-0a1b2c3d4e5f60722"
    );
    assert_eq!(params["ExistingSecurityGroupId"], "");

    // Another VPC, or the NAT beside this one: refused, nothing sent.
    let sent = fake.seen().len();
    let t = tool(&aws, "aws.stack.plan");
    for (given, says) in [
        (
            json!({"ExistingVpcId": "vpc-0b1c2d3e4f5a60729"}),
            "ExistingVpcId comes from the config's [aws.accounts.111122223333.hands_network] (vpc-0a1b2c3d4e5f60718)",
        ),
        (
            json!({"NatGateway": "enabled"}),
            "the stack never makes a NAT beside it",
        ),
    ] {
        let input = json!({"stack": "theseus-hands-network", "template": path, "parameters": given});
        let e = t.plan(&input, &plain_ctx()).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    assert_eq!(fake.seen().len(), sent);

    // No network in the config: the stack's own VPC, and a plan may not
    // name one.
    let aws = layer(&fake, true, None);
    let t = tool(&aws, "aws.stack.plan");
    let input = json!({"stack": "theseus-hands-network", "template": path,
                       "parameters": {"ExistingVpcId": "vpc-0a1b2c3d4e5f60718"}});
    let e = t.plan(&input, &plain_ctx()).unwrap_err();
    assert!(
        e.contains("[aws.accounts.111122223333.hands_network] (none)"),
        "{e}"
    );
}
