//! A task that started itself, and why (theseus-o1h3f.1): the read of the
//! turn that made the call, the `because` a self-started call must give and
//! its cap, and the old record that reads unchanged. The pure rule is
//! `task_self::Own::of`.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::tasks::{TaskRecord, BECAUSE_MAX};

use crate::node::ResultStatus;
use crate::provider::Scripted;
use crate::task_self::Own;
use crate::tests_task_graph::{results, until};
use crate::tests_tasks::{answers_a_call, arranged, first_user, parent_session, rig, Rig, ASK};

/// A parent that starts one task whose report wakes it, and then, in that
/// wake's turn (no one wrote in it), makes `wake_call`. A turn that opens with
/// `PLAN` makes `owner_call` at once.
fn rig_calling(owner_call: Value, wake_call: Value) -> Rig {
    let calls = Arc::new(Mutex::new((owner_call, wake_call)));
    rig(
        move |req| {
            let first = first_user(req);
            if first.contains("CHILD") {
                return Scripted::text("Surveyed.");
            }
            if answers_a_call(req) {
                return Scripted::text("Done.");
            }
            let (owner_call, wake_call) = calls.lock().unwrap().clone();
            let seen = serde_json::to_string(&req.messages).unwrap();
            if seen.contains("[Report from task") {
                return Scripted::tools("Following up.", &[("t_wake", "task_create", wake_call)]);
            }
            if first.starts_with("PLAN") {
                return Scripted::tools("Planning.", &[("t_plan", "task_create", owner_call)]);
            }
            Scripted::tools(
                "Starting.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: survey the north channel", "budget_usd": 1.5,
                           "wake_parent": true, "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
    )
}

/// Start the task and wait for the wake's turn to make its call.
async fn after_the_wake(r: &Rig) -> String {
    let parent = parent_session(&r.core);
    crate::tests_tasks::turn(&r.core, &parent, ASK).await;
    until("the wake's call", || {
        !results(&r.core, &parent, "task.create").is_empty()
            && results(&r.core, &parent, "task.create").len() >= 2
    })
    .await;
    parent
}

fn titled(r: &Rig, title: &str) -> Option<TaskRecord> {
    crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.title == title)
}

/// The owner's turn: both flags are as they were, and the stored record's
/// bytes carry neither new key.
#[tokio::test]
async fn a_task_the_owner_asked_for_carries_no_new_key() {
    let r = rig_calling(json!({"title": "Chart the buoys"}), json!({}));
    let parent = parent_session(&r.core);
    crate::tests_tasks::turn(&r.core, &parent, "PLAN: chart the buoys").await;
    let t = titled(&r, "Chart the buoys").unwrap();
    assert!(!t.origin.self_started);
    assert!(t.origin.because.is_none());
    assert!(t.origin.by_model, "a plan item is the model's, as before");
    let bytes = serde_json::to_string(&t.origin).unwrap();
    assert!(
        !bytes.contains("self_started") && !bytes.contains("because"),
        "{bytes}"
    );
}

/// A `because` the owner's turn gives is not kept: the task is the owner's.
#[tokio::test]
async fn a_because_on_the_owners_turn_is_not_kept() {
    let r = rig_calling(
        json!({"title": "Chart the buoys", "because": "no one needs this"}),
        json!({}),
    );
    let parent = parent_session(&r.core);
    crate::tests_tasks::turn(&r.core, &parent, "PLAN: chart the buoys").await;
    let t = titled(&r, "Chart the buoys").unwrap();
    assert!(!t.origin.self_started && t.origin.because.is_none());
}

/// A wake's turn has no owner input: its task is self-started, by the model,
/// with the reason it gave.
#[tokio::test]
async fn a_task_a_wake_opens_is_self_started_with_its_reason() {
    let r = rig_calling(
        json!({}),
        json!({"title": "Re-chart the shoal", "because": "the survey report named a shoal it missed"}),
    );
    after_the_wake(&r).await;
    let t = titled(&r, "Re-chart the shoal").expect("the wake's task was opened");
    assert!(t.origin.self_started);
    assert!(t.origin.by_model);
    assert_eq!(
        t.origin.because.as_deref(),
        Some("the survey report named a shoal it missed")
    );
    assert_eq!(
        t.origin.self_line().as_deref(),
        Some("◇ self: the survey report named a shoal it missed")
    );
}

/// A self-started call with no `because` is refused in words that name it,
/// and opens nothing.
#[tokio::test]
async fn a_self_started_call_without_a_reason_is_refused_and_opens_nothing() {
    let r = rig_calling(json!({}), json!({"title": "Re-chart the shoal"}));
    let parent = after_the_wake(&r).await;
    let last = results(&r.core, &parent, "task.create").pop().unwrap();
    assert_eq!(last.0, ResultStatus::Error);
    assert!(last.1.contains("`because`"), "{}", last.1);
    assert!(
        titled(&r, "Re-chart the shoal").is_none(),
        "no task is created"
    );
}

/// The refusal asks for `because`, so the declared schema, which closes itself
/// to other properties, must let the model send it: a model that honours the
/// schema could otherwise never start a task on its own.
#[test]
fn the_schema_declares_the_because_the_refusal_asks_for() {
    use theseus_tools::Tool;
    let schema = crate::task::TaskCreate.input_schema();
    assert_eq!(
        schema["properties"]["because"]["type"], "string",
        "{schema}"
    );
    assert_eq!(schema["additionalProperties"], false);
}

/// The cap, as it is enforced: a reason over 160 characters is refused with
/// the limit named; one of 160 is kept whole.
#[test]
fn the_reason_is_held_to_160_characters() {
    let long = "x".repeat(BECAUSE_MAX + 1);
    let e = Own::of(true, Some(&long)).unwrap_err();
    assert!(e.contains("161") && e.contains("160"), "{e}");
    assert!(Own::of(false, Some(&long)).is_err(), "on any turn");
    let fits = "y".repeat(BECAUSE_MAX);
    let own = Own::of(true, Some(&fits)).unwrap();
    assert_eq!(own.because.as_deref(), Some(fits.as_str()));
    // Characters, not bytes.
    let wide = "é".repeat(BECAUSE_MAX);
    assert!(Own::of(true, Some(&wide)).is_ok());
}

/// The rule itself: required on a self-started turn (blank counts as none),
/// optional and dropped on the owner's.
#[test]
fn the_reason_is_required_only_when_the_task_starts_itself() {
    assert!(Own::of(true, None).unwrap_err().contains("`because`"));
    assert!(Own::of(true, Some("  ")).is_err());
    assert_eq!(Own::of(false, None).unwrap(), Own::owners());
    assert_eq!(Own::of(false, Some("why")).unwrap(), Own::owners());
}

/// A record written before the fields reads as the owner's, and says so.
#[test]
fn a_record_from_before_reads_as_not_self_started() {
    let old = r#"{"id":"tsk_reef","version":1,"title":"Chart the reef","state":"accepted",
        "owner":"agent","origin":{"session":"ses_0000reef","principal":"agent","by_model":true},
        "created_at_ms":1,"updated_at_ms":1}"#;
    let t: TaskRecord = serde_json::from_str(old).unwrap();
    assert!(!t.origin.self_started && t.origin.because.is_none());
    assert!(t.origin.self_line().is_none());
    let origin = r#"{"session":"ses_0000reef","principal":"agent"}"#;
    let o: theseus_protocol::tasks::TaskOrigin = serde_json::from_str(origin).unwrap();
    assert!(!o.by_model && !o.self_started);
}
