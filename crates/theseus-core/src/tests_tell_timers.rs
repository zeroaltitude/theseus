//! A routine a conversation sets up, and a project with a review routine
//! (issue 57): with nothing set on their tools they are made at once and the
//! owner is told, with `routine.delete` as the undo. What a run spends does
//! not come back, so the first run waits at least ten minutes after it is set
//! up; where the owner set the tool to `approve`, the card asks and his yes
//! leaves the schedule as it is.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::owner_note::told::Undo;
use crate::policy::Posture;
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_tasks::{answers_a_call, rig, turn, until, Rig};
use crate::tests_tell_tasks::{notes, press};
use crate::{Config, Core};

const BRIEF: &str = "Read the tide table and say when the next high tide is.";
/// The least wait before a told routine's first run, in ms: ten minutes.
const HOLD_MS: u64 = 10 * 60_000;

/// A conversation in the owner's private DM.
fn dm(core: &Arc<Core>) -> String {
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place("dm:42", &rec.session_id).unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    rec.session_id
}

/// The model makes one call, `name` with `input`, then says it is done.
fn calling(
    name: &'static str,
    input: Value,
) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    move |req| {
        if answers_a_call(req) {
            return Scripted::text("Done.");
        }
        Scripted::tools("Setting it up.", &[("t_1", name, input.clone())])
    }
}

/// A schedule a minute apart is allowed here.
fn minute_apart(cfg: &mut Config) {
    cfg.kernel.min_repeat_minutes = 1;
}

fn routines(r: &Rig) -> Vec<Arc<theseus_protocol::routines::RoutineSpec>> {
    r.core.runner.tools.routines.all(&r.core.store).unwrap()
}

/// When a routine's first run is due.
fn first_run(r: &Rig, execution: &str) -> u64 {
    r.core
        .kernel
        .execution(execution)
        .unwrap()
        .unwrap()
        .wakes
        .first()
        .unwrap()
        .due_at_ms
}

/// With nothing set on `routine.create`, a routine every minute is made with
/// no question, its first run is ten minutes away at least, and its delete
/// (the note's undo) ends it.
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_is_made_unasked_its_first_run_waits_and_its_delete_ends_it() {
    let r = rig(
        calling(
            "routine_create",
            json!({"title": "Tide check", "schedule": "every 1m", "brief": BRIEF}),
        ),
        minute_apart,
    );
    let s = dm(&r.core);
    let t0 = r.core.kernel.now_ms();
    turn(&r.core, &s, "check the tide every minute").await;
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "nothing is asked"
    );
    let made = routines(&r);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        first_run(&r, &made[0].execution_id) >= t0 + HOLD_MS,
        "the first run waits ten minutes"
    );
    let told = notes(&r.core);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    let (text, how) = &told[0];
    assert!(
        text.starts_with("ℹ️ FYI · A routine was set up\nIn conversation "),
        "{text}"
    );
    assert!(
        text.contains("without asking you")
            && text.contains("runs every 1m")
            && text.contains("$0.50"),
        "{text}"
    );
    assert!(
        !text.contains("Tide check") && !text.contains("tide table"),
        "{text}"
    );
    assert_eq!(*how, Undo::RoutineDelete(made[0].session_id.clone()));
    let gone = press(&r.core, how).await;
    assert!(gone.is_ok(), "{gone:?}");
    assert!(routines(&r).is_empty(), "its delete ends it");
}

/// With `routine.create = "approve"` set, the routine waits on its card, and
/// the owner's yes makes it on its own schedule: he saw it before it ran.
#[tokio::test(flavor = "multi_thread")]
async fn his_approve_on_routine_create_asks_and_his_yes_keeps_its_schedule() {
    let r = rig(
        calling(
            "routine_create",
            json!({"title": "Tide check", "schedule": "every 1m", "brief": BRIEF}),
        ),
        |cfg| {
            minute_apart(cfg);
            cfg.policy
                .tools
                .insert("routine.create".into(), Posture::Approve);
        },
    );
    let s = dm(&r.core);
    turn(&r.core, &s, "check the tide every minute").await;
    let asked = r.core.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(routines(&r).is_empty(), "nothing exists before the yes");
    let t0 = r.core.kernel.now_ms();
    r.core
        .confirm_action(&asked[0].correlation_id, true, None, "cli")
        .unwrap();
    until("the routine", || routines(&r).len() == 1).await;
    assert!(
        first_run(&r, &routines(&r)[0].execution_id) < t0 + HOLD_MS,
        "his yes holds nothing back"
    );
    assert!(notes(&r.core).is_empty(), "asked, not told");
}

/// With nothing set on `task.create`, a project with a review every minute
/// is made with no question, and its review's first run waits ten minutes.
#[tokio::test(flavor = "multi_thread")]
async fn a_projects_review_is_made_unasked_and_its_first_run_waits() {
    let r = rig(
        calling(
            "task_create",
            json!({"kind": "project", "title": "Harbour survey", "review": "every 1m"}),
        ),
        minute_apart,
    );
    let s = dm(&r.core);
    let t0 = r.core.kernel.now_ms();
    turn(&r.core, &s, "start a harbour survey project").await;
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "nothing is asked"
    );
    let made = routines(&r);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        first_run(&r, &made[0].execution_id) >= t0 + HOLD_MS,
        "the first run waits ten minutes"
    );
    let told = notes(&r.core);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    assert!(
        told[0]
            .0
            .starts_with("ℹ️ FYI · A project's weekly review was set up\n"),
        "{}",
        told[0].0
    );
    assert_eq!(told[0].1, Undo::RoutineDelete(made[0].session_id.clone()));
}
