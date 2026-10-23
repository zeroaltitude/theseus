//! A message that asks for a task loads the tasks family for that turn (theseus-gg78p). In a DM
//! the family is deferred (the owner's rule), and a model that never searched took `proc_run`'s
//! `background` job for "start two background tasks". The rig is `tests_places`'s, in a DM.

use serde_json::Value;

use crate::catalog::CatalogRow;
use crate::provider::ProviderRequest;
use crate::tests_places::{rig_deferring, session, turn, Rig, OWNER};
use crate::tools_defer::PlaceKind;

/// The owner's words of the locked flow's first step, with invented paths.
const STEP_ONE: &str =
    "Start two background tasks, $1 each: A) bash ~/work/slow.sh census 3; B) slow.sh load 3";

fn rig(search: bool) -> Rig {
    rig_deferring(
        move |c| {
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                CatalogRow {
                    tool_search: Some(search),
                    ..Default::default()
                },
            );
        },
        |_| crate::provider::Scripted::text("ok"),
    )
}

/// Whether the first request to the model loads `task_create`: offered, and not deferred.
async fn loads_tasks(search: bool, message: &str) -> bool {
    let r = rig(search);
    let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &sid, message).await;
    let req: ProviderRequest = r.requests().remove(0);
    let tool = req
        .tools
        .iter()
        .find(|t: &&Value| t["name"] == "task_create");
    tool.is_some_and(|t| t["defer_loading"] != true)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_message_asking_for_background_tasks_loads_the_tasks_family() {
    assert!(loads_tasks(true, STEP_ONE).await, "tool search on");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_message_asking_for_background_tasks_loads_the_tasks_family_without_tool_search() {
    assert!(loads_tasks(false, STEP_ONE).await, "tool search off");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn everyday_task_words_do_not_load_the_family() {
    for message in [
        "what tasks are open?",
        "my tasks today are mostly email",
        "how do I finish a task faster",
        "list the tasks you did yesterday",
        "good morning",
    ] {
        for search in [true, false] {
            assert!(!loads_tasks(search, message).await, "{message} ({search})");
        }
    }
}

#[test]
fn the_dm_defaults_still_defer_tasks() {
    assert_eq!(PlaceKind::Dm.defaults(), ["memory", "reminders", "web"]);
}
