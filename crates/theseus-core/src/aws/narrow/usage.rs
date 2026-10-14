//! What a role called, from CloudTrail's event history: the distinct
//! `service:Action` pairs and the resource ARNs, over a window.
//!
//! CloudTrail's `LookupEvents` records management events only. A data event
//! (an S3 object's read or write, a Lambda invoke, a DynamoDB item) never
//! reaches it, so the result names the data actions the role's policy allows
//! as **not visible** rather than counting them unused ([`DATA_EVENTS`]). A
//! call that failed with an access error is not a use: it is counted apart.
//! Pages are read at CloudTrail's rate (2 a second) up to a cap; a window
//! that did not finish is `complete = false`, and nothing is proposed from it.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::time::Duration;

use serde_json::Value;

/// Events one `LookupEvents` page holds.
pub const PAGE: u32 = 50;
/// The window's default, in days.
pub const DAYS: u32 = 14;
/// CloudTrail's event history keeps this many days.
pub const HISTORY_DAYS: u32 = 90;
/// The pause between pages: `LookupEvents` allows 2 requests a second.
const PACE: Duration = Duration::from_millis(520);

/// Actions the trail does not record (data events), as policy patterns. The
/// policy's allows of these are kept as they are.
pub const DATA_EVENTS: &[&str] = &[
    "s3:GetObject",
    "s3:GetObjectVersion",
    "s3:PutObject",
    "s3:DeleteObject",
    "s3:DeleteObjectVersion",
    "s3:AbortMultipartUpload",
    "s3:ListMultipartUploadParts",
    "lambda:InvokeFunction",
    "lambda:InvokeAsync",
    "dynamodb:GetItem",
    "dynamodb:PutItem",
    "dynamodb:UpdateItem",
    "dynamodb:DeleteItem",
    "dynamodb:BatchGetItem",
    "dynamodb:BatchWriteItem",
    "dynamodb:Query",
    "dynamodb:Scan",
];

/// One action's use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Use {
    pub calls: u64,
    /// The ARNs the calls named.
    pub resources: BTreeSet<String>,
    /// Some call named no ARN, so the action's resources stay as they are.
    pub unresolved: bool,
}

/// What a role called over a window.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Observed {
    pub role: String,
    pub days: u32,
    pub events: u64,
    /// Events read, of any role.
    pub read: u64,
    pub pages: u32,
    /// The window was read to its start.
    pub complete: bool,
    pub used: BTreeMap<String, Use>,
    /// Calls that failed with an access error, by action.
    pub denied: BTreeMap<String, u64>,
    /// The actions the history does not record (data events): not seen in
    /// this window, and so not shown unused.
    pub not_visible: Vec<String>,
    /// Events before this time (unix seconds) are not read: a watch counts
    /// only what came after an apply.
    pub since_s: u64,
}

/// How the history is read: a page of events from a start time, after a
/// token. The account's CloudTrail in production; a fake in tests.
pub trait Source {
    fn page(
        &self,
        start_s: u64,
        end_s: u64,
        token: Option<&str>,
    ) -> impl Future<Output = Result<(Vec<Value>, Option<String>), String>> + Send;
}

/// IAM's prefix for an event source.
fn prefix_of(source: &str) -> String {
    let p = source.trim_end_matches(".amazonaws.com");
    match p {
        "monitoring" => "cloudwatch",
        "email" => "ses",
        "events" => "events",
        "elasticloadbalancing" => "elasticloadbalancing",
        other => other,
    }
    .to_string()
}

/// The IAM action an event authorised, where its name differs from it.
pub fn action_of(source: &str, name: &str) -> String {
    let prefix = prefix_of(source);
    let name = match (prefix.as_str(), name) {
        ("s3", "ListObjects" | "ListObjectsV2" | "HeadBucket") => "ListBucket",
        ("s3", "ListBuckets") => "ListAllMyBuckets",
        ("s3", "HeadObject") => "GetObject",
        ("s3", "CompleteMultipartUpload" | "CreateMultipartUpload" | "UploadPart") => "PutObject",
        ("logs", "StartQuery") => "StartQuery",
        ("sts", "AssumeRoleWithWebIdentity") => "AssumeRoleWithWebIdentity",
        (_, n) => n,
    };
    format!("{prefix}:{name}")
}

/// The role a CloudTrail record's caller assumed, when it assumed one.
fn role_of(record: &Value) -> Option<String> {
    let id = &record["userIdentity"];
    if let Some(n) = id["sessionContext"]["sessionIssuer"]["userName"].as_str() {
        return Some(n.to_string());
    }
    let arn = id["arn"].as_str()?;
    let rest = arn.split(":assumed-role/").nth(1)?;
    rest.split('/').next().map(String::from)
}

fn denied(code: &str) -> bool {
    matches!(
        code,
        "AccessDenied"
            | "AccessDeniedException"
            | "UnauthorizedOperation"
            | "Client.UnauthorizedOperation"
    ) || code.ends_with("AccessDenied")
}

impl Observed {
    /// Take one `LookupEvents` event: the record is the JSON in its
    /// `CloudTrailEvent` string.
    pub fn take(&mut self, e: &Value) {
        self.read += 1;
        if e["EventTime"]
            .as_f64()
            .is_some_and(|t| (t as u64) < self.since_s)
        {
            return;
        }
        let Some(record) = e["CloudTrailEvent"]
            .as_str()
            .and_then(|r| serde_json::from_str::<Value>(r).ok())
        else {
            return;
        };
        if role_of(&record).as_deref() != Some(self.role.as_str()) {
            return;
        }
        let (Some(src), Some(name)) =
            (record["eventSource"].as_str(), record["eventName"].as_str())
        else {
            return;
        };
        self.events += 1;
        let action = action_of(src, name);
        if record["errorCode"].as_str().is_some_and(denied) {
            *self.denied.entry(action).or_default() += 1;
            return;
        }
        let u = self.used.entry(action).or_default();
        u.calls += 1;
        let arns: Vec<&str> = record["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["ARN"].as_str())
            .collect();
        if arns.is_empty() {
            u.unresolved = true;
        }
        u.resources.extend(arns.into_iter().map(String::from));
    }
}

/// Read `role`'s calls over the last `days` days, at most `max_pages` pages,
/// at CloudTrail's rate. `now_s` is the window's end.
pub async fn observed(
    src: &impl Source,
    role: &str,
    days: u32,
    now_s: u64,
    max_pages: u32,
) -> Result<Observed, String> {
    observed_since(src, role, days, now_s, max_pages, 0).await
}

/// [`observed`], counting only events at or after `since_s`.
pub async fn observed_since(
    src: &impl Source,
    role: &str,
    days: u32,
    now_s: u64,
    max_pages: u32,
    since_s: u64,
) -> Result<Observed, String> {
    if days == 0 || days > HISTORY_DAYS {
        return Err(format!("days must be 1 to {HISTORY_DAYS}"));
    }
    let start = now_s.saturating_sub(u64::from(days) * 86_400);
    let mut o = Observed {
        role: role.into(),
        days,
        since_s,
        not_visible: DATA_EVENTS.iter().map(|d| (*d).to_string()).collect(),
        ..Default::default()
    };
    let mut token: Option<String> = None;
    loop {
        if o.pages >= max_pages {
            o.complete = false;
            return Ok(o);
        }
        if o.pages > 0 {
            tokio::time::sleep(PACE).await;
        }
        let (events, next) = src.page(start, now_s, token.as_deref()).await?;
        o.pages += 1;
        for e in &events {
            o.take(e);
        }
        match next {
            Some(t) => token = Some(t),
            None => {
                o.complete = true;
                return Ok(o);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    /// A trail of fixed pages, and the tokens it was asked for.
    pub(crate) struct Pages {
        pub pages: Vec<Vec<Value>>,
        pub asked: Mutex<Vec<Option<String>>>,
    }

    impl Source for Pages {
        async fn page(
            &self,
            _s: u64,
            _e: u64,
            token: Option<&str>,
        ) -> Result<(Vec<Value>, Option<String>), String> {
            self.asked.lock().unwrap().push(token.map(String::from));
            let i: usize = token.map_or(0, |t| t.parse().unwrap());
            let next = (i + 1 < self.pages.len()).then(|| (i + 1).to_string());
            Ok((self.pages[i].clone(), next))
        }
    }

    pub(crate) fn event(
        role: &str,
        source: &str,
        name: &str,
        arns: &[&str],
        err: Option<&str>,
    ) -> Value {
        let mut r = json!({
            "eventSource": source, "eventName": name,
            "userIdentity": {"arn": format!("arn:aws:sts::111122223333:assumed-role/{role}/task-1"),
                             "sessionContext": {"sessionIssuer": {"userName": role}}},
            "resources": arns.iter().map(|a| json!({"ARN": a})).collect::<Vec<_>>(),
        });
        if let Some(e) = err {
            r["errorCode"] = json!(e);
        }
        json!({"EventName": name, "EventSource": source, "CloudTrailEvent": r.to_string()})
    }

    #[tokio::test(start_paused = true)]
    async fn pages_are_read_to_the_start_and_two_roles_are_kept_apart() {
        let src = Pages {
            pages: vec![
                vec![
                    event(
                        "theseus-hand-owner",
                        "ec2.amazonaws.com",
                        "DescribeInstances",
                        &[],
                        None,
                    ),
                    event(
                        "theseus-hand-read",
                        "s3.amazonaws.com",
                        "ListBuckets",
                        &[],
                        None,
                    ),
                ],
                vec![
                    event(
                        "theseus-hand-owner",
                        "dynamodb.amazonaws.com",
                        "DescribeTable",
                        &["arn:aws:dynamodb:us-west-2:111122223333:table/t"],
                        None,
                    ),
                    event(
                        "theseus-hand-owner",
                        "ec2.amazonaws.com",
                        "DescribeInstances",
                        &[],
                        None,
                    ),
                ],
            ],
            asked: Mutex::default(),
        };
        let o = observed(&src, "theseus-hand-owner", 14, 2_000_000_000, 10)
            .await
            .unwrap();
        assert!(o.complete);
        assert_eq!((o.pages, o.read, o.events), (2, 4, 3));
        // What the history cannot record is named, not counted unused.
        assert!(o.not_visible.contains(&"s3:GetObject".to_string()));
        assert!(o.not_visible.contains(&"lambda:InvokeFunction".to_string()));
        assert!(!o.used.contains_key("s3:GetObject"));
        assert_eq!(o.used["ec2:DescribeInstances"].calls, 2);
        assert!(o.used["ec2:DescribeInstances"].unresolved);
        assert!(!o.used.contains_key("s3:ListAllMyBuckets"));
        assert_eq!(
            o.used["dynamodb:DescribeTable"]
                .resources
                .iter()
                .collect::<Vec<_>>(),
            ["arn:aws:dynamodb:us-west-2:111122223333:table/t"]
        );
        assert_eq!(*src.asked.lock().unwrap(), [None, Some("1".to_string())]);
        let r = observed(&src, "theseus-hand-read", 14, 2_000_000_000, 10)
            .await
            .unwrap();
        assert_eq!(r.used.keys().collect::<Vec<_>>(), ["s3:ListAllMyBuckets"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_window_the_page_cap_cut_is_not_complete() {
        let page = vec![event(
            "theseus-hand-owner",
            "ec2.amazonaws.com",
            "DescribeVpcs",
            &[],
            None,
        )];
        let src = Pages {
            pages: vec![page.clone(), page.clone(), page],
            asked: Mutex::default(),
        };
        let o = observed(&src, "theseus-hand-owner", 14, 2_000_000_000, 2)
            .await
            .unwrap();
        assert!(!o.complete);
        assert_eq!(o.pages, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_denied_call_is_not_a_use_and_is_counted_apart() {
        let src = Pages {
            pages: vec![vec![
                event(
                    "theseus-hand-owner",
                    "iam.amazonaws.com",
                    "ListUsers",
                    &[],
                    Some("AccessDenied"),
                ),
                event(
                    "theseus-hand-owner",
                    "ec2.amazonaws.com",
                    "RunInstances",
                    &[],
                    Some("Client.UnauthorizedOperation"),
                ),
            ]],
            asked: Mutex::default(),
        };
        let o = observed(&src, "theseus-hand-owner", 14, 2_000_000_000, 10)
            .await
            .unwrap();
        assert!(o.used.is_empty());
        assert_eq!(o.denied["iam:ListUsers"], 1);
        assert_eq!(o.denied["ec2:RunInstances"], 1);
    }

    #[test]
    fn an_events_name_becomes_the_action_that_authorised_it() {
        assert_eq!(
            action_of("s3.amazonaws.com", "ListObjectsV2"),
            "s3:ListBucket"
        );
        assert_eq!(
            action_of("monitoring.amazonaws.com", "PutMetricData"),
            "cloudwatch:PutMetricData"
        );
        assert_eq!(
            action_of("logs.amazonaws.com", "PutLogEvents"),
            "logs:PutLogEvents"
        );
    }

    #[tokio::test]
    async fn the_window_is_one_to_ninety_days() {
        let src = Pages {
            pages: vec![vec![]],
            asked: Mutex::default(),
        };
        assert!(observed(&src, "r", 0, 1, 1).await.is_err());
        assert!(observed(&src, "r", 91, 1, 1).await.is_err());
    }
}
