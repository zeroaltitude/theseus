//! The candidate: a policy that allows only what the role was seen to call,
//! never wider than the policy it replaces.
//!
//! - an observed action is allowed on the ARNs its calls named when every
//!   call named some and those ARNs sit inside the statement that allowed the
//!   action today, else on that statement's own resources;
//! - the data actions the trail cannot see ([`DATA_EVENTS`]) the policy
//!   allows today are kept as they are, and named as not visible;
//! - the hand's own actions ([`KEEP`]), taken from the code, are kept;
//! - every statement that denies is copied whole, so the guards stay.
//!
//! The first allow statement covering an action decides its resources and its
//! condition, and [`super::policy::wider`] proves the result.

use std::collections::BTreeMap;

use serde_json::Value;

use super::glob::covers;
use super::policy::{Doc, Effect, Stmt};
use super::usage::{Observed, DATA_EVENTS};

/// What a hand does itself, from the code (`hands/hand.rs`): its logs, its
/// result and logs' objects under its prefix, and the completion queue. Rare
/// for a job's own work, so the trail may not show them in a window.
pub const KEEP: &[&str] = &[
    "logs:CreateLogStream",
    "logs:PutLogEvents",
    "s3:PutObject",
    "s3:GetObject",
    "s3:ListBucket",
    "s3:AbortMultipartUpload",
    "s3:ListMultipartUploadParts",
    "sqs:SendMessage",
];

/// The most ARNs one action keeps by name; more collapse to prefixes.
const MAX_ARNS: usize = 20;

/// The services an account's catalog has, with their operation counts, for
/// the size of a policy.
pub type Universe = [(String, u32)];

/// A statement's condition and its actions, while the candidate is built.
type Group = (Option<Value>, Vec<String>);

/// A candidate and what it was built from.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub doc: Doc,
    /// Actions the policy reached before and after (about: a wildcard counts
    /// its service's operations).
    pub reach_before: u64,
    pub reach_after: u64,
    /// Data actions kept because the trail cannot show them.
    pub not_visible: Vec<String>,
    /// Hand actions kept from the code.
    pub kept: Vec<String>,
    /// Observed actions the replaced policy did not allow (another policy of
    /// the role does): nothing to keep for them.
    pub elsewhere: Vec<String>,
    /// Observed actions no policy of the role allows: they should not have
    /// succeeded; the proposal does not hide them.
    pub unexplained: Vec<String>,
}

fn collapse(arns: &std::collections::BTreeSet<String>) -> Vec<String> {
    let mut stems = arns.clone();
    let mut cut = false;
    while stems.len() > MAX_ARNS {
        let next: std::collections::BTreeSet<String> = stems
            .iter()
            .map(|s| {
                let t = s.trim_end_matches(['/', ':']);
                match t.rfind(['/', ':']) {
                    Some(i) => t[..=i].to_string(),
                    None => t.to_string(),
                }
            })
            .collect();
        if next == stems {
            break;
        }
        stems = next;
        cut = true;
    }
    stems
        .into_iter()
        .map(|s| if cut { format!("{s}*") } else { s })
        .collect()
}

/// Resources for an observed action: the ARNs the calls named, or the
/// replaced statement's own.
fn resources_for(
    arns: &std::collections::BTreeSet<String>,
    unresolved: bool,
    s: &Stmt,
) -> Vec<String> {
    if unresolved || arns.is_empty() {
        return s.resources.clone();
    }
    let named = collapse(arns);
    if named.len() > MAX_ARNS
        || !named
            .iter()
            .all(|r| s.resources.iter().any(|p| covers(p, r, false)))
    {
        return s.resources.clone();
    }
    named
}

fn cond_key(c: &Option<Value>) -> String {
    c.as_ref().map(|v| v.to_string()).unwrap_or_default()
}

/// Build the candidate. `wide` holds the statements being replaced (the
/// role's wide policies), `rest` the allows that stay attached (the hand's
/// basics): the proof is against both.
pub fn build(o: &Observed, wide: &Doc, rest: &Doc, universe: &Universe) -> Candidate {
    // (resources, condition) -> (is data, actions)
    let mut groups: BTreeMap<(Vec<String>, String, bool), Group> = BTreeMap::new();
    let mut elsewhere = Vec::new();
    let mut unexplained = Vec::new();
    for (action, u) in &o.used {
        let Some(s) = wide.covering(action) else {
            if rest.covering(action).is_some() {
                elsewhere.push(action.clone());
            } else {
                unexplained.push(action.clone());
            }
            continue;
        };
        let mut res = resources_for(&u.resources, u.unresolved, s);
        res.sort();
        let e = groups
            .entry((res, cond_key(&s.condition), false))
            .or_insert_with(|| (s.condition.clone(), Vec::new()));
        e.1.push(action.clone());
    }
    let mut not_visible = Vec::new();
    for d in DATA_EVENTS {
        if o.used.contains_key(*d) {
            continue;
        }
        if let Some(s) = wide.covering(d) {
            let mut res = s.resources.clone();
            res.sort();
            let e = groups
                .entry((res, cond_key(&s.condition), true))
                .or_insert_with(|| (s.condition.clone(), Vec::new()));
            e.1.push((*d).to_string());
            not_visible.push((*d).to_string());
        }
    }
    let mut kept = Vec::new();
    let mut keep_groups: BTreeMap<(Vec<String>, String), Group> = BTreeMap::new();
    for k in KEEP {
        let s = rest.covering(k).or_else(|| wide.covering(k));
        let Some(s) = s else { continue };
        let mut res = s.resources.clone();
        res.sort();
        let e = keep_groups
            .entry((res, cond_key(&s.condition)))
            .or_insert_with(|| (s.condition.clone(), Vec::new()));
        e.1.push((*k).to_string());
        kept.push((*k).to_string());
    }
    let mut stmts = Vec::new();
    for (i, ((res, _, data), (cond, mut acts))) in groups.into_iter().enumerate() {
        acts.sort();
        acts.dedup();
        stmts.push(Stmt {
            sid: Some(format!(
                "{}{}",
                if data { "DataNotVisible" } else { "Observed" },
                i + 1
            )),
            effect: Effect::Allow,
            actions: acts,
            resources: res,
            condition: cond,
            raw_not: None,
        });
    }
    for (i, ((res, _), (cond, acts))) in keep_groups.into_iter().enumerate() {
        stmts.push(Stmt {
            sid: Some(format!("Keep{}", i + 1)),
            effect: Effect::Allow,
            actions: acts,
            resources: res,
            condition: cond,
            raw_not: None,
        });
    }
    stmts.extend(wide.denies().cloned());
    let doc = Doc { stmts };
    // What the role reaches is the replaced policies and the ones that stay.
    let mut before = wide.allowed_actions();
    before.extend(rest.allowed_actions());
    let mut after = doc.allowed_actions();
    after.extend(rest.allowed_actions());
    Candidate {
        reach_before: reach(&before, universe),
        reach_after: reach(&after, universe),
        doc,
        not_visible,
        kept,
        elsewhere,
        unexplained,
    }
}

/// About how many actions a set of patterns reaches: a name counts one, a
/// wildcard its services' operations.
pub fn reach(patterns: &std::collections::BTreeSet<String>, universe: &Universe) -> u64 {
    let live: Vec<&String> = patterns
        .iter()
        .filter(|p| !patterns.iter().any(|q| q != *p && covers(q, p, true)))
        .collect();
    live.into_iter()
        .map(|p| {
            if p == "*" {
                return universe.iter().map(|(_, n)| u64::from(*n)).sum();
            }
            let Some((svc, act)) = p.split_once(':') else {
                return 1;
            };
            if !act.contains(['*', '?']) && !svc.contains(['*', '?']) {
                return 1;
            }
            universe
                .iter()
                .filter(|(s, _)| covers(svc, s, true))
                .map(|(_, n)| u64::from(*n))
                .sum::<u64>()
                .max(1)
        })
        .sum()
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::super::policy::wider;
    use super::super::usage::Use;
    use super::*;

    pub(crate) fn allow_all() -> Doc {
        Doc::from_value(&json!({"Statement": [
            {"Sid": "AllowAll", "Effect": "Allow", "Action": "*", "Resource": "*"},
            {"Sid": "Guards", "Effect": "Deny", "Action": ["iam:Create*"], "Resource": "*"}]}))
        .unwrap()
    }

    pub(crate) fn basics() -> Doc {
        Doc::from_value(&json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject", "s3:PutObject", "s3:AbortMultipartUpload",
              "s3:ListMultipartUploadParts"], "Resource": "arn:aws:s3:::b/hands/*"},
            {"Effect": "Allow", "Action": "s3:ListBucket", "Resource": "arn:aws:s3:::b",
             "Condition": {"StringLike": {"s3:prefix": "hands/*"}}},
            {"Effect": "Allow", "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
             "Resource": "arn:aws:logs:us-west-2:111122223333:log-group:hands:*"},
            {"Effect": "Allow", "Action": "sqs:SendMessage", "Resource": "arn:aws:sqs:us-west-2:111122223333:done"}]}))
        .unwrap()
    }

    pub(crate) fn universe() -> Vec<(String, u32)> {
        vec![
            ("ec2".into(), 700),
            ("s3".into(), 100),
            ("dynamodb".into(), 50),
            ("logs".into(), 80),
        ]
    }

    pub(crate) fn seen(pairs: &[(&str, &[&str])]) -> Observed {
        let mut o = Observed {
            role: "theseus-hand-owner".into(),
            days: 14,
            complete: true,
            ..Default::default()
        };
        for (a, arns) in pairs {
            o.used.insert(
                (*a).into(),
                Use {
                    calls: 3,
                    resources: arns.iter().map(|s| s.to_string()).collect(),
                    unresolved: arns.is_empty(),
                },
            );
        }
        o
    }

    #[test]
    fn the_candidate_allows_the_observed_actions_the_keep_list_and_the_guards() {
        let o = seen(&[
            ("ec2:DescribeInstances", &[]),
            (
                "dynamodb:DescribeTable",
                &["arn:aws:dynamodb:us-west-2:111122223333:table/t"],
            ),
        ]);
        let (wide, rest) = (allow_all(), basics());
        let c = build(&o, &wide, &rest, &universe());
        let acts = c.doc.allowed_actions();
        assert!(acts.contains("ec2:DescribeInstances"));
        assert!(acts.contains("dynamodb:DescribeTable"));
        assert!(!acts.contains("*"));
        for k in KEEP {
            assert!(acts.contains(*k), "{k} must stay");
        }
        assert_eq!(c.doc.denies().count(), 1, "the guards are copied");
        // The table's statement names the table, not `*`.
        let t = c
            .doc
            .allows()
            .find(|s| s.actions.contains(&"dynamodb:DescribeTable".to_string()))
            .unwrap();
        assert_eq!(
            t.resources,
            ["arn:aws:dynamodb:us-west-2:111122223333:table/t"]
        );
        // Data actions the policy allows are kept and named, never counted unused.
        assert!(c.not_visible.contains(&"s3:DeleteObject".to_string()));
        assert!(acts.contains("s3:DeleteObject"));
        assert!(
            c.reach_after < c.reach_before,
            "{} < {}",
            c.reach_after,
            c.reach_before
        );
        let whole = Doc {
            stmts: wide
                .stmts
                .iter()
                .chain(rest.stmts.iter())
                .cloned()
                .collect(),
        };
        assert!(wider(&c.doc, &whole).is_empty());
    }

    #[test]
    fn many_arns_collapse_to_prefixes_inside_the_current_statement() {
        let arns: Vec<String> = (0..30)
            .map(|i| format!("arn:aws:s3:::b/hands/run{i}/out"))
            .collect();
        let refs: Vec<&str> = arns.iter().map(String::as_str).collect();
        let o = seen(&[("s3:DeleteObject", &refs)]);
        let cur = Doc::from_value(&json!({"Statement": [
            {"Effect": "Allow", "Action": "s3:*", "Resource": "arn:aws:s3:::b/hands/*"}]}))
        .unwrap();
        let c = build(&o, &cur, &Doc::default(), &universe());
        let s = c
            .doc
            .allows()
            .find(|s| s.actions.contains(&"s3:DeleteObject".to_string()))
            .unwrap();
        assert_eq!(s.resources, ["arn:aws:s3:::b/hands/*"]);
        assert!(wider(&c.doc, &cur).is_empty());
    }

    #[test]
    fn arns_outside_the_current_statement_fall_back_to_its_resources() {
        let o = seen(&[("s3:GetObject", &["arn:aws:s3:::other/x"])]);
        let cur = Doc::from_value(&json!({"Statement": [
            {"Effect": "Allow", "Action": "s3:*", "Resource": "arn:aws:s3:::b/*"}]}))
        .unwrap();
        let c = build(&o, &cur, &Doc::default(), &universe());
        let s = c
            .doc
            .allows()
            .find(|s| s.actions.contains(&"s3:GetObject".to_string()))
            .unwrap();
        assert_eq!(s.resources, ["arn:aws:s3:::b/*"]);
        assert!(wider(&c.doc, &cur).is_empty());
    }

    #[test]
    fn an_action_only_the_basics_allow_is_elsewhere_and_one_no_policy_allows_is_unexplained() {
        let o = seen(&[("sqs:SendMessage", &[]), ("iam:CreateUser", &[])]);
        let cur = Doc::from_value(&json!({"Statement": [
            {"Effect": "Allow", "Action": "ec2:*", "Resource": "*"}]}))
        .unwrap();
        let c = build(&o, &cur, &basics(), &universe());
        assert_eq!(c.elsewhere, ["sqs:SendMessage"]);
        assert_eq!(c.unexplained, ["iam:CreateUser"]);
    }

    #[test]
    fn reach_counts_a_wildcard_by_its_services_operations() {
        let u = universe();
        let all: std::collections::BTreeSet<String> = ["*".to_string()].into();
        assert_eq!(reach(&all, &u), 930);
        let some: std::collections::BTreeSet<String> = [
            "ec2:*".to_string(),
            "ec2:Describe*".to_string(),
            "s3:GetObject".to_string(),
        ]
        .into();
        assert_eq!(reach(&some, &u), 701);
    }

    mod property {
        use proptest::prelude::*;

        use super::*;

        const ACTION_PATTERNS: &[&str] = &[
            "*",
            "s3:*",
            "s3:Get*",
            "s3:GetObject",
            "ec2:Describe*",
            "ec2:*",
            "dynamodb:DescribeTable",
            "dynamodb:*",
            "logs:*",
            "logs:Put*",
            "sqs:SendMessage",
            "iam:List*",
        ];
        const RESOURCE_PATTERNS: &[&str] = &[
            "*",
            "arn:aws:s3:::b/*",
            "arn:aws:s3:::b/hands/*",
            "arn:aws:s3:::b",
            "arn:aws:dynamodb:us-west-2:111122223333:table/*",
            "arn:aws:logs:*:*:log-group:hands:*",
        ];
        const USED: &[&str] = &[
            "s3:GetObject",
            "s3:ListBucket",
            "s3:DeleteObject",
            "ec2:DescribeVpcs",
            "ec2:RunInstances",
            "dynamodb:DescribeTable",
            "logs:PutLogEvents",
            "sqs:SendMessage",
            "iam:ListUsers",
        ];

        fn stmt() -> impl Strategy<Value = Value> {
            (
                proptest::sample::subsequence(ACTION_PATTERNS.to_vec(), 1..4),
                proptest::sample::subsequence(RESOURCE_PATTERNS.to_vec(), 1..3),
                any::<bool>(),
            )
                .prop_map(|(a, r, cond)| {
                    let mut s = json!({"Effect": "Allow", "Action": a, "Resource": r});
                    if cond {
                        s["Condition"] =
                            json!({"StringEquals": {"aws:RequestedRegion": "us-west-2"}});
                    }
                    s
                })
        }

        fn arn() -> impl Strategy<Value = String> {
            prop_oneof![
                (0..40u32).prop_map(|i| format!("arn:aws:s3:::b/hands/run{i}/out")),
                (0..10u32).prop_map(|i| format!("arn:aws:s3:::other/x{i}")),
                (0..5u32)
                    .prop_map(|i| format!("arn:aws:dynamodb:us-west-2:111122223333:table/t{i}")),
                (0..3u32).prop_map(|i| format!(
                    "arn:aws:logs:us-west-2:111122223333:log-group:hands:s{i}"
                )),
            ]
        }

        proptest! {
            /// Whatever the current policies and whatever the trail showed,
            /// the candidate is no wider than the role's whole policy on any
            /// statement, and it holds the keep list wherever the role has it.
            #[test]
            fn the_candidate_is_never_wider_than_the_current_policy(
                wide in proptest::collection::vec(stmt(), 1..4),
                with_basics in any::<bool>(),
                used in proptest::collection::vec(
                    (proptest::sample::select(USED.to_vec()),
                     proptest::collection::vec(arn(), 0..35), any::<bool>()), 0..8),
            ) {
                let wide = Doc::from_value(&json!({"Statement": wide})).unwrap();
                let rest = if with_basics { basics() } else { Doc::default() };
                let mut o = Observed { role: "r".into(), days: 14, complete: true, ..Default::default() };
                for (a, arns, unresolved) in used {
                    let u = o.used.entry(a.to_string()).or_default();
                    u.calls += 1;
                    u.unresolved |= unresolved || arns.is_empty();
                    u.resources.extend(arns);
                }
                let c = build(&o, &wide, &rest, &universe());
                let whole = Doc { stmts: wide.stmts.iter().chain(rest.stmts.iter()).cloned().collect() };
                prop_assert_eq!(wider(&c.doc, &whole), Vec::<String>::new());
                for k in KEEP {
                    if whole.covering(k).is_some() {
                        prop_assert!(c.doc.allows_action(k), "{} must stay", k);
                    }
                }
            }
        }
    }
}
