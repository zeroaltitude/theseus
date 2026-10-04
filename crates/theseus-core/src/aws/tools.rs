//! The AWS tools (AWS design §3.1, §3.2, §4's ★ rows): `aws.call`,
//! `aws.describe`, `aws.whoami`, and `aws.s3.list`.
//!
//! The three that call AWS are async tools (`Backend::Async`): a call is a
//! future on the daemon's runtime that holds no core while it waits. Each
//! checks its whole call in `plan`, before the gate, with no network: the
//! account and region, the operation in the catalog, and the input against
//! its shape. So a bad call is invalid input, and nothing is sent.
//!
//! `aws.call` makes reads, writes, and runs (C2 = 14b), and its plan reads
//! the guard list (§3.4, §3.6, `theseus_aws_guard`) in its order: a direct
//! guardrail is the floor; a stack write or durable infrastructure is
//! invalid input that points to the stack tools; any other guardrail is the
//! floor; a deletion of what holds state waits for approval. An approved
//! floor call that AWS's guards would refuse runs in a floor session. A call
//! that returns a secret stays invalid input until 14c. `aws.describe` reads
//! the local catalog alone.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use theseus_aws::catalog::{describe_operation, describe_service, Catalog, Class, SecretBearing};
use theseus_aws::{Attribution, Call, CallError, Output};
use theseus_aws_guard::{Context, GuardList, Verdict};
use theseus_tools::{
    parse, AsyncRun, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::session::Kind;
use super::{Account, Aws, Failure, Request, Signer};

/// The most pages one `aws.call` follows.
pub const PAGES_MAX: u32 = 10;
/// Entries `aws.s3.list` shows when the call does not say, and at most.
pub const LIST_DEFAULT: u32 = 200;
pub const LIST_MAX: u32 = 5000;
/// What S3 returns in one page of a listing.
const S3_PAGE: u32 = 1000;

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(CallTool(aws.clone())),
        Arc::new(Describe),
        Arc::new(Whoami(aws.clone())),
        Arc::new(S3List(aws.clone())),
    ]
}

/// A call's words for an error: what it was, and what happened.
pub(super) fn failure(what: &str, f: Failure) -> ToolFailure {
    let message = match &f {
        Failure::Unbound(why) => format!("{what} was not sent: {why}."),
        Failure::Call(CallError::Aws(e)) => match &e.denial {
            Some(d) => format!("{what}: AWS answered {e}. {}", refused_by(d)),
            None => format!("{what}: AWS answered {e}."),
        },
        Failure::Call(e @ CallError::NotSent { .. }) => format!("{what} was not sent: {e}."),
        Failure::Call(CallError::Unsupported(m)) => {
            format!("{what} was not sent: {m}. The aws CLI can make it, through proc_run.")
        }
        Failure::Call(e) => format!("{what}: {e}."),
    };
    ToolFailure::new(message)
}

/// Who refused a call, in words (§3.6): Theseus's own guard, a session's
/// narrowing, IAM, or an Organization's policy.
fn refused_by(d: &theseus_aws::Denial) -> String {
    use theseus_aws::Enforcer;
    let policy = d
        .policy
        .as_deref()
        .map(|p| format!(" ({p})"))
        .unwrap_or_default();
    match d.enforcer {
        Enforcer::Guard => format!(
            "Theseus's own guard refused it: an explicit deny in a session policy{policy}, \
             the theseus-guard-* policies every work session carries. An approved floor call \
             runs without them."
        ),
        Enforcer::Iam if d.policy_type == "session policy" => format!(
            "The session policy refused it: the narrowing this session was minted with \
             allows too little{policy}."
        ),
        Enforcer::Iam => format!(
            "IAM refused it: {} {}{policy}.",
            if d.explicit {
                "an explicit deny in an"
            } else {
                "no"
            },
            d.policy_type
        ),
        Enforcer::Scp | Enforcer::Rcp => format!(
            "The Organization refused it: a {}{policy}, which Theseus cannot change.",
            d.policy_type
        ),
    }
}

/// `1,234`.
pub(super) fn count(n: u64) -> String {
    crate::narrative::thousands(n)
}

/// Cents as dollars: `1234` is `12.34`.
pub(super) fn cents(c: u64) -> String {
    format!("{}.{:02}", c / 100, c % 100)
}

/// A dollar amount AWS writes as text (`"12.3456"`), in cents.
pub(super) fn to_cents(amount: &str) -> Option<u64> {
    let v: f64 = amount.trim().parse().ok()?;
    (v.is_finite() && v >= 0.0).then(|| (v * 100.0).round() as u64)
}

/// What the client's check says of a call, as the model reads it.
pub(super) fn checked_error(e: CallError) -> String {
    match e {
        CallError::InvalidInput(m) => m,
        CallError::Unsupported(m) => {
            format!("{m}: aws.call cannot make it; the aws CLI can, through proc_run")
        }
        other => other.to_string(),
    }
}

// ------------------------------------------------------------------ aws.call

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallArgs {
    service: String,
    operation: String,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    pages: Option<u32>,
}

/// A call as `plan` checked it.
struct Planned {
    account: Arc<Account>,
    region: String,
    service: String,
    operation: String,
    input: Value,
    pages: u32,
    cost_bearing: bool,
    class: ToolClass,
    /// The floor's confirm line, when a guardrail hits (§3.6).
    guardrail: Option<String>,
    /// A hit AWS's guards refuse in a work session: the approved call runs
    /// in a floor session, with no guard, for this one call.
    floor_session: bool,
    /// It deletes something that holds state (§3.9's approve list).
    destructive: bool,
}

impl Planned {
    fn name(&self) -> String {
        format!("{}:{}", self.service, self.operation)
    }

    fn what(&self) -> String {
        format!("{} in {}", self.name(), self.region)
    }

    fn aws(&self) -> AwsPlan {
        AwsPlan {
            account: self.account.id.clone(),
            region: self.region.clone(),
            service: self.service.clone(),
            operation: self.operation.clone(),
            cost_bearing: self.cost_bearing,
            resources: resources(&self.input),
            guardrail: self.guardrail.clone(),
            destructive: self.destructive,
        }
    }

    fn signer(&self) -> Signer<'static> {
        Signer::As(if self.floor_session {
            Kind::Floor
        } else {
            Kind::Work
        })
    }
}

/// What the guard list says of a call (§3.4, §3.6), in its order: the floor's
/// confirm line, whether AWS's guards would refuse it in a work session, and
/// whether it deletes what holds state; or the invalid input a stack write or
/// durable infrastructure is.
pub(super) fn guard(
    account: &str,
    region: &str,
    op: &str,
    input: &Value,
) -> Result<(Option<String>, bool, bool), String> {
    let ctx = Context {
        account: account.into(),
        region: region.into(),
    };
    match theseus_aws_guard::embedded().check_call(&ctx, op, input) {
        Verdict::Clear => Ok((None, false, false)),
        Verdict::Destructive => Ok((None, false, true)),
        Verdict::StackOnly => Err(format!(
            "{}. Nothing was sent.",
            GuardList::stack_message(op)
        )),
        Verdict::IacOnly => Err(format!("{}. Nothing was sent.", GuardList::iac_message(op))),
        Verdict::Floor(hits) => Ok((
            Some(
                hits.iter()
                    .map(|h| h.confirm(op))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            hits.iter().any(|h| h.guarded()),
            false,
        )),
    }
}

/// The names, ids, and ARNs a call's input gives at its top level: members
/// whose names end in `Arn`, `Name`, `Id`, or `Bucket`, S3's `Key` and
/// `Prefix`, and lists of them (§3.1, "Resources"), at most ten.
fn resources(input: &Value) -> Vec<String> {
    let Some(o) = input.as_object() else {
        return Vec::new();
    };
    let named = |k: &str| {
        ["Arn", "Name", "Id", "Bucket", "Arns", "Names", "Ids"]
            .iter()
            .any(|s| k.ends_with(s))
            || k == "Key"
            || k == "Prefix"
    };
    o.iter()
        .filter(|(k, _)| named(k))
        .flat_map(|(_, v)| match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            _ => Vec::new(),
        })
        .take(10)
        .collect()
}

pub struct CallTool(Arc<Aws>);

impl CallTool {
    /// The call, checked: the account and region, the operation, its input
    /// against the shape, its class, and what the guard list says of it.
    fn planned(&self, input: &Value) -> Result<Planned, String> {
        let a: CallArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let pages = a.pages.unwrap_or(1);
        if !(1..=PAGES_MAX).contains(&pages) {
            return Err(format!("pages must be 1 to {PAGES_MAX}"));
        }
        let body = a.input.unwrap_or_else(|| json!({}));
        if !body.is_object() {
            return Err(
                "input must be an object keyed by the operation's member names, as \
                 aws_describe shows them"
                    .into(),
            );
        }
        let attribution = Attribution::default();
        let call = Call {
            service: &a.service,
            operation: &a.operation,
            input: &body,
            region: Some(&region),
            pages,
            attribution: &attribution,
        };
        let checked = account.client().check(&call).map_err(checked_error)?;
        let c = &checked.classification;
        let name = format!("{}:{}", checked.service, checked.operation);
        let class = match c.class {
            Class::Read => ToolClass::Read,
            Class::Write => ToolClass::Write,
            Class::Run => ToolClass::Run,
        };
        if c.secret != SecretBearing::No && c.secret.for_input(&body) {
            return Err(format!(
                "{name} returns a secret value: secret-bearing reads arrive with step 14c \
                 (AWS's C3), as a handle the model never reads. Nothing was sent."
            ));
        }
        let (guardrail, floor_session, destructive) = guard(&account.id, &region, &name, &body)?;
        Ok(Planned {
            account,
            region,
            service: checked.service,
            operation: checked.operation,
            input: body,
            pages,
            cost_bearing: c.cost_bearing,
            class,
            guardrail,
            floor_session,
            destructive,
        })
    }
}

impl Tool for CallTool {
    fn name(&self) -> &'static str {
        "aws.call"
    }
    fn description(&self) -> &'static str {
        "Call any AWS API operation of any service on Theseus's own AWS account, with \
         Theseus's client: a read, a write, or a run. `service` and `operation` as aws_describe \
         names them (cloudformation, DescribeStacks); `input` as JSON keyed by the operation's \
         member names, as aws_describe shows them (timestamps as RFC 3339 strings, blobs as \
         text or {\"base64\": …}). It returns the operation's output as JSON, and `pages` \
         follows its paginator. Durable infrastructure (buckets, roles, networks, functions, \
         alarms, rules) is made only through stacks: such an operation is invalid input that \
         points to aws_stack_plan. A guardrail (public ingress, the audit trail, the budget, \
         long-lived credentials) and deleting what holds state wait for the operator. An \
         operation that returns a secret is invalid input until step 14c. Event streams, \
         SigV2 services, and S3 directory buckets need the aws CLI, through proc_run."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": {"type": "string", "description": "The service, as aws_describe names it: s3, ec2, cloudformation, sts."},
                "operation": {"type": "string", "description": "The operation: DescribeStacks, ListObjectsV2."},
                "input": {"type": "object", "description": "The operation's input, keyed by its member names (default {})."},
                "region": {"type": "string", "description": "The region (default the account's own); it must be one of the account's regions."},
                "account": {"type": "string", "description": "The account's id, when several are bound."},
                "pages": {"type": "integer", "minimum": 1, "maximum": PAGES_MAX, "description": "Pages to read when the operation pages (default 1)."}
            },
            "required": ["service", "operation"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A write may have run when a crash cut it: it is unknown, never run
        // again; a read loses nothing by being asked again.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let p = self.planned(input)?;
        Ok(Plan {
            summary: format!("{} {}", p.class.as_str(), p.what()),
            class: Some(p.class),
            aws: Some(p.aws()),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let planned = self.planned(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let p = planned.map_err(ToolFailure::new)?;
            let req = Request {
                service: &p.service,
                operation: &p.operation,
                input: &p.input,
                region: &p.region,
                pages: p.pages,
                class: p.class.as_str(),
                signer: p.signer(),
            };
            let out = p
                .account
                .request(binding.as_deref(), &req)
                .await
                .map_err(|f| failure(&p.what(), f))?;
            Ok((
                ToolOutput {
                    text: call_text(&p, &out),
                    meta: meta(&p.account.id, &p.region, &p.name(), &out),
                },
                None,
            ))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower call returns them: fewer pages, or a filter in \"input\" (aws_describe shows \
         the operation's members)"
            .into()
    }
}

/// What a call's result says: the call, AWS's request id, the pages, the
/// output as JSON, and how to read the next page.
fn call_text(p: &Planned, out: &Output) -> String {
    let mut text = format!(
        "{}, account {}: HTTP {} · request {} · {} {}\n",
        p.what(),
        p.account.id,
        out.status,
        out.request_id.as_deref().unwrap_or("(none)"),
        out.pages,
        if out.pages == 1 { "page" } else { "pages" },
    );
    text.push_str(&serde_json::to_string_pretty(&out.body).unwrap_or_default());
    text.push('\n');
    if let Some(next) = &out.next {
        text.push_str(&format!(
            "More pages remain: ask for more \"pages\" (at most {PAGES_MAX}), or call again with \
             these in \"input\": {}\n",
            Value::Object(next.clone())
        ));
    }
    text
}

/// A call's meta, for the ledger and the web UI: never the result.
pub(super) fn meta(account: &str, region: &str, name: &str, out: &Output) -> Value {
    json!({
        "account": account,
        "region": region,
        "operation": name,
        "http_status": out.status,
        "request_id": out.request_id,
        "pages": out.pages,
        "attempts": out.attempts,
        "more": out.next.is_some(),
    })
}

// ------------------------------------------------------------------ aws.describe

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeArgs {
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    operation: Option<String>,
}

pub struct Describe;

impl Describe {
    fn args(input: &Value) -> Result<DescribeArgs, String> {
        let a: DescribeArgs = parse(input)?;
        if a.service.is_none() && a.operation.is_some() {
            return Err("an operation needs its service".into());
        }
        Ok(a)
    }

    /// The catalog's answer, from the embedded models: no network.
    fn describe(a: &DescribeArgs) -> Result<String, String> {
        let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
        let Some(name) = &a.service else {
            let names: Vec<&str> = catalog.services().iter().map(|e| e.name.as_str()).collect();
            return Ok(format!(
                "{} AWS services, from the models of {}. Describe one for its operations by \
                 class:\n{}\n",
                names.len(),
                catalog.snapshot(),
                names.join(", ")
            ));
        };
        let svc = catalog.service(name).map_err(|e| e.to_string())?;
        match &a.operation {
            None => {
                let v = describe_service(&svc);
                let list = |k: &str| -> String {
                    v.pointer(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default()
                };
                let n = |k: &str| v.pointer(k).and_then(Value::as_array).map_or(0, Vec::len);
                let mut text = format!(
                    "{} ({}; protocol {}, API {}): {} operations.\n",
                    svc.name(),
                    v["name"].as_str().unwrap_or_default(),
                    v["protocol"].as_str().unwrap_or_default(),
                    v["api_version"].as_str().unwrap_or_default(),
                    n("/operations/read") + n("/operations/write") + n("/operations/run"),
                );
                for (label, key) in [
                    ("read", "/operations/read"),
                    ("write", "/operations/write"),
                    ("run code", "/operations/run"),
                    ("cost-bearing", "/cost_bearing"),
                    ("secret-bearing", "/secret_bearing"),
                    ("through a stack only (IaC)", "/iac_only"),
                    ("deprecated", "/deprecated"),
                ] {
                    if n(key) > 0 {
                        text.push_str(&format!("{label} ({}): {}\n", n(key), list(key)));
                    }
                }
                text.push_str(
                    "aws.call makes the reads; writes and runs arrive with step 14b. Describe \
                     an operation for its input's members.\n",
                );
                Ok(text)
            }
            Some(op) => {
                let o = svc.operation(op).ok_or_else(|| {
                    format!(
                        "{} has no operation {op:?}: aws_describe {{\"service\": \"{}\"}} lists them",
                        svc.name(),
                        svc.name()
                    )
                })?;
                Ok(serde_json::to_string_pretty(&describe_operation(o)).unwrap_or_default() + "\n")
            }
        }
    }
}

impl Tool for Describe {
    fn name(&self) -> &'static str {
        "aws.describe"
    }
    fn description(&self) -> &'static str {
        "Look up AWS's API in Theseus's local catalog, with no network: with no argument, every \
         service; with a service, its operations by class (read, write, run code) and their \
         flags; with a service and an operation, its input and output as compact JSON schemas, \
         its class, and its paginator. Use it to write an aws_call's input."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": {"type": "string", "description": "A service: s3, ec2, cloudformation (or its id or signing name)."},
                "operation": {"type": "string", "description": "An operation of the service: ListObjectsV2 (or list-objects-v2)."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = Self::args(input)?;
        let summary = match (&a.service, &a.operation) {
            (None, _) => "describe the AWS catalog's services".to_string(),
            (Some(s), None) => format!("describe the AWS service {s}"),
            (Some(s), Some(o)) => format!("describe the AWS operation {s}:{o}"),
        };
        Ok(Plan {
            summary,
            ..Default::default()
        })
    }
    fn run(&self, input: &Value, _ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a = Self::args(input).map_err(ToolFailure::new)?;
        let text = Self::describe(&a).map_err(ToolFailure::new)?;
        Ok(ToolOutput {
            text,
            meta: json!({"service": a.service, "operation": a.operation}),
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "describe one service, or one operation".into()
    }
}

// ------------------------------------------------------------------ aws.whoami

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhoamiArgs {
    #[serde(default)]
    account: Option<String>,
}

pub struct Whoami(Arc<Aws>);

impl Tool for Whoami {
    fn name(&self) -> &'static str {
        "aws.whoami"
    }
    fn description(&self) -> &'static str {
        "Who Theseus is on its AWS account, asked of AWS now (sts:GetCallerIdentity): the \
         account, the identity its key signs as, the region and the regions a call may name, \
         its session, and its budget."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: WhoamiArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?;
        Ok(Plan {
            summary: format!("ask AWS who Theseus is on account {}", account.id),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region: account.cfg.region.clone(),
                service: "sts".into(),
                operation: "GetCallerIdentity".into(),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let account =
            parse::<WhoamiArgs>(input).and_then(|a| self.0.account(a.account.as_deref()).cloned());
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let account = account.map_err(ToolFailure::new)?;
            let region = account.cfg.region.clone();
            let input = json!({});
            let req = Request {
                service: "sts",
                operation: "GetCallerIdentity",
                input: &input,
                region: &region,
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Work),
            };
            let out = account
                .request(binding.as_deref(), &req)
                .await
                .map_err(|f| failure(&format!("sts:GetCallerIdentity in {region}"), f))?;
            let text_of = |k: &str| out.body[k].as_str().unwrap_or("?").to_string();
            let s = account.status();
            let budget = match &s.budget {
                Some(b) => format!(
                    "${} of ${} this month{}, as AWS Budgets said at its last read",
                    cents(b.actual_cents),
                    cents(b.limit_cents),
                    b.forecast_cents
                        .map(|f| format!(" (forecast ${})", cents(f)))
                        .unwrap_or_default()
                ),
                None => "not read yet (theseus-monthly, from the foundation stack; read every \
                         six hours once the owner role is named)"
                    .into(),
            };
            let text = format!(
                "AWS account {}, as sts:GetCallerIdentity answered now (request {}):\n\
                 - identity: {} (user id {})\n\
                 - region: {}; the regions a call may name: {}\n\
                 - signs with: {}\n\
                 - budget: {budget}\n\
                 - its check: {}; AWS requests since the daemon started: {} ({} failed)\n",
                text_of("Account"),
                out.request_id.as_deref().unwrap_or("(none)"),
                text_of("Arn"),
                text_of("UserId"),
                region,
                s.regions.join(", "),
                account.signer(),
                s.state,
                count(s.calls),
                count(s.failed),
            );
            Ok((
                ToolOutput {
                    text,
                    meta: meta(&account.id, &region, "sts:GetCallerIdentity", &out),
                },
                None,
            ))
        })
    }
}

// ------------------------------------------------------------------ aws.s3.list

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    max: Option<u32>,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// A listing as `plan` checked it.
struct Listing {
    account: Arc<Account>,
    /// The account's own region, or the one the call names.
    region: String,
    /// None: the buckets.
    bucket: Option<String>,
    prefix: String,
    max: u32,
    recursive: bool,
    after: Option<String>,
}

impl Listing {
    fn what(&self) -> String {
        match &self.bucket {
            None => format!("the S3 buckets of account {}", self.account.id),
            Some(b) => format!("s3://{b}/{}", self.prefix),
        }
    }

    fn operation(&self) -> &'static str {
        if self.bucket.is_some() {
            "ListObjectsV2"
        } else {
            "ListBuckets"
        }
    }

    fn pages(&self) -> u32 {
        self.max.div_ceil(S3_PAGE).max(1)
    }

    /// The listing's input, for its operation.
    fn input(&self) -> Value {
        let mut o = Map::new();
        let page = self.max.min(S3_PAGE);
        match &self.bucket {
            None => {
                o.insert("MaxBuckets".into(), json!(page));
            }
            Some(b) => {
                o.insert("Bucket".into(), json!(b));
                if !self.prefix.is_empty() {
                    o.insert("Prefix".into(), json!(self.prefix));
                }
                if !self.recursive {
                    o.insert("Delimiter".into(), json!("/"));
                }
                o.insert("MaxKeys".into(), json!(page));
            }
        }
        if let Some(t) = &self.after {
            o.insert("ContinuationToken".into(), json!(t));
        }
        Value::Object(o)
    }
}

/// `s3://bucket/prefix`, `bucket/prefix`, or `bucket`: the bucket and the
/// prefix. A bucket's name is 3 to 63 lowercase letters, digits, dots, and
/// hyphens.
pub(super) fn split_path(path: &str) -> Result<(String, String), String> {
    let rest = path.strip_prefix("s3://").unwrap_or(path);
    let (bucket, prefix) = rest.split_once('/').unwrap_or((rest, ""));
    let ok = (3..=63).contains(&bucket.len())
        && bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
    if !ok {
        return Err(format!(
            "{bucket:?} is not an S3 bucket's name (3 to 63 lowercase letters, digits, dots, \
             and hyphens); a path is s3://bucket/prefix"
        ));
    }
    Ok((bucket.to_string(), prefix.to_string()))
}

pub struct S3List(Arc<Aws>);

impl S3List {
    fn listing(&self, input: &Value) -> Result<Listing, String> {
        let a: ListArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let max = a.max.unwrap_or(LIST_DEFAULT);
        if !(1..=LIST_MAX).contains(&max) {
            return Err(format!("max must be 1 to {LIST_MAX}"));
        }
        let (bucket, prefix) = match a.path.as_deref().map(str::trim) {
            None | Some("") | Some("s3://") => (None, String::new()),
            Some(p) => {
                let (b, p) = split_path(p)?;
                (Some(b), p)
            }
        };
        let l = Listing {
            account,
            region,
            bucket,
            prefix,
            max,
            recursive: a.recursive,
            after: a.after,
        };
        // The client's own check: the input against the operation's shape.
        let input = l.input();
        let attribution = Attribution::default();
        let call = Call {
            service: "s3",
            operation: l.operation(),
            input: &input,
            region: Some(&l.region),
            pages: l.pages(),
            attribution: &attribution,
        };
        l.account.client().check(&call).map_err(checked_error)?;
        Ok(l)
    }
}

impl Tool for S3List {
    fn name(&self) -> &'static str {
        "aws.s3.list"
    }
    fn description(&self) -> &'static str {
        "List S3 on Theseus's own AWS account: with no path, its buckets (each with its region \
         and when it was made); with a path (s3://bucket/prefix), what is under the prefix, \
         folder by folder (recursive: every key), at most `max` entries, with a summary of \
         their count, size, and newest. `after` goes on where a listing stopped."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "s3://bucket/prefix (or bucket/prefix); none for the buckets."},
                "max": {"type": "integer", "minimum": 1, "maximum": LIST_MAX, "description": format!("The most entries (default {LIST_DEFAULT}).")},
                "recursive": {"type": "boolean", "description": "Every key under the prefix, rather than its folders (default false)."},
                "after": {"type": "string", "description": "Go on from where a listing stopped: the token it gave."},
                "region": {"type": "string", "description": "The bucket's region (default the account's own; a bucket elsewhere is found)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let l = self.listing(input)?;
        let resources = match &l.bucket {
            Some(b) if l.prefix.is_empty() => vec![b.clone()],
            Some(b) => vec![b.clone(), l.prefix.clone()],
            None => Vec::new(),
        };
        Ok(Plan {
            summary: format!("list {} in {}", l.what(), l.region),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: l.account.id.clone(),
                region: l.region.clone(),
                service: "s3".into(),
                operation: l.operation().into(),
                resources,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let listing = self.listing(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let mut l = listing.map_err(ToolFailure::new)?;
            let b = binding.as_deref();
            let out = match list(&l, b).await {
                // A bucket in another region: S3 names the error, not the
                // region, so the account's own listing of its buckets finds
                // it, and the listing goes there once, if the account allows.
                Err(Failure::Call(CallError::Aws(e)))
                    if l.bucket.is_some() && moved(&e.code, e.status) =>
                {
                    let bucket = l.bucket.clone().unwrap_or_default();
                    let found = region_of(&l, &bucket, b).await;
                    match found {
                        Some(r) if r != l.region => {
                            l.region = l.account.region(Some(&r)).map_err(|why| {
                                ToolFailure::new(format!(
                                    "s3://{bucket} is in {r}, and {why}. Nothing was listed."
                                ))
                            })?;
                            list(&l, b).await
                        }
                        _ => Err(Failure::Call(CallError::Aws(e))),
                    }
                }
                other => other,
            }
            .map_err(|f| failure(&format!("listing {} in {}", l.what(), l.region), f))?;
            let text = match &l.bucket {
                None => buckets_text(&l, &out),
                Some(_) => objects_text(&l, &out),
            };
            let op = format!("s3:{}", l.operation());
            Ok((
                ToolOutput {
                    text,
                    meta: meta(&l.account.id, &l.region, &op, &out),
                },
                None,
            ))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower prefix returns them, or a smaller max with \"after\" to go on".into()
    }
}

/// S3's words for a bucket addressed in the wrong region.
fn moved(code: &str, status: u16) -> bool {
    status == 301
        || matches!(
            code,
            "PermanentRedirect"
                | "AuthorizationHeaderMalformed"
                | "IllegalLocationConstraintException"
        )
}

async fn list(l: &Listing, b: Option<&theseus_tools::AwsBinding>) -> Result<Output, Failure> {
    let input = l.input();
    let req = Request {
        service: "s3",
        operation: l.operation(),
        input: &input,
        region: &l.region,
        pages: l.pages(),
        class: "read",
        signer: Signer::As(Kind::Work),
    };
    l.account.request(b, &req).await
}

/// A bucket's region, from the account's own listing of its buckets.
async fn region_of(
    l: &Listing,
    bucket: &str,
    b: Option<&theseus_tools::AwsBinding>,
) -> Option<String> {
    let input = json!({"Prefix": bucket, "MaxBuckets": S3_PAGE});
    let region = l.account.cfg.region.clone();
    let req = Request {
        service: "s3",
        operation: "ListBuckets",
        input: &input,
        region: &region,
        pages: 1,
        class: "read",
        signer: Signer::As(Kind::Work),
    };
    let out = l.account.request(b, &req).await.ok()?;
    out.body["Buckets"]
        .as_array()?
        .iter()
        .find(|x| x["Name"] == bucket)
        .and_then(|x| x["BucketRegion"].as_str())
        .map(String::from)
}

fn buckets_text(l: &Listing, out: &Output) -> String {
    let none = Vec::new();
    let buckets = out.body["Buckets"].as_array().unwrap_or(&none);
    let mut text = format!(
        "{} S3 {} in account {} (request {}):\n",
        count(buckets.len() as u64),
        if buckets.len() == 1 {
            "bucket"
        } else {
            "buckets"
        },
        l.account.id,
        out.request_id.as_deref().unwrap_or("(none)"),
    );
    for b in buckets {
        let s = |k: &str| b[k].as_str().unwrap_or("?").to_string();
        text.push_str(&format!(
            "  {} · {} · made {}\n",
            s("Name"),
            s("BucketRegion"),
            s("CreationDate")
        ));
    }
    if let Some(t) = out.next.as_ref().and_then(|n| n.get("ContinuationToken")) {
        text.push_str(&format!(
            "More buckets remain: call again with \"after\": {t}.\n"
        ));
    }
    text
}

fn objects_text(l: &Listing, out: &Output) -> String {
    let none = Vec::new();
    let objects = out.body["Contents"].as_array().unwrap_or(&none);
    let folders = out.body["CommonPrefixes"].as_array().unwrap_or(&none);
    let bytes: u64 = objects.iter().filter_map(|o| o["Size"].as_u64()).sum();
    let newest = objects
        .iter()
        .filter_map(|o| o["LastModified"].as_str())
        .max()
        .map(|t| format!(", the newest {t}"))
        .unwrap_or_default();
    let mut text = format!(
        "{} in {} (request {}): {} {} and {} {} ({} bytes){newest}\n",
        l.what(),
        l.region,
        out.request_id.as_deref().unwrap_or("(none)"),
        count(folders.len() as u64),
        if folders.len() == 1 {
            "folder"
        } else {
            "folders"
        },
        count(objects.len() as u64),
        if objects.len() == 1 {
            "object"
        } else {
            "objects"
        },
        count(bytes),
    );
    if !folders.is_empty() {
        text.push_str("folders:\n");
        for f in folders {
            text.push_str(&format!("  {}\n", f["Prefix"].as_str().unwrap_or("?")));
        }
    }
    if !objects.is_empty() {
        text.push_str("objects (key · bytes · modified):\n");
        for o in objects {
            text.push_str(&format!(
                "  {} · {} · {}\n",
                o["Key"].as_str().unwrap_or("?"),
                count(o["Size"].as_u64().unwrap_or(0)),
                o["LastModified"].as_str().unwrap_or("?")
            ));
        }
    }
    if let Some(t) = out.next.as_ref().and_then(|n| n.get("ContinuationToken")) {
        text.push_str(&format!(
            "The listing stopped at {} entries; more remain: call again with \"after\": {t}.\n",
            count(u64::from(l.max))
        ));
    }
    text
}
