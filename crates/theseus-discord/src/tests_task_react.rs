//! A task's messages as one-tap controls (theseus-bh3t.2), end to end: the
//! owner's reaction in a task's thread acts on the task, the bot adds the
//! controls itself, and a guest's, an unknown emoji and a shared place's do
//! nothing. The rig and the helpers are `tests_task_threads`'.

use theseus_core::provider::{ProviderError, Scripted};
use theseus_sim::fake_discord::{FakeThread, Pressed};

use super::*;

const PAUSE: &str = "\u{23f8}\u{fe0f}";
const RESUME: &str = "\u{25b6}\u{fe0f}";
const STOP: &str = "\u{1f6d1}";
const RETRY: &str = "\u{1f501}";
const FOLLOW: &str = "\u{1f440}";
const PROJECT: &str = "\u{1f4cc}";
const DONE: &str = "\u{2705}";

/// A task working in its thread (gated until the test opens the gate), then
/// reporting; the thread's first message.
async fn working() -> (Rig, FakeThread, String) {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, vec![gate(dir), Scripted::text("Reef charted.")])],
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    until(&r, "the thread's first message", || {
        !r.posted(t.id).is_empty()
    })
    .await;
    let first = r.posted(t.id)[0].id.clone();
    (r, t, first)
}

fn exec_id(r: &Rig) -> String {
    r.core.kernel.tasks(None).unwrap().remove(0).id
}

fn paused_key(r: &Rig) -> Option<bool> {
    r.core
        .store
        .get_meta::<bool>(&format!("task.paused.{}", task_session(r)))
        .unwrap()
}

fn state(r: &Rig) -> String {
    r.core
        .kernel
        .execution(&exec_id(r))
        .unwrap()
        .unwrap()
        .state
        .as_str()
        .to_string()
}

fn react(r: &Rig, user: u64, channel: u64, message: &str, emoji: &str) {
    r.fake.react(user, channel, message, emoji).unwrap();
}

/// A little time for something that should not happen to have happened.
async fn quiet() {
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

#[tokio::test]
async fn the_bot_adds_the_controls_to_a_tasks_first_message_in_order() {
    let (r, _t, first) = working().await;
    until(&r, "the four controls", || {
        r.fake.reactions(&first).len() == 4
    })
    .await;
    assert_eq!(
        r.fake.reactions(&first),
        vec![PAUSE, STOP, FOLLOW, PROJECT],
        "pause, stop, follow, project, in that order"
    );
    open_gate(&r);
}

#[tokio::test]
async fn pause_parks_a_running_task_without_a_report_and_resume_lets_it_report() {
    let (r, t, first) = working().await;
    until(&r, "the controls", || r.fake.reactions(&first).len() == 4).await;
    react(&r, ANA, t.id, &first, PAUSE);
    until(&r, "the hold", || paused_key(&r) == Some(true)).await;
    until(&r, "the owner's tap acknowledged", || {
        r.fake.reactions(&first).contains(&DONE.to_string())
    })
    .await;
    // The pair flips: ▶️ in the place of ⏸.
    until(&r, "the pair flipped", || {
        let held = r.fake.reactions(&first);
        held.contains(&RESUME.to_string()) && !held.contains(&PAUSE.to_string())
    })
    .await;
    // Let its running step end: at its next loop it parks, and reports nothing.
    open_gate(&r);
    until(&r, "the task parks", || state(&r) == "waiting").await;
    quiet().await;
    assert_eq!(state(&r), "waiting");
    assert!(
        !r.posted(LAB).iter().any(|m| m.content.contains("finished")),
        "a paused task reports nothing: {:#?}",
        r.posted(LAB)
    );
    // ▶️: it goes on and reports.
    react(&r, ANA, t.id, &first, RESUME);
    until(&r, "the report", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("finished") && m.content.contains("Reef charted."))
    })
    .await;
    assert_eq!(paused_key(&r), Some(false));
}

#[tokio::test]
async fn stop_asks_in_the_place_and_only_yes_stops_the_task() {
    let (r, t, first) = working().await;
    react(&r, ANA, t.id, &first, STOP);
    until(&r, "the question", || {
        r.posted(t.id)
            .iter()
            .any(|m| m.content.contains("Stop **Reef"))
    })
    .await;
    quiet().await;
    assert_ne!(state(&r), "cancelled", "the question stops nothing");
    let exe = exec_id(&r);
    let q = r
        .posted(t.id)
        .into_iter()
        .find(|m| m.content.contains("Stop **Reef"))
        .unwrap();
    r.fake
        .press(&Pressed {
            message: &q.id,
            button: &format!("tb:yes:{exe}"),
            user: ANA,
            name: "ana",
        })
        .unwrap();
    until(&r, "the task cancelled", || state(&r) == "cancelled").await;
}

#[tokio::test]
async fn retry_on_a_failed_task_opens_a_linked_new_one() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| {
            vec![(
                REEF,
                vec![
                    gate(dir),
                    Scripted::Fail(ProviderError::InvalidRequest {
                        status: 400,
                        message: "the tool schema was refused".into(),
                    }),
                ],
            )]
        },
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    until(&r, "the thread's first message", || {
        !r.posted(t.id).is_empty()
    })
    .await;
    let first = r.posted(t.id)[0].id.clone();
    let exe = exec_id(&r);
    open_gate(&r);
    until(&r, "the task failed", || state(&r) == "failed").await;
    react(&r, ANA, t.id, &first, RETRY);
    until(&r, "a second task", || {
        r.core.kernel.tasks(None).unwrap().len() == 2
    })
    .await;
    let again = r
        .core
        .kernel
        .tasks(None)
        .unwrap()
        .into_iter()
        .find(|x| x.id != exe)
        .unwrap();
    assert_eq!(
        again.retry_of.as_deref(),
        Some(exe.as_str()),
        "linked to the failed one"
    );
    until(&r, "the tap acknowledged", || {
        r.fake.reactions(&first).contains(&DONE.to_string())
    })
    .await;
}

#[tokio::test]
async fn retry_on_a_cancelled_task_begins_it_again_from_its_brief() {
    let (r, t, first) = working().await;
    let exe = exec_id(&r);
    r.core.task_cancel_by(&exe, "the test").await.unwrap();
    until(&r, "the task cancelled", || state(&r) == "cancelled").await;
    react(&r, ANA, t.id, &first, RETRY);
    until(&r, "a second task", || {
        r.core.kernel.tasks(None).unwrap().len() == 2
    })
    .await;
    let again = r
        .core
        .kernel
        .tasks(None)
        .unwrap()
        .into_iter()
        .find(|x| x.id != exe)
        .unwrap();
    assert_eq!(
        again.retry_of, None,
        "the core does not retry a cancelled task: a new one"
    );
    let brief = theseus_core::task::brief(&r.core.store, &again.session_id)
        .unwrap()
        .expect("a brief");
    assert!(
        format!("{:?}", brief.body).contains("Reef-chart"),
        "{brief:?}"
    );
}

#[tokio::test]
async fn follow_mirrors_the_task_in_the_owners_dm() {
    let (r, t, first) = working().await;
    until(&r, "the controls", || r.fake.reactions(&first).len() == 4).await;
    react(&r, ANA, t.id, &first, FOLLOW);
    until(&r, "a mirror in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("following"))
    })
    .await;
    open_gate(&r);
}

#[tokio::test]
async fn the_pin_makes_a_project_and_attaches_the_task() {
    let (r, t, first) = working().await;
    react(&r, ANA, t.id, &first, PROJECT);
    until(&r, "the task attached to a project", || {
        r.core
            .store
            .get_session::<theseus_core::session::SessionRecord>(&task_session(&r))
            .unwrap()
            .is_some_and(|s| s.project.is_some())
    })
    .await;
    // Again: one quiet line says which, and no second project is made.
    react(&r, ANA, t.id, &first, PROJECT);
    until(&r, "the quiet line", || {
        r.posted(t.id).iter().any(|m| m.content.contains("already"))
    })
    .await;
    assert_eq!(
        theseus_core::task_graph::all(&r.core.store)
            .unwrap()
            .iter()
            .filter(|x| x.kind == theseus_protocol::tasks::TaskKind::Project)
            .count(),
        1
    );
    open_gate(&r);
}

#[tokio::test]
async fn a_guest_an_unknown_emoji_and_a_shared_place_do_nothing() {
    let (r, t, first) = working().await;
    until(&r, "the controls", || r.fake.reactions(&first).len() == 4).await;
    let before = r.posted(t.id).len();
    // A guest of the channel taps ⏸.
    react(&r, CY, t.id, &first, PAUSE);
    // The owner taps something that is no control.
    react(&r, ANA, t.id, &first, "\u{1f642}");
    quiet().await;
    assert_eq!(paused_key(&r), None, "nothing was held");
    assert_eq!(state(&r), "running");
    assert_eq!(r.posted(t.id).len(), before, "nothing was said");
    assert!(!r.fake.reactions(&first).contains(&DONE.to_string()));
    open_gate(&r);
}

const NOT_THIS: &str = "\u{1f44e}";

/// The task's record says it started itself, as a `task.create` in a turn
/// with no owner input would have written it.
fn started_itself(r: &Rig) {
    let id = theseus_core::task_graph::of_session(&task_session(r));
    let mut t = theseus_core::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap();
    t.origin.self_started = true;
    t.origin.because = Some("the reef survey was left unfinished".into());
    r.core
        .store
        .append(&[theseus_core::task_graph::record(&t).unwrap()])
        .unwrap();
}

fn preferences(r: &Rig) -> usize {
    let all = r.core.runner.standing.all(&r.core.store).unwrap();
    theseus_core::standing::live(&all, None, theseus_protocol::now_unix_ms())
        .into_iter()
        .filter(|p| p.source == theseus_protocol::task_not_this::SOURCE)
        .count()
}

/// 👎 on a task that started itself cancels it and keeps one preference.
#[tokio::test]
async fn a_thumbs_down_on_a_task_that_started_itself_says_not_this() {
    let (r, t, first) = working().await;
    until(&r, "the controls", || r.fake.reactions(&first).len() == 4).await;
    started_itself(&r);
    react(&r, ANA, t.id, &first, NOT_THIS);
    until(&r, "the task is cancelled", || state(&r) == "cancelled").await;
    assert_eq!(preferences(&r), 1);
    open_gate(&r);
}

/// 👎 on a task the owner asked for is no control: nothing happens to it.
#[tokio::test]
async fn a_thumbs_down_on_a_task_the_owner_asked_for_does_nothing() {
    let (r, t, first) = working().await;
    until(&r, "the controls", || r.fake.reactions(&first).len() == 4).await;
    react(&r, ANA, t.id, &first, NOT_THIS);
    quiet().await;
    assert_eq!(state(&r), "running");
    assert_eq!(preferences(&r), 0);
    open_gate(&r);
}

#[tokio::test]
async fn in_a_shared_channel_a_tap_on_a_tasks_thread_does_nothing() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |_| vec![(REEF, answers("Reef charted."))],
        false,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    until(&r, "the thread's first message", || {
        !r.posted(t.id).is_empty()
    })
    .await;
    let first = r.posted(t.id)[0].id.clone();
    until(&r, "the task ended", || state(&r) == "complete").await;
    react(&r, ANA, t.id, &first, PAUSE);
    react(&r, ANA, t.id, &first, STOP);
    quiet().await;
    assert!(
        !r.posted(t.id).iter().any(|m| m.content.contains("Stop **")
            || m.content.contains('⚠')
            || m.reply_to.is_some()),
        "no question, no line: {:#?}",
        r.posted(t.id)
    );
    assert!(!r.fake.reactions(&first).contains(&DONE.to_string()));
    assert_eq!(paused_key(&r), None);
}
