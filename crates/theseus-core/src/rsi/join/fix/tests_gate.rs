//! The fix at the gate, played through the whole core (issue 36): a stand-in model calls
//! `self.propose_change`, `tests_tree`'s throwaway repository's stand-in gate judges, and a
//! stand-in fixer and approver ([`StandIn`]) answer. Each case ends where the issue says: a red
//! lint repaired, approved by the other model, reviewed and joined; and each refusal, rejection and
//! red that ends as a failed try, one try and no more, with a fix's words.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfMode;
use theseus_protocol::tasks::{TaskEvidence, TaskOrigin, TaskRecord, TaskState};

use super::tests::{scripted, signing, StandIn};
use crate::rsi::backlog::attempts;
use crate::rsi::join::tests::Scripts;
use crate::rsi::pipeline::main_red;
use crate::rsi::pr::GateSeams;
use crate::rsi::tests_pr::{rig_on, Rig};
use crate::rsi::tests_tree::{fixture_with, git};
use crate::rsi::tree::Place;
use crate::Config;

pub(super) const WIRE: &str = "self_propose_change";
pub(super) const ITEM: &str = "trim-the-sails";

/// Red in clippy while `lint.txt` is in the tree and `lint-fixed.txt` is not, red in the suite at
/// `broken.txt`, a keel stop at `keel.txt`, a fault of the machine at `machine.txt` (exit 127, the
/// shell's "not found"); and a second long, as a gate that judged something is.
pub(super) const GATE: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
sleep 1
if [ -f keel.txt ]; then echo "gate: FAILED in keel"; exit 1; fi
if [ -f machine.txt ]; then echo "gate: FAILED in suite"; exit 127; fi
if [ -f lint.txt ] && [ ! -f lint-fixed.txt ]; then echo "warning: unused variable x"; echo "gate: FAILED in clippy"; exit 1; fi
if [ -f broken.txt ]; then echo "gate: FAILED in suite"; exit 1; fi
echo "gate: ok"
"#;

/// The change: a lint the gate fails on, and a fix with its test.
pub(super) const LINTED: &[(&str, &str)] = &[
    ("lint.txt", "let x = 1;\n"),
    ("fix.txt", "fixed\n"),
    ("tests/fix.sh", "test -f fix.txt\n"),
];
/// The fixer's repair of it.
pub(super) const REPAIR: &[(&str, &str)] = &[("lint-fixed.txt", "let _x = 1;\n")];

/// Main's last gate was red.
struct RedMain;

impl main_red::Main for RedMain {
    fn red(&self, _: &Place) -> Option<main_red::Red> {
        Some(main_red::Red {
            commit: "0badc0de".into(),
            at_ms: 1_792_000_000_000,
        })
    }
}

pub(super) fn rig(edit: impl FnOnce(&mut Config)) -> Rig {
    rig_on(fixture_with(GATE), SelfMode::Act, Some("origin"), |c| {
        c.self_improve.reviewer_profile = Some("fable".into());
        edit(c);
    })
}

pub(super) fn input() -> Value {
    json!({"item": ITEM, "issue": "theseus-sail1", "why": "Trim the sails before the wind."})
}

/// The item a builder works on: a self task in progress in its session.
pub(super) fn item(r: &Rig) -> String {
    let sid = r.session("self");
    let t = TaskRecord {
        id: ITEM.into(),
        version: 1,
        title: "Trim the sails".into(),
        state: TaskState::InProgress,
        session: Some(sid.clone()),
        origin: TaskOrigin {
            session: "self:backlog".into(),
            principal: "self".into(),
            ..Default::default()
        },
        evidence: vec![TaskEvidence {
            node: None,
            identity: format!("session:{sid}"),
            note: None,
            by: "self-backlog".into(),
            at_ms: 1,
        }],
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    sid
}

pub(super) fn tree_of(r: &Rig) -> PathBuf {
    r.f.place.trees().canonicalize().unwrap().join(ITEM)
}

pub(super) fn write(dir: &Path, files: &[(&str, &str)]) {
    for (p, t) in files {
        let p = dir.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    }
}

/// A build with `fixer` and `main` as the fix's seams: the tree opened, `files` written, the
/// attempt made. Its session and what the builder read.
pub(super) async fn build(
    r: &Rig,
    fixer: &Arc<StandIn>,
    main: Arc<dyn main_red::Main>,
    files: &[(&str, &str)],
) -> (String, String) {
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: fixer.clone(),
        main,
    });
    let sid = item(r);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("Tree open"), "{said}");
    write(&tree_of(r), files);
    let said = r.turn(&sid, WIRE, input()).await;
    (sid, said)
}

pub(super) fn green() -> Arc<dyn main_red::Main> {
    Arc::new(main_red::Unrecorded)
}

pub(super) fn rows(r: &Rig, kind: &str) -> Vec<Value> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

fn fix_rows(r: &Rig) -> usize {
    super::KINDS.iter().map(|k| rows(r, k).len()).sum()
}

/// The item's tries, and its last gate word's note.
fn tries(r: &Rig) -> (usize, String) {
    let t = crate::task_graph::get(&r.core.store, ITEM)
        .unwrap()
        .unwrap();
    let note = t
        .evidence
        .iter()
        .rev()
        .find(|e| e.identity.starts_with("gate:"))
        .and_then(|e| e.note.clone())
        .unwrap_or_default();
    (attempts::tries(&t, 0).len(), note)
}

pub(super) fn built(r: &Rig) -> Value {
    rows(r, "self.branch.built").pop().expect("a built row")
}

/// The whole of the gate's path: a red lint is given to the reviewer's profile (fable), which
/// repairs it in the tree; the code commits it; the gate runs again and is green; the builder's
/// profile (sonnet) reads the fix alone and accepts it; the branch goes on green, uses no try, is
/// reviewed by another model, its planted revert fails as it should, and it joins main.
#[tokio::test]
async fn a_red_lint_is_fixed_approved_by_the_other_model_reviewed_and_joined() {
    let r = rig(|_| {});
    let fixer = StandIn::writing(REPAIR);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is green"), "{said}");
    assert!(said.contains("A fix was tried at the gate"), "{said}");
    // The fixer was the reviewer's profile, given the tree, the failure and the diff.
    let ask = fixer.fixes.lock().unwrap()[0].clone();
    assert_eq!(ask.target.profile, "fable");
    assert!(
        ask.failure.contains("gate: FAILED in clippy"),
        "{}",
        ask.failure
    );
    assert!(ask.diff.contains("+let x = 1;"), "{}", ask.diff);
    assert_eq!(Path::new(&ask.tree), tree_of(&r));
    let started = rows(&r, "self.fix.started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0]["point"], "gate");
    assert_eq!(started[0]["fixer"], "fable (claude-fable-5-2)");
    let made = rows(&r, "self.fix.made").pop().unwrap();
    assert_eq!(
        (made["lines"].as_u64(), made["files"].as_u64()),
        (Some(1), Some(1))
    );
    // The builder's profile read the fix's commit alone.
    let approval = fixer.approvals.lock().unwrap()[0].clone();
    assert_eq!(approval.target.model, "claude-sonnet-5-5");
    assert!(approval.diff.contains("+let _x = 1;"), "{}", approval.diff);
    assert!(
        !approval.diff.contains("tests/fix.sh"),
        "the fix alone: {}",
        approval.diff
    );
    let v = rows(&r, "self.fix.verdict").pop().unwrap();
    assert_eq!(v["verdict"], "accept");
    assert_eq!(v["approver"], "sonnet (claude-sonnet-5-5)");
    // The branch is green at the fix's head, its report says so, and the commit says what it is.
    let b = built(&r);
    assert_eq!(b["gate"], "green");
    assert_eq!(b["head"], made["head"]);
    let report = std::fs::read_to_string(b["report"].as_str().unwrap()).unwrap();
    for want in [
        "## The fix",
        "for a red in clippy",
        "Fixer: fable (claude-fable-5-2)",
        "Approved by: sonnet (claude-sonnet-5-5)",
    ] {
        assert!(report.contains(want), "{want}: {report}");
    }
    let head = made["head"].as_str().unwrap();
    let subject = git(&r.f.place.repo, &["log", "-1", "--format=%s", head]);
    assert_eq!(
        subject,
        "self: fix at the gate by claude-fable-5-2 for clippy (trim-the-sails)"
    );
    assert_eq!(tries(&r).0, 0, "a fix uses no try");
    // Reviewed by another model, proved, and joined.
    signing(&r.f.place.repo);
    let review = scripted(&["accept"]);
    let seams = crate::rsi::join::Seams {
        reviewer: review.clone(),
        tests: Arc::new(Scripts),
        lock_every: std::time::Duration::from_millis(50),
        lock_wait: std::time::Duration::from_secs(60),
        ..crate::rsi::join::Seams::default()
    };
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    let branch = b["branch"].as_str().unwrap().to_string();
    assert_eq!(pass.reviewed, [(branch, "accept".to_string())], "{pass:?}");
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(
        rows(&r, "self.review.verdict")[0]["planted_revert"],
        "failed"
    );
    let merge = &pass.joined[0].1;
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", merge]);
    assert!(parents.ends_with(head), "the fix's head joined: {parents}");
}

/// Over the cap: the commit is refused before the gate runs again, the attempt is red and uses
/// one try, and its next attempt starts from before the fix.
#[tokio::test]
async fn a_fix_over_the_cap_is_refused_and_the_attempt_is_a_failed_try() {
    let r = rig(|c| c.self_fix.lines = 2);
    let fixer = StandIn::writing(&[("lint-fixed.txt", "a\nb\nc\n")]);
    let (sid, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["bound"], "size");
    assert!(refused[0]["why"]
        .as_str()
        .unwrap()
        .contains("at most 2 lines"));
    assert_eq!(fixer.approved(), 0, "nothing to approve");
    assert_eq!(built(&r)["gate"], "red");
    let (n, note) = tries(&r);
    assert_eq!(n, 1, "one try");
    assert!(
        note.contains("a fix was tried at the gate and refused"),
        "{note}"
    );
    // The tree is put back to before the fix as soon as it is refused, before the builder reads
    // the red: nothing of the fixer's is in it, and the builder's next attempt starts from it.
    let before = refused[0]["from"].as_str().unwrap().to_string();
    let tree = tree_of(&r);
    assert_eq!(git(&tree, &["rev-parse", "HEAD"]), before);
    assert!(
        !tree.join("lint-fixed.txt").exists(),
        "the fixer's lines are gone"
    );
    assert_eq!(built(&r)["head"], before.as_str());
    r.turn(&sid, WIRE, input()).await;
    let next = git(&tree_of(&r), &["log", "--format=%H", "-1", "HEAD"]);
    assert_eq!(
        next, before,
        "the next attempt starts where the tree was put back"
    );
}

/// A fix that touches the loop's own judge, or a path the keel holds, is refused.
#[tokio::test]
async fn a_fix_that_touches_a_guarded_path_is_refused() {
    let r = rig(|_| {});
    let mut writes = REPAIR.to_vec();
    // The pipeline is the loop's own judge, and in no class of the keel's table.
    writes.push((
        "crates/theseus-core/src/rsi/pipeline/queues.rs",
        "// looser\n",
    ));
    let fixer = StandIn::writing(&writes);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "guarded-path", "{refused:?}");
    assert!(refused[0]["why"].as_str().unwrap().contains("own judge"));
    assert_eq!(built(&r)["gate"], "red");
    assert_eq!(tries(&r).0, 1);
}

/// Main holds a test before the branch: a fix that edits it is refused, whatever it does to it.
#[tokio::test]
async fn a_fix_that_edits_a_test_that_existed_before_is_refused() {
    let r = rig(|_| {});
    let root = r.f.upstream.parent().unwrap().to_path_buf();
    let work = root.join("old-test");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    write(&work, &[("tests/old.sh", "test -f README.md\n")]);
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "an old test"]);
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
    let mut writes = REPAIR.to_vec();
    writes.push(("tests/old.sh", "true\n"));
    let fixer = StandIn::writing(&writes);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "tests", "{refused:?}");
    assert!(refused[0]["why"].as_str().unwrap().contains("tests/old.sh"));
    assert_eq!(tries(&r).0, 1);
}

/// The builder's model rejects the fix: the attempt is a failed try, with its words.
#[tokio::test]
async fn a_fix_the_other_model_rejects_is_a_failed_try_with_its_words() {
    let r = rig(|_| {});
    let fixer = Arc::new(StandIn {
        approve: false,
        ..Arc::try_unwrap(StandIn::writing(REPAIR)).ok().unwrap()
    });
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert!(said.contains("it changes what the change is for"), "{said}");
    let v = rows(&r, "self.fix.verdict").pop().unwrap();
    assert_eq!(v["verdict"], "reject");
    let (n, note) = tries(&r);
    assert_eq!(n, 1);
    assert!(
        note.contains("rejected it: it changes what the change is for"),
        "{note}"
    );
}

/// Red again after the fix: a failed try, and no second fix.
#[tokio::test]
async fn a_second_red_after_a_fix_is_a_failed_try_and_no_second_fix() {
    let r = rig(|_| {});
    let fixer = StandIn::writing(&[("notes.txt", "looked at it\n")]);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert_eq!(fixer.fixed(), 1, "one fix, never two");
    assert_eq!(rows(&r, "self.fix.started").len(), 1);
    let none = rows(&r, "self.fix.none").pop().unwrap();
    assert!(
        none["why"].as_str().unwrap().contains("red again"),
        "{none}"
    );
    assert_eq!(
        fixer.approved(),
        0,
        "no model call for a fix that did not pass"
    );
    assert_eq!(tries(&r).0, 1);
}

/// The fixer and the builder are one model: no fix is tried, and it is said once.
#[tokio::test]
async fn one_model_on_both_profiles_tries_no_fix_and_says_so_once() {
    let r = rig(|c| c.self_improve.reviewer_profile = Some("sonnet".into()));
    let fixer = StandIn::writing(REPAIR);
    let (sid, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    // Another attempt, red again.
    r.turn(&sid, WIRE, input()).await;
    assert_eq!(fixer.fixed(), 0, "never tried");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused.len(), 1, "said once: {refused:?}");
    assert_eq!(refused[0]["bound"], "not-independent");
    assert_eq!(rows(&r, "self.fix.started").len(), 0);
}

/// `[self_fix] mode = "off"`: none of it runs.
#[tokio::test]
async fn with_fix_off_nothing_of_it_runs() {
    let r = rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    let fixer = StandIn::writing(REPAIR);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert!(!said.contains("fix"), "{said}");
    assert_eq!(fixer.fixed(), 0);
    assert_eq!(fix_rows(&r), 0);
    assert_eq!(tries(&r).0, 1);
}

/// A case: its name, main's last gate, and the change.
type Case = (
    &'static str,
    Arc<dyn main_red::Main>,
    &'static [(&'static str, &'static str)],
);

/// A red main, a fault of the machine and a keel stop are never given to a fixer.
#[tokio::test]
async fn a_red_main_a_machine_fault_and_a_keel_stop_are_never_a_fixers() {
    let cases: [Case; 3] = [
        ("red main", Arc::new(RedMain), LINTED),
        (
            "machine",
            green(),
            &[("machine.txt", "x\n"), ("lint.txt", "x\n")],
        ),
        ("keel", green(), &[("keel.txt", "x\n"), ("lint.txt", "x\n")]),
    ];
    for (name, main, files) in cases {
        let r = rig(|_| {});
        let fixer = StandIn::writing(REPAIR);
        let (_, said) = build(&r, &fixer, main, files).await;
        assert!(!said.contains("the gate is green"), "{name}: {said}");
        assert_eq!(fixer.fixed(), 0, "{name}");
        assert_eq!(fix_rows(&r), 0, "{name}");
    }
}

/// The fixer may not touch the branch's history: only the code commits a fix.
#[tokio::test]
async fn a_fixer_that_commits_itself_is_refused() {
    let r = rig(|_| {});
    let fixer = Arc::new(StandIn {
        git: Some(
            [
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "mine",
            ]
            .map(str::to_string)
            .to_vec(),
        ),
        ..Arc::try_unwrap(StandIn::writing(REPAIR)).ok().unwrap()
    });
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "history", "{refused:?}");
    assert_eq!(rows(&r, "self.fix.made").len(), 0);
}

/// The fixer's and the approver's model calls are the self budget's, reserved and settled as the
/// review's are: each is a turn in a `"self"` session of its own.
#[tokio::test]
async fn the_fixers_and_the_approvers_calls_are_reserved_and_settled() {
    use super::Fixer;
    let r = rig(|_| {});
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: Arc::new(super::turns::Turns),
        main: green(),
    });
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    write(&tree_of(&r), LINTED);
    r.turn(&sid, WIRE, input()).await;
    // The stand-in model answers "Done.": no FIX line, so no fix, from a turn that was paid for.
    let none = rows(&r, "self.fix.none")
        .pop()
        .expect("the fixer's turn ran");
    let fixer_session = none["session"].as_str().unwrap().to_string();
    let paid = |session: &str| {
        let on = |k: &str| {
            rows(&r, k)
                .iter()
                .filter(|v| v["branch"] == session)
                .count()
        };
        (on("self.budget.reserved"), on("self.budget.settled"))
    };
    let (reserved, settled) = paid(&fixer_session);
    assert!(reserved > 0 && reserved == settled, "{reserved} {settled}");
    let live = r.core.live_profile().0;
    let ask = super::ApproveAsk {
        item: ITEM.into(),
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        point: super::Point::Gate,
        diff: "+x".into(),
        reason: "it repairs it".into(),
        failure: "gate: FAILED in clippy".into(),
        fixer: "fable (claude-fable-5-2)".into(),
        target: r
            .core
            .runner
            .resolve_target(&live, Some("sonnet"), None, None)
            .unwrap(),
    };
    let review = super::turns::Turns.approve(&r.core, &ask).await.unwrap();
    assert!(!review.accept, "no verdict line is a reject");
    let (reserved, settled) = paid(review.session.as_deref().unwrap());
    assert!(reserved > 0 && reserved == settled, "{reserved} {settled}");
}

/// A build whose red is being fixed is named so in `theseus self pipeline`: where, and by which
/// model; and its fix's end ends the word.
#[tokio::test]
async fn a_build_being_fixed_at_its_gate_is_named_so() {
    let r = rig(|_| {});
    let filed = r
        .core
        .self_file(
            &theseus_protocol::rsi::SelfFileParams {
                finding: "The sails are not trimmed before the wind, so the ship loses speed."
                    .into(),
                title: Some("Trim the sails".into()),
                ..Default::default()
            },
            &crate::approval::Answerer::from("the CLI"),
        )
        .unwrap();
    // Its build began now, so it is building (within the grace) whatever its turn does.
    let sid = r.session("self");
    let mut t = crate::task_graph::get(&r.core.store, &filed.item)
        .unwrap()
        .unwrap();
    t.state = TaskState::InProgress;
    t.session = Some(sid.clone());
    t.version += 1;
    t.evidence.push(TaskEvidence {
        node: None,
        identity: format!("session:{sid}"),
        note: None,
        by: "self-backlog".into(),
        at_ms: theseus_protocol::now_unix_ms(),
    });
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let m = crate::fact::rsi_fix::Moved::new(&filed.item, "given to a fixer")
        .with("branch", "self/20261120-trim-the-sails")
        .with("head", "abc")
        .with("point", "gate")
        .with("fixer", "fable (claude-fable-5-2)");
    r.core
        .rec(None)
        .record(&crate::fact::rsi_fix::Started(m.clone()));
    let params = theseus_protocol::rsi::SelfPipelineParams::default();
    let view = r.core.self_pipeline(&params).unwrap();
    assert_eq!(view.building.len(), 1, "{view:?}");
    assert_eq!(
        view.building[0].fixing.as_deref(),
        Some("at the gate, by fable (claude-fable-5-2)")
    );
    r.core
        .rec(None)
        .record(&crate::fact::rsi_fix::Refused(m.with("bound", "size")));
    let view = r.core.self_pipeline(&params).unwrap();
    assert_eq!(view.building[0].fixing, None);
}

/// A branch is fixed once at each point: asked again at the same point of the same branch (a
/// restart, a second call), the fix is refused before any model is called.
#[tokio::test]
async fn a_second_fix_at_one_point_of_a_branch_is_refused() {
    let r = rig(|_| {});
    let fixer = StandIn::writing(&[("notes.txt", "looked at it\n")]);
    let (sid, _) = build(&r, &fixer, green(), LINTED).await;
    assert_eq!(fixer.fixed(), 1);
    let t = crate::rsi::tree::find(&r.f.place, ITEM).expect("the red attempt's tree stays");
    let main = crate::rsi::join::git::main_of(&r.f.place, "origin").unwrap();
    let job = super::make::Job {
        core: &r.core,
        place: &r.f.place,
        tree: &t,
        item: ITEM,
        point: super::Point::Gate,
        builder: Some(&sid),
        why: "Trim the sails before the wind.",
        fault: "clippy".into(),
        failure: "gate: FAILED in clippy".into(),
        main,
        fixer: fixer.clone(),
    };
    let why = job.make().await.unwrap_err();
    assert!(why.contains("tried already"), "{why}");
    assert_eq!(fixer.fixed(), 1, "no model was called");
    assert_eq!(
        rows(&r, "self.fix.refused").pop().unwrap()["bound"],
        "second"
    );
}

// ---------------------------------------------------------------- round 2 (review B1, B2, notes)

/// The green `self.branch.built` rows: (session, head).
pub(super) fn green_builds(r: &Rig) -> Vec<(Option<String>, String)> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "self.branch.built" && row.data["gate"] == "green")
        .map(|(_, row)| {
            let head = row.data["head"].as_str().unwrap_or("").to_string();
            (row.session_id, head)
        })
        .collect()
}

/// The tool names a request offered.
fn offered(q: &crate::provider::ProviderRequest) -> Vec<String> {
    q.tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

/// The first request whose newest user message says `words`: the first request of the turn that
/// input opens. Not its first message: a fixer reads at the widest, so its place's earlier
/// exchanges come first.
fn request_of(r: &Rig, words: &str) -> crate::provider::ProviderRequest {
    r.requests()
        .into_iter()
        .find(|q| crate::tests_places::last_user(q).contains(words))
        .unwrap_or_else(|| panic!("no request says {words:?}"))
}

/// A fixer that writes the repair and then runs the core's REAL fixer turn, whose stand-in
/// model makes whatever call the test queued; it approves as its stand-in does.
struct Proposing(Arc<StandIn>);

impl super::Fixer for Proposing {
    fn fix<'a>(
        &'a self,
        core: &'a Arc<crate::rpc::Core>,
        ask: &'a super::FixAsk,
    ) -> super::FixFut<'a> {
        write(Path::new(&ask.tree), REPAIR);
        super::Fixer::fix(&super::turns::Turns, core, ask)
    }
    fn approve<'a>(
        &'a self,
        core: &'a Arc<crate::rpc::Core>,
        ask: &'a super::ApproveAsk,
    ) -> crate::rsi::join::review::Fut<'a> {
        self.0.approve(core, ask)
    }
}

/// Review round 1, B1: the fixer's own turn calls `self.propose_change` for the item whose tree
/// it was lent. It is not offered the verb (nor any self verb: only its tree's file and command
/// tools, and the history tool, which acts on nothing), its call is refused, and no green branch
/// exists that a session other than the builder's built.
#[tokio::test]
async fn a_fixer_turn_that_calls_propose_change_builds_no_branch() {
    let r = rig(|_| {});
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: Arc::new(Proposing(StandIn::writing(&[]))),
        main: green(),
    });
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    write(&tree_of(&r), LINTED);
    // Taken by the first request after the builder's own: the fixer's turn.
    r.queue_call(WIRE, input());
    r.turn(&sid, WIRE, input()).await;
    let tools = offered(&request_of(&r, "You are the fixer"));
    assert!(!tools.iter().any(|n| n == WIRE), "{tools:?}");
    for n in &tools {
        let family = n.split('_').next().unwrap_or(n);
        assert!(
            ["fs", "proc", "git", "job", "tools"].contains(&family) || n == "context_history",
            "a fixer is offered {n}: {tools:?}"
        );
    }
    assert!(tools.iter().any(|n| n == "fs_write"), "{tools:?}");
    for (s, _) in green_builds(&r) {
        assert_eq!(
            s.as_deref(),
            Some(sid.as_str()),
            "built by a session not the builder's"
        );
    }
    let said = serde_json::to_string(&r.requests()).unwrap();
    assert!(
        said.contains("is not offered in a fixer's session"),
        "the gate refused it"
    );
}

/// The same, with the role's ceiling switched off: `self.propose_change` refuses a role's session
/// itself, whatever the session was offered.
#[tokio::test]
async fn propose_change_refuses_a_fixers_session_whatever_it_was_offered() {
    let r = rig(|_| {});
    r.core.tools.self_pr.set_role_ceilings_off();
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: Arc::new(Proposing(StandIn::writing(&[]))),
        main: green(),
    });
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    write(&tree_of(&r), LINTED);
    r.queue_call(WIRE, input());
    r.turn(&sid, WIRE, input()).await;
    let tools = offered(&request_of(&r, "You are the fixer"));
    assert!(
        tools.iter().any(|n| n == WIRE),
        "the ceiling is off: {tools:?}"
    );
    let said = serde_json::to_string(&r.requests()).unwrap();
    assert!(
        said.contains("this session is a fixer of a self branch"),
        "propose refused it itself"
    );
    for (s, _) in green_builds(&r) {
        assert_eq!(s.as_deref(), Some(sid.as_str()));
    }
}

/// An approver's turn is offered no tool, and its call of the builder's verb builds nothing.
#[tokio::test]
async fn an_approvers_turn_is_offered_nothing_and_builds_no_branch() {
    use super::Fixer;
    let r = rig(|_| {});
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    write(&tree_of(&r), LINTED);
    let live = r.core.live_profile().0;
    let ask = super::ApproveAsk {
        item: ITEM.into(),
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        point: super::Point::Gate,
        diff: "+x".into(),
        reason: "it repairs it".into(),
        failure: "gate: FAILED in clippy".into(),
        fixer: "fable (claude-fable-5-2)".into(),
        target: r
            .core
            .runner
            .resolve_target(&live, Some("sonnet"), None, None)
            .unwrap(),
    };
    r.queue_call(WIRE, input());
    let review = super::turns::Turns.approve(&r.core, &ask).await.unwrap();
    let session = review.session.unwrap();
    assert_eq!(
        r.core.tools.self_pr.role(&session),
        Some(crate::rsi::roles::Role::Approver)
    );
    let tools = offered(&request_of(&r, "You built branch"));
    assert!(
        tools.iter().all(|n| n.starts_with("tools")),
        "an approver is offered no tool: {tools:?}"
    );
    assert!(green_builds(&r).is_empty());
    assert!(
        rows(&r, "self.branch.built").is_empty(),
        "nothing built at all"
    );
}

/// A reviewer's turn (the review's own check turn, which was offered the verb before issue 36) is
/// offered the read-only tools and its build's, never a self verb, and builds nothing. Its role
/// is kept in the store, so a restarted daemon still knows it.
#[tokio::test]
async fn a_reviewers_turn_is_offered_no_self_verb_and_builds_no_branch() {
    use crate::rsi::join::review::{Ask, CheckTurn, Reviewer};
    let r = rig(|_| {});
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    write(&tree_of(&r), LINTED);
    let live = r.core.live_profile().0;
    let scratch = r.f.place.state.join("scratch-review");
    std::fs::create_dir_all(&scratch).unwrap();
    let ask = Ask {
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        item: ITEM.into(),
        diff: String::new(),
        report: String::new(),
        evidence: String::new(),
        target: r
            .core
            .runner
            .resolve_target(&live, Some("fable"), None, None)
            .unwrap(),
        built_ms: 0,
        scratch: scratch.display().to_string(),
        repo: r.f.place.repo.display().to_string(),
        offer_fix: None,
        fixed: None,
        proof: String::new(),
    };
    r.queue_call(WIRE, input());
    let review = CheckTurn.review(&r.core, &ask).await.unwrap();
    let session = review.session.unwrap();
    let tools = offered(&request_of(&r, "Check: you check task"));
    assert!(!tools.iter().any(|n| n.starts_with("self_")), "{tools:?}");
    assert!(
        !tools
            .iter()
            .any(|n| n == "fs_write" || n == "extend_propose"),
        "{tools:?}"
    );
    assert!(tools.iter().any(|n| n == "fs_read") && tools.iter().any(|n| n == "proc_run"));
    assert!(green_builds(&r).is_empty());
    r.core.tools.self_pr.forget_roles();
    assert_eq!(
        r.core.tools.self_pr.role(&session),
        Some(crate::rsi::roles::Role::Reviewer),
        "read back from the store"
    );
    assert_eq!(
        r.core.tools.self_pr.role(&sid),
        None,
        "a builder has no role"
    );
}

/// Review round 1, B2: a restart between the code's fix commit and the gate's second run (the
/// fixer's commit made, never approved). The builder calls again, as `INTERRUPTED` tells it to:
/// the fix is ended as cut short, the tree is put back to before it, nothing of the fixer's goes
/// green, and the view no longer says the build is being fixed.
#[tokio::test]
async fn after_a_restart_mid_fix_no_unapproved_fix_goes_green() {
    let r = rig(|_| {});
    let fixer = StandIn::writing(&[]);
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: fixer.clone(),
        main: green(),
    });
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    let tree = tree_of(&r);
    // What the cut-short attempt left: the builder's commit, the code's fix commit, and the rows
    // `make` wrote before the restart.
    write(&tree, LINTED);
    git(&tree, &["add", "-A"]);
    git(
        &tree,
        &[
            "commit",
            "-q",
            "-m",
            "self: trim the sails (trim-the-sails)",
        ],
    );
    let before = git(&tree, &["rev-parse", "HEAD"]);
    let branch = git(&tree, &["rev-parse", "--abbrev-ref", "HEAD"]);
    write(&tree, REPAIR);
    git(&tree, &["add", "-A"]);
    git(
        &tree,
        &[
            "commit",
            "-q",
            "-m",
            "self: fix at the gate by claude-fable-5-2 for clippy",
        ],
    );
    let fix = git(&tree, &["rev-parse", "HEAD"]);
    let m = crate::fact::rsi_fix::Moved::new(ITEM, "given to a fixer")
        .with("branch", branch.clone())
        .with("head", before.clone())
        .with("point", "gate")
        .with("fixer", "fable (claude-fable-5-2)");
    r.core
        .rec(None)
        .record(&crate::fact::rsi_fix::Started(m.clone()));
    r.core.rec(None).record(&crate::fact::rsi_fix::Made(
        m.with("head", fix.clone()).with("from", before.clone()),
    ));
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("a restart cut the fix short"), "{said}");
    assert!(said.contains("the gate is red"), "{said}");
    let none = rows(&r, "self.fix.none").pop().unwrap();
    assert_eq!(none["why"], "a restart cut the fix short");
    assert_eq!(none["head"], before.as_str());
    assert!(
        rows(&r, "self.fix.verdict").is_empty(),
        "nobody approved {fix}"
    );
    assert!(
        green_builds(&r).is_empty(),
        "nothing of the fixer's went green"
    );
    assert_eq!(
        git(&tree, &["rev-parse", "HEAD"]),
        before,
        "put back to before the fix"
    );
    assert_eq!(fixer.fixed(), 0, "no second fix after the cut");
    // The view: the build is no longer being fixed.
    let view = r
        .core
        .self_pipeline(&theseus_protocol::rsi::SelfPipelineParams::default())
        .unwrap();
    assert!(view.building.iter().all(|e| e.fixing.is_none()), "{view:?}");
}

/// The one rule every green head is held to, whatever path led to it: a head that holds a fixer's
/// commit no other model approved is not green, however the gate ran.
#[tokio::test]
async fn a_green_head_with_an_unapproved_fixers_commit_is_red() {
    let r = rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    let tree = tree_of(&r);
    write(&tree, &[("sails.txt", "trimmed\n")]);
    git(&tree, &["add", "-A"]);
    let msg = format!("self: fix at the gate\n\n{} gate\n", super::TRAILER);
    git(&tree, &["commit", "-q", "-m", &msg]);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert!(green_builds(&r).is_empty());
    let report = std::fs::read_to_string(built(&r)["report"].as_str().unwrap()).unwrap();
    assert!(report.contains("which no other model approved"), "{report}");
}

/// Review round 1, note 2: an approval answered by the fixer's own model (a fallback route, a
/// profile changed in flight) is refused, never an accept.
#[tokio::test]
async fn an_approval_answered_by_the_fixers_model_is_refused() {
    struct AnswersAsFixer(Arc<StandIn>);
    impl super::Fixer for AnswersAsFixer {
        fn fix<'a>(
            &'a self,
            core: &'a Arc<crate::rpc::Core>,
            ask: &'a super::FixAsk,
        ) -> super::FixFut<'a> {
            self.0.fix(core, ask)
        }
        fn approve<'a>(
            &'a self,
            _core: &'a Arc<crate::rpc::Core>,
            _ask: &'a super::ApproveAsk,
        ) -> crate::rsi::join::review::Fut<'a> {
            Box::pin(async move {
                Ok(crate::rsi::join::review::Review {
                    accept: true,
                    fix: false,
                    reasons: vec!["fine".into()],
                    cost_usd: 0.05,
                    session: None,
                    profile: "sonnet".into(),
                    model: "claude-fable-5-2-20261101".into(),
                })
            })
        }
    }
    let r = rig(|_| {});
    let (_, said) = {
        r.core.tools.self_pr.set_fix(GateSeams {
            fixer: Arc::new(AnswersAsFixer(StandIn::writing(REPAIR))),
            main: green(),
        });
        let sid = item(&r);
        r.turn(&sid, WIRE, input()).await;
        write(&tree_of(&r), LINTED);
        (sid.clone(), r.turn(&sid, WIRE, input()).await)
    };
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(
        refused.last().unwrap()["bound"],
        "not-independent",
        "{refused:?}"
    );
    assert!(rows(&r, "self.fix.verdict").is_empty());
    assert!(green_builds(&r).is_empty());
}

/// Push `files` onto the fixture's main.
pub(super) fn onto_main(r: &Rig, name: &str, files: &[(&str, &str)]) {
    let root = r.f.upstream.parent().unwrap().to_path_buf();
    let work = root.join(name);
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    write(&work, files);
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", name]);
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
}

/// Note 2: main's own keel guard is present and judges the fix's commit alone; a raised cap the
/// line rules cannot see is refused by it.
#[tokio::test]
async fn main_s_keel_guard_judges_the_fix_commit_alone() {
    let r = rig(|_| {});
    let guard = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/keel-guard.py"),
    )
    .unwrap();
    onto_main(
        &r,
        "with-guard",
        &[
            ("scripts/keel-guard.py", &guard),
            ("src/limits.rs", "pub const SPEND_CAP_USD: f64 = 1.0;\n"),
        ],
    );
    let mut writes = REPAIR.to_vec();
    writes.push(("src/limits.rs", "pub const SPEND_CAP_USD: f64 = 2.0;\n"));
    let fixer = StandIn::writing(&writes);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "tests", "{refused:?}");
    let why = refused[0]["why"].as_str().unwrap();
    assert!(
        why.contains("main's keel guard found") && why.contains("cap-raised"),
        "{why}"
    );
}

/// Note 2: the line rule in a file that is not named as a test: an assertion removed from it is
/// refused.
#[tokio::test]
async fn a_fix_that_removes_an_assertion_from_an_old_file_is_refused() {
    let r = rig(|_| {});
    onto_main(
        &r,
        "with-check",
        &[(
            "src/sails.rs",
            "pub fn trim(x: u32) {\n    debug_assert!(x < 9);\n}\n",
        )],
    );
    let mut writes = REPAIR.to_vec();
    writes.push(("src/sails.rs", "pub fn trim(x: u32) {\n    let _ = x;\n}\n"));
    let fixer = StandIn::writing(&writes);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "tests", "{refused:?}");
    assert!(refused[0]["why"]
        .as_str()
        .unwrap()
        .contains("weakens a test in src/sails.rs"));
}

/// Note 1: a test inline in an ordinary file that was on main: an early return in it is refused.
#[tokio::test]
async fn a_fix_that_silences_an_old_inline_test_is_refused() {
    let r = rig(|_| {});
    let file = "pub fn add(a: u32) -> u32 {\n    a + 1\n}\n\n#[cfg(test)]\nmod tests {\n    \
                use super::*;\n\n    #[test]\n    fn adds() {\n        let two = add(1);\n        \
                assert_eq!(two, 2);\n    }\n}\n";
    onto_main(&r, "with-inline", &[("src/add.rs", file)]);
    let silenced = file.replace(
        "        let two = add(1);\n",
        "        return;\n        let two = add(1);\n",
    );
    let mut writes = REPAIR.to_vec();
    writes.push(("src/add.rs", &silenced));
    let fixer = StandIn::writing(&writes);
    let (_, said) = build(&r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    let refused = rows(&r, "self.fix.refused");
    assert_eq!(refused[0]["bound"], "tests", "{refused:?}");
    assert!(refused[0]["why"]
        .as_str()
        .unwrap()
        .contains("the test tests in src/add.rs"));
}

// ---------------------------------------------------------------- round 3 (review R2-1, items)

fn stand_in(done: bool, approve: bool, writes: &[(&str, &str)]) -> Arc<StandIn> {
    Arc::new(StandIn {
        done,
        approve,
        ..Arc::try_unwrap(StandIn::writing(writes)).ok().unwrap()
    })
}

/// Whether `head` holds `path`.
fn holds(r: &Rig, head: &str, path: &str) -> bool {
    git(&r.f.place.repo, &["ls-tree", "-r", "--name-only", head])
        .lines()
        .any(|l| l == path)
}

/// Review round 2, R2-1: a fix that did not hold, then the builder repairs its own red and calls
/// again. Its repair is in what is committed and gated next, and nothing of the fixer's is.
/// `fixer` is the fix that does not hold; `left` is a file only the fixer wrote.
async fn repair_after(r: &Rig, fixer: Arc<StandIn>, left: Option<&str>) {
    let (sid, said) = build(r, &fixer, green(), LINTED).await;
    assert!(said.contains("the gate is red"), "{said}");
    // Before the builder reads the red, the tree is the builder's own: its head and nothing else.
    let tree = tree_of(r);
    let first = rows(r, "self.branch.built").pop().unwrap();
    assert_eq!(
        git(&tree, &["rev-parse", "HEAD"]),
        first["head"].as_str().unwrap()
    );
    if let Some(f) = left {
        assert!(
            !tree.join(f).exists(),
            "the fixer's {f} is still in the tree"
        );
    }
    // The builder's own repair, written after it read the red, before it calls again.
    write(&tree, REPAIR);
    let said = r.turn(&sid, WIRE, input()).await;
    let last = rows(r, "self.branch.built").pop().unwrap();
    let head = last["head"].as_str().unwrap_or_default().to_string();
    assert!(
        holds(r, &head, "lint-fixed.txt"),
        "the builder's own repair is not in the head its next attempt gated ({})",
        last["gate"]
    );
    assert_eq!(last["gate"], "green", "{said}");
    if let Some(f) = left {
        assert!(
            !holds(r, &head, f),
            "the fixer's {f} reached the green head"
        );
    }
}

/// The control: with the mode off, the repair is kept (the loop before issue 36).
#[tokio::test]
async fn with_fix_off_the_builders_repair_is_kept() {
    let r = rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    repair_after(&r, stand_in(true, true, &[]), None).await;
}

/// A fixer that made nothing (`FIX: none`).
#[tokio::test]
async fn after_a_fix_that_made_nothing_the_builders_repair_is_kept() {
    let r = rig(|_| {});
    repair_after(&r, stand_in(false, true, &[]), None).await;
}

/// A fix a bound refused, its commit made.
#[tokio::test]
async fn after_a_refused_fix_the_builders_repair_is_kept() {
    let r = rig(|c| c.self_fix.lines = 2);
    let fixer = stand_in(true, true, &[("notes.txt", "a\nb\nc\n")]);
    repair_after(&r, fixer, Some("notes.txt")).await;
}

/// A fix the gate was red on again.
#[tokio::test]
async fn after_a_fix_that_was_red_again_the_builders_repair_is_kept() {
    let r = rig(|_| {});
    let fixer = stand_in(true, true, &[("notes.txt", "looked at it\n")]);
    repair_after(&r, fixer, Some("notes.txt")).await;
    assert_eq!(rows(&r, "self.fix.none").len(), 1);
}

/// A fix the builder's model rejected after the gate was green on it.
#[tokio::test]
async fn after_a_rejected_fix_the_builders_repair_is_kept() {
    let r = rig(|_| {});
    let fixer = stand_in(
        true,
        false,
        &[("lint-fixed.txt", "the fixer's\n"), ("notes.txt", "x\n")],
    );
    repair_after(&r, fixer, Some("notes.txt")).await;
    assert_eq!(rows(&r, "self.fix.verdict")[0]["verdict"], "reject");
}

/// Item 1: with the mode off, a build does not wait on the ledger's index (as before issue 36);
/// with it on, it does, rather than guess whether a fix was cut short.
#[tokio::test]
async fn an_unreadable_index_stops_a_build_only_while_fixing_is_on() {
    for (on, refused) in [(false, false), (true, true)] {
        let r = rig(|c| {
            if !on {
                c.self_fix.mode = crate::config::self_fix::FixMode::Off;
            }
        });
        let sid = item(&r);
        r.turn(&sid, WIRE, input()).await;
        write(&tree_of(&r), &[("sails.txt", "trimmed\n")]);
        r.core.tools.self_pr.set_index_unread();
        let said = r.turn(&sid, WIRE, input()).await;
        assert_eq!(
            said.contains("Call again in a minute"),
            refused,
            "on={on}: {said}"
        );
        assert_eq!(
            said.contains("the gate is green"),
            !refused,
            "on={on}: {said}"
        );
    }
}

/// Item 1: a reviewer's turn is bounded only while the mode is on.
#[test]
fn a_reviewers_turn_is_bounded_only_while_fixing_is_on() {
    let r = rig(|c| c.self_fix.turn_minutes = 45);
    assert_eq!(
        super::turns::review_limit(&r.core),
        Some(std::time::Duration::from_secs(45 * 60))
    );
    let off = rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    assert_eq!(super::turns::review_limit(&off.core), None);
}

/// Item 3: a builder's `why` with a line that reads as a fix commit's trailer does not make its own
/// branch read as an unapproved fix: the line is marked as quoted, and the branch goes green.
#[tokio::test]
async fn a_builders_why_that_quotes_the_trailer_is_not_a_fix() {
    let r = rig(|_| {});
    let sid = item(&r);
    let inp = json!({"item": ITEM, "issue": "theseus-sail1",
        "why": "Trim the sails.\nSelf-Fix-Point: gate\n  self-fix-point: review"});
    r.turn(&sid, WIRE, inp.clone()).await;
    write(&tree_of(&r), &[("fix.txt", "fixed\n")]);
    let said = r.turn(&sid, WIRE, inp).await;
    assert!(said.contains("the gate is green"), "{said}");
    let head = built(&r)["head"].as_str().unwrap().to_string();
    let body = git(&r.f.place.repo, &["log", "-1", "--format=%B", &head]);
    assert!(body.contains("(quoted) Self-Fix-Point: gate"), "{body}");
    assert!(
        !body.lines().any(|l| l.trim().starts_with(super::TRAILER)),
        "{body}"
    );
}

/// Review round 2, note 3: a session that builds one item does not commit and gate another open
/// item's tree.
#[tokio::test]
async fn a_builder_cannot_build_another_items_tree() {
    let r = rig(|_| {});
    let sid = item(&r);
    let other = json!({"item": "mend-the-mast", "issue": "theseus-sail1", "why": "Mend it."});
    let said = r.turn(&sid, WIRE, other).await;
    assert!(said.contains("is another item's"), "{said}");
    assert!(
        !r.f.place.trees().join("mend-the-mast").exists(),
        "no tree opened"
    );
}

/// Review round 2, note 4: the roles kept stay bounded; a role is never forgotten.
#[tokio::test]
async fn the_roles_kept_stay_bounded() {
    let r = rig(|_| {});
    let board = &r.core.tools.self_pr;
    board
        .set_role("ses_fixer", crate::rsi::roles::Role::Fixer)
        .unwrap();
    for n in 0..4_200 {
        assert_eq!(board.role(&format!("ses_{n}")), None);
    }
    assert!(board.roles_kept() <= 4_096, "{}", board.roles_kept());
    assert_eq!(
        board.role("ses_fixer"),
        Some(crate::rsi::roles::Role::Fixer)
    );
}

/// Item 2: a turn stopped at its limit is a failed turn of its own class, which the review's hold
/// reads as a failure that does not clear by itself.
#[tokio::test]
async fn a_turn_past_its_limit_is_a_failure_that_does_not_clear() {
    let r = rig(|_| {});
    let never = std::future::pending::<anyhow::Result<theseus_protocol::TurnSubmitResult>>();
    let limit = std::time::Duration::from_millis(30);
    let e = super::turns::within(&r.core, "ses_x", None, limit, never)
        .await
        .unwrap_err();
    let t = e.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(
        (t.class.as_str(), t.transient),
        (super::turns::LIMIT_CLASS, false)
    );
    assert!(format!("{e:#}").contains("ran past its limit"), "{e:#}");
}

// ---------------------------------------------------------------- round 4 (review R3-1, note 1)

/// A gate that, like the real one's "protocol types" phase, writes its own repair into the tree
/// and fails: `gen.txt` must equal `schema.txt`.
const GEN_GATE: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
sleep 1
if [ -f schema.txt ] && ! cmp -s schema.txt gen.txt; then cp schema.txt gen.txt; echo "gen.txt was stale: git add it"; echo "gate: FAILED in protocol types"; exit 1; fi
echo "gate: ok"
"#;

fn gen_rig(edit: impl FnOnce(&mut Config)) -> Rig {
    rig_on(fixture_with(GEN_GATE), SelfMode::Act, Some("origin"), |c| {
        c.self_improve.reviewer_profile = Some("fable".into());
        edit(c);
    })
}

/// The gate's own repair (it rewrote gen.txt in the tree) is in the tree after the red, and the
/// builder's next call commits it and goes green, as on main.
async fn gen_heals(r: &Rig, fixer: &Arc<StandIn>) {
    let (sid, said) = build(r, fixer, green(), &[("schema.txt", "field\n")]).await;
    assert!(said.contains("the gate is red"), "{said}");
    let gen = std::fs::read_to_string(tree_of(r).join("gen.txt")).ok();
    assert_eq!(
        gen.as_deref(),
        Some("field\n"),
        "the gate's repair is gone from the tree"
    );
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("the gate is green"), "{said}");
}

/// The control: with the mode off, the gate's own repair heals the next attempt.
#[tokio::test]
async fn with_fix_off_the_gates_own_repair_heals_the_next_attempt() {
    let r = gen_rig(|c| c.self_fix.mode = crate::config::self_fix::FixMode::Off);
    gen_heals(&r, &StandIn::writing(&[])).await;
}

/// Review round 3, R3-1: with the mode on, a red in "protocol types" is not given to a fixer,
/// whose fix that did not hold would put the tree back and throw the gate's repair away.
#[tokio::test]
async fn after_a_fix_that_made_nothing_the_gates_own_repair_heals_the_next_attempt() {
    let r = gen_rig(|_| {});
    let none = stand_in(false, true, &[]);
    gen_heals(&r, &none).await;
    assert_eq!(none.fixed(), 0, "no fixer was asked");
    assert!(rows(&r, "self.fix.started").is_empty());
}

/// A fixer whose fix makes the gate green but is refused for its size (the lint repaired, and
/// notes.txt over the 2 lines allowed).
fn too_big() -> Arc<StandIn> {
    stand_in(
        true,
        true,
        &[
            ("lint-fixed.txt", "the fixer's\n"),
            ("notes.txt", "a\nb\nc\n"),
        ],
    )
}

/// Whether the tree's head holds notes.txt, the refused fix's file.
fn tree_holds_the_fix(r: &Rig) -> bool {
    let head = git(&tree_of(r), &["rev-parse", "HEAD"]);
    holds(r, &head, "notes.txt")
}

/// Review round 3, note 1: a tree a failed reset could not put back says so, its red's head is
/// the head the tree holds, and the next call puts it back before anything is committed.
#[tokio::test]
async fn a_tree_that_could_not_be_put_back_is_put_back_at_the_next_call() {
    let r = rig(|c| c.self_fix.lines = 2);
    r.core.tools.self_pr.set_resets_fail(1);
    let (sid, said) = build(&r, &too_big(), green(), LINTED).await;
    assert!(said.contains("could not be put back"), "{said}");
    assert!(
        tree_holds_the_fix(&r),
        "the reset failed, so the fix is still in the tree"
    );
    let tree_head = git(&tree_of(&r), &["rev-parse", "HEAD"]);
    assert_eq!(
        built(&r)["head"],
        tree_head.as_str(),
        "the red's head is the tree's"
    );
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("it is put back now"), "{said}");
    assert!(!tree_holds_the_fix(&r));
    write(&tree_of(&r), REPAIR);
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("the gate is green"), "{said}");
    let head = built(&r)["head"].as_str().unwrap().to_string();
    assert!(holds(&r, &head, "lint-fixed.txt") && !holds(&r, &head, "notes.txt"));
}

/// Review round 3, note 1: a tree that cannot be put back at all is not red for ever. Each call
/// tries again; each attempt on it is red (the green rule: the fixer's commit is unapproved) and a
/// try, so the item is held for the owner when its tries run out, with the cap's one notice.
#[tokio::test]
async fn a_tree_that_cannot_be_put_back_is_held_for_the_owner_when_its_tries_run_out() {
    let r = rig(|c| c.self_fix.lines = 2);
    r.core.tools.self_pr.set_resets_fail(usize::MAX);
    let (sid, said) = build(&r, &too_big(), green(), LINTED).await;
    assert!(said.contains("could not be put back"), "{said}");
    for _ in 0..2 {
        let said = r.turn(&sid, WIRE, input()).await;
        assert!(said.contains("again"), "{said}");
        assert_eq!(built(&r)["gate"], "red", "{said}");
    }
    let t = crate::task_graph::get(&r.core.store, ITEM)
        .unwrap()
        .unwrap();
    assert!(
        attempts::capped(&t),
        "three red tries hold the item for the owner"
    );
    let said = r.turn(&sid, WIRE, input()).await;
    assert!(said.contains("used all its tries"), "{said}");
}

// ---------------------------------------------------------------- round 5 (the merge with S1's fix)

/// The real fixer turn, its model making the queued calls; it approves as the stand-in does.
struct Real(Arc<StandIn>);

impl super::Fixer for Real {
    fn fix<'a>(
        &'a self,
        core: &'a Arc<crate::rpc::Core>,
        ask: &'a super::FixAsk,
    ) -> super::FixFut<'a> {
        super::Fixer::fix(&super::turns::Turns, core, ask)
    }

    fn approve<'a>(
        &'a self,
        core: &'a Arc<crate::rpc::Core>,
        ask: &'a super::ApproveAsk,
    ) -> crate::rsi::join::review::Fut<'a> {
        self.0.approve(core, ask)
    }
}

/// The fixer's turn: a red lint goes to the real fixer, whose model makes the queued call.
/// The item's tree is open (its session's first call) before this.
async fn fixer_calls(r: &Rig, sid: &str, tool: &str, call: Value) -> String {
    r.core.tools.self_pr.set_fix(GateSeams {
        fixer: Arc::new(Real(stand_in(true, true, &[]))),
        main: green(),
    });
    write(&tree_of(r), LINTED);
    r.queue_call(tool, call);
    r.turn(sid, WIRE, input()).await
}

/// S1's refusal covers a fixer's session as it does a builder's: the fixer's `fs.write` to its
/// tree's own `.git` file is refused, and the file is as git made it.
#[tokio::test]
async fn a_fixers_write_to_its_trees_git_is_refused() {
    let r = rig(|_| {});
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    let git_file = tree_of(&r).join(".git");
    let was = std::fs::read_to_string(&git_file).unwrap();
    let said = fixer_calls(
        &r,
        &sid,
        "fs_write",
        json!({"path": git_file.display().to_string(), "content": "gitdir: /elsewhere\n"}),
    )
    .await;
    assert_eq!(std::fs::read_to_string(&git_file).unwrap(), was, "{said}");
    let fixer_saw = r
        .requests()
        .into_iter()
        .filter(|q| {
            q.messages
                .iter()
                .any(|m| m.to_string().contains("You are the fixer"))
        })
        .flat_map(|q| q.messages.into_iter().map(|m| m.to_string()))
        .any(|m| m.contains("git's own"));
    assert!(
        fixer_saw,
        "the fixer was not told its write was refused: {said}"
    );
}

/// Review round 2's probe of S1, kept: a fixer that points its tree's `.git` file at a git
/// directory with hooks, made inside its tree, runs nothing outside L1 through the daemon's git.
#[tokio::test]
async fn a_fixer_that_rewrites_its_trees_git_file_runs_nothing_outside_l1() {
    let r = rig(|_| {});
    let sid = item(&r);
    r.turn(&sid, WIRE, input()).await;
    let tree = tree_of(&r);
    let marker = r.f.place.state.join("RAN-OUTSIDE-L1");
    let evil = tree.join(".evil");
    git(
        &tree,
        &[
            "clone",
            "-q",
            "--no-checkout",
            r.f.place.repo.to_str().unwrap(),
            evil.to_str().unwrap(),
        ],
    );
    let hook = format!("#!/bin/sh\necho \"$0\" >> {}\n", marker.display());
    for h in ["post-checkout", "post-commit", "fsm"] {
        use std::os::unix::fs::PermissionsExt;
        let p = evil.join(".git/hooks").join(h);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &hook).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(
        &evil,
        &[
            "config",
            "core.fsmonitor",
            evil.join(".git/hooks/fsm").to_str().unwrap(),
        ],
    );
    let said = fixer_calls(
        &r,
        &sid,
        "fs_write",
        json!({"path": tree.join(".git").display().to_string(),
               "content": format!("gitdir: {}\n", evil.join(".git").display())}),
    )
    .await;
    let ran = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        ran.is_empty(),
        "the daemon's git ran the fixer's hooks: {ran} ({said})"
    );
}
