//! `aws.logs.query` and `aws.logs.tail` (AWS design §3.2, ranks 4 and 5; C3
//! = 14c): CloudWatch Logs Insights, waited on until its answer is ready and
//! returned as a table with the bytes it scanned and what that cost; and a
//! group's recent lines, filtered, and followed for a while by polling.
//!
//! Both are recipes over the account's one caller, so every request they
//! make is an `aws.called` row and a span. A wait polls with tokio's timer,
//! never a blocking sleep, and holds no core. Log lines are outside text
//! (§3.9, T1): the hold that marks them is `aws::external`'s.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_aws::Output;
use theseus_tools::{
    parse, AsyncRun, AwsBinding, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx,
    ToolFailure, ToolOutput,
};

use super::session::Kind;
use super::tools::{count, failure, meta};
use super::{Account, Aws, Failure, Request, Signer};

/// How long a query waits for its answer when the call does not say, and
/// at most.
pub const WAIT_DEFAULT: u64 = 60;
pub const WAIT_MAX: u64 = 300;
/// Rows a query returns when the call does not say, and at most (Logs
/// Insights' own cap).
pub const ROWS_DEFAULT: u32 = 100;
pub const ROWS_MAX: u32 = 10_000;
/// Lines a tail returns when the call does not say, and at most.
pub const LINES_DEFAULT: u32 = 100;
pub const LINES_MAX: u32 = 1_000;
/// The longest a tail follows its group.
pub const FOLLOW_MAX: u64 = 60;
/// The most log groups one query reads (Logs Insights' cap).
const GROUPS_MAX: usize = 50;
/// Logs Insights' price per GB scanned, in tenths of a cent ($0.005).
const MILLS_PER_GB: f64 = 5.0;

pub(super) fn all(aws: &Arc<Aws>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Query(aws.clone())), Arc::new(Tail(aws.clone()))]
}

/// `30s`, `15m`, `2h`, `7d`: a span of time, in seconds.
pub(super) fn ago(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let bad = || format!("{s:?} is not a span of time: a number and s, m, h, or d (15m, 2h, 7d)");
    let unit = s.chars().last().ok_or_else(bad)?;
    let n: u64 = s[..s.len() - unit.len_utf8()].parse().map_err(|_| bad())?;
    let per = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return Err(bad()),
    };
    n.checked_mul(per)
        .filter(|v| *v <= 400 * 86_400)
        .ok_or_else(|| format!("{s:?} reaches back more than 400 days"))
}

/// Unix milliseconds as `2026-10-04T12:00:00Z`.
pub(super) fn utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, mo, d) = crate::wake::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        t / 60 % 60,
        t % 60
    )
}

fn now_ms() -> i64 {
    theseus_protocol::now_unix_ms() as i64
}

/// One Logs request, signed in the call's work session.
async fn logs(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
    operation: &str,
    input: &Value,
    pages: u32,
) -> Result<Output, Failure> {
    let req = Request {
        service: "logs",
        operation,
        input,
        region,
        pages,
        class: "read",
        signer: Signer::As(Kind::Work),
    };
    account.request(binding, &req).await
}

/// A plan's AWS half for a Logs read.
fn plan_aws(
    account: &Account,
    region: &str,
    operation: &str,
    groups: &[String],
    cost: bool,
) -> AwsPlan {
    AwsPlan {
        account: account.id.clone(),
        region: region.into(),
        service: "logs".into(),
        operation: operation.into(),
        cost_bearing: cost,
        resources: groups.iter().take(10).cloned().collect(),
        ..Default::default()
    }
}

// ------------------------------------------------------------------ aws.logs.query

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    wait: Option<u64>,
    #[serde(default)]
    query_id: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// A query as `plan` checked it.
struct Querying {
    account: Arc<Account>,
    region: String,
    /// A new query; None: the answer of `query_id`, started before.
    start: Option<NewQuery>,
    query_id: Option<String>,
    limit: u32,
    wait: Duration,
}

struct NewQuery {
    query: String,
    groups: Vec<String>,
    prefix: Option<String>,
    since_s: u64,
    until_s: u64,
}

pub struct Query(Arc<Aws>);

impl Query {
    fn querying(&self, input: &Value) -> Result<Querying, String> {
        let a: QueryArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let limit = a.limit.unwrap_or(ROWS_DEFAULT);
        if !(1..=ROWS_MAX).contains(&limit) {
            return Err(format!("limit must be 1 to {ROWS_MAX}"));
        }
        let wait = a.wait.unwrap_or(WAIT_DEFAULT);
        if wait > WAIT_MAX {
            return Err(format!("wait must be at most {WAIT_MAX} seconds"));
        }
        let start = match (&a.query_id, a.query) {
            (Some(_), None) if a.groups.is_empty() && a.prefix.is_none() => None,
            (Some(_), _) => {
                return Err(
                    "query_id reads a query already started: give it alone, without a query \
                     or its groups"
                        .into(),
                )
            }
            (None, None) => {
                return Err("give a query (Logs Insights' language), or a query_id".into())
            }
            (None, Some(q)) => {
                if q.trim().is_empty() {
                    return Err("the query is empty".into());
                }
                if a.groups.is_empty() == a.prefix.is_none() {
                    return Err(
                        "name the log groups to read: \"groups\" (their names), or \"prefix\" \
                         (every group whose name starts with it), not both"
                            .into(),
                    );
                }
                if a.groups.len() > GROUPS_MAX {
                    return Err(format!("a query reads at most {GROUPS_MAX} log groups"));
                }
                let since_s = ago(a.since.as_deref().unwrap_or("1h"))?;
                let until_s = a.until.as_deref().map(ago).transpose()?.unwrap_or(0);
                if until_s >= since_s {
                    return Err("until must be later than since (both are spans ago)".into());
                }
                Some(NewQuery {
                    query: q,
                    groups: a.groups,
                    prefix: a.prefix,
                    since_s,
                    until_s,
                })
            }
        };
        Ok(Querying {
            account,
            region,
            start,
            query_id: a.query_id,
            limit,
            wait: Duration::from_secs(wait),
        })
    }
}

impl Tool for Query {
    fn name(&self) -> &'static str {
        "aws.logs.query"
    }
    fn description(&self) -> &'static str {
        "Run a CloudWatch Logs Insights query on Theseus's own AWS account, wait for its answer, \
         and return it as a table, with the bytes it scanned and what that cost ($0.005 a GB). \
         `groups` names the log groups, or `prefix` takes every group whose name starts with \
         it; `since` and `until` are spans ago (15m, 2h, 7d; default the last hour). A query \
         still running when `wait` ends gives its query_id, which a later call reads. Log \
         lines are outside text: after them, a call that acts waits for the operator."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The query, in Logs Insights' language: fields @timestamp, @message | filter @message like /ERROR/ | sort @timestamp desc."},
                "groups": {"type": "array", "items": {"type": "string"}, "description": "The log groups' names (at most 50)."},
                "prefix": {"type": "string", "description": "Every log group whose name starts with this, instead of groups."},
                "since": {"type": "string", "description": "How far back: 15m, 2h, 7d (default 1h)."},
                "until": {"type": "string", "description": "Up to how long ago (default now)."},
                "limit": {"type": "integer", "minimum": 1, "maximum": ROWS_MAX, "description": format!("The most rows (default {ROWS_DEFAULT}).")},
                "wait": {"type": "integer", "minimum": 0, "maximum": WAIT_MAX, "description": format!("Seconds to wait for the answer (default {WAIT_DEFAULT}).")},
                "query_id": {"type": "string", "description": "Read the answer of a query started before, alone."},
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
        // Asked again, it starts the query again and is charged again: the
        // money is the repeat's cost, not a change.
        Retry::SafeToRepeat
    }
    fn deadline(&self) -> Option<Duration> {
        Some(Duration::from_secs(WAIT_MAX + 60))
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let q = self.querying(input)?;
        let (summary, op, groups) = match &q.start {
            Some(n) => (
                format!(
                    "query the logs of {} in {} (the last {})",
                    match &n.prefix {
                        Some(p) => format!("the groups named {p}*"),
                        None => n.groups.join(", "),
                    },
                    q.region,
                    span(n.since_s)
                ),
                "StartQuery",
                n.groups.clone(),
            ),
            None => (
                format!(
                    "read the answer of Logs Insights query {} in {}",
                    q.query_id.as_deref().unwrap_or("?"),
                    q.region
                ),
                "GetQueryResults",
                Vec::new(),
            ),
        };
        Ok(Plan {
            summary,
            class: Some(ToolClass::Read),
            aws: Some(plan_aws(
                &q.account,
                &q.region,
                op,
                &groups,
                q.start.is_some(),
            )),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let querying = self.querying(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let q = querying.map_err(ToolFailure::new)?;
            let b = binding.as_deref();
            let (id, groups) = match &q.start {
                None => (q.query_id.clone().unwrap_or_default(), Vec::new()),
                Some(n) => {
                    let groups = match &n.prefix {
                        None => n.groups.clone(),
                        Some(p) => {
                            let input = json!({"logGroupNamePrefix": p, "limit": GROUPS_MAX});
                            let out =
                                logs(&q.account, b, &q.region, "DescribeLogGroups", &input, 1)
                                    .await
                                    .map_err(|f| {
                                        failure(
                                            &format!(
                                                "finding the log groups named {p}* in {}",
                                                q.region
                                            ),
                                            f,
                                        )
                                    })?;
                            let found: Vec<String> = out.body["logGroups"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|g| g["logGroupName"].as_str().map(String::from))
                                .collect();
                            if found.is_empty() {
                                return Err(ToolFailure::new(format!(
                                    "no log group in {} has a name that starts with {p:?}; nothing \
                                     was queried",
                                    q.region
                                )));
                            }
                            found
                        }
                    };
                    let now = now_ms() / 1000;
                    let input = json!({
                        "logGroupNames": groups,
                        "queryString": n.query,
                        "startTime": now - n.since_s as i64,
                        "endTime": now - n.until_s as i64,
                        "limit": q.limit,
                    });
                    let out = logs(&q.account, b, &q.region, "StartQuery", &input, 1)
                        .await
                        .map_err(|f| failure(&format!("starting the query in {}", q.region), f))?;
                    let id = out.body["queryId"].as_str().unwrap_or_default().to_string();
                    (id, groups)
                }
            };
            let waited = tokio::time::Instant::now();
            let mut pause = Duration::from_millis(500);
            let out = loop {
                let input = json!({"queryId": id});
                let out = logs(&q.account, b, &q.region, "GetQueryResults", &input, 1)
                    .await
                    .map_err(|f| failure(&format!("reading query {id} in {}", q.region), f))?;
                let status = out.body["status"].as_str().unwrap_or("Unknown");
                if !matches!(status, "Scheduled" | "Running") || waited.elapsed() >= q.wait {
                    break out;
                }
                tokio::time::sleep(pause.min(q.wait.saturating_sub(waited.elapsed()))).await;
                pause = (pause * 2).min(Duration::from_secs(5));
            };
            let text = table(&q, &id, &groups, &out);
            let mut m = meta(&q.account.id, &q.region, "logs:GetQueryResults", &out);
            m["query_id"] = json!(id);
            m["status"] = out.body["status"].clone();
            m["bytes_scanned"] = out.body["statistics"]["bytesScanned"].clone();
            m["groups"] = json!(groups.len());
            Ok((ToolOutput { text, meta: m }, None))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower query returns them: a filter, a stats line, or a smaller limit".into()
    }
}

/// `2h`, `3d`, `45m`: seconds as the largest whole unit.
fn span(s: u64) -> String {
    match s {
        s if s % 86_400 == 0 => format!("{}d", s / 86_400),
        s if s % 3600 == 0 => format!("{}h", s / 3600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// A query's answer as the model reads it: its state, what it scanned and
/// cost, then its rows as a table (tab-separated, under a header of its
/// fields, `@ptr` left out).
fn table(q: &Querying, id: &str, groups: &[String], out: &Output) -> String {
    let status = out.body["status"].as_str().unwrap_or("Unknown");
    let stats = &out.body["statistics"];
    let scanned = stats["bytesScanned"].as_f64().unwrap_or(0.0);
    let mills = scanned / 1e9 * MILLS_PER_GB;
    let mut text = format!(
        "Logs Insights query {id} in {}{}: {status}; {} records matched of {} scanned, {} bytes \
         scanned (about ${:.4})\n",
        q.region,
        if groups.is_empty() {
            String::new()
        } else {
            format!(
                " over {} log groups ({})",
                groups.len(),
                groups
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        count(stats["recordsMatched"].as_f64().unwrap_or(0.0) as u64),
        count(stats["recordsScanned"].as_f64().unwrap_or(0.0) as u64),
        count(scanned as u64),
        mills / 1000.0,
    );
    if matches!(status, "Scheduled" | "Running") {
        text.push_str(&format!(
            "It is still running: call again with \"query_id\": \"{id}\" for its answer.\n"
        ));
    }
    let rows: Vec<&Vec<Value>> = out.body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .collect();
    let mut fields: Vec<String> = Vec::new();
    for r in &rows {
        for f in r.iter().filter_map(|c| c["field"].as_str()) {
            if f != "@ptr" && !fields.iter().any(|x| x == f) {
                fields.push(f.to_string());
            }
        }
    }
    if rows.is_empty() {
        text.push_str("No rows.\n");
        return text;
    }
    text.push_str(&format!(
        "{} rows:\n{}\n",
        count(rows.len() as u64),
        fields.join("\t")
    ));
    for r in rows {
        let cell = |f: &str| {
            r.iter()
                .find(|c| c["field"] == f)
                .and_then(|c| c["value"].as_str())
                .unwrap_or("")
                .replace(['\t', '\n'], " ")
        };
        text.push_str(
            &fields
                .iter()
                .map(|f| cell(f))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        text.push('\n');
    }
    text
}

// ------------------------------------------------------------------ aws.logs.tail

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TailArgs {
    group: String,
    #[serde(default)]
    stream_prefix: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    lines: Option<u32>,
    #[serde(default)]
    follow: Option<u64>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

struct Tailing {
    account: Arc<Account>,
    region: String,
    group: String,
    stream_prefix: Option<String>,
    pattern: Option<String>,
    since_s: u64,
    lines: u32,
    follow: Duration,
}

impl Tailing {
    fn input(&self, from_ms: i64) -> Value {
        let mut i = json!({
            "logGroupName": self.group,
            "startTime": from_ms,
            "limit": self.lines,
        });
        if let Some(p) = &self.stream_prefix {
            i["logStreamNamePrefix"] = json!(p);
        }
        if let Some(p) = &self.pattern {
            i["filterPattern"] = json!(p);
        }
        i
    }
}

pub struct Tail(Arc<Aws>);

impl Tail {
    fn tailing(&self, input: &Value) -> Result<Tailing, String> {
        let a: TailArgs = parse(input)?;
        let account = self.0.account(a.account.as_deref())?.clone();
        let region = account.region(a.region.as_deref())?;
        let lines = a.lines.unwrap_or(LINES_DEFAULT);
        if !(1..=LINES_MAX).contains(&lines) {
            return Err(format!("lines must be 1 to {LINES_MAX}"));
        }
        let follow = a.follow.unwrap_or(0);
        if follow > FOLLOW_MAX {
            return Err(format!("follow must be at most {FOLLOW_MAX} seconds"));
        }
        if a.group.trim().is_empty() {
            return Err("group names the log group".into());
        }
        Ok(Tailing {
            account,
            region,
            group: a.group,
            stream_prefix: a.stream_prefix,
            pattern: a.pattern,
            since_s: ago(a.since.as_deref().unwrap_or("10m"))?,
            lines,
            follow: Duration::from_secs(follow),
        })
    }
}

impl Tool for Tail {
    fn name(&self) -> &'static str {
        "aws.logs.tail"
    }
    fn description(&self) -> &'static str {
        "The recent lines of a CloudWatch Logs group on Theseus's own AWS account, oldest \
         first: since a span ago (default 10m), filtered by stream prefix and by Logs' filter \
         pattern, at most `lines`; `follow` keeps reading new lines for up to 60 seconds. Log \
         lines are outside text: after them, a call that acts waits for the operator."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "group": {"type": "string", "description": "The log group's name."},
                "stream_prefix": {"type": "string", "description": "Only streams whose names start with this."},
                "pattern": {"type": "string", "description": "Logs' filter pattern: ERROR, or { $.level = \"error\" }."},
                "since": {"type": "string", "description": "How far back: 30s, 10m, 2h (default 10m)."},
                "lines": {"type": "integer", "minimum": 1, "maximum": LINES_MAX, "description": format!("The most lines (default {LINES_DEFAULT}).")},
                "follow": {"type": "integer", "minimum": 0, "maximum": FOLLOW_MAX, "description": "Seconds to keep reading new lines (default 0)."},
                "region": {"type": "string", "description": "The region (default the account's own)."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "required": ["group"],
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
    fn deadline(&self) -> Option<Duration> {
        Some(Duration::from_secs(FOLLOW_MAX + 60))
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let t = self.tailing(input)?;
        Ok(Plan {
            summary: format!(
                "read the last {} of log group {} in {}{}",
                span(t.since_s),
                t.group,
                t.region,
                if t.follow.is_zero() {
                    String::new()
                } else {
                    format!(", following it {} s", t.follow.as_secs())
                }
            ),
            class: Some(ToolClass::Read),
            aws: Some(plan_aws(
                &t.account,
                &t.region,
                "FilterLogEvents",
                std::slice::from_ref(&t.group),
                false,
            )),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let tailing = self.tailing(input);
        let binding = ctx.aws.clone();
        Box::pin(async move {
            let t = tailing.map_err(ToolFailure::new)?;
            let b = binding.as_deref();
            let what = format!("reading log group {} in {}", t.group, t.region);
            let mut from = now_ms() - (t.since_s as i64) * 1000;
            let ends = tokio::time::Instant::now() + t.follow;
            let mut seen: Vec<String> = Vec::new();
            let mut events: Vec<Value> = Vec::new();
            let mut cut = false;
            let out = loop {
                let out = logs(
                    &t.account,
                    b,
                    &t.region,
                    "FilterLogEvents",
                    &t.input(from),
                    5,
                )
                .await
                .map_err(|f| failure(&what, f))?;
                for e in out.body["events"].as_array().into_iter().flatten() {
                    let id = e["eventId"].as_str().unwrap_or_default().to_string();
                    if seen.contains(&id) {
                        continue;
                    }
                    if events.len() >= t.lines as usize {
                        cut = true;
                        break;
                    }
                    if let Some(ts) = e["timestamp"].as_i64() {
                        from = from.max(ts);
                    }
                    seen.push(id);
                    events.push(e.clone());
                }
                cut |= out.next.is_some();
                if cut || tokio::time::Instant::now() >= ends {
                    break out;
                }
                tokio::time::sleep(Duration::from_secs(2).min(ends - tokio::time::Instant::now()))
                    .await;
            };
            let mut text = format!(
                "{} {} of log group {} in {} (request {}), since {}{}:\n",
                count(events.len() as u64),
                if events.len() == 1 { "line" } else { "lines" },
                t.group,
                t.region,
                out.request_id.as_deref().unwrap_or("(none)"),
                span(t.since_s),
                if t.follow.is_zero() {
                    String::new()
                } else {
                    format!(" ago, followed {} s", t.follow.as_secs())
                },
            );
            for e in &events {
                text.push_str(&format!(
                    "{} {} {}\n",
                    e["timestamp"]
                        .as_i64()
                        .map(utc)
                        .unwrap_or_else(|| "?".into()),
                    e["logStreamName"].as_str().unwrap_or("?"),
                    e["message"].as_str().unwrap_or("").trim_end()
                ));
            }
            if cut {
                text.push_str(&format!(
                    "More lines remain past these {}: a shorter since, a stream prefix, or a \
                     pattern narrows them.\n",
                    count(events.len() as u64)
                ));
            }
            let mut m = meta(&t.account.id, &t.region, "logs:FilterLogEvents", &out);
            m["lines"] = json!(events.len());
            m["group"] = json!(t.group);
            Ok((ToolOutput { text, meta: m }, None))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a shorter since, a stream prefix, or a pattern narrows them".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_of_time_read_as_seconds() {
        assert_eq!(ago("30s"), Ok(30));
        assert_eq!(ago("15m"), Ok(900));
        assert_eq!(ago("2h"), Ok(7200));
        assert_eq!(ago("7d"), Ok(604_800));
        for bad in ["", "h", "2", "2w", "-2h", "1000d", "2 h"] {
            assert!(ago(bad).is_err(), "{bad:?}");
        }
        assert_eq!(span(7200), "2h");
        assert_eq!(span(90), "90s");
    }

    #[test]
    fn utc_names_the_instant() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        // 2026-10-03T12:00:00Z.
        assert_eq!(utc(1_791_028_800_000), "2026-10-03T12:00:00Z");
    }
}
