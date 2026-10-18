//! The self-join's sealed check (theseus-pw1q.21.7), through the whole core
//! over `tests`' throwaway repository: a branch that touches a behaviour
//! path is scored on the sealed sets beside main and held when it scores
//! worse; one that does not is not scored; no set, or a mode that is off,
//! scores nothing. The scorer is a stand-in (`sealed::tests_hold::Canned`):
//! its answer for main's head is main's score, and any other head is the
//! candidate's.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfMode;
use theseus_protocol::tasks::{TaskRecord, TaskState};

use super::tests::{rig_with, Rig, SONNET};
use super::Seams;
use crate::rsi::sealed::tests_hold::{scores, Canned};

const PROVED: &[(&str, &str)] = &[
    ("fix.txt", "fixed\n"),
    ("tests/fix.sh", "test -f fix.txt\n"),
];
/// A behaviour path, changed beside the fix.
const BEHAVIOUR: (&str, &str) = ("crates/theseus-core/src/compiler.rs", "// trimmed\n");

struct Sealed {
    r: Rig,
    main: String,
    _sets: tempfile::TempDir,
}

/// A rig whose config names a sealed dir with `items` items in it, and the
/// remote's main head.
fn rigged(mode: SelfMode, every_branch: bool, items: u64) -> Sealed {
    let sets = tempfile::tempdir().unwrap();
    let dir = sets.path().canonicalize().unwrap().join("sealed");
    let d = dir.display().to_string();
    let r = rig_with(mode, |cfg| {
        cfg.self_improve.sealed.dir = Some(d);
        cfg.self_improve.sealed.every_branch = every_branch;
    });
    for i in 0..items {
        let item =
            json!({"v": 1, "task": format!("Tide question {i}?"), "check": "reply has \"x\""});
        r.core
            .sealed_put("tides", "fact", item.to_string().as_bytes(), "reviewer-a")
            .unwrap();
    }
    let main = std::process::Command::new("git")
        .arg("-C")
        .arg(&r.f.upstream)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .unwrap();
    Sealed {
        main: String::from_utf8_lossy(&main.stdout).trim().to_string(),
        r,
        _sets: sets,
    }
}

impl Sealed {
    fn seams(&self, scorer: &Arc<Canned>) -> Seams {
        Seams {
            sealed: scorer.clone(),
            ..self.r.seams()
        }
    }

    fn upstream_main(&self) -> String {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.r.f.upstream)
            .args(["rev-parse", "refs/heads/main"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn files(behaviour: bool) -> Vec<(&'static str, &'static str)> {
        let mut f = PROVED.to_vec();
        if behaviour {
            f.push(BEHAVIOUR);
        }
        f
    }
}

fn six(set: &str, fact: u64, route: u64) -> Vec<crate::rsi::sealed::Scores> {
    vec![scores(set, &[("fact", 6, fact), ("route", 6, route)])]
}

/// A six-item family loses two: the join stops at `sealed` with the numbers
/// and opaque ids, main is untouched, the head is held (not tried again),
/// and the item goes back to the backlog with the evidence.
#[tokio::test]
async fn a_family_drop_holds_the_join() {
    let s = rigged(SelfMode::Act, false, 2);
    let item = TaskRecord {
        id: "tsk_sails".into(),
        version: 3,
        title: "Trim the sails".into(),
        state: TaskState::InProgress,
        session: Some("ses_builder".into()),
        owner: "self".into(),
        ..TaskRecord::default()
    };
    s.r.core
        .store
        .append(&[crate::task_graph::record(&item).unwrap()])
        .unwrap();
    let (branch, head) = s.r.seed("tsk_sails", &Sealed::files(true), SONNET);
    // The family loses two; the other gains two: the total is level.
    let scorer = Canned::new(&s.main, six("tides", 6, 3), six("tides", 4, 5));
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(
        pass.stopped,
        [(branch.clone(), "sealed".to_string())],
        "{pass:?}"
    );
    assert!(pass.joined.is_empty());
    assert_eq!(s.upstream_main(), s.main, "main is untouched");
    let stop = &s.r.rows("self.join.stopped")[0];
    assert_eq!(stop["phase"], "sealed");
    assert_eq!(stop["numbers"]["held"], true);
    assert_eq!(stop["numbers"]["holds"][0]["family"], "fact");
    assert_eq!(stop["numbers"]["holds"][0]["before"], 6);
    assert_eq!(stop["numbers"]["holds"][0]["after"], 4);
    assert_eq!(stop["numbers"]["broken"].as_array().unwrap().len(), 2);
    assert!(
        stop["why"].as_str().unwrap().contains("family fact"),
        "{stop}"
    );
    let checked = &s.r.rows("self.sealed.checked")[0];
    assert_eq!(
        (checked["outcome"].clone(), checked["head"].clone()),
        (json!("held"), json!(head))
    );
    assert_eq!(
        checked["what"],
        format!("sealed 9/12 → 9/12 for {branch}: held")
    );
    // The item is back on the backlog with the numbers, as a review's reject.
    let t = crate::task_graph::get(&s.r.core.store, "tsk_sails")
        .unwrap()
        .unwrap();
    assert_eq!((t.state, t.session), (TaskState::Proposed, None));
    let e = t.evidence.last().unwrap();
    assert!(
        e.identity.starts_with("sealed:self/20261014-tsk_sails@"),
        "{e:?}"
    );
    assert!(
        e.note.as_deref().unwrap().contains("sealed 9/12 → 9/12"),
        "{e:?}"
    );
    // Nothing of an item's text is in any row.
    let all: String =
        s.r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .iter()
            .map(|(_, row)| serde_json::to_string(row).unwrap())
            .collect();
    assert!(!all.contains("Tide question"), "an item's text is in a row");
    // The head is held: a second pass scores nothing more.
    let n = scorer.asked().len();
    s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(scorer.asked().len(), n);
}

/// A branch that scores level joins, and its row says `sealed 9/12 → 9/12`
/// for the log. Main's score is cached for a second branch on the same main.
#[tokio::test]
async fn equal_scores_join_and_a_second_branch_reuses_mains_score() {
    let s = rigged(SelfMode::Act, false, 2);
    let (branch, _) = s.r.seed("trim-the-sails", &Sealed::files(true), SONNET);
    let scorer = Canned::new(&s.main, six("tides", 6, 3), six("tides", 6, 3));
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let checked = &s.r.rows("self.sealed.checked")[0];
    assert_eq!(checked["outcome"], "passed");
    assert_eq!(checked["cached"], false);
    assert_eq!(
        checked["what"],
        format!("sealed 9/12 → 9/12 for {branch}: passed")
    );
    assert!(s.r.rows("self.join.stopped").is_empty());
    // Main scored once (its head), the candidate once.
    assert_eq!(scorer.asked().iter().filter(|h| **h == s.main).count(), 1);
}

/// With the mode off, or the switch halted, nothing is scored and nothing
/// is written by the new phase.
#[tokio::test]
async fn off_and_halted_score_nothing() {
    let s = rigged(SelfMode::Off, false, 2);
    s.r.seed("trim-the-sails", &Sealed::files(true), SONNET);
    let scorer = Canned::new(&s.main, six("tides", 6, 3), six("tides", 0, 0));
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(pass.gate, "off");
    assert!(scorer.asked().is_empty());
    assert!(s.r.rows("self.sealed.checked").is_empty());
}

/// A branch that touches no behaviour path skips the phase while
/// `every_branch` is false, and is scored when it is true.
#[tokio::test]
async fn a_branch_touching_no_behaviour_path_skips_the_phase() {
    let s = rigged(SelfMode::Act, false, 2);
    s.r.seed("trim-the-sails", &Sealed::files(false), SONNET);
    let scorer = Canned::new(&s.main, six("tides", 6, 3), six("tides", 0, 0));
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert!(
        scorer.asked().is_empty(),
        "no scoring for a docs-shaped change"
    );
    assert!(s.r.rows("self.sealed.checked").is_empty());

    // The same change with every_branch on is scored, and held for its fall.
    let s = rigged(SelfMode::Act, true, 2);
    s.r.seed("trim-the-sails", &Sealed::files(false), SONNET);
    let scorer = Canned::new(&s.main, six("tides", 6, 3), six("tides", 0, 0));
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(pass.stopped.len(), 1, "{pass:?}");
    assert_eq!(scorer.asked().len(), 2, "main and the candidate");
}

/// With no sealed dir, or a set with no item, the phase is skipped and its
/// row says why; the join goes on.
#[tokio::test]
async fn no_sealed_set_skips_the_phase_and_says_so() {
    let s = rigged(SelfMode::Act, false, 0);
    s.r.seed("trim-the-sails", &Sealed::files(true), SONNET);
    let scorer = Canned::new(&s.main, vec![], vec![]);
    let pass = s.r.core.self_join_pass(&s.seams(&scorer)).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert!(scorer.asked().is_empty());
    let row = &s.r.rows("self.sealed.checked")[0];
    assert_eq!(row["outcome"], "skipped");
    assert!(
        row["what"].as_str().unwrap().contains("hold no item"),
        "{row}"
    );

    // No `dir` at all: the same, with its own reason.
    let r = rig_with(SelfMode::Act, |_| {});
    r.seed("trim-the-sails", &Sealed::files(true), SONNET);
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let row: Value = r.rows("self.sealed.checked")[0].clone();
    assert!(
        row["what"].as_str().unwrap().contains("dir is not set"),
        "{row}"
    );
}
