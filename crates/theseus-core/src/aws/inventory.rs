//! The inventory, and the reaper in report mode (AWS design §3.7; C3 =
//! 14c): `aws.inventory`.
//!
//! - **The inventory** is what Theseus made: the Resource Groups Tagging
//!   API's `GetResources`, filtered on `theseus:owner = theseus`, in each of
//!   the account's regions, with each stack's age from CloudFormation and,
//!   when asked, each stack's spend this month from Cost Explorer (by its
//!   `theseus:stack` tag, a call that costs $0.01).
//! - **Never an untagged resource.** The account also holds another
//!   project's resources, which carry no Theseus tag. The inventory asks
//!   AWS only for resources tagged `theseus:owner = theseus`, and keeps only
//!   those whose tags it reads back say so ([`owned`]): what AWS returns
//!   beyond that is left out, named nowhere.
//! - **The reaper reports, and deletes nothing.** For each resource it says
//!   what it would do once it acts ([`verdict`]): a stack whose
//!   `theseus:ttl` has passed would be deleted, whole, never its resources
//!   one by one; a loose run (an ECS task, a Batch job) would be stopped; a
//!   loose resource would be deleted, or, when it holds state (a bucket, a
//!   table, a database), would wait for the operator's approval. A resource
//!   with no `theseus:ttl`, or one not yet passed, is kept; one whose
//!   `theseus:ttl` cannot be read is kept, and said so. The design's 14 days
//!   of reports come before any reaping; acting is a later step.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{
    parse, AsyncRun, AwsBinding, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

use super::session::Kind;
use super::tools::{cents, count, failure, to_cents};
use super::{Account, Aws, Request, Signer};

/// The tag that marks what Theseus made, and its one value.
pub const OWNER_TAG: &str = "theseus:owner";
pub const OWNER: &str = "theseus";
/// The tag that says when a resource is meant to die.
pub const TTL_TAG: &str = "theseus:ttl";
/// The pages of `GetResources` one region reads, 100 resources a page.
const PAGES: u32 = 10;

/// Kinds that hold state: deleting one waits for the operator (§3.9).
const STATEFUL: &[&str] = &[
    "s3:bucket",
    "dynamodb:table",
    "rds:db",
    "rds:cluster",
    "elasticfilesystem:file-system",
    "logs:log-group",
    "sqs:queue",
    "kinesis:stream",
    "backup:backup-vault",
];

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Inventory(aws.clone()))]
}

/// One resource Theseus made.
#[derive(Debug, Clone, PartialEq)]
pub struct Owned {
    pub arn: String,
    pub region: String,
    /// `service:type`: `cloudformation:stack`, `s3:bucket`, `ecs:task`.
    pub kind: String,
    pub name: String,
    pub tags: BTreeMap<String, String>,
    /// When it was made, in Unix ms, where AWS says (a stack's creation).
    pub made_ms: Option<u64>,
    /// Its stack's spend this month, in cents, when asked for.
    pub cents: Option<u64>,
}

/// Whether tags mark a resource as Theseus's own.
pub fn owned(tags: &BTreeMap<String, String>) -> bool {
    tags.get(OWNER_TAG).map(String::as_str) == Some(OWNER)
}

/// An ARN's `service:type` and name: `arn:aws:cloudformation:r:a:stack/x/id`
/// is `cloudformation:stack` and `x`; `arn:aws:s3:::b` is `s3:bucket` and `b`.
pub fn kind_of(arn: &str) -> (String, String) {
    let parts: Vec<&str> = arn.splitn(6, ':').collect();
    let (service, resource) = match parts.as_slice() {
        [_, _, s, _, _, r] => (*s, *r),
        _ => return ("?".into(), arn.into()),
    };
    let (ty, name) = if let Some((t, rest)) = resource.split_once('/') {
        (t, rest.split('/').next().unwrap_or(rest))
    } else if let Some((t, rest)) = resource.split_once(':') {
        (t, rest)
    } else {
        let t = match service {
            "s3" => "bucket",
            "sns" => "topic",
            "sqs" => "queue",
            _ => "resource",
        };
        (t, resource)
    };
    (format!("{service}:{ty}"), name.to_string())
}

/// A `theseus:ttl` (ISO 8601, UTC when it names no offset) in Unix ms.
pub fn ttl_of(tags: &BTreeMap<String, String>) -> Option<Result<u64, String>> {
    let t = tags.get(TTL_TAG)?.trim();
    Some(
        crate::wake::parse_at(t)
            .or_else(|_| crate::wake::parse_at(&format!("{t}Z")))
            .or_else(|_| crate::wake::parse_at(&format!("{t}T00:00:00Z")))
            .map_err(|_| format!("{TTL_TAG} {t:?} is not a time")),
    )
}

/// What the reaper would do with one resource, once it acts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Kept: no `theseus:ttl`, or not passed yet (ms until it does).
    Keep(Option<u64>),
    /// Kept: its `theseus:ttl` cannot be read.
    Unreadable(String),
    /// Part of a stack: the stack's own `theseus:ttl` decides, never the
    /// resource's.
    InStack(String),
    /// Its stack would be deleted, whole (ms since its ttl passed).
    DeleteStack(u64),
    /// A loose run would be stopped.
    Stop(u64),
    /// A loose resource would be deleted.
    Delete(u64),
    /// A loose resource that holds state: its deletion would wait for the
    /// operator's approval.
    Ask(u64),
}

/// The reaper's rule for one resource at `now_ms`. Only an owned resource
/// ever reaches it.
pub fn verdict(r: &Owned, now_ms: u64) -> Verdict {
    if r.kind != "cloudformation:stack" {
        let stack = r
            .tags
            .get("aws:cloudformation:stack-name")
            .or_else(|| r.tags.get("theseus:stack"));
        if let Some(s) = stack {
            return Verdict::InStack(s.clone());
        }
    }
    let ttl = match ttl_of(&r.tags) {
        None => return Verdict::Keep(None),
        Some(Err(why)) => return Verdict::Unreadable(why),
        Some(Ok(t)) => t,
    };
    if ttl > now_ms {
        return Verdict::Keep(Some(ttl - now_ms));
    }
    let ago = now_ms - ttl;
    match r.kind.as_str() {
        "cloudformation:stack" => Verdict::DeleteStack(ago),
        "ecs:task" | "batch:job" => Verdict::Stop(ago),
        k if STATEFUL.contains(&k) => Verdict::Ask(ago),
        _ => Verdict::Delete(ago),
    }
}

impl Verdict {
    /// The report's line for a resource, when the reaper would act on it.
    fn line(&self, r: &Owned) -> Option<String> {
        let ago = |ms: &u64| crate::wake::span(*ms);
        let cost = r
            .cents
            .map(|c| format!(", ${} this month", cents(c)))
            .unwrap_or_default();
        Some(match self {
            Verdict::DeleteStack(ms) => format!(
                "would delete stack {} ({}): its {TTL_TAG} passed {} ago{cost}",
                r.name,
                r.region,
                ago(ms)
            ),
            Verdict::Stop(ms) => format!(
                "would stop {} {} ({}): its {TTL_TAG} passed {} ago",
                r.kind,
                r.name,
                r.region,
                ago(ms)
            ),
            Verdict::Delete(ms) => format!(
                "would delete {} {} ({}): its {TTL_TAG} passed {} ago",
                r.kind,
                r.name,
                r.region,
                ago(ms)
            ),
            Verdict::Ask(ms) => format!(
                "would ask the operator before deleting {} {} ({}), which holds state: its \
                 {TTL_TAG} passed {} ago",
                r.kind,
                r.name,
                r.region,
                ago(ms)
            ),
            Verdict::Unreadable(why) => {
                format!("keeps {} {} ({}): {why}", r.kind, r.name, r.region)
            }
            Verdict::Keep(_) | Verdict::InStack(_) => return None,
        })
    }
}

/// Each resource of `GetResources`' output that Theseus owns, and how many
/// it left out because their tags do not say so.
pub fn owned_of(body: &Value, region: &str) -> (Vec<Owned>, usize) {
    let mut out = Vec::new();
    let mut left_out = 0;
    for m in body["ResourceTagMappingList"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let tags: BTreeMap<String, String> = m["Tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["Key"].as_str()?.into(), t["Value"].as_str()?.into())))
            .collect();
        if !owned(&tags) {
            left_out += 1;
            continue;
        }
        let arn = m["ResourceARN"].as_str().unwrap_or_default().to_string();
        let (kind, name) = kind_of(&arn);
        out.push(Owned {
            arn,
            region: region.into(),
            kind,
            name,
            tags,
            made_ms: None,
            cents: None,
        });
    }
    (out, left_out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryArgs {
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    costs: bool,
    #[serde(default)]
    account: Option<String>,
}

pub struct Inventory(Arc<Aws>);

impl Inventory {
    fn args(&self, input: &Value) -> Result<(Arc<Account>, Vec<String>, bool), String> {
        let a: InventoryArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let regions = match a.region.as_deref() {
            Some(r) => vec![account.region(Some(r))?],
            None => account.cfg.allowed_regions(),
        };
        Ok((account, regions, a.costs))
    }
}

/// The inventory of `regions`: each owned resource, and how many AWS
/// returned that were left out.
pub async fn take(
    account: &Arc<Account>,
    regions: &[String],
    costs: bool,
    b: Option<&AwsBinding>,
    signer: Signer<'_>,
) -> Result<(Vec<Owned>, usize), ToolFailure> {
    let mut all = Vec::new();
    let mut left_out = 0;
    for region in regions {
        let input = json!({
            "TagFilters": [{"Key": OWNER_TAG, "Values": [OWNER]}],
            "ResourcesPerPage": 100,
        });
        let req = Request {
            service: "resourcegroupstaggingapi",
            operation: "GetResources",
            input: &input,
            region,
            pages: PAGES,
            class: "read",
            signer,
        };
        let out = account
            .request(b, &req)
            .await
            .map_err(|f| failure(&format!("taking the inventory of {region}"), f))?;
        let (mut owned, out_of) = owned_of(&out.body, region);
        left_out += out_of;
        if owned.iter().any(|r| r.kind == "cloudformation:stack") {
            // Each stack's age, from CloudFormation's own listing.
            let input = json!({});
            let req = Request {
                service: "cloudformation",
                operation: "DescribeStacks",
                input: &input,
                region,
                pages: PAGES,
                class: "read",
                signer,
            };
            if let Ok(s) = account.request(b, &req).await {
                for st in s.body["Stacks"].as_array().into_iter().flatten() {
                    let id = st["StackId"].as_str().unwrap_or_default();
                    let made = st["CreationTime"]
                        .as_str()
                        .and_then(|t| crate::wake::parse_at(t).ok());
                    if let Some(r) = owned.iter_mut().find(|r| r.arn == id) {
                        r.made_ms = made;
                    }
                }
            }
        }
        all.append(&mut owned);
    }
    if costs && !all.is_empty() {
        let by_stack = stack_costs(account, b, signer).await?;
        for r in &mut all {
            let stack = r
                .tags
                .get("theseus:stack")
                .cloned()
                .or_else(|| (r.kind == "cloudformation:stack").then(|| r.name.clone()));
            r.cents = stack.and_then(|s| by_stack.get(&s).copied());
        }
    }
    Ok((all, left_out))
}

/// This month's spend by `theseus:stack`, from Cost Explorer ($0.01).
async fn stack_costs(
    account: &Arc<Account>,
    b: Option<&AwsBinding>,
    signer: Signer<'_>,
) -> Result<BTreeMap<String, u64>, ToolFailure> {
    let (start, end) =
        super::cost::period("month", theseus_protocol::now_unix_ms()).map_err(ToolFailure::new)?;
    let input = json!({
        "TimePeriod": {"Start": start, "End": end},
        "Granularity": "MONTHLY",
        "Metrics": ["UnblendedCost"],
        "GroupBy": [{"Type": "TAG", "Key": "theseus:stack"}],
    });
    let req = Request {
        service: "ce",
        operation: "GetCostAndUsage",
        input: &input,
        region: "us-east-1",
        pages: 1,
        class: "read",
        signer,
    };
    let out = account
        .request(b, &req)
        .await
        .map_err(|f| failure("reading the spend by theseus:stack", f))?;
    Ok(out.body["ResultsByTime"][0]["Groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let key = g["Keys"][0].as_str()?;
            let stack = key.strip_prefix("theseus:stack$")?;
            let c = to_cents(g["Metrics"]["UnblendedCost"]["Amount"].as_str()?)?;
            (!stack.is_empty()).then(|| (stack.to_string(), c))
        })
        .collect())
}

/// The inventory and the reaper's report, as the model reads them.
pub fn report(
    account: &str,
    regions: &[String],
    owned: &[Owned],
    left_out: usize,
    now_ms: u64,
) -> String {
    let mut text = format!(
        "Theseus's AWS inventory, account {account}, {} ({OWNER_TAG} = {OWNER} only: nothing \
         untagged is ever listed): {} {}\n",
        regions.join(", "),
        count(owned.len() as u64),
        if owned.len() == 1 {
            "resource"
        } else {
            "resources"
        },
    );
    for r in owned {
        let mut l = format!("  {} {} · {}", r.kind, r.name, r.region);
        if let Some(m) = r.made_ms {
            l.push_str(&format!(
                " · made {} ago",
                crate::wake::span(now_ms.saturating_sub(m))
            ));
        }
        match ttl_of(&r.tags) {
            Some(Ok(t)) if t > now_ms => {
                l.push_str(&format!(" · ttl in {}", crate::wake::span(t - now_ms)))
            }
            Some(Ok(t)) => l.push_str(&format!(
                " · ttl passed {} ago",
                crate::wake::span(now_ms - t)
            )),
            Some(Err(_)) => l.push_str(" · ttl unreadable"),
            None => {}
        }
        if let Some(c) = r.cents {
            l.push_str(&format!(" · ${} this month", cents(c)));
        }
        text.push_str(&l);
        text.push('\n');
    }
    if left_out > 0 {
        text.push_str(&format!(
            "({} more that AWS returned do not carry {OWNER_TAG} = {OWNER}: left out.)\n",
            count(left_out as u64)
        ));
    }
    let lines: Vec<String> = owned
        .iter()
        .filter_map(|r| verdict(r, now_ms).line(r))
        .collect();
    text.push_str("The reaper, in report mode (it deletes nothing):\n");
    if lines.is_empty() {
        text.push_str("  nothing to reap: no theseus:ttl has passed.\n");
    }
    for l in lines {
        text.push_str(&format!("  {l}\n"));
    }
    text
}

impl Tool for Inventory {
    fn name(&self) -> &'static str {
        "aws.inventory"
    }
    fn description(&self) -> &'static str {
        "What Theseus made on its AWS account: every resource tagged theseus:owner = theseus, in \
         each of the account's regions (never an untagged one: another project's resources are \
         not listed), with each stack's age, its theseus:ttl, and, with costs, each stack's \
         spend this month (Cost Explorer, $0.01 a call). Then the reaper's report: what it \
         would delete or stop once its theseus:ttl has passed. It deletes nothing."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "region": {"type": "string", "description": "One region (default each of the account's regions)."},
                "costs": {"type": "boolean", "description": "Each stack's spend this month, from Cost Explorer ($0.01; default false)."},
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
        let (account, regions, costs) = self.args(input)?;
        Ok(Plan {
            summary: format!(
                "take the inventory of account {} in {}{}",
                account.id,
                regions.join(", "),
                if costs {
                    ", with its stacks' spend"
                } else {
                    ""
                }
            ),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region: regions.first().cloned().unwrap_or_default(),
                service: "resourcegroupstaggingapi".into(),
                operation: "GetResources".into(),
                cost_bearing: costs,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let args = self.args(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let (account, regions, costs) = args.map_err(ToolFailure::new)?;
            let (owned, left_out) = take(
                &account,
                &regions,
                costs,
                binding.as_deref(),
                Signer::As(Kind::Work),
            )
            .await?;
            let now = theseus_protocol::now_unix_ms();
            let text = report(&account.id, &regions, &owned, left_out, now);
            let reap = owned
                .iter()
                .filter(|r| verdict(r, now).line(r).is_some())
                .count();
            Ok((
                ToolOutput {
                    text,
                    meta: json!({
                        "account": account.id,
                        "regions": regions,
                        "resources": owned.len(),
                        "left_out": left_out,
                        "would_reap": reap,
                    }),
                },
                None,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(kv: &[(&str, &str)]) -> BTreeMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn res(arn: &str, kv: &[(&str, &str)]) -> Owned {
        let (kind, name) = kind_of(arn);
        Owned {
            arn: arn.into(),
            region: "us-west-2".into(),
            kind,
            name,
            tags: tags(kv),
            made_ms: None,
            cents: None,
        }
    }

    #[test]
    fn an_arn_names_its_kind_and_name() {
        for (arn, kind, name) in [
            (
                "arn:aws:cloudformation:us-west-2:111122223333:stack/theseus-exp-1/abc",
                "cloudformation:stack",
                "theseus-exp-1",
            ),
            (
                "arn:aws:s3:::theseus-scratch",
                "s3:bucket",
                "theseus-scratch",
            ),
            (
                "arn:aws:ecs:us-west-2:111122223333:task/hands/0a1b",
                "ecs:task",
                "hands",
            ),
            (
                "arn:aws:rds:us-west-2:111122223333:db:example-db",
                "rds:db",
                "example-db",
            ),
            (
                "arn:aws:sns:us-west-2:111122223333:theseus-alerts",
                "sns:topic",
                "theseus-alerts",
            ),
        ] {
            assert_eq!(kind_of(arn), (kind.to_string(), name.to_string()), "{arn}");
        }
    }

    #[test]
    fn the_reaper_reaps_stacks_whole_runs_by_stopping_and_asks_for_state() {
        // 2026-10-03T12:00:00Z.
        let now = 1_791_028_800_000;
        let passed = [(TTL_TAG, "2026-10-03T09:00:00Z"), (OWNER_TAG, OWNER)];
        let later = [(TTL_TAG, "2026-10-04T12:00:00+00:00"), (OWNER_TAG, OWNER)];
        let stack = "arn:aws:cloudformation:us-west-2:1:stack/theseus-exp-1/x";
        assert_eq!(
            verdict(&res(stack, &passed), now),
            Verdict::DeleteStack(3 * 3_600_000)
        );
        assert_eq!(
            verdict(&res(stack, &later), now),
            Verdict::Keep(Some(86_400_000))
        );
        assert_eq!(
            verdict(&res(stack, &[(OWNER_TAG, OWNER)]), now),
            Verdict::Keep(None)
        );
        let mut in_stack = passed.to_vec();
        in_stack.push(("aws:cloudformation:stack-name", "theseus-exp-1"));
        assert_eq!(
            verdict(&res("arn:aws:s3:::theseus-exp-1-data", &in_stack), now),
            Verdict::InStack("theseus-exp-1".into())
        );
        assert_eq!(
            verdict(&res("arn:aws:ecs:us-west-2:1:task/hands/1", &passed), now),
            Verdict::Stop(3 * 3_600_000)
        );
        assert_eq!(
            verdict(&res("arn:aws:s3:::theseus-loose", &passed), now),
            Verdict::Ask(3 * 3_600_000)
        );
        assert_eq!(
            verdict(&res("arn:aws:sns:us-west-2:1:theseus-t", &passed), now),
            Verdict::Delete(3 * 3_600_000)
        );
        // A naive time is UTC; a date is its midnight; anything else is kept.
        let naive = [(TTL_TAG, "2026-10-03T09:00:00"), (OWNER_TAG, OWNER)];
        assert_eq!(
            verdict(&res(stack, &naive), now),
            Verdict::DeleteStack(3 * 3_600_000)
        );
        let date = [(TTL_TAG, "2026-10-03"), (OWNER_TAG, OWNER)];
        assert_eq!(
            verdict(&res(stack, &date), now),
            Verdict::DeleteStack(12 * 3_600_000)
        );
        let bad = [(TTL_TAG, "tomorrow"), (OWNER_TAG, OWNER)];
        assert!(matches!(
            verdict(&res(stack, &bad), now),
            Verdict::Unreadable(_)
        ));
    }
}
