//! Layer 1 guards only the owner's tasks (theseus-ext.10): a plan item's
//! objective, acceptance, and abandoning, and a split's child's, apply at
//! once, versioned and visible; a record from before the model's mark reads
//! as the owner's and its change waits; an expired layer-1 question leaves no
//! proposal on its record. The owner's task session's change waiting on its
//! card is `tests_task_graph`'s layer-one test.

use serde_json::json;
use theseus_store::{kinds, NewRecord};

use crate::node::ResultStatus;
use crate::task_graph::{self as graph, TaskState};
use crate::tests_layouts::TASK_BEFORE_MODEL_MARK;
use crate::tests_task_graph::{results, rig, rows, session, turn, until};

/// A plan item's acceptance change and its abandoning apply at once, with no
/// question at the template's posture (notify), each a new version with its
/// row; so does a split's child's. The view marks neither as the operator's,
/// and its head says only a marked task's change waits.
#[tokio::test]
async fn a_plan_items_layer_one_applies_at_once_and_so_does_a_split_childs() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    r.calls(&[(
        "task_create",
        json!({"title": "Chart the outer harbour", "acceptance": ["a chart exists"]}),
    )]);
    turn(c, &s, "plan it").await;
    let t = r.by_title("Chart the outer harbour");
    assert!(t.origin.by_model && !t.is_owners(), "{t:?}");

    r.calls(&[(
        "task_update",
        json!({"id": t.id, "version": 1, "patch": {"acceptance": ["every buoy has a depth"], "objective": "chart the buoys"}}),
    )]);
    turn(c, &s, "change its acceptance").await;
    assert!(c.confirm_list().unwrap().is_empty(), "nothing waits");
    let (status, text) = results(c, &s, "task.update").pop().unwrap();
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let now = r.task(&t.id);
    assert_eq!(now.version, 2);
    assert_eq!(now.acceptance, ["every buoy has a depth"]);
    assert_eq!(now.objective, "chart the buoys");
    assert!(now.proposal.is_none());
    let updated = rows(c, "task.updated");
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].data["fields"], "objective,acceptance");
    assert!(rows(c, "task.change_proposed").is_empty());
    assert!(rows(c, "task.change_accepted").is_empty());

    // Split it; a child's acceptance changes at once too.
    r.calls(&[(
        "task_split",
        json!({"id": t.id, "version": 2, "into": ["Chart the north side", "Chart the south side"]}),
    )]);
    turn(c, &s, "split it").await;
    let north = r.by_title("Chart the north side");
    assert!(north.origin.by_model, "{north:?}");
    r.calls(&[(
        "task_update",
        json!({"id": north.id, "version": 1, "patch": {"acceptance": ["the north buoys have depths"]}}),
    )]);
    turn(c, &s, "say what the north side takes").await;
    assert!(c.confirm_list().unwrap().is_empty(), "nothing waits");
    let north = r.task(&north.id);
    assert_eq!(
        (north.version, north.acceptance.as_slice()),
        (2, &["the north buoys have depths".to_string()][..])
    );

    // Abandoning the south side applies at once: closed, its row written.
    let south = r.by_title("Chart the south side");
    r.calls(&[(
        "task_close",
        json!({"id": south.id, "version": 1, "outcome": "abandoned"}),
    )]);
    turn(c, &s, "drop the south side").await;
    assert!(c.confirm_list().unwrap().is_empty(), "nothing waits");
    let south = r.task(&south.id);
    assert_eq!((south.state, south.version), (TaskState::Abandoned, 2));
    let closed = rows(c, "task.closed");
    assert_eq!(closed.last().unwrap().data["state"], "abandoned");
    assert!(rows(c, "task.change_accepted").is_empty());

    // The view marks no plan item, and says which changes wait.
    turn(c, &s, "where are we").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let view = serde_json::to_string(&req.messages).unwrap();
    assert!(
        view.contains(&format!("On a task marked \\\"{}\\\"", graph::OWNERS_MARK)),
        "{view}"
    );
    assert!(
        !view.contains(&format!(", {}", graph::OWNERS_MARK)),
        "{view}"
    );
}

/// A record written before the model's mark (the layout sample) reads as the
/// owner's: a change to its acceptance waits on its card, its proposal on the
/// record; and that question, expired unanswered, leaves no proposal, with
/// `task.change_expired` written in the expiry's frame.
#[tokio::test]
async fn an_old_records_change_waits_and_its_expiry_clears_the_proposal() {
    let r = rig();
    let c = &r.core;
    let s = session(c);
    c.store
        .append(&[NewRecord::bytes(
            kinds::TASK,
            Some("tsk_00000000000000000000000000000091"),
            TASK_BEFORE_MODEL_MARK.as_bytes().to_vec(),
        )])
        .unwrap();
    let old = r.task("tsk_00000000000000000000000000000091");
    assert!(old.is_owners(), "{old:?}");
    r.calls(&[(
        "task_update",
        json!({"id": old.id, "version": 1, "patch": {"acceptance": ["the pier has a depth too"]}}),
    )]);
    turn(c, &s, "change what it takes").await;
    let asked = c.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let q = &asked[0];
    assert!(q.reason.contains("layer 1"), "{}", q.reason);
    let waiting = r.task(&old.id);
    assert_eq!(waiting.version, 1);
    assert_eq!(
        waiting.proposal.as_ref().map(|p| p.card.as_str()),
        Some(q.correlation_id.as_str())
    );

    // Nobody answers: the expiry clears the proposal in its own frame.
    let frames = theseus_store::frames_written_here();
    assert_eq!(c.expire_questions(q.expires_at_ms - 1), 0);
    assert_eq!(c.expire_questions(q.expires_at_ms), 1);
    // The clear rides in the decline's frame: two frames, as any expiry
    // with a card writes (the decline, then its card's settle).
    assert_eq!(
        theseus_store::frames_written_here(),
        frames + 2,
        "no frame of its own"
    );
    let left = r.task(&old.id);
    assert!(left.proposal.is_none(), "{left:?}");
    assert_eq!(left.version, 1);
    assert_eq!(left.acceptance, ["every buoy has a depth on the chart"]);
    let expired = rows(c, "task.change_expired");
    assert_eq!(expired.len(), 1, "{expired:?}");
    assert_eq!(expired[0].data["card"], q.correlation_id.as_str());
    until("the expired call's answer", || {
        results(c, &s, "task.update")
            .last()
            .is_some_and(|(st, _)| *st == ResultStatus::Declined)
    })
    .await;
    assert!(r.task(&old.id).proposal.is_none());
    assert_eq!(r.tasks().len(), 1);
}
