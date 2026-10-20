//! A self item's tree through the pipeline's rig: open before its first
//! turn and named in it (theseus-qh2tt), gone when the item ends however it
//! ended (theseus-czlzg), and the owner's retry of a failed item.

use theseus_protocol::rsi::SelfRetryParams;
use theseus_protocol::tasks::TaskState;

use super::tests::{rig_with, Rig};
use crate::approval::Answerer;
use crate::rsi::tree;
use crate::session::SessionRecord;

const CLI: &str = "the CLI";

/// The execution of the session the pipeline started `item` in, ended: what
/// a turn that ends with no green branch leaves, as the backlog's sweep finds it.
fn end_session(r: &Rig, item: &str) {
    let t = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    let sid = t.session.unwrap();
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    r.core
        .kernel
        .cancel_execution(&rec.execution_id.unwrap(), "test")
        .unwrap();
}

/// What the backlog's night does to an ended item: its sweep marks it failed.
fn sweep(r: &Rig) {
    r.core
        .backlog_night(theseus_protocol::now_unix_ms())
        .unwrap();
}

fn open_trees(r: &Rig) -> usize {
    crate::rsi::capacity::trees_open(&r.f.place.trees())
}

/// KEY: a started item's tree is open before its first turn, its session
/// works in it, and the turn's input names its absolute path and branch.
#[tokio::test]
async fn a_started_item_has_its_tree_open_and_named_before_its_first_turn() {
    let r = rig_with(1, 30.0);
    let t = r.tick().await;
    let item = t.started[0].clone();
    let open = tree::find(&r.f.place, &item).expect("no tree open at start");
    let session = r.rows("self.pipeline.started")[0]["session"]
        .as_str()
        .unwrap()
        .to_string();
    let rec: SessionRecord = r.core.store.get_session(&session).unwrap().unwrap();
    assert_eq!(
        rec.dir.as_deref(),
        Some(open.dir.to_str().unwrap()),
        "its working directory is the tree"
    );
    let objective = r.fake.objectives.lock().unwrap()[0].clone();
    assert!(
        objective.contains(&format!(
            "Your tree is {} on branch {}; read and edit there, by absolute path.",
            open.dir.display(),
            open.branch
        )),
        "{objective}"
    );
    assert!(open.dir.is_absolute() && open.branch.starts_with("self/"));
}

/// An item whose tree cannot be opened does not start: the next start is
/// held, saying why, and no session is made.
#[tokio::test]
async fn an_item_whose_tree_cannot_be_opened_is_held_with_the_reason() {
    let r = rig_with(1, 30.0);
    std::fs::remove_dir_all(&r.f.place.repo).unwrap();
    let t = r.tick().await;
    assert!(t.started.is_empty(), "{t:?}");
    assert_eq!(t.hold.as_deref(), Some("tree"));
    assert!(r.fake.starts.lock().unwrap().is_empty());
    let held = r.rows("self.pipeline.held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(
        held[0]["why"]
            .as_str()
            .unwrap()
            .contains("could not be opened"),
        "{held:?}"
    );
    r.tick().await;
    assert_eq!(r.rows("self.pipeline.held").len(), 1, "said once");
}

/// An item the backlog's sweep failed (not the pipeline's own ending) leaves
/// no tree: its slot is free at the next tick and a ready item starts in it.
/// A branch with a commit beyond its base stays; one with none is gone.
#[tokio::test]
async fn an_item_failed_by_the_sweep_leaves_no_tree() {
    let r = rig_with(2, 30.0);
    let t = r.tick().await;
    let (kept, empty) = (t.started[0].clone(), t.started[1].clone());
    assert_eq!(open_trees(&r), 2);
    let held = r.tick().await;
    assert_eq!(held.hold.as_deref(), Some("slots"), "{held:?}");
    let tk = tree::find(&r.f.place, &kept).unwrap();
    let te = tree::find(&r.f.place, &empty).unwrap();
    std::fs::write(tk.dir.join("fix.txt"), "mended\n").unwrap();
    assert!(tree::commit(&r.f.place, &tk, "self: mend", "")
        .unwrap()
        .is_some());
    for item in [&kept, &empty] {
        end_session(&r, item);
    }
    sweep(&r);
    assert_eq!(r.state(&kept), TaskState::Failed, "the sweep failed it");
    assert_eq!(r.state(&empty), TaskState::Failed);
    assert!(
        tk.dir.exists() && te.dir.exists(),
        "the sweep alone leaves them"
    );
    let t = r.tick().await;
    assert!(
        !tk.dir.exists() && !te.dir.exists(),
        "the tick closes both trees"
    );
    assert!(!tk.target.exists() && !te.target.exists());
    let branches: Vec<String> = tree::branches(&r.f.place)
        .unwrap()
        .into_iter()
        .map(|(b, _)| b)
        .collect();
    assert!(
        branches.contains(&tk.branch),
        "a branch with a commit stays: {branches:?}"
    );
    assert!(
        !branches.contains(&te.branch),
        "an empty one is gone: {branches:?}"
    );
    let cleared = r.rows("self.pipeline.cleared");
    assert_eq!(cleared.len(), 2, "{cleared:?}");
    assert!(cleared.iter().all(|c| c["reason"] == "closed"));
    assert_eq!(
        t.started.len(),
        1,
        "the third starts in a freed slot: {t:?}"
    );
    assert_eq!(open_trees(&r), 1, "only its tree is open now");
}

/// A tree whose item is still building, or waits at the keel, is never touched.
#[tokio::test]
async fn a_building_items_tree_is_left_alone() {
    let r = rig_with(1, 30.0);
    let t = r.tick().await;
    let open = tree::find(&r.f.place, &t.started[0]).unwrap();
    for _ in 0..3 {
        r.tick().await;
    }
    assert!(open.dir.exists() && open.target.exists());
    assert!(r.rows("self.pipeline.cleared").is_empty());
}

/// The owner's retry: a failed item is proposed again and the pipeline
/// starts it; any other state is refused with words; a job's shell is refused.
#[tokio::test]
async fn retry_queues_a_failed_item_and_refuses_the_rest() {
    let r = rig_with(3, 30.0);
    let item = r.tick().await.started[0].clone();
    let who = Answerer::from(CLI);
    let retry = |item: &str, from_job: Option<&str>| {
        r.core.self_retry(
            &SelfRetryParams {
                item: item.into(),
                from_job: from_job.map(str::to_string),
                ..Default::default()
            },
            &who,
        )
    };
    let building = retry(&item, None).unwrap_err().to_string();
    assert!(
        building.contains("in_progress") && building.contains("only a failed item"),
        "{building}"
    );
    end_session(&r, &item);
    sweep(&r);
    assert_eq!(r.state(&item), TaskState::Failed);
    // The tick closes its tree, so its slot is free at once.
    r.tick().await;
    assert!(tree::find(&r.f.place, &item).is_none());
    let job = retry(&item, Some("ses_somejob")).unwrap_err().to_string();
    assert!(job.contains("job's shell"), "{job}");
    assert_eq!(
        r.state(&item),
        TaskState::Failed,
        "a refused retry changes nothing"
    );
    let done = retry(&item, None).unwrap();
    assert_eq!(done.item, item);
    assert_eq!(r.state(&item), TaskState::Proposed);
    let t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    assert!(t.session.is_none());
    assert!(
        t.evidence
            .iter()
            .any(|e| e.note.as_deref() == Some("retried by the owner")),
        "{:?}",
        t.evidence
    );
    let again = retry(&item, None).unwrap_err().to_string();
    assert!(again.contains("proposed"), "{again}");
    // Done is refused too.
    let mut d = t.clone();
    d.state = TaskState::Done;
    d.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&d).unwrap()])
        .unwrap();
    let done = retry(&item, None).unwrap_err().to_string();
    assert!(
        done.contains("done") && done.contains("only a failed item"),
        "{done}"
    );
    let mut p = d;
    p.state = TaskState::Failed;
    p.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&p).unwrap()])
        .unwrap();
    retry(&item, None).unwrap();
    // The pipeline starts it again on its next free slot, in a new tree.
    let t = r.tick().await;
    assert!(t.started.contains(&item), "{t:?}");
    assert_eq!(r.state(&item), TaskState::InProgress);
    assert_eq!(
        r.fake
            .starts
            .lock()
            .unwrap()
            .iter()
            .filter(|(i, _)| *i == item)
            .count(),
        2
    );
}
