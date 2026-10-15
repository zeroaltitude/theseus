//! Self-narrow through the core (theseus-pw1q.15): the weekly pass and the
//! owner's answers, over a fake account (`aws::narrow::run::tests::World`:
//! CloudTrail's pages, the role's policies, the stack's template, the
//! analyzer and the simulator). The pure parts are tested in
//! `aws/narrow/*`.
//!
//! - mode off, or the switch thrown: nothing is read, written or posted;
//! - a role with evidence gets one proposal, one row and one card, and is
//!   not asked again until the owner answers and a week has passed;
//! - an apply plans the patched template and records itself, a failed one
//!   leaves the proposal open; a revert applies the template from before;
//! - a job's shell cannot answer; the week's watch tells of a denied call
//!   once and stops after the week.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfMode, SelfNarrowParams, SelfResumeParams};

use crate::approval::Answerer;
use crate::aws::narrow::live::{Applied, Stack};
use crate::aws::narrow::proposal::State;
use crate::aws::narrow::run::tests::World;
use crate::aws::narrow::usage::tests::event;
use crate::provider::FakeProvider;
use crate::rpc::{Core, Parts};
use crate::store::Store;
use crate::Config;

const DAY: u64 = 86_400_000;
const NOW: u64 = 2_000_000_000_000;

impl Stack for World {
    async fn apply(&self, body: &str, expect: &[String], _why: &str) -> Result<Applied, String> {
        if let Some(e) = &self.apply_error {
            return Err(e.clone());
        }
        self.applied
            .lock()
            .unwrap()
            .push((body.to_string(), expect.to_vec()));
        Ok(Applied {
            change_set: format!("cs-{}", self.applied.lock().unwrap().len()),
            status: "UPDATE_COMPLETE".into(),
            changed: expect.to_vec(),
        })
    }
}

fn core(mode: SelfMode, resumed: bool) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.self_improve.mode = mode;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap();
    if resumed {
        core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
            .unwrap();
    }
    (core, dir)
}

fn universe() -> Vec<(String, u32)> {
    vec![
        ("ec2".into(), 700),
        ("s3".into(), 100),
        ("dynamodb".into(), 50),
        ("logs".into(), 80),
    ]
}

fn rows(c: &Core, kind: &str) -> Vec<Value> {
    c.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

fn cards(c: &Core, kind: &str) -> usize {
    crate::tests_day_ceiling::posts(c, kind).len()
}

fn cli() -> Answerer {
    Answerer::from("the CLI")
}

async fn pass(c: &Core, w: &World, at: u64, force: bool) -> Vec<String> {
    c.narrow_pass(w, &universe(), at, None, force, false)
        .await
        .unwrap()
}

fn open_id(c: &Core) -> String {
    c.narrow_record().unwrap().proposals[0].id.clone()
}

#[tokio::test(start_paused = true)]
async fn with_the_mode_off_or_the_switch_thrown_nothing_is_read_written_or_posted() {
    for (mode, resumed) in [(SelfMode::Off, true), (SelfMode::Act, false)] {
        let (c, _d) = core(mode, resumed);
        let w = World::owner(40);
        let lines = pass(&c, &w, NOW, false).await;
        assert!(lines[0].contains("halted or off"), "{lines:?}");
        assert_eq!(*w.reads.lock().unwrap(), 0, "no page was read");
        assert!(c
            .store
            .get_meta::<Value>(super::narrow::KEY)
            .unwrap()
            .is_none());
        assert!(rows(&c, "self.narrow.proposed").is_empty());
        assert_eq!(cards(&c, "self_narrow"), 0);
        c.self_narrow_nightly().await;
        assert!(c
            .store
            .get_meta::<Value>(super::narrow::KEY)
            .unwrap()
            .is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn a_role_with_evidence_gets_one_proposal_row_and_card_and_is_not_asked_again() {
    let (c, _d) = core(SelfMode::Act, true);
    let w = World::owner(40);
    let lines = pass(&c, &w, NOW, false).await;
    assert!(
        lines
            .iter()
            .any(|l| l.contains("theseus-hand-owner: proposed")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("theseus-hand-read: no proposal")),
        "{lines:?}"
    );
    let proposed = rows(&c, "self.narrow.proposed");
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0]["role"], "theseus-hand-owner");
    assert_eq!(proposed[0]["evidence_days"], 14);
    assert!(proposed[0]["actions_after"].as_u64() < proposed[0]["actions_before"].as_u64());
    assert_eq!(cards(&c, "self_narrow"), 1);
    let text = crate::tests_day_ceiling::posts(&c, "self_narrow")[0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        text.contains("theseus self narrow apply") && text.contains("theseus self narrow decline"),
        "{text}"
    );
    // Asked again a day later: neither the week nor the open card allows it.
    let reads = *w.reads.lock().unwrap();
    let again = pass(&c, &w, NOW + DAY, false).await;
    assert!(
        again.iter().all(|l| l.contains("looked at in the last")),
        "{again:?}"
    );
    assert_eq!(*w.reads.lock().unwrap(), reads, "nothing was read");
    assert_eq!(rows(&c, "self.narrow.proposed").len(), 1);
    // A week on, the owner's open card still holds the role.
    let later = pass(&c, &w, NOW + 8 * DAY, false).await;
    assert!(
        later.iter().any(|l| l.contains("waits for the owner")),
        "{later:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn not_now_closes_the_card_and_the_role_is_looked_at_again_after_a_week() {
    let (c, _d) = core(SelfMode::Act, true);
    let w = World::owner(40);
    pass(&c, &w, NOW, false).await;
    let id = open_id(&c);
    let said = c.narrow_decline(&id, "the CLI", NOW).unwrap();
    assert!(said.contains("Not now"), "{said}");
    assert_eq!(
        c.narrow_record().unwrap().proposals[0].state,
        State::Declined
    );
    // The owner's word through the method, from the CLI, reaches the same place.
    let p = SelfNarrowParams {
        action: "decline".into(),
        proposal: Some(id.clone()),
        ..Default::default()
    };
    let e = c.self_narrow(&p, &cli()).await.unwrap_err();
    assert!(format!("{e}").contains("not waiting for an answer"), "{e}");
    assert_eq!(rows(&c, "self.narrow.declined").len(), 1);
    assert_eq!(
        pass(&c, &w, NOW + DAY, false)
            .await
            .iter()
            .filter(|l| l.contains("looked at in the last"))
            .count(),
        2
    );
    let after = pass(&c, &w, NOW + 8 * DAY, false).await;
    assert!(
        after
            .iter()
            .any(|l| l.contains("theseus-hand-owner: proposed")),
        "{after:?}"
    );
    assert_eq!(rows(&c, "self.narrow.proposed").len(), 2);
}

#[tokio::test(start_paused = true)]
async fn an_apply_plans_the_patched_template_and_a_revert_applies_the_one_from_before() {
    let (c, d) = core(SelfMode::Act, true);
    let w = World::owner(40);
    pass(&c, &w, NOW, false).await;
    let id = open_id(&c);
    let text = c
        .narrow_answer(&w, "apply", &id[id.len() - 5..], "the CLI", NOW + 1000)
        .await
        .unwrap();
    assert!(
        text.contains("Applied") && text.contains("theseus self narrow revert"),
        "{text}"
    );
    {
        let applied = w.applied.lock().unwrap();
        assert_eq!(applied.len(), 1);
        assert!(applied[0]
            .0
            .contains("ManagedPolicyName: theseus-hand-owner-narrow"));
        assert_eq!(applied[0].1, ["HandOwnerRole", "HandOwnerNarrowPolicy"]);
    }
    let rec = c.narrow_record().unwrap();
    assert_eq!(rec.proposals[0].state, State::Applied);
    assert_eq!(rows(&c, "self.narrow.applied").len(), 1);
    let before = d
        .path()
        .join("self/narrow")
        .join(format!("{id}.before.yaml"));
    assert_eq!(std::fs::read_to_string(&before).unwrap(), w.template);
    // An applied proposal is neither declined nor applied again.
    assert!(c.narrow_decline(&id, "x", NOW).is_err());
    assert!(c.narrow_answer(&w, "apply", &id, "x", NOW).await.is_err());
    // The revert applies the template as it was.
    let text = c
        .narrow_answer(&w, "revert", &id, "the CLI", NOW + 2000)
        .await
        .unwrap();
    assert!(text.contains("Reverted"), "{text}");
    assert_eq!(w.applied.lock().unwrap()[1].0, w.template);
    assert_eq!(
        c.narrow_record().unwrap().proposals[0].state,
        State::Reverted
    );
    assert_eq!(rows(&c, "self.narrow.reverted").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_apply_the_stack_refuses_leaves_the_proposal_open_and_writes_nothing() {
    let (c, _d) = core(SelfMode::Act, true);
    let mut w = World::owner(40);
    pass(&c, &w, NOW, false).await;
    let id = open_id(&c);
    w.apply_error = Some("the change set touches Boom".into());
    let e = c
        .narrow_answer(&w, "apply", &id, "the CLI", NOW)
        .await
        .unwrap_err();
    assert!(
        format!("{e}").contains("not applied") && format!("{e}").contains("Boom"),
        "{e}"
    );
    assert_eq!(c.narrow_record().unwrap().proposals[0].state, State::Open);
    assert!(rows(&c, "self.narrow.applied").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_job_shell_cannot_answer_and_the_refusal_is_ledgered() {
    let (c, _d) = core(SelfMode::Act, true);
    let w = World::owner(40);
    pass(&c, &w, NOW, false).await;
    let id = open_id(&c);
    let p = SelfNarrowParams {
        action: "decline".into(),
        proposal: Some(id),
        from_job: Some("ses_job".into()),
        ..Default::default()
    };
    let e = c.self_narrow(&p, &cli()).await.unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    assert_eq!(c.narrow_record().unwrap().proposals[0].state, State::Open);
    assert_eq!(rows(&c, "approval.refused").len(), 1);
    // Reading is anyone's.
    let list = c
        .self_narrow(&SelfNarrowParams::default(), &cli())
        .await
        .unwrap();
    assert_eq!(list.proposals.len(), 1);
    let show = c
        .self_narrow(
            &SelfNarrowParams {
                action: "show".into(),
                proposal: Some(list.proposals[0].id.clone()),
                ..Default::default()
            },
            &cli(),
        )
        .await
        .unwrap();
    assert!(show.text.contains("\"actions_before\""), "{}", show.text);
}

#[tokio::test(start_paused = true)]
async fn the_week_after_an_apply_a_denied_call_is_told_once_with_the_revert_and_the_watch_ends() {
    let (c, _d) = core(SelfMode::Act, true);
    let mut w = World::owner(40);
    pass(&c, &w, NOW, false).await;
    let id = open_id(&c);
    c.narrow_answer(&w, "apply", &id, "the CLI", NOW)
        .await
        .unwrap();
    // Nothing denied yet.
    let quiet = c.narrow_watch(&w, NOW + DAY).await.unwrap();
    assert!(quiet[0].contains("no AccessDenied"), "{quiet:?}");
    assert!(rows(&c, "self.narrow.red").is_empty());
    // A denied call after the apply, and an older one that does not count.
    let mut denied = event(
        "theseus-hand-owner",
        "s3.amazonaws.com",
        "DeleteBucket",
        &[],
        Some("AccessDenied"),
    );
    denied["EventTime"] = json!((NOW / 1000 + 100) as f64);
    let mut old = event(
        "theseus-hand-owner",
        "iam.amazonaws.com",
        "ListUsers",
        &[],
        Some("AccessDenied"),
    );
    old["EventTime"] = json!((NOW / 1000 - 100) as f64);
    w.pages = vec![vec![denied, old]];
    let red = c.narrow_watch(&w, NOW + 2 * DAY).await.unwrap();
    assert!(red[0].contains("denied s3:DeleteBucket"), "{red:?}");
    let r = rows(&c, "self.narrow.red");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["denied"], "s3:DeleteBucket");
    assert_eq!(cards(&c, "self_narrow_red"), 1);
    let card = crate::tests_day_ceiling::posts(&c, "self_narrow_red")[0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        card.contains(&format!("theseus self narrow revert {id}")),
        "{card}"
    );
    // The same denial is not told twice.
    c.narrow_watch(&w, NOW + 3 * DAY).await.unwrap();
    assert_eq!(rows(&c, "self.narrow.red").len(), 1);
    // Past the week, the watch reads nothing.
    let reads = *w.reads.lock().unwrap();
    assert!(c.narrow_watch(&w, NOW + 9 * DAY).await.unwrap().is_empty());
    assert_eq!(*w.reads.lock().unwrap(), reads);
}

#[tokio::test(start_paused = true)]
async fn a_dry_look_reads_and_prints_and_keeps_and_posts_nothing_even_with_the_mode_off() {
    let (c, _d) = core(SelfMode::Off, false);
    let w = World::owner(40);
    let lines = c
        .narrow_pass(&w, &universe(), NOW, None, true, true)
        .await
        .unwrap();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("theseus-hand-owner: would propose:")),
        "{lines:?}"
    );
    assert!(lines
        .iter()
        .any(|l| l.contains("theseus self narrow apply")));
    assert!(*w.reads.lock().unwrap() > 0, "the reads were made");
    assert!(c
        .store
        .get_meta::<Value>(super::narrow::KEY)
        .unwrap()
        .is_none());
    assert!(rows(&c, "self.narrow.proposed").is_empty());
    assert_eq!(cards(&c, "self_narrow"), 0);
}
