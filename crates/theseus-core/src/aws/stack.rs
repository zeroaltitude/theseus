//! The stack tools (AWS design §3.4; C2 = 14b): durable infrastructure is
//! made only through CloudFormation change sets, which the deployer role
//! applies.
//!
//! - `aws.stack.plan` reads a template, scans it against the guard list,
//!   makes a change set under the deployer, and returns its diff: each
//!   resource added, modified (with what changes), replaced, or removed, the
//!   scan's hits, and the change set's digest. It changes nothing.
//! - `aws.stack.apply` executes exactly the change set a plan showed: its
//!   input names the digest, its gate reads the plan's verdict (a guardrail
//!   is the floor, a stateful resource replaced or removed waits), and it
//!   refuses a change set that changed since. It waits for the stack to
//!   settle.
//! - `aws.stack.status` reads stacks, their recent events, outputs, and drift.
//! - `aws.stack.delete` waits for approval, and says what the stack keeps.
//!
//! They work once the bootstrap made the deployer and the config names the
//! owner role; before that, each is invalid input that says so.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_aws::Output;
use theseus_aws_guard::{ChangeAction, Context, ResourceChange};
use theseus_tools::{
    parse, Access, AsyncRun, AwsBinding, AwsPlan, Backend, Plan, Resource, Retry, Tool, ToolClass,
    ToolCtx, ToolFailure, ToolOutput,
};

use super::session::Kind;
use super::tools::failure;
use super::{Account, Aws, Failure, Request, Signer};

/// The deployer role, CloudFormation's service role (§3.4).
pub const DEPLOYER: &str = "theseus-cfn-deployer";
/// The largest template a change set takes by body.
pub const BODY_LIMIT: usize = 51_200;
/// How long a stack may take to settle before `apply` and `delete` say it
/// is still going.
const SETTLE: Duration = Duration::from_secs(30 * 60);
/// How long a change set may take to be ready.
const READY: Duration = Duration::from_secs(4 * 60);

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(PlanTool(aws.clone())),
        Arc::new(ApplyTool(aws.clone())),
        Arc::new(StatusTool(aws.clone())),
        Arc::new(DeleteTool(aws.clone())),
    ]
}

/// A change set a plan showed, by its digest: what `aws.stack.apply`'s gate
/// reads before it runs, with no network.
#[derive(Clone, Debug)]
pub struct Shown {
    pub account: String,
    pub region: String,
    pub stack: String,
    pub change_set: String,
    /// The floor's lines: the template scan's hits and the change set's.
    pub floor: Vec<String>,
    /// Stateful resources replaced or removed.
    pub destructive: Vec<String>,
}

/// The change sets plans showed in this daemon's life.
#[derive(Default)]
pub struct Shows(Mutex<HashMap<String, Shown>>);

impl Shows {
    fn put(&self, digest: &str, s: Shown) {
        self.0.lock().unwrap().insert(digest.into(), s);
    }
    fn get(&self, digest: &str) -> Option<Shown> {
        self.0.lock().unwrap().get(digest).cloned()
    }
}

/// The ARN of an account's role named `name`.
pub fn role_arn(account: &str, name: &str) -> String {
    format!("arn:aws:iam::{account}:role/{name}")
}

// ------------------------------------------------------------------ CloudFormation

/// CloudFormation for one account and region, as the stack tools, the
/// bootstrap, and the budget's reconcile call it.
pub struct Cfn<'a> {
    pub account: &'a Arc<Account>,
    pub region: &'a str,
    pub binding: Option<&'a AwsBinding>,
    pub signer: Signer<'a>,
}

/// A parameter's value in a change set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Param {
    Value(String),
    Previous,
}

/// What a change set is made from.
pub struct ChangeSetSpec<'a> {
    pub stack: &'a str,
    pub name: String,
    /// `CREATE` or `UPDATE`.
    pub kind: &'a str,
    /// The template by body; none keeps the stack's own.
    pub body: Option<&'a str>,
    /// A template too big for a body, uploaded first.
    pub url: Option<String>,
    pub parameters: Vec<(String, Param)>,
    pub role: Option<String>,
    pub tags: Vec<(String, String)>,
    pub description: String,
}

/// How a change set came out.
pub enum Ready {
    Changes {
        id: String,
        changes: Vec<Value>,
    },
    /// CloudFormation found nothing to change.
    Empty {
        id: String,
    },
    Failed {
        id: String,
        reason: String,
    },
}

/// How a stack settled.
pub struct Settled {
    pub status: String,
    pub reason: Option<String>,
    /// Each failed resource's event: `LogicalId: reason`.
    pub failures: Vec<String>,
    /// It is still going: the wait ran out.
    pub going: bool,
}

/// A missing stack, as CloudFormation says it.
fn missing(f: &Failure) -> bool {
    matches!(f, Failure::Call(e) if e.aws().is_some_and(|a| a.code == "ValidationError" && a.message.contains("does not exist")))
}

impl Cfn<'_> {
    async fn call(&self, op: &str, input: &Value, class: &str) -> Result<Output, Failure> {
        let r = Request {
            service: "cloudformation",
            operation: op,
            input,
            region: self.region,
            pages: 1,
            class,
            signer: self.signer,
        };
        self.account.request(self.binding, &r).await
    }

    /// The stack as `DescribeStacks` says it, or none.
    pub async fn stack(&self, name: &str) -> Result<Option<Value>, Failure> {
        match self
            .call("DescribeStacks", &json!({"StackName": name}), "read")
            .await
        {
            Ok(out) => Ok(out.body["Stacks"].get(0).cloned()),
            Err(f) if missing(&f) => Ok(None),
            Err(f) => Err(f),
        }
    }

    /// The template the stack was made from, as written.
    pub async fn template(&self, name: &str) -> Result<String, Failure> {
        let input = json!({"StackName": name, "TemplateStage": "Original"});
        let out = self.call("GetTemplate", &input, "read").await?;
        Ok(out.body["TemplateBody"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// The stack's policy, or none: `GetStackPolicy` gives no body, or an
    /// empty one, when no policy is set.
    pub async fn policy(&self, name: &str) -> Result<Option<String>, Failure> {
        let out = self
            .call("GetStackPolicy", &json!({"StackName": name}), "read")
            .await?;
        Ok(out.body["StackPolicyBody"]
            .as_str()
            .filter(|b| !b.trim().is_empty())
            .map(String::from))
    }

    /// Make a change set; its id.
    pub async fn create_change_set(&self, s: &ChangeSetSpec<'_>) -> Result<String, Failure> {
        let mut input = json!({
            "StackName": s.stack,
            "ChangeSetName": s.name,
            "ChangeSetType": s.kind,
            "Capabilities": ["CAPABILITY_IAM", "CAPABILITY_NAMED_IAM", "CAPABILITY_AUTO_EXPAND"],
            "Description": s.description,
            "Parameters": s.parameters.iter().map(|(k, v)| match v {
                Param::Value(v) => json!({"ParameterKey": k, "ParameterValue": v}),
                Param::Previous => json!({"ParameterKey": k, "UsePreviousValue": true}),
            }).collect::<Vec<_>>(),
            "Tags": s.tags.iter().map(|(k, v)| json!({"Key": k, "Value": v})).collect::<Vec<_>>(),
        });
        match (s.body, &s.url) {
            (Some(b), _) => input["TemplateBody"] = json!(b),
            (None, Some(u)) => input["TemplateURL"] = json!(u),
            (None, None) => input["UsePreviousTemplate"] = json!(true),
        }
        if let Some(r) = &s.role {
            input["RoleARN"] = json!(r);
        }
        let out = self.call("CreateChangeSet", &input, "write").await?;
        Ok(out.body["Id"].as_str().unwrap_or(&s.name).to_string())
    }

    /// Wait until the change set is ready, then its changes, every page.
    pub async fn ready(&self, stack: &str, id: &str) -> Result<Ready, Failure> {
        let until = Instant::now() + READY;
        let mut pause = Duration::from_secs(1);
        loop {
            let input = json!({"StackName": stack, "ChangeSetName": id});
            let out = self.call("DescribeChangeSet", &input, "read").await?;
            let b = &out.body;
            match b["Status"].as_str().unwrap_or_default() {
                "CREATE_COMPLETE" => {
                    let mut changes = b["Changes"].as_array().cloned().unwrap_or_default();
                    let mut next = b["NextToken"].as_str().map(String::from);
                    while let Some(t) = next.take() {
                        let input =
                            json!({"StackName": stack, "ChangeSetName": id, "NextToken": t});
                        let more = self.call("DescribeChangeSet", &input, "read").await?;
                        changes
                            .extend(more.body["Changes"].as_array().cloned().unwrap_or_default());
                        next = more.body["NextToken"].as_str().map(String::from);
                    }
                    return Ok(Ready::Changes {
                        id: id.into(),
                        changes,
                    });
                }
                "FAILED" => {
                    let reason = b["StatusReason"].as_str().unwrap_or_default().to_string();
                    let empty = reason.contains("didn't contain changes")
                        || reason.contains("No updates are to be performed");
                    return Ok(if empty {
                        Ready::Empty { id: id.into() }
                    } else {
                        Ready::Failed {
                            id: id.into(),
                            reason,
                        }
                    });
                }
                _ if Instant::now() >= until => {
                    return Ok(Ready::Failed {
                        id: id.into(),
                        reason: format!("not ready after {} s", READY.as_secs()),
                    })
                }
                _ => {}
            }
            tokio::time::sleep(pause).await;
            pause = (pause * 2).min(Duration::from_secs(5));
        }
    }

    pub async fn execute(&self, stack: &str, id: &str) -> Result<(), Failure> {
        let input = json!({"StackName": stack, "ChangeSetName": id});
        self.call("ExecuteChangeSet", &input, "write")
            .await
            .map(|_| ())
    }

    pub async fn delete_change_set(&self, stack: &str, id: &str) -> Result<(), Failure> {
        let input = json!({"StackName": stack, "ChangeSetName": id});
        self.call("DeleteChangeSet", &input, "write")
            .await
            .map(|_| ())
    }

    /// Wait for the stack to settle, at most `within`: its status, and each
    /// resource that failed.
    pub async fn settle(&self, stack: &str, within: Duration) -> Result<Settled, Failure> {
        let until = Instant::now() + within;
        loop {
            let s = self.stack(stack).await?;
            let status = s.as_ref().map_or("DELETE_COMPLETE".to_string(), |s| {
                s["StackStatus"].as_str().unwrap_or_default().to_string()
            });
            let going = status.ends_with("_IN_PROGRESS");
            if !going || Instant::now() >= until {
                let failures = if status.contains("FAILED") || status.contains("ROLLBACK") {
                    self.failures(stack).await.unwrap_or_default()
                } else {
                    Vec::new()
                };
                return Ok(Settled {
                    reason: s.and_then(|s| s["StackStatusReason"].as_str().map(String::from)),
                    status,
                    failures,
                    going,
                });
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// The stack's recent events that failed, newest first: `LogicalId: reason`.
    async fn failures(&self, stack: &str) -> Result<Vec<String>, Failure> {
        let out = self
            .call("DescribeStackEvents", &json!({"StackName": stack}), "read")
            .await?;
        Ok(out.body["StackEvents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| {
                e["ResourceStatus"]
                    .as_str()
                    .is_some_and(|s| s.ends_with("_FAILED"))
            })
            .take(10)
            .map(|e| {
                format!(
                    "{}: {}",
                    e["LogicalResourceId"].as_str().unwrap_or("?"),
                    e["ResourceStatusReason"]
                        .as_str()
                        .unwrap_or("no reason given")
                )
            })
            .collect())
    }

    pub async fn set_policy(&self, stack: &str, policy: &str) -> Result<(), Failure> {
        let input = json!({"StackName": stack, "StackPolicyBody": policy});
        self.call("SetStackPolicy", &input, "write")
            .await
            .map(|_| ())
    }

    pub async fn protect(&self, stack: &str) -> Result<(), Failure> {
        let input = json!({"StackName": stack, "EnableTerminationProtection": true});
        self.call("UpdateTerminationProtection", &input, "write")
            .await
            .map(|_| ())
    }
}

/// A change set's changes as the guard list reads them (§3.4).
pub fn resource_changes(changes: &[Value]) -> Vec<ResourceChange> {
    changes
        .iter()
        .filter_map(|c| {
            let r = c.get("ResourceChange")?;
            let action = match r["Action"].as_str()? {
                "Add" => ChangeAction::Add,
                "Modify" => ChangeAction::Modify,
                "Remove" => ChangeAction::Remove,
                "Import" => ChangeAction::Import,
                _ => ChangeAction::Dynamic,
            };
            Some(ResourceChange {
                logical_id: r["LogicalResourceId"].as_str()?.to_string(),
                physical_id: r["PhysicalResourceId"].as_str().map(String::from),
                resource_type: r["ResourceType"].as_str().unwrap_or("?").to_string(),
                action,
                replacement: matches!(r["Replacement"].as_str(), Some("True" | "Conditional")),
            })
        })
        .collect()
}

/// One change, as the diff shows it: `+ Bucket (AWS::S3::Bucket)`, `~ Role
/// (AWS::IAM::Role): Policies, Tags`, `! Table (…) replaced`, `- Queue (…)`.
fn change_line(c: &Value) -> Option<String> {
    let r = c.get("ResourceChange")?;
    let id = r["LogicalResourceId"].as_str()?;
    let ty = r["ResourceType"].as_str().unwrap_or("?");
    let replaced = matches!(r["Replacement"].as_str(), Some("True" | "Conditional"));
    let props: Vec<&str> = r["Details"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["Target"]["Name"].as_str())
        .collect();
    Some(match r["Action"].as_str().unwrap_or("?") {
        "Add" => format!("+ {id} ({ty})"),
        "Remove" => format!("- {id} ({ty})"),
        _ if replaced => format!("! {id} ({ty}) replaced{}", props_of(&props)),
        _ => format!("~ {id} ({ty}){}", props_of(&props)),
    })
}

fn props_of(p: &[&str]) -> String {
    if p.is_empty() {
        String::new()
    } else {
        let mut p = p.to_vec();
        p.dedup();
        format!(": {}", p.join(", "))
    }
}

/// The change set's digest: what `apply` names, over its stack, its id, and
/// each change as CloudFormation describes it.
pub fn digest(stack: &str, id: &str, changes: &[Value]) -> String {
    let text = json!({"stack": stack, "change_set": id, "changes": changes}).to_string();
    hex::encode(&Sha256::digest(text.as_bytes())[..12])
}

/// A change set's name from a correlation id: a letter first, then letters,
/// digits, and hyphens.
fn change_set_name(correlation: &str) -> String {
    let tail: String = correlation
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(100)
        .collect();
    format!("theseus-{tail}")
}

/// The stack tools need the deployer, which the bootstrap makes.
fn needs_bootstrap(a: &Account) -> Result<(), String> {
    match a.cfg.owner_role {
        Some(_) => Ok(()),
        None => Err(format!(
            "the stack tools apply through the deployer role ({DEPLOYER}), which theseus aws \
             bootstrap makes; account {} names no owner_role yet, so nothing was sent",
            a.id
        )),
    }
}

fn stack_name_ok(s: &str) -> Result<(), String> {
    let ok = s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{s:?} is not a stack's name: a letter, then letters, digits, and hyphens"
        ))
    }
}

// ------------------------------------------------------------------ aws.stack.plan

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanArgs {
    stack: String,
    template: String,
    #[serde(default)]
    parameters: BTreeMap<String, String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

struct Planning {
    account: Arc<Account>,
    region: String,
    stack: String,
    path: std::path::PathBuf,
    body: String,
    parameters: BTreeMap<String, String>,
    /// The template scan's floor lines.
    scan: Vec<String>,
    notes: Vec<String>,
}

pub struct PlanTool(Arc<Aws>);

impl PlanTool {
    fn planning(&self, input: &Value, ctx: &ToolCtx) -> Result<Planning, String> {
        let a: PlanArgs = parse(input)?;
        stack_name_ok(&a.stack)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        needs_bootstrap(&account)?;
        let region = account.region(a.region.as_deref())?;
        let path = ctx.resolve(&a.template);
        let body = std::fs::read_to_string(&path)
            .map_err(|e| format!("the template {} could not be read: {e}", path.display()))?;
        let template = theseus_aws_guard::parse_template(&body).map_err(|e| e.to_string())?;
        let gctx = Context {
            account: account.id.clone(),
            region: region.clone(),
        };
        let scan = theseus_aws_guard::embedded()
            .scan(&template, &gctx, &a.parameters)
            .map_err(|e| e.to_string())?;
        Ok(Planning {
            account,
            region,
            stack: a.stack,
            path,
            body,
            parameters: a.parameters,
            scan: scan.hits.iter().map(|h| h.confirm()).collect(),
            notes: scan.notes,
        })
    }
}

impl Tool for PlanTool {
    fn name(&self) -> &'static str {
        "aws.stack.plan"
    }
    fn description(&self) -> &'static str {
        "Plan a change to a CloudFormation stack on Theseus's AWS account: the template (a YAML \
         or JSON file), its parameters, and the stack's name. It scans the template against the \
         guardrails, makes a change set that the deployer role will apply, and returns the diff \
         (+ added, ~ modified with what changes, ! replaced, - removed), the guardrails it \
         touches, and the change set's digest. It changes nothing: aws_stack_apply with the \
         digest applies exactly this change set. Durable infrastructure is made only this way."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "stack": {"type": "string", "description": "The stack's name: theseus-<something>."},
                "template": {"type": "string", "description": "The template file, YAML or JSON (relative to the working directory)."},
                "parameters": {"type": "object", "additionalProperties": {"type": "string"}, "description": "The template's parameters by name; one left out keeps its value (or its default)."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["stack", "template"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        // A change set changes nothing until applied.
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn deadline(&self) -> Option<Duration> {
        Some(READY + Duration::from_secs(60))
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let p = self.planning(input, ctx)?;
        Ok(Plan {
            resources: vec![Resource {
                access: Access::Read,
                path: p.path.clone(),
            }],
            summary: format!(
                "plan stack {} in {} from {}",
                p.stack,
                p.region,
                p.path.display()
            ),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: p.account.id.clone(),
                region: p.region.clone(),
                service: "cloudformation".into(),
                operation: "CreateChangeSet".into(),
                resources: vec![p.stack.clone()],
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let planning = self.planning(input, ctx);
        let binding = ctx.aws.clone();
        let shows = self.0.clone();
        Box::pin(async move {
            let p = planning.map_err(ToolFailure::new)?;
            run_plan(&shows, &p, binding.as_deref()).await
        })
    }
}

async fn run_plan(
    aws: &Aws,
    p: &Planning,
    binding: Option<&AwsBinding>,
) -> theseus_tools::AsyncResult {
    let cfn = Cfn {
        account: &p.account,
        region: &p.region,
        binding,
        signer: Signer::As(Kind::Work),
    };
    let what = format!("planning stack {} in {}", p.stack, p.region);
    let exists = cfn.stack(&p.stack).await.map_err(|f| failure(&what, f))?;
    let id = make_change_set(&cfn, p, exists.is_some(), binding).await?;
    let (id, changes) = match cfn
        .ready(&p.stack, &id)
        .await
        .map_err(|f| failure(&what, f))?
    {
        Ready::Changes { id, changes } => (id, changes),
        Ready::Empty { id } => {
            let _ = cfn.delete_change_set(&p.stack, &id).await;
            return Ok((
                ToolOutput {
                    text: format!(
                        "Stack {} in {}: the template and parameters change nothing; no change \
                         set is kept.\n",
                        p.stack, p.region
                    ),
                    meta: json!({"stack": p.stack, "region": p.region, "changes": 0}),
                },
                None,
            ));
        }
        Ready::Failed { reason, .. } => {
            return Err(ToolFailure::new(format!(
                "{what}: the change set failed: {reason}"
            )))
        }
    };
    let verdict = theseus_aws_guard::embedded().check_change_set(&resource_changes(&changes));
    let mut floor = p.scan.clone();
    floor.extend(verdict.floor.iter().map(|h| h.why.clone()));
    let destructive: Vec<String> = verdict.destructive.iter().map(|h| h.why.clone()).collect();
    let digest = digest(&p.stack, &id, &changes);
    let shown = Shown {
        account: p.account.id.clone(),
        region: p.region.clone(),
        stack: p.stack.clone(),
        change_set: id,
        floor,
        destructive,
    };
    let out = plan_output(p, exists.is_some(), &changes, &digest, &shown);
    aws.shows.put(&digest, shown);
    Ok((out, None))
}

/// The plan's change set, made from the template (by body, or uploaded when
/// too big for one) under the deployer: its id.
async fn make_change_set(
    cfn: &Cfn<'_>,
    p: &Planning,
    exists: bool,
    binding: Option<&AwsBinding>,
) -> Result<String, ToolFailure> {
    let url = if p.body.len() > BODY_LIMIT {
        Some(
            upload(&p.account, &p.region, &p.stack, &p.body, binding)
                .await
                .map_err(|f| failure(&format!("uploading {}'s template", p.stack), f))?,
        )
    } else {
        None
    };
    let correlation = binding.map_or("plan", |b| b.correlation_id.as_str());
    let execution = binding.map_or("none", |b| b.execution_id.as_str());
    let spec = ChangeSetSpec {
        stack: &p.stack,
        name: change_set_name(correlation),
        kind: if exists { "UPDATE" } else { "CREATE" },
        body: url.is_none().then_some(p.body.as_str()),
        url,
        parameters: p
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), Param::Value(v.clone())))
            .collect(),
        role: Some(role_arn(&p.account.id, DEPLOYER)),
        tags: vec![
            ("theseus:owner".into(), "theseus".into()),
            ("theseus:stack".into(), p.stack.clone()),
            ("theseus:execution".into(), execution.into()),
        ],
        description: format!("theseus {execution} from {}", p.path.display()),
    };
    cfn.create_change_set(&spec)
        .await
        .map_err(|f| failure(&format!("planning stack {} in {}", p.stack, p.region), f))
}

/// The diff, as the model reads it: each change, what asks at apply, and
/// how to apply exactly this.
fn plan_output(
    p: &Planning,
    exists: bool,
    changes: &[Value],
    digest: &str,
    s: &Shown,
) -> ToolOutput {
    let mut text = format!(
        "Stack {} in {} ({}): {} {} in change set {}\n",
        p.stack,
        p.region,
        if exists { "an update" } else { "a new stack" },
        changes.len(),
        if changes.len() == 1 {
            "change"
        } else {
            "changes"
        },
        s.change_set,
    );
    for line in changes.iter().filter_map(change_line) {
        text.push_str(&format!("  {line}\n"));
    }
    for (label, lines) in [
        ("At the floor (the operator approves at apply)", &s.floor),
        ("Waits for approval (state lost)", &s.destructive),
        ("The scan could not see", &p.notes),
    ] {
        if !lines.is_empty() {
            text.push_str(&format!("{label}:\n"));
            for l in lines {
                text.push_str(&format!("  {l}\n"));
            }
        }
    }
    text.push_str(&format!(
        "Apply exactly this with aws_stack_apply {{\"stack\": \"{}\", \"digest\": \"{digest}\"}}.\n",
        p.stack
    ));
    ToolOutput {
        text,
        meta: json!({
            "stack": p.stack, "region": p.region, "change_set": s.change_set, "digest": digest,
            "changes": changes.len(), "floor": s.floor.len(), "destructive": s.destructive.len(),
        }),
    }
}

/// A template too big for a change set's body goes to the Theseus bucket
/// first (`stacks/`): its URL.
async fn upload(
    account: &Arc<Account>,
    region: &str,
    stack: &str,
    body: &str,
    binding: Option<&AwsBinding>,
) -> Result<String, Failure> {
    let bucket = format!("theseus-{}-{region}", account.id);
    let key = format!(
        "stacks/{stack}/{}.yaml",
        hex::encode(&Sha256::digest(body.as_bytes())[..12])
    );
    let input = json!({"Bucket": bucket, "Key": key, "Body": body});
    let r = Request {
        service: "s3",
        operation: "PutObject",
        input: &input,
        region,
        pages: 1,
        class: "write",
        signer: Signer::As(Kind::Work),
    };
    account.request(binding, &r).await?;
    Ok(format!("https://{bucket}.s3.{region}.amazonaws.com/{key}"))
}

// ------------------------------------------------------------------ aws.stack.apply

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArgs {
    stack: String,
    digest: String,
}

pub struct ApplyTool(Arc<Aws>);

impl ApplyTool {
    fn shown(&self, input: &Value) -> Result<(Shown, String), String> {
        let a: ApplyArgs = parse(input)?;
        let s = self.0.shows.get(&a.digest).ok_or_else(|| {
            format!(
                "no plan in this daemon's life showed a change set with digest {}: run \
                 aws_stack_plan, and apply the digest it returns",
                a.digest
            )
        })?;
        if s.stack != a.stack {
            return Err(format!(
                "digest {} is stack {}'s change set, not {}'s",
                a.digest, s.stack, a.stack
            ));
        }
        Ok((s, a.digest))
    }
}

impl Tool for ApplyTool {
    fn name(&self) -> &'static str {
        "aws.stack.apply"
    }
    fn description(&self) -> &'static str {
        "Apply the change set an aws_stack_plan showed, by its digest: exactly that change set, \
         and nothing that changed since. A change that touches a guardrail, or replaces or \
         removes a resource that holds state, waits for the operator first. It waits for the \
         stack to settle, and says how it ended; CloudFormation rolls back a failure."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "stack": {"type": "string", "description": "The stack the plan named."},
                "digest": {"type": "string", "description": "The change set's digest, as aws_stack_plan returned it."}
            },
            "required": ["stack", "digest"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn deadline(&self) -> Option<Duration> {
        Some(SETTLE + Duration::from_secs(60))
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (s, digest) = self.shown(input)?;
        Ok(Plan {
            summary: format!(
                "apply change set {digest} to stack {} in {}",
                s.stack, s.region
            ),
            class: Some(ToolClass::Write),
            aws: Some(AwsPlan {
                account: s.account.clone(),
                region: s.region.clone(),
                service: "cloudformation".into(),
                operation: "ExecuteChangeSet".into(),
                resources: vec![s.stack.clone()],
                guardrail: (!s.floor.is_empty()).then(|| s.floor.join("; ")),
                destructive: !s.destructive.is_empty(),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let shown = self.shown(input);
        let aws = self.0.clone();
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let (s, digest) = shown.map_err(ToolFailure::new)?;
            let account = aws
                .account(Some(&s.account))
                .map_err(ToolFailure::new)?
                .clone();
            let cfn = Cfn {
                account: &account,
                region: &s.region,
                binding: binding.as_deref(),
                signer: Signer::As(Kind::Work),
            };
            let what = format!("applying change set {digest} to stack {}", s.stack);
            let now = match cfn
                .ready(&s.stack, &s.change_set)
                .await
                .map_err(|f| failure(&what, f))?
            {
                Ready::Changes { id, changes } => self::digest(&s.stack, &id, &changes),
                _ => String::new(),
            };
            if now != digest {
                return Err(ToolFailure::new(format!(
                    "{what}: the change set is not the one the plan showed any more (its digest \
                     is now {}); plan again",
                    if now.is_empty() { "gone" } else { &now }
                )));
            }
            cfn.execute(&s.stack, &s.change_set)
                .await
                .map_err(|f| failure(&what, f))?;
            let settled = cfn
                .settle(&s.stack, SETTLE)
                .await
                .map_err(|f| failure(&what, f))?;
            Ok((settled_output(&s.stack, &s.region, &settled, None), None))
        })
    }
}

fn settled_output(stack: &str, region: &str, s: &Settled, kept: Option<&[String]>) -> ToolOutput {
    let mut text = format!("Stack {stack} in {region}: {}", s.status);
    if let Some(r) = &s.reason {
        text.push_str(&format!(" ({r})"));
    }
    text.push('\n');
    if s.going {
        text.push_str(&format!(
            "It is still going after {} minutes: aws_stack_status shows how it ends.\n",
            SETTLE.as_secs() / 60
        ));
    }
    for f in &s.failures {
        text.push_str(&format!("  failed: {f}\n"));
    }
    if let Some(k) = kept.filter(|k| !k.is_empty()) {
        text.push_str(&format!(
            "Kept by DeletionPolicy Retain (they outlive the stack): {}\n",
            k.join(", ")
        ));
    }
    ToolOutput {
        text,
        meta: json!({"stack": stack, "region": region, "status": s.status, "going": s.going}),
    }
}

// ------------------------------------------------------------------ aws.stack.status

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusArgs {
    #[serde(default)]
    stack: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

pub struct StatusTool(Arc<Aws>);

impl Tool for StatusTool {
    fn name(&self) -> &'static str {
        "aws.stack.status"
    }
    fn description(&self) -> &'static str {
        "The CloudFormation stacks on Theseus's AWS account: with no stack, each stack's name, \
         status, and when it last changed; with one, its status, outputs, drift, and recent \
         events."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "stack": {"type": "string", "description": "One stack; none lists them all."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
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
        let a: StatusArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?;
        let region = account.region(a.region.as_deref())?;
        Ok(Plan {
            summary: format!(
                "read {} in {region}",
                a.stack
                    .as_deref()
                    .map_or("the stacks".into(), |s| format!("stack {s}"))
            ),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region,
                service: "cloudformation".into(),
                operation: "DescribeStacks".into(),
                resources: a.stack.into_iter().collect(),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let args = parse::<StatusArgs>(input).and_then(|a| {
            let account = self.0.account(a.account.as_deref())?.clone();
            let region = account.region(a.region.as_deref())?;
            Ok((account, region, a.stack))
        });
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let (account, region, stack) = args.map_err(ToolFailure::new)?;
            let cfn = Cfn {
                account: &account,
                region: &region,
                binding: binding.as_deref(),
                signer: Signer::As(Kind::Work),
            };
            let text = match &stack {
                None => all_stacks(&cfn).await,
                Some(s) => one_stack(&cfn, s).await,
            }
            .map_err(|f| failure(&format!("reading the stacks in {region}"), f))?;
            Ok((
                ToolOutput {
                    text,
                    meta: json!({"account": account.id, "region": region, "stack": stack}),
                },
                None,
            ))
        })
    }
}

async fn all_stacks(cfn: &Cfn<'_>) -> Result<String, Failure> {
    let out = cfn.call("DescribeStacks", &json!({}), "read").await?;
    let stacks = out.body["Stacks"].as_array().cloned().unwrap_or_default();
    let mut text = format!("{} stacks in {}:\n", stacks.len(), cfn.region);
    for s in &stacks {
        text.push_str(&format!(
            "  {} · {} · {}\n",
            s["StackName"].as_str().unwrap_or("?"),
            s["StackStatus"].as_str().unwrap_or("?"),
            s["LastUpdatedTime"]
                .as_str()
                .or(s["CreationTime"].as_str())
                .unwrap_or("?")
        ));
    }
    Ok(text)
}

async fn one_stack(cfn: &Cfn<'_>, name: &str) -> Result<String, Failure> {
    let Some(s) = cfn.stack(name).await? else {
        return Ok(format!("No stack {name} in {}.\n", cfn.region));
    };
    let mut text = format!(
        "Stack {name} in {}: {}{}\n  termination protection: {}; drift: {}\n",
        cfn.region,
        s["StackStatus"].as_str().unwrap_or("?"),
        s["StackStatusReason"]
            .as_str()
            .map(|r| format!(" ({r})"))
            .unwrap_or_default(),
        if s["EnableTerminationProtection"].as_bool() == Some(true) {
            "on"
        } else {
            "off"
        },
        s["DriftInformation"]["StackDriftStatus"]
            .as_str()
            .unwrap_or("never checked"),
    );
    for o in s["Outputs"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  output {} = {}\n",
            o["OutputKey"].as_str().unwrap_or("?"),
            o["OutputValue"].as_str().unwrap_or("?")
        ));
    }
    let events = cfn
        .call("DescribeStackEvents", &json!({"StackName": name}), "read")
        .await?;
    text.push_str("  recent events:\n");
    for e in events.body["StackEvents"]
        .as_array()
        .into_iter()
        .flatten()
        .take(10)
    {
        text.push_str(&format!(
            "    {} {} {}{}\n",
            e["Timestamp"].as_str().unwrap_or("?"),
            e["LogicalResourceId"].as_str().unwrap_or("?"),
            e["ResourceStatus"].as_str().unwrap_or("?"),
            e["ResourceStatusReason"]
                .as_str()
                .map(|r| format!(": {r}"))
                .unwrap_or_default()
        ));
    }
    Ok(text)
}

// ------------------------------------------------------------------ aws.stack.delete

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteArgs {
    stack: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

pub struct DeleteTool(Arc<Aws>);

impl DeleteTool {
    fn args(&self, input: &Value) -> Result<(Arc<Account>, String, String), String> {
        let a: DeleteArgs = parse(input)?;
        stack_name_ok(&a.stack)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        needs_bootstrap(&account)?;
        let region = account.region(a.region.as_deref())?;
        Ok((account, region, a.stack))
    }
}

impl Tool for DeleteTool {
    fn name(&self) -> &'static str {
        "aws.stack.delete"
    }
    fn description(&self) -> &'static str {
        "Delete a CloudFormation stack on Theseus's AWS account, through the deployer role. It \
         waits for the operator's approval first, then for the stack to go, and says what the \
         stack keeps (resources with DeletionPolicy Retain outlive it)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "stack": {"type": "string", "description": "The stack to delete."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["stack"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn deadline(&self) -> Option<Duration> {
        Some(SETTLE + Duration::from_secs(60))
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (account, region, stack) = self.args(input)?;
        Ok(Plan {
            summary: format!("delete stack {stack} in {region}"),
            class: Some(ToolClass::Write),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region,
                service: "cloudformation".into(),
                operation: "DeleteStack".into(),
                resources: vec![stack],
                destructive: true,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let args = self.args(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let (account, region, stack) = args.map_err(ToolFailure::new)?;
            let cfn = Cfn {
                account: &account,
                region: &region,
                binding: binding.as_deref(),
                signer: Signer::As(Kind::Work),
            };
            let what = format!("deleting stack {stack} in {region}");
            let body = cfn.template(&stack).await.map_err(|f| failure(&what, f))?;
            let kept = retained(&body);
            let input = json!({"StackName": stack, "RoleARN": role_arn(&account.id, DEPLOYER)});
            cfn.call("DeleteStack", &input, "write")
                .await
                .map_err(|f| failure(&what, f))?;
            let settled = cfn
                .settle(&stack, SETTLE)
                .await
                .map_err(|f| failure(&what, f))?;
            Ok((settled_output(&stack, &region, &settled, Some(&kept)), None))
        })
    }
}

/// The logical ids a template keeps when its stack goes (`DeletionPolicy:
/// Retain`).
pub fn retained(body: &str) -> Vec<String> {
    let Ok(t) = theseus_aws_guard::parse_template(body) else {
        return Vec::new();
    };
    match t.get("Resources") {
        Some(theseus_aws_guard::Node::Map(m)) => m
            .iter()
            .filter(|(_, r)| {
                r.get("DeletionPolicy")
                    .and_then(theseus_aws_guard::Node::as_str)
                    == Some("Retain")
            })
            .map(|(id, _)| id.clone())
            .collect(),
        _ => Vec::new(),
    }
}
