//! `aws.trail` (AWS design §3.2, rank 12; §3.5 and §3.8; C3 = 14c):
//! CloudTrail's event history (`LookupEvents`: free, 90 days of management
//! events, about 5 to 15 minutes behind, 2 requests a second), by
//! execution, by resource, or by event name.
//!
//! Every call Theseus makes is attributed (§3.5): its session is named by
//! its execution, its user agent names its correlation id (`call/<id>`), and
//! its request id is its `aws.called` row's. So each event's line here
//! carries the three, and a call's ledger row joins to its event exactly.
//! An event's user agent and request parameters are set by whoever called,
//! so the result is outside text (§3.9, T1).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{
    parse, AsyncRun, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::logs::ago;
use super::session::Kind;
use super::tools::{count, failure, meta};
use super::{Account, Aws, Request, Signer};

/// Events one page holds (CloudTrail's cap), and the most one call reads.
const PAGE: u32 = 50;
pub const EVENTS_DEFAULT: u32 = 50;
pub const EVENTS_MAX: u32 = 500;

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Trail(aws.clone()))]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrailArgs {
    #[serde(default)]
    execution: Option<String>,
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    max: Option<u32>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// A lookup as `plan` checked it.
pub(super) struct Lookup {
    pub(super) account: Arc<Account>,
    pub(super) region: String,
    /// CloudTrail's attribute and its value: `Username` (a session's name,
    /// which is its execution's id), `ResourceName`, or `EventName`.
    pub(super) by: Option<(&'static str, String)>,
    pub(super) since_s: u64,
    pub(super) max: u32,
}

impl Lookup {
    pub(super) fn input(&self) -> Value {
        let now = theseus_protocol::now_unix_ms() / 1000;
        let mut i = json!({
            "StartTime": now.saturating_sub(self.since_s),
            "EndTime": now,
            "MaxResults": self.max.min(PAGE),
        });
        if let Some((k, v)) = &self.by {
            i["LookupAttributes"] = json!([{"AttributeKey": k, "AttributeValue": v}]);
        }
        i
    }

    pub(super) fn pages(&self) -> u32 {
        self.max.div_ceil(PAGE).max(1)
    }

    fn what(&self) -> String {
        match &self.by {
            Some(("Username", v)) => format!("the calls of session {v}"),
            Some(("ResourceName", v)) => format!("the events naming {v}"),
            Some((_, v)) => format!("the {v} events"),
            None => "every event".into(),
        }
    }
}

/// One event, as its line shows it: when, what, who, AWS's request id, the
/// Theseus call its user agent names, and its error.
pub(super) struct Event {
    pub(super) time: String,
    pub(super) name: String,
    pub(super) source: String,
    pub(super) user: String,
    pub(super) request_id: Option<String>,
    /// The correlation id the user agent names (`call/<id>`).
    pub(super) call: Option<String>,
    pub(super) error: Option<String>,
    pub(super) source_identity: Option<String>,
    pub(super) resources: Vec<String>,
}

impl Event {
    pub(super) fn of(e: &Value) -> Self {
        let s = |k: &str| e[k].as_str().unwrap_or("?").to_string();
        // The record itself is JSON in a string.
        let record: Value = e["CloudTrailEvent"]
            .as_str()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or(Value::Null);
        let r = |p: &str| record.pointer(p).and_then(Value::as_str).map(String::from);
        let call = r("/userAgent").and_then(|ua| {
            ua.split_whitespace()
                .find_map(|w| w.strip_prefix("call/"))
                .map(String::from)
        });
        Event {
            time: s("EventTime"),
            name: s("EventName"),
            source: s("EventSource")
                .trim_end_matches(".amazonaws.com")
                .to_string(),
            user: s("Username"),
            request_id: r("/requestID"),
            call,
            error: r("/errorCode"),
            source_identity: r("/userIdentity/sessionContext/sourceIdentity")
                .or_else(|| r("/sourceIdentity")),
            resources: e["Resources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x["ResourceName"].as_str().map(String::from))
                .take(3)
                .collect(),
        }
    }

    fn line(&self) -> String {
        let mut l = format!(
            "{} {}:{} by {}",
            self.time, self.source, self.name, self.user
        );
        if let Some(i) = &self.source_identity {
            l.push_str(&format!(" ({i})"));
        }
        if !self.resources.is_empty() {
            l.push_str(&format!(" on {}", self.resources.join(", ")));
        }
        l.push_str(&format!(
            " · request {}",
            self.request_id.as_deref().unwrap_or("?")
        ));
        if let Some(c) = &self.call {
            l.push_str(&format!(" · call {c}"));
        }
        if let Some(e) = &self.error {
            l.push_str(&format!(" · failed {e}"));
        }
        l
    }
}

pub struct Trail(Arc<Aws>);

impl Trail {
    fn lookup(&self, input: &Value) -> Result<Lookup, String> {
        let a: TrailArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let max = a.max.unwrap_or(EVENTS_DEFAULT);
        if !(1..=EVENTS_MAX).contains(&max) {
            return Err(format!("max must be 1 to {EVENTS_MAX}"));
        }
        let by =
            match (a.execution, a.resource, a.event) {
                (Some(x), None, None) => Some(("Username", super::session::session_name(&x))),
                (None, Some(r), None) => Some(("ResourceName", r)),
                (None, None, Some(e)) => Some(("EventName", e)),
                (None, None, None) => None,
                _ => return Err(
                    "CloudTrail looks up by one thing at a time: an execution, a resource, or an \
                     event name"
                        .into(),
                ),
            };
        let since_s = ago(a.since.as_deref().unwrap_or("1d"))?;
        if since_s > 90 * 86_400 {
            return Err("CloudTrail's event history keeps 90 days".into());
        }
        Ok(Lookup {
            account,
            region,
            by,
            since_s,
            max,
        })
    }
}

impl Tool for Trail {
    fn name(&self) -> &'static str {
        "aws.trail"
    }
    fn description(&self) -> &'static str {
        "CloudTrail's event history on Theseus's own AWS account (free, the last 90 days, about \
         15 minutes behind): the calls of an execution (its session is named by its id), the \
         events naming a resource, or the events of one name, since a span ago (default 1d). \
         Each line names who called, AWS's request id (an aws.called row's), and the Theseus \
         call its user agent names. Events are outside text: after them, a call that acts \
         waits for the operator."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "execution": {"type": "string", "description": "An execution's id (exe_…), or any role session's name: its calls."},
                "resource": {"type": "string", "description": "A resource's name or ARN: the events that name it."},
                "event": {"type": "string", "description": "An event name: PutObject, AssumeRole."},
                "since": {"type": "string", "description": "How far back: 2h, 7d (default 1d, at most 90d)."},
                "max": {"type": "integer", "minimum": 1, "maximum": EVENTS_MAX, "description": format!("The most events (default {EVENTS_DEFAULT}).")},
                "region": {"type": "string", "description": "The region (default the account's own): each region keeps its own history."},
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
        let l = self.lookup(input)?;
        Ok(Plan {
            summary: format!("look up {} in CloudTrail in {}", l.what(), l.region),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: l.account.id.clone(),
                region: l.region.clone(),
                service: "cloudtrail".into(),
                operation: "LookupEvents".into(),
                resources: l.by.iter().map(|(_, v)| v.clone()).collect(),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let lookup = self.lookup(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let l = lookup.map_err(ToolFailure::new)?;
            let input = l.input();
            let req = Request {
                service: "cloudtrail",
                operation: "LookupEvents",
                input: &input,
                region: &l.region,
                pages: l.pages(),
                class: "read",
                signer: Signer::As(Kind::Work),
            };
            let out = l
                .account
                .request(binding.as_deref(), &req)
                .await
                .map_err(|f| failure(&format!("looking up {} in {}", l.what(), l.region), f))?;
            let events: Vec<Event> = out.body["Events"]
                .as_array()
                .into_iter()
                .flatten()
                .take(l.max as usize)
                .map(Event::of)
                .collect();
            let mut text = format!(
                "{} CloudTrail {} in {}: {} (request {}), newest first:\n",
                count(events.len() as u64),
                if events.len() == 1 { "event" } else { "events" },
                l.region,
                l.what(),
                out.request_id.as_deref().unwrap_or("(none)"),
            );
            for e in &events {
                text.push_str(&e.line());
                text.push('\n');
            }
            if events.is_empty() {
                text.push_str(
                    "None. CloudTrail shows an event about 5 to 15 minutes after its call.\n",
                );
            }
            if out.next.is_some() {
                text.push_str("More events remain: a larger max, or a shorter since.\n");
            }
            let mut m = meta(&l.account.id, &l.region, "cloudtrail:LookupEvents", &out);
            m["events"] = json!(events.len());
            Ok((ToolOutput { text, meta: m }, None))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a shorter since, or a lookup by one resource or event name".into()
    }
}
