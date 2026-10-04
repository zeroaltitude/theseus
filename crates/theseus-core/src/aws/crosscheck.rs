//! The CloudTrail cross-check (AWS design §3.8; C3 = 14c): a tender that,
//! once a day after serving, compares CloudTrail's event history with the
//! ledger, and finds the events of Theseus's own identities that nothing in
//! the ledger accounts for. AWS saw a call Theseus did not record: a
//! security notice.
//!
//! - **Theseus's identities.** A role session of the owner role
//!   (`…:assumed-role/<owner role>/<session>`), and the key's own user.
//! - **Accounted for.** A role session's event is accounted for when its
//!   request id is an `aws.called` row's; a job's session is named by its
//!   correlation id, which a job's ledger rows carry, so its calls are
//!   accounted for whole; the tenders' own sessions (`theseus-<tender>`)
//!   are Theseus's background reads, which no call ledgers, so they are
//!   accounted for by name. The key signs only `sts:GetCallerIdentity` and
//!   `sts:AssumeRole` once the owner role exists: any other event of the
//!   key's is unaccounted, whatever the ledger says.
//! - **The window.** Events arrive 5 to 15 minutes late, so a check reads
//!   the 24 hours that ended an hour ago, at most [`PAGES`] pages.
//!
//! Each check is one `aws.trail.checked` row: what it read, and each event
//! it found unaccounted for (its time, name, session, and request id), with
//! a warning in the log when there is one. Health's AWS panel, when the
//! cockpit has one, reads the row.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::session::Kind;
use super::trail::Event;
use super::{Account, Failure, Request, Signer};

/// How often the check runs, and how late it reads.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);
pub const LAG: Duration = Duration::from_secs(3600);
/// The pages of `LookupEvents` one check reads, 50 events a page.
pub const PAGES: u32 = 20;

/// What the ledger knows: every `aws.called` row's request ids, and every
/// correlation id a row names (a job's session is named by its own).
#[derive(Debug, Default)]
pub struct Ledgered {
    pub request_ids: HashSet<String>,
    pub correlation_ids: HashSet<String>,
}

impl Ledgered {
    /// From the ledger's rows: `(kind, data)`.
    pub fn of<'a>(rows: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Self {
        let mut l = Ledgered::default();
        for (kind, data) in rows {
            if let Some(c) = data["correlation_id"].as_str() {
                l.correlation_ids.insert(c.to_string());
            }
            if kind != "aws.called" {
                continue;
            }
            if let Some(r) = data["request_id"].as_str() {
                l.request_ids.insert(r.to_string());
            }
            for r in data["request_ids"].as_array().into_iter().flatten() {
                if let Some(r) = r.as_str() {
                    l.request_ids.insert(r.to_string());
                }
            }
        }
        l
    }
}

/// One event nothing accounts for, and why it is Theseus's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unaccounted {
    pub time: String,
    pub event: String,
    /// The role session's name, or `key` for the key's own user.
    pub session: String,
    pub request_id: Option<String>,
    pub why: String,
}

/// The events of `events` that are Theseus's and that nothing accounts for.
/// `owner_role` is the owner role's name; `key_arn` the key's user.
pub(super) fn unaccounted(
    events: &[Event],
    ledgered: &Ledgered,
    owner_role: &str,
    key_arn: Option<&str>,
) -> (usize, Vec<Unaccounted>) {
    let marker = format!(":assumed-role/{owner_role}/");
    let mut ours = 0;
    let mut out = Vec::new();
    for e in events {
        let Some(arn) = e.arn.as_deref() else {
            continue;
        };
        let event = format!("{}:{}", e.source, e.name);
        if key_arn == Some(arn) {
            ours += 1;
            if !matches!(event.as_str(), "sts:GetCallerIdentity" | "sts:AssumeRole") {
                out.push(Unaccounted {
                    time: e.time.clone(),
                    event,
                    session: "key".into(),
                    request_id: e.request_id.clone(),
                    why: "the key signs only sts:GetCallerIdentity and sts:AssumeRole".into(),
                });
            }
            continue;
        }
        let Some((_, session)) = arn.split_once(&marker) else {
            continue;
        };
        ours += 1;
        let by_request = e
            .request_id
            .as_ref()
            .is_some_and(|r| ledgered.request_ids.contains(r));
        let a_job = ledgered.correlation_ids.contains(session);
        let a_tender = session.starts_with("theseus-");
        if by_request || a_job || a_tender {
            continue;
        }
        out.push(Unaccounted {
            time: e.time.clone(),
            event,
            session: session.to_string(),
            request_id: e.request_id.clone(),
            why: "no aws.called row has its request id, and its session is no job's".into(),
        });
    }
    (ours, out)
}

/// The window's events, from CloudTrail's history in the account's region.
pub(super) async fn events(account: &Arc<Account>, now_ms: u64) -> Result<Vec<Event>, Failure> {
    let end = now_ms / 1000 - LAG.as_secs();
    let input = json!({
        "StartTime": end - EVERY.as_secs(),
        "EndTime": end,
        "MaxResults": 50,
    });
    let region = account.cfg.region.clone();
    let req = Request {
        service: "cloudtrail",
        operation: "LookupEvents",
        input: &input,
        region: &region,
        pages: PAGES,
        class: "read",
        signer: Signer::As(Kind::Tender(super::tend::TENDER)),
    };
    let out = account.request(None, &req).await?;
    Ok(out.body["Events"]
        .as_array()
        .into_iter()
        .flatten()
        .map(Event::of)
        .collect())
}

/// One check's `aws.trail.checked` row.
pub fn row(account: &str, read: usize, ours: usize, found: &[Unaccounted]) -> Value {
    json!({
        "account": account,
        "events": read,
        "theseus_events": ours,
        "unaccounted": found.iter().map(|u| json!({
            "time": u.time,
            "event": u.event,
            "session": u.session,
            "request_id": u.request_id,
            "why": u.why,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROLE: &str = "arn:aws:sts::111122223333:assumed-role/theseus-owner";

    fn event(name: &str, source: &str, arn: &str, request: &str) -> Event {
        Event::of(&json!({
            "EventName": name,
            "EventTime": "2026-10-03T12:00:00Z",
            "EventSource": format!("{source}.amazonaws.com"),
            "Username": "x",
            "CloudTrailEvent": json!({"requestID": request, "userIdentity": {"arn": arn}}).to_string(),
        }))
    }

    /// A role session's event with a ledgered request id, a job's, and a
    /// tender's are accounted for; another project's events are not
    /// Theseus's and are not judged; an unledgered work-session call, and
    /// the key signing anything but STS, are found.
    #[test]
    fn the_check_finds_what_nothing_accounts_for() {
        let rows = [
            json!({"request_id": "req-1", "correlation_id": "act_1"}),
            json!({"request_id": "req-2", "request_ids": ["req-2", "req-3"], "correlation_id": "act_2"}),
        ];
        let mut ledgered = Ledgered::of(rows.iter().map(|r| ("aws.called", r)));
        ledgered.correlation_ids.insert("act_job_9".into());
        let key = "arn:aws:iam::111122223333:user/example";
        let events = [
            event(
                "DescribeStacks",
                "cloudformation",
                &format!("{ROLE}/exe_a"),
                "req-1",
            ),
            event("ListObjectsV2", "s3", &format!("{ROLE}/exe_a"), "req-3"),
            event("PutObject", "s3", &format!("{ROLE}/act_job_9"), "req-j"),
            event(
                "ViewBudget",
                "budgets",
                &format!("{ROLE}/theseus-tender"),
                "req-t",
            ),
            event("AssumeRole", "sts", key, "req-k1"),
            event(
                "RunInstances",
                "ec2",
                "arn:aws:sts::111122223333:assumed-role/nimbus-ops/someone",
                "req-n",
            ),
            event("DeleteBucket", "s3", &format!("{ROLE}/exe_b"), "req-x"),
            event("CreateUser", "iam", key, "req-k2"),
        ];
        let (ours, found) = unaccounted(&events, &ledgered, "theseus-owner", Some(key));
        assert_eq!(ours, 7, "another project's event is not Theseus's");
        let got: Vec<(&str, &str, Option<&str>)> = found
            .iter()
            .map(|u| {
                (
                    u.event.as_str(),
                    u.session.as_str(),
                    u.request_id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("s3:DeleteBucket", "exe_b", Some("req-x")),
                ("iam:CreateUser", "key", Some("req-k2")),
            ]
        );
        let r = row("111122223333", events.len(), ours, &found);
        assert_eq!(r["unaccounted"].as_array().unwrap().len(), 2);
        assert!(!r.to_string().contains("nimbus"));
    }

    /// A check reads the 24 hours that ended an hour ago, in pages, from the
    /// account's own region, and each event comes back with its caller.
    #[tokio::test]
    async fn a_check_reads_the_day_that_ended_an_hour_ago() {
        use crate::aws::tests::{board, layer, sts, Fake, ACCOUNT};
        use crate::aws::tests_c3::{json_reply, target};
        let fake = Fake::start(|s, n| match target(s) {
            Some("LookupEvents") => json_reply(
                n,
                json!({"Events": [{"EventName": "PutObject", "EventTime": 1_791_028_800.0,
                    "EventSource": "s3.amazonaws.com", "Username": "exe_b",
                    "CloudTrailEvent": json!({"requestID": "req-x", "userIdentity": {"arn": format!("{ROLE}/exe_b")}}).to_string()}]}),
            ),
            _ => sts(ACCOUNT, n),
        });
        let aws = layer(&fake, board());
        let account = aws.account(None).unwrap().clone();
        let now_ms = 1_791_028_800_000;
        let events = events(&account, now_ms).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].arn.as_deref(), Some(&*format!("{ROLE}/exe_b")));
        let sent = fake
            .seen()
            .into_iter()
            .find(|s| target(s) == Some("LookupEvents"))
            .unwrap();
        let body: Value = serde_json::from_str(&sent.body).unwrap();
        let end = body["EndTime"].as_f64().unwrap() as u64;
        assert_eq!(end, now_ms / 1000 - 3600);
        assert_eq!(end - body["StartTime"].as_f64().unwrap() as u64, 86_400);
        assert_eq!(sent.region(), Some("us-west-2"));
        let (ours, found) = unaccounted(&events, &Ledgered::default(), "theseus-owner", None);
        assert_eq!((ours, found.len()), (1, 1));
    }
}
