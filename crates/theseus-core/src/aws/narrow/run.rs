//! One role's proposal, start to end: the policies it holds, what it called,
//! the candidate, the proof that it is no wider, the template that applies
//! it, and the checks. Generic over [`Env`] (the account's CloudTrail, IAM,
//! Access Analyzer and CloudFormation in production; a fake in tests), so
//! every step is tested without a network.

use std::collections::BTreeSet;
use std::future::Future;

use theseus_aws_guard::{parse_template, Context};

use super::apply::{self, Profile};
use super::build::{build, Universe};
use super::policy::{wider, Doc};
use super::proposal::{id_of, Proposal, State};
use super::usage::{observed, Source};
use super::verify::{verify, Checker};

/// Fewer events than this over the window are noise, not evidence.
pub const MIN_EVENTS: u64 = 10;

/// What a role holds: the policies being replaced, and what stays.
#[derive(Debug, Clone, Default)]
pub struct RoleDocs {
    pub wide: Doc,
    pub wide_names: Vec<String>,
    pub rest: Doc,
}

pub trait Roles {
    /// The role's attached and inline policies, split into the wide ones
    /// and the hand basics.
    fn role_docs(&self, role: &str) -> impl Future<Output = Result<RoleDocs, String>> + Send;
    /// The hands stack's template as deployed.
    fn template(&self) -> impl Future<Output = Result<String, String>> + Send;
}

pub trait Env: Source + Checker + Roles {}
impl<T: Source + Checker + Roles> Env for T {}

/// What a run needs beside the environment.
pub struct Settings<'a> {
    pub days: u32,
    pub max_pages: u32,
    pub now_ms: u64,
    pub account: &'a str,
    pub region: &'a str,
    pub universe: &'a Universe,
}

/// A proposal, or why there is none.
#[derive(Debug)]
pub enum Made {
    Proposal(Box<Proposal>),
    Not(String),
}

fn hits(template: &str, ctx: &Context) -> Result<BTreeSet<(String, String)>, String> {
    let t = parse_template(template).map_err(|e| e.to_string())?;
    let scan = theseus_aws_guard::embedded()
        .scan(&t, ctx, &Default::default())
        .map_err(|e| e.to_string())?;
    Ok(scan
        .hits
        .iter()
        .map(|h| (h.guardrail.name.clone(), h.logical_id.clone()))
        .collect())
}

/// Propose a narrower policy for `profile`, or say why not.
pub async fn propose(env: &impl Env, profile: Profile, s: &Settings<'_>) -> Result<Made, String> {
    let not = |w: String| Ok(Made::Not(w));
    let docs = env.role_docs(profile.role).await?;
    if docs.wide.allows().next().is_none() {
        return not(format!(
            "{} holds only the hand basics: there is nothing wide to narrow",
            profile.role
        ));
    }
    let o = observed(env, profile.role, s.days, s.now_ms / 1000, s.max_pages).await?;
    if !o.complete {
        return not(format!(
            "the history was cut at {} pages before the window's start: no proposal from partial evidence",
            o.pages
        ));
    }
    if o.events < MIN_EVENTS {
        return not(format!(
            "{} made {} calls in {} days: too few to say what it needs",
            profile.role, o.events, s.days
        ));
    }
    let cand = build(&o, &docs.wide, &docs.rest, s.universe);
    let whole = Doc {
        stmts: docs
            .wide
            .stmts
            .iter()
            .chain(docs.rest.stmts.iter())
            .cloned()
            .collect(),
    };
    let wide_by = wider(&cand.doc, &whole);
    if !wide_by.is_empty() {
        return Err(format!(
            "the candidate is wider than today's policy ({}): a defect, nothing proposed",
            wide_by[0]
        ));
    }
    if cand.reach_after >= cand.reach_before {
        return not("the role's policy is already as narrow as its calls".into());
    }
    let value = cand.doc.to_value();
    let template = env.template().await?;
    let patched = match apply::patch(&template, profile, &value) {
        Ok(p) => p,
        Err(e) => return not(format!("the hands stack's template cannot take it: {e}")),
    };
    let ctx = Context {
        account: s.account.into(),
        region: s.region.into(),
    };
    let before = hits(&template, &ctx)?;
    let new: Vec<_> = hits(&patched, &ctx)?.difference(&before).cloned().collect();
    if !new.is_empty() {
        return not(format!(
            "the guards would stop the template that applies it: {} on {}",
            new[0].0, new[0].1
        ));
    }
    let v = verify(env, &o, &cand, &docs.rest).await?;
    if let Some(why) = v.why_not() {
        return not(why);
    }
    let mut analyzer: Vec<String> = v
        .listed
        .iter()
        .map(|f| format!("{} {}: {}", f.kind, f.code, f.detail))
        .collect();
    analyzer.sort();
    Ok(Made::Proposal(Box::new(Proposal {
        id: id_of(profile.role, &value),
        role: profile.role.into(),
        profile: profile.name.into(),
        at_ms: s.now_ms,
        days: s.days,
        events: o.events,
        pages: o.pages,
        reach_before: cand.reach_before,
        reach_after: cand.reach_after,
        wide_names: docs.wide_names,
        current: docs.wide.to_value(),
        candidate: value,
        not_visible: cand.not_visible,
        kept: cand.kept,
        elsewhere: cand.elsewhere,
        unexplained: cand.unexplained,
        analyzer,
        simulated: v.simulated,
        observed: o.used.keys().cloned().collect(),
        state: State::Open,
        applied_ms: None,
        change_set: None,
        red: Vec::new(),
    })))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use serde_json::{json, Value};

    use super::super::apply::PROFILES;
    use super::super::build::tests::{allow_all, basics, universe};
    use super::super::usage::tests::event;
    use super::super::verify::{Answer, Call, Finding};
    use super::*;

    pub(crate) const HANDS: &str = include_str!("../../../../../infra/aws/theseus-hands.yaml");

    /// A whole fake account: pages of events, the role's policies, the
    /// stack's template, and the analyzer's findings.
    pub(crate) struct World {
        pub pages: Vec<Vec<Value>>,
        pub wide: Doc,
        pub rest: Doc,
        pub template: String,
        pub findings: Vec<Finding>,
        pub simulator_denies: bool,
        pub reads: Mutex<u32>,
        /// The templates the stack was asked to apply, with the ids the owner expected to change.
        pub applied: Mutex<Vec<(String, Vec<String>)>>,
        pub apply_error: Option<String>,
    }

    impl World {
        pub(crate) fn owner(events: usize) -> World {
            let page: Vec<Value> = (0..events)
                .map(|i| {
                    if i % 2 == 0 {
                        event(
                            "theseus-hand-owner",
                            "ec2.amazonaws.com",
                            "DescribeInstances",
                            &[],
                            None,
                        )
                    } else {
                        event(
                            "theseus-hand-owner",
                            "ec2.amazonaws.com",
                            "DescribeVpcs",
                            &[],
                            None,
                        )
                    }
                })
                .collect();
            World {
                pages: vec![page],
                wide: allow_all(),
                rest: basics(),
                template: HANDS.into(),
                findings: vec![],
                simulator_denies: false,
                reads: Mutex::default(),
                applied: Mutex::default(),
                apply_error: None,
            }
        }
    }

    impl Source for World {
        async fn page(
            &self,
            _s: u64,
            _e: u64,
            t: Option<&str>,
        ) -> Result<(Vec<Value>, Option<String>), String> {
            *self.reads.lock().unwrap() += 1;
            let i: usize = t.map_or(0, |t| t.parse().unwrap());
            Ok((
                self.pages[i].clone(),
                (i + 1 < self.pages.len()).then(|| (i + 1).to_string()),
            ))
        }
    }

    impl Checker for World {
        async fn validate(&self, _d: &Value) -> Result<Vec<Finding>, String> {
            Ok(self.findings.clone())
        }
        async fn simulate(&self, _p: &[Value], calls: &[Call]) -> Result<Vec<Answer>, String> {
            Ok(calls
                .iter()
                .map(|c| Answer {
                    action: c.action.clone(),
                    resource: None,
                    decision: if self.simulator_denies {
                        "implicitDeny"
                    } else {
                        "allowed"
                    }
                    .into(),
                    missing: vec![],
                })
                .collect())
        }
    }

    impl Roles for World {
        async fn role_docs(&self, _r: &str) -> Result<RoleDocs, String> {
            Ok(RoleDocs {
                wide: self.wide.clone(),
                wide_names: vec!["theseus-allow-all".into()],
                rest: self.rest.clone(),
            })
        }
        async fn template(&self) -> Result<String, String> {
            Ok(self.template.clone())
        }
    }

    pub(crate) fn settings(u: &Universe) -> Settings<'_> {
        Settings {
            days: 14,
            max_pages: 10,
            now_ms: 2_000_000_000_000,
            account: "111122223333",
            region: "us-west-2",
            universe: u,
        }
    }

    async fn run(w: &World) -> Made {
        let u = universe();
        propose(w, PROFILES[0], &settings(&u)).await.unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_role_with_evidence_gets_a_smaller_policy_proposed() {
        let w = World::owner(40);
        let Made::Proposal(p) = run(&w).await else {
            panic!("a proposal")
        };
        assert_eq!(p.role, "theseus-hand-owner");
        assert_eq!(p.events, 40);
        assert!(p.reach_after < p.reach_before);
        assert_eq!(p.observed, ["ec2:DescribeInstances", "ec2:DescribeVpcs"]);
        assert_eq!(p.simulated, 2);
        assert_eq!(p.state, State::Open);
    }

    #[tokio::test(start_paused = true)]
    async fn too_few_events_a_cut_window_and_an_idle_role_propose_nothing() {
        let few = World::owner(3);
        let Made::Not(w) = run(&few).await else {
            panic!()
        };
        assert!(w.contains("too few"), "{w}");
        let mut cut = World::owner(40);
        cut.pages = vec![cut.pages[0].clone(); 30];
        let u = universe();
        let mut s = settings(&u);
        s.max_pages = 3;
        let Made::Not(w) = propose(&cut, PROFILES[0], &s).await.unwrap() else {
            panic!()
        };
        assert!(w.contains("partial evidence"), "{w}");
        assert_eq!(*cut.reads.lock().unwrap(), 3, "the read budget holds");
    }

    #[tokio::test(start_paused = true)]
    async fn an_analyzer_error_or_a_denying_simulator_blocks_the_proposal() {
        let mut w = World::owner(40);
        w.findings = vec![Finding {
            kind: "ERROR".into(),
            code: "INVALID_ACTION".into(),
            detail: "no such action".into(),
        }];
        let Made::Not(why) = run(&w).await else {
            panic!()
        };
        assert!(why.contains("Access Analyzer ERROR"), "{why}");
        let mut w = World::owner(40);
        w.simulator_denies = true;
        let Made::Not(why) = run(&w).await else {
            panic!()
        };
        assert!(why.contains("would deny an observed call"), "{why}");
        let mut w = World::owner(40);
        w.findings = vec![Finding {
            kind: "SUGGESTION".into(),
            code: "S".into(),
            detail: "d".into(),
        }];
        let Made::Proposal(p) = run(&w).await else {
            panic!("suggestions only list")
        };
        assert_eq!(p.analyzer.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_role_that_holds_only_the_basics_or_a_template_of_another_shape_is_left_alone() {
        let mut w = World::owner(40);
        w.wide = Doc::default();
        let Made::Not(why) = run(&w).await else {
            panic!()
        };
        assert!(why.contains("nothing wide"), "{why}");
        let mut w = World::owner(40);
        w.template = HANDS.replace("  HandOwnerRole:", "  HandOwnerRoleX:");
        let Made::Not(why) = run(&w).await else {
            panic!()
        };
        assert!(why.contains("template cannot take it"), "{why}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_role_already_as_narrow_as_its_calls_gets_no_proposal() {
        let mut w = World::owner(40);
        w.wide = Doc::from_value(&json!({"Statement": [
            {"Effect": "Allow", "Action": ["ec2:DescribeInstances", "ec2:DescribeVpcs"], "Resource": "*"}]})).unwrap();
        let Made::Not(why) = run(&w).await else {
            panic!()
        };
        assert!(
            why.contains("already as narrow") || why.contains("reach"),
            "{why}"
        );
    }
}
