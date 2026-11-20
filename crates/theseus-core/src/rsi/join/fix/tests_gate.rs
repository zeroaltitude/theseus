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

const WIRE: &str = "self_propose_change";
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
const LINTED: &[(&str, &str)] = &[
    ("lint.txt", "let x = 1;\n"),
    ("fix.txt", "fixed\n"),
    ("tests/fix.sh", "test -f fix.txt\n"),
];
/// The fixer's repair of it.
const REPAIR: &[(&str, &str)] = &[("lint-fixed.txt", "let _x = 1;\n")];

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

fn input() -> Value {
    json!({"item": ITEM, "issue": "theseus-sail1", "why": "Trim the sails before the wind."})
}

/// The item a builder works on: a self task in progress in its session.
fn item(r: &Rig) -> String {
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

fn tree_of(r: &Rig) -> PathBuf {
    r.f.place.trees().canonicalize().unwrap().join(ITEM)
}

fn write(dir: &Path, files: &[(&str, &str)]) {
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

fn green() -> Arc<dyn main_red::Main> {
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

fn built(r: &Rig) -> Value {
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
    // The next attempt is a new branch from before the fix.
    let before = refused[0]["from"].as_str().unwrap().to_string();
    write(&tree_of(&r), &[("lint-fixed.txt", "")]);
    r.turn(&sid, WIRE, input()).await;
    let next = git(&tree_of(&r), &["log", "--format=%H", "-1", "HEAD^"]);
    assert_eq!(
        next, before,
        "the refused fix is not under the next attempt"
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
