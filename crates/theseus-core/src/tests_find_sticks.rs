//! A family a `tools_find` loaded is not dropped as if a wake had loaded it (theseus-hhxqn): the
//! drop `Deferral::dropped` makes at a person's turn is for the tasks family a wake's loosening
//! put in (theseus-o1h3f.3), never for one the model found itself (theseus-342qy). The model has
//! no tool search here, so `tools_find` is its way to a hidden family.

use std::sync::Arc;

use serde_json::json;

use crate::bus::EventSink;
use crate::catalog::CatalogRow;
use crate::provider::{ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_places::{answers_a_call, last_user, offered, rig_deferring, session, Rig};
use crate::turn::TurnRequest;
use crate::Core;

const DM: &str = "dm:271828182845904523";

/// `FINDTASK` and `FINDTERM` ask `tools_find` for the task tools and for a terminal; `WAKE`
/// sets a one-second wake; anything else answers.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = last_user(req);
    let find = |q: &str| Scripted::tools("", &[("f1", "tools_find", json!({ "query": q }))]);
    if last.contains("FINDTASK") {
        return find("create a task");
    }
    if last.contains("FINDTERM") {
        return find("open a terminal");
    }
    if last.starts_with("WAKE") {
        return Scripted::tools(
            "",
            &[(
                "w1",
                "wake_at",
                json!({"after": "1s", "note": "look again"}),
            )],
        );
    }
    Scripted::text("Hello.")
}

fn rig() -> Rig {
    rig_deferring(
        |c| {
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                CatalogRow {
                    tool_search: Some(false),
                    ..Default::default()
                },
            );
        },
        script,
    )
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            answer_at: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
        })
        .await
        .unwrap();
}

fn has(r: &Rig, tool: &str) -> bool {
    offered(&r.requests().pop().expect("a request")).contains(&tool.to_string())
}

/// The wake's turn: a person's `WAKE` sets it, and it is the third request.
async fn wake(r: &Rig, sid: &str) {
    let before = r.requests().len();
    say(&r.core, sid, "WAKE me").await;
    r.until("the wake's turn", 30, move |r| {
        r.requests().len() >= before + 3
    })
    .await;
}

/// Guard, not red-first: a family a wake loaded is dropped at the next person's turn.
#[tokio::test]
async fn a_family_a_wake_loaded_is_still_dropped_at_the_next_persons_turn() {
    let r = rig();
    let sid = session(&r.core, Some(DM));
    wake(&r, &sid).await;
    assert!(has(&r, "task_create"), "the wake's turn has the tasks");
    say(&r.core, &sid, "hello").await;
    assert!(!has(&r, "task_create"), "a person's turn drops the wake's");
}

/// The row: a family the model found in a person's turn is kept at the next person's turn.
#[tokio::test]
async fn a_family_tools_find_loaded_is_not_dropped_at_the_next_persons_turn() {
    let r = rig();
    let sid = session(&r.core, Some(DM));
    say(&r.core, &sid, "FINDTASK").await;
    assert!(has(&r, "task_create"), "the find loaded the tasks");
    say(&r.core, &sid, "hello").await;
    assert!(has(&r, "task_create"), "the find's family stays");
}

/// Both mechanisms in one session, on DIFFERENT families: the find's terminals stay through a wake
/// and a person's turn, the wake's tasks go. This does NOT demonstrate the Find/wake distinction --
/// `Deferral::dropped` can only ever hold the tasks family, so the `term_open` assertion is outside
/// its reach and would hold however the find were treated. The distinction test is the next one.
#[tokio::test]
async fn a_finds_family_stays_and_a_wakes_goes_in_one_session() {
    let r = rig();
    let sid = session(&r.core, Some(DM));
    say(&r.core, &sid, "FINDTERM").await;
    assert!(has(&r, "term_open"), "the find loaded the terminals");
    wake(&r, &sid).await;
    assert!(has(&r, "task_create"), "the wake loaded the tasks");
    assert!(has(&r, "term_open"));
    say(&r.core, &sid, "hello").await;
    assert!(!has(&r, "task_create"), "the wake's family is dropped");
    assert!(has(&r, "term_open"), "the find's family stays");
}

/// THE DISTINCTION TEST: a find of the tasks, then a wake that loads them too, then a person's turn --
/// the find's holds. Both mechanisms on the SAME family, which is the only shape that can tell them
/// apart, because the tasks family is the only one a wake loads and the only one `dropped` can name.
#[tokio::test]
async fn the_tasks_a_find_loaded_survive_a_wake_that_loads_them_too() {
    let r = rig();
    let sid = session(&r.core, Some(DM));
    say(&r.core, &sid, "FINDTASK").await;
    wake(&r, &sid).await;
    say(&r.core, &sid, "hello").await;
    assert!(has(&r, "task_create"), "the find's tasks stay");
}
