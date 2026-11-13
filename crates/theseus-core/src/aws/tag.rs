//! `aws.tag.list`, `aws.tag.set` and `aws.tag.clear` (theseus-mgw.15, row
//! B6): the first **typed per-service tool-let family**, and the pattern the
//! next service follows.
//!
//! The service is the Resource Groups Tagging API
//! (`resourcegroupstaggingapi`), whose three operations read and change the
//! tags of any resource of the account by ARN. Tags were chosen as the
//! worked example for three reasons the recipe below leans on: they are the
//! one write the IaC rule carves out (the catalog's `TAGGING` table, so
//! `aws.call` does not send them to the stack tools), the guard list holds
//! no guardrail and no destructive row for them, and a wrong tag is undone
//! by the next call, so the family proves the shape without widening what
//! Theseus can break.
//!
//! **Why a tool-let at all, when `aws.call` already reaches these three.**
//! A tool-let earns its place by three things `aws.call` cannot do, and the
//! tests (`tests_tag`) prove each:
//!
//! 1. **A bad input is invalid input in `plan`** — before the gate, with no
//!    network — **and the message names the field.** `aws.call` takes a
//!    free-form `input` object and can only say what the catalog's shape
//!    says; a tool-let knows that `arns` is a list of 1 to 20 ARNs and that
//!    a tag key of 129 characters is too long, and says which one is wrong.
//! 2. **It refuses an operator limit itself, rather than leaning on the
//!    gate** (the AWS design §3.9, `docs/design/aws-toolset.md:551`, whose
//!    exemplar is `aws.call`'s own IaC refusal). The limit here is the
//!    account: a tool-let that could tag another account's resource is
//!    refused by [`Tagging::check_arns`], naming the ARN and the bound
//!    account, with nothing sent and no question asked of the operator.
//! 3. **Its `tool_class` is its own**, so a write cannot be mistaken for a
//!    read: `aws.tag.list` is a `read`, `aws.tag.set` and `aws.tag.clear`
//!    are `write`s (`super::tool_class`), and the gate reads `[policy.aws]
//!    write` for them whatever `aws.call`'s own line says.
//!
//! **The recipe for the next service** (what a builder repeats; nothing
//! here is generated, and the schema is not hand-copied either):
//!
//! 1. Pick the service and the two to four operations that carry its common
//!    work. Read `aws.describe {service}` for their classes and flags, and
//!    **stop** if any is cost-bearing or touches AWS Budgets: a
//!    spend-bearing tool-let lands behind a config default that is off, and
//!    a budgets-bearing one halts and asks the owner (his standing word).
//! 2. One module under `aws/`, one `Tool` impl per tool-let, and an
//!    `all(aws)` that `super::Aws::tools` extends — as `s3.rs`, `logs.rs`,
//!    `stack.rs`, `trail.rs` and `inventory.rs` do.
//! 3. The input schema is **hand-written and narrow** (named fields, no
//!    free-form object), and the *shape* check is the catalog's:
//!    [`check`] here, which is `s3::check`'s twin, asks
//!    `Account::client().check()` so the operation's own members are
//!    validated by `theseus-aws-catalog` and never retyped. A member the
//!    catalog does not know is a build-time bug, not a runtime refusal.
//! 4. `plan` does the whole check — account, region, every field, the
//!    operator limits, then the shape — and returns `class`, the `AwsPlan`
//!    and the guard list's verdict (`tools::guard`), so the gate's floor,
//!    approve list and `[policy.aws]` line read exactly what `aws.call`
//!    gives them.
//! 5. `run_async` is a thin recipe over `Account::request`, so signing,
//!    retries, the `aws.called` row and the span stay the one caller's.
//! 6. Add each name to `super::NAMES` (the config's census) and to the
//!    template's `[policy.tools]`, give the family a row in
//!    `tools_defer::family` under the per-family token cap, and set
//!    `super::tool_class` if the class is not `read`.
//! 7. Prove it offline: a refusal test per field, the operator limit's own
//!    refusal, the `tool_class`, and one call against the fake endpoint.
//!    **Proving it against the real bound account is not a cloud card.**

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use theseus_aws::{Attribution, Call};
use theseus_tools::{
    parse, AsyncRun, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::session::Kind;
use super::tools::{checked_error, failure, guard, meta};
use super::{Account, Aws, Request, Signer};

/// The service, as the catalog names it.
const SERVICE: &str = "resourcegroupstaggingapi";

/// ARNs one `TagResources` or `UntagResources` takes: AWS's own limit.
pub const ARNS_MAX: usize = 20;
/// Tag keys one call sets or clears: AWS's own limit.
pub const TAGS_MAX: usize = 50;
/// A tag key's and a tag value's longest, in characters: AWS's own.
pub const KEY_CHARS: usize = 128;
pub const VALUE_CHARS: usize = 256;
/// Resources one `aws.tag.list` page holds, by default and at most.
pub const PAGE_DEFAULT: u32 = 50;
pub const PAGE_MAX: u32 = 100;
/// Pages one `aws.tag.list` follows, at most.
pub const PAGES_MAX: u32 = 10;
/// The prefix AWS reserves for its own tags.
const RESERVED: &str = "aws:";

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(List(aws.clone())),
        Arc::new(Set(aws.clone())),
        Arc::new(Clear(aws.clone())),
    ]
}

/// The client's own check of a call: the input against the operation's
/// shape, with no network (the recipe's step 3).
fn check(account: &Account, operation: &str, input: &Value, region: &str) -> Result<(), String> {
    let attribution = Attribution::default();
    let call = Call {
        service: SERVICE,
        operation,
        input,
        region: Some(region),
        pages: 1,
        attribution: &attribution,
    };
    account
        .client()
        .check(&call)
        .map(|_| ())
        .map_err(checked_error)
}

/// A call as `plan` checked it.
struct Tagging {
    account: Arc<Account>,
    region: String,
    operation: &'static str,
    input: Value,
    pages: u32,
    /// The ARNs the call names, for the plan's resources.
    arns: Vec<String>,
}

impl Tagging {
    /// Every ARN, checked: the shape, and **the operator limit** — it must
    /// name the bound account, so no tool-let of this family reaches
    /// another account's resources. The refusal is the tool-let's own
    /// (§3.9): nothing is sent, and the gate is never asked.
    fn check_arns(arns: &[String], account: &str) -> Result<(), String> {
        if arns.is_empty() {
            return Err(format!(
                "\"arns\" must name 1 to {ARNS_MAX} resources by ARN (aws_tag_list shows them)"
            ));
        }
        if arns.len() > ARNS_MAX {
            return Err(format!(
                "\"arns\" names {} resources, over the {ARNS_MAX} one call takes: send them in \
                 batches",
                arns.len()
            ));
        }
        for arn in arns {
            let parts: Vec<&str> = arn.split(':').collect();
            if parts.len() < 6 || parts[0] != "arn" || parts[5].is_empty() {
                return Err(format!(
                    "\"arns\" holds {arn:?}, which is no ARN: an ARN is \
                     arn:<partition>:<service>:<region>:<account>:<resource>"
                ));
            }
            if !parts[4].is_empty() && parts[4] != account {
                return Err(format!(
                    "\"arns\" holds {arn:?}, of account {}, and Theseus tags only the account it \
                     is bound to ({account}): that is an operator limit, and nothing was sent",
                    parts[4]
                ));
            }
        }
        Ok(())
    }

    /// Every tag key, checked: AWS's own lengths, and `aws:`, which AWS
    /// reserves for itself.
    fn check_keys(keys: &[String], field: &str) -> Result<(), String> {
        if keys.is_empty() {
            return Err(format!("\"{field}\" must name 1 to {TAGS_MAX} tag keys"));
        }
        if keys.len() > TAGS_MAX {
            return Err(format!(
                "\"{field}\" names {} tag keys, over the {TAGS_MAX} one call takes",
                keys.len()
            ));
        }
        let mut seen = BTreeSet::new();
        for k in keys {
            if k.is_empty() || k.chars().count() > KEY_CHARS {
                return Err(format!(
                    "\"{field}\" holds a tag key of {} characters; a key is 1 to {KEY_CHARS}",
                    k.chars().count()
                ));
            }
            if k.starts_with(RESERVED) {
                return Err(format!(
                    "\"{field}\" holds {k:?}: AWS reserves the {RESERVED} prefix for its own tags"
                ));
            }
            if !seen.insert(k) {
                return Err(format!("\"{field}\" holds the tag key {k:?} twice"));
            }
        }
        Ok(())
    }

    fn what(&self) -> String {
        format!("{SERVICE}:{} in {}", self.operation, self.region)
    }

    /// What the call is about, for its summary: the ARNs it names, or the
    /// whole account.
    fn scope(&self) -> String {
        match self.arns.len() {
            0 => "every tagged resource".to_string(),
            1 => self.arns[0].clone(),
            n => format!("{} resources ({}, ...)", n, self.arns[0]),
        }
    }

    fn plan(&self, class: ToolClass, summary: String) -> Result<Plan, String> {
        // Keyed exactly as `aws.call` keys the same operation
        // (`<catalog service>:<Operation>`), so the gate's floor, approve
        // list and `[policy.aws]` line answer a tool-let and an `aws.call`
        // of one operation alike.
        let op = format!("{SERVICE}:{}", self.operation);
        let (guardrail, _, destructive) = guard(&self.account.id, &self.region, &op, &self.input)?;
        Ok(Plan {
            summary,
            class: Some(class),
            aws: Some(AwsPlan {
                account: self.account.id.clone(),
                region: self.region.clone(),
                service: SERVICE.into(),
                operation: self.operation.into(),
                resources: self.arns.iter().take(10).cloned().collect(),
                guardrail,
                destructive,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    /// The one request every tool-let of the family sends.
    fn request(&self) -> Request<'_> {
        Request {
            service: SERVICE,
            operation: self.operation,
            input: &self.input,
            region: &self.region,
            pages: self.pages,
            class: if self.operation == "GetResources" {
                "read"
            } else {
                "write"
            },
            signer: Signer::As(Kind::Work),
        }
    }
}

// ------------------------------------------------------------------ aws.tag.list

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    #[serde(default)]
    arns: Option<Vec<String>>,
    #[serde(default)]
    types: Option<Vec<String>>,
    #[serde(default)]
    tags: Option<Map<String, Value>>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    pages: Option<u32>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

pub struct List(Arc<Aws>);

impl List {
    /// Every filter, checked by name, then the operation's shape.
    fn listing(&self, input: &Value) -> Result<Tagging, String> {
        let a: ListArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let limit = a.limit.unwrap_or(PAGE_DEFAULT);
        if !(1..=PAGE_MAX).contains(&limit) {
            return Err(format!("\"limit\" must be 1 to {PAGE_MAX}"));
        }
        let pages = a.pages.unwrap_or(1);
        if !(1..=PAGES_MAX).contains(&pages) {
            return Err(format!("\"pages\" must be 1 to {PAGES_MAX}"));
        }
        let mut o = Map::new();
        o.insert("ResourcesPerPage".into(), json!(limit));
        let arns = a.arns.unwrap_or_default();
        if !arns.is_empty() {
            Tagging::check_arns(&arns, &account.id)?;
            o.insert("ResourceARNList".into(), json!(arns));
        }
        if let Some(types) = a.types {
            if types.is_empty() || types.len() > 100 {
                return Err("\"types\" must name 1 to 100 resource types".into());
            }
            o.insert("ResourceTypeFilters".into(), json!(types));
        }
        if let Some(tags) = a.tags {
            o.insert("TagFilters".into(), json!(tag_filters(&tags)?));
        }
        let t = Tagging {
            account,
            region,
            operation: "GetResources",
            input: Value::Object(o),
            pages,
            arns,
        };
        check(&t.account, t.operation, &t.input, &t.region)?;
        Ok(t)
    }
}

/// `{"Env": "prod", "Owner": ""}` as the operation's `TagFilters`: a value
/// of `""` asks for the key whatever its value.
fn tag_filters(tags: &Map<String, Value>) -> Result<Vec<Value>, String> {
    let keys: Vec<String> = tags.keys().cloned().collect();
    Tagging::check_keys(&keys, "tags")?;
    tags.iter()
        .map(|(k, v)| match v {
            Value::String(s) if s.is_empty() => Ok(json!({"Key": k})),
            Value::String(s) => Ok(json!({"Key": k, "Values": [s]})),
            _ => Err(format!(
                "\"tags\" gives {k:?} a {}; a filter's value is a string, and \"\" matches any",
                kind_of(v)
            )),
        })
        .collect()
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "list",
        Value::Object(_) => "object",
    }
}

/// What a listing says: one line per resource, its tags after it.
fn list_text(t: &Tagging, out: &theseus_aws::Output) -> String {
    let rows = out.body["ResourceTagMappingList"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let mut text = format!(
        "{}, account {}: {} {} · {} {}\n",
        t.what(),
        t.account.id,
        rows.len(),
        if rows.len() == 1 {
            "resource"
        } else {
            "resources"
        },
        out.pages,
        if out.pages == 1 { "page" } else { "pages" },
    );
    for r in rows {
        let tags: Vec<String> = r["Tags"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|p| {
                format!(
                    "{}={}",
                    p["Key"].as_str().unwrap_or_default(),
                    p["Value"].as_str().unwrap_or_default()
                )
            })
            .collect();
        text.push_str(&format!(
            "{}  {}\n",
            r["ResourceARN"].as_str().unwrap_or("(no ARN)"),
            if tags.is_empty() {
                "(no tags)".to_string()
            } else {
                tags.join(", ")
            }
        ));
    }
    if out.next.is_some() {
        text.push_str(&format!(
            "More remain: ask for more \"pages\" (at most {PAGES_MAX}).\n"
        ));
    }
    text
}

impl Tool for List {
    fn name(&self) -> &'static str {
        "aws.tag.list"
    }
    fn description(&self) -> &'static str {
        "List the resources of Theseus's own AWS account with their tags, through the Resource \
         Groups Tagging API: with no filter, every tagged resource of the region; `arns` for \
         named resources, `types` for resource types (s3, ec2:instance), `tags` for a tag \
         filter ({\"Env\": \"prod\"}, or \"\" as the value for the key whatever its value). A \
         read: its posture is [policy.aws] read's."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "arns": {"type": "array", "items": {"type": "string"}, "description": "Resources by ARN, at most 20; they must be of this account."},
                "types": {"type": "array", "items": {"type": "string"}, "description": "Resource types: s3, ec2:instance, lambda:function."},
                "tags": {"type": "object", "additionalProperties": {"type": "string"}, "description": "A tag filter; \"\" as a value matches the key whatever its value."},
                "limit": {"type": "integer", "minimum": 1, "maximum": PAGE_MAX, "description": "Resources in one page (default 50)."},
                "pages": {"type": "integer", "minimum": 1, "maximum": PAGES_MAX, "description": "Pages to read (default 1)."},
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
        let t = self.listing(input)?;
        let summary = format!("read the tags of {} in {}", t.scope(), t.region);
        t.plan(ToolClass::Read, summary)
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let listing = self.listing(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let t = listing.map_err(ToolFailure::new)?;
            let out = t
                .account
                .request(binding.as_deref(), &t.request())
                .await
                .map_err(|f| failure(&t.what(), f))?;
            let m = meta(
                &t.account.id,
                &t.region,
                &format!("{SERVICE}:{}", t.operation),
                &out,
            );
            Ok((
                ToolOutput {
                    text: list_text(&t, &out),
                    meta: m,
                },
                None,
            ))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower listing returns them: \"arns\", \"types\" or \"tags\"".into()
    }
}

// ------------------------------------------------------------- aws.tag.set / clear

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetArgs {
    arns: Vec<String>,
    tags: Map<String, Value>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClearArgs {
    arns: Vec<String>,
    keys: Vec<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// `{"Env": "prod"}` as the operation's `Tags`: every value a string, each
/// within AWS's own lengths.
fn tags_of(tags: &Map<String, Value>) -> Result<Value, String> {
    let keys: Vec<String> = tags.keys().cloned().collect();
    Tagging::check_keys(&keys, "tags")?;
    let mut o = Map::new();
    for (k, v) in tags {
        let Some(s) = v.as_str() else {
            return Err(format!(
                "\"tags\" gives {k:?} a {}; a tag's value is a string",
                kind_of(v)
            ));
        };
        if s.chars().count() > VALUE_CHARS {
            return Err(format!(
                "\"tags\" gives {k:?} a value of {} characters; a value is at most {VALUE_CHARS}",
                s.chars().count()
            ));
        }
        o.insert(k.clone(), json!(s));
    }
    Ok(Value::Object(o))
}

pub struct Set(Arc<Aws>);

impl Set {
    fn setting(&self, input: &Value) -> Result<(Tagging, Vec<String>), String> {
        let a: SetArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        Tagging::check_arns(&a.arns, &account.id)?;
        let tags = tags_of(&a.tags)?;
        let keys: Vec<String> = a.tags.keys().cloned().collect();
        let t = Tagging {
            account,
            region,
            operation: "TagResources",
            input: json!({"ResourceARNList": a.arns, "Tags": tags}),
            pages: 1,
            arns: a.arns,
        };
        check(&t.account, t.operation, &t.input, &t.region)?;
        Ok((t, keys))
    }
}

pub struct Clear(Arc<Aws>);

impl Clear {
    fn clearing(&self, input: &Value) -> Result<(Tagging, Vec<String>), String> {
        let a: ClearArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        Tagging::check_arns(&a.arns, &account.id)?;
        Tagging::check_keys(&a.keys, "keys")?;
        let t = Tagging {
            account,
            region,
            operation: "UntagResources",
            input: json!({"ResourceARNList": a.arns, "TagKeys": a.keys}),
            pages: 1,
            arns: a.arns,
        };
        check(&t.account, t.operation, &t.input, &t.region)?;
        Ok((t, a.keys))
    }
}

/// What a write says: what it changed, and every resource AWS refused, with
/// its reason, so a partial failure is never read as a success.
fn wrote_text(t: &Tagging, keys: &[String], verb: &str, out: &theseus_aws::Output) -> String {
    let failed = out.body["FailedResourcesMap"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let done = t.arns.len() - failed.len();
    let mut text = format!(
        "{verb} {} on {done} of {} {} in {} (request {}).\n",
        keys.join(", "),
        t.arns.len(),
        if t.arns.len() == 1 {
            "resource"
        } else {
            "resources"
        },
        t.region,
        out.request_id.as_deref().unwrap_or("(none)"),
    );
    for (arn, why) in &failed {
        text.push_str(&format!(
            "refused {arn}: {} {}\n",
            why["ErrorCode"].as_str().unwrap_or("(no code)"),
            why["ErrorMessage"].as_str().unwrap_or_default()
        ));
    }
    text
}

/// One write of the family: its plan and its run are the same shape, so the
/// two tool-lets differ only in their names, schemas and words.
fn write_async(
    made: Result<(Tagging, Vec<String>), String>,
    verb: &'static str,
    ctx_aws: Option<Arc<theseus_tools::AwsBinding>>,
) -> AsyncRun {
    Box::pin(async move {
        let (t, keys) = made.map_err(ToolFailure::new)?;
        let out = t
            .account
            .request(ctx_aws.as_deref(), &t.request())
            .await
            .map_err(|f| failure(&t.what(), f))?;
        let mut m = meta(
            &t.account.id,
            &t.region,
            &format!("{SERVICE}:{}", t.operation),
            &out,
        );
        m["resources"] = json!(t.arns.len());
        m["tag_keys"] = json!(keys);
        Ok((
            ToolOutput {
                text: wrote_text(&t, &keys, verb, &out),
                meta: m,
            },
            None,
        ))
    })
}

impl Tool for Set {
    fn name(&self) -> &'static str {
        "aws.tag.set"
    }
    fn description(&self) -> &'static str {
        "Set tags on resources of Theseus's own AWS account by ARN, through the Resource Groups \
         Tagging API: at most 20 resources and 50 tags a call, each key 1 to 128 characters and \
         each value at most 256, and never a key beginning aws:, which AWS reserves. A tag that \
         exists is overwritten; the others are left. A write: its posture is [policy.aws] \
         write's."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "arns": {"type": "array", "items": {"type": "string"}, "description": "The resources by ARN, 1 to 20, of this account."},
                "tags": {"type": "object", "additionalProperties": {"type": "string"}, "description": "The tags to set: {\"Env\": \"prod\"}."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["arns", "tags"],
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
        // Setting the same tags again sets the same tags: safe, and the
        // one write of this family that is.
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (t, keys) = self.setting(input)?;
        let summary = format!("set {} on {} in {}", keys.join(", "), t.scope(), t.region);
        t.plan(ToolClass::Write, summary)
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        write_async(self.setting(input), "set", ctx.aws.clone())
    }
}

impl Tool for Clear {
    fn name(&self) -> &'static str {
        "aws.tag.clear"
    }
    fn description(&self) -> &'static str {
        "Remove tags from resources of Theseus's own AWS account by ARN, through the Resource \
         Groups Tagging API: at most 20 resources and 50 tag keys a call. A key a resource does \
         not carry is no error. A write: its posture is [policy.aws] write's."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "arns": {"type": "array", "items": {"type": "string"}, "description": "The resources by ARN, 1 to 20, of this account."},
                "keys": {"type": "array", "items": {"type": "string"}, "description": "The tag keys to remove, 1 to 50."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["arns", "keys"],
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
        // Removing a tag that is already gone is no error: a repeat leaves
        // the same resources without the same keys.
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (t, keys) = self.clearing(input)?;
        let summary = format!(
            "remove {} from {} in {}",
            keys.join(", "),
            t.scope(),
            t.region
        );
        t.plan(ToolClass::Write, summary)
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        write_async(self.clearing(input), "removed", ctx.aws.clone())
    }
}
