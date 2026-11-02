//! `profile.pins` and `profile.unpin` (theseus-h66v4.2): the owner's "unpin all current live profiles and
//! tasks and sessions". A session or a task is pinned when `session.profile` holds it to a profile (META
//! `session.profile.<id>`; a task's own session is its task). Every test counts the positive: N pinned
//! before, N rows written, 0 after, beside the totals that make "0 pinned" more than "0 sessions".
//! The live profile's own pin is `profile.release`'s, and the same-name tests below hold the `dq3m1` line:
//! `released` and `cleared` are carried, never read off two names that happen to match.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::ops::{ProfilePinsResult, ProfileUnpinResult, SessionProfileParams};
use theseus_protocol::SessionKind;

use super::Core;
use crate::approval::{Client, Surface};
use crate::provider::FakeProvider;
use crate::rpc::Parts;
use crate::session::{SessionRecord, TaskOf};
use crate::store::Store;
use crate::test_support::{owner_cli, rows, stranger};
use crate::Config;

fn core_on(store: Store) -> Arc<Core> {
    let mut c = Config::example();
    c.serve_glm_unrestricted();
    Core::build(Parts::for_tests(
        c,
        Arc::new(FakeProvider::default()),
        store,
    ))
    .unwrap()
}

fn core() -> Arc<Core> {
    let dir = crate::scratch::dir("t");
    core_on(Store::open(&dir.join("store")).unwrap())
}

fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

fn task(core: &Core) -> String {
    let parent = session(core);
    let mut rec = SessionRecord::new(SessionKind::Task, None);
    rec.task = Some(TaskOf {
        parent_session: parent,
        parent_execution: "exe_none".into(),
        by: "call_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    core.store.put_session(&rec.session_id, &rec).unwrap();
    // Opened with no profile: its brief picks its model, so it is not start-pinned.
    core.store
        .append(&[crate::turn::brief_step::mark(&rec.session_id).unwrap()])
        .unwrap();
    rec.session_id
}

/// A task `task.start --profile` opened and whose first turn has not run: its own execution, no brief mark,
/// the model it was started on in `last_target`, and the `task.started_by_owner` row only that call writes.
/// Returns the task's session and its execution.
fn start_pinned(core: &Core, profile: &str) -> (String, String) {
    made(core, profile, true)
}

/// A task Theseus made on a model of its own choosing (a child task, a check, a reviewer, a routine's run, a
/// retry): the same record, and no `task.started_by_owner` row.
fn fixed(core: &Core, profile: &str) -> (String, String) {
    made(core, profile, false)
}

fn made(core: &Core, profile: &str, by_owner: bool) -> (String, String) {
    let parent = session(core);
    let rec = core
        .open_session(theseus_protocol::SessionOpenParams {
            kind: Some(SessionKind::Task),
            ..Default::default()
        })
        .unwrap();
    let exec = rec.execution_id.clone().unwrap();
    let mut rec = core
        .store
        .get_session::<SessionRecord>(&rec.session_id)
        .unwrap()
        .unwrap();
    rec.task = Some(TaskOf {
        parent_session: parent,
        parent_execution: "exe_none".into(),
        by: "call_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    rec.last_target = Some(crate::session::TargetRef {
        profile: profile.into(),
        provider: "p".into(),
        model: "m".into(),
    });
    core.store.put_session(&rec.session_id, &rec).unwrap();
    if by_owner {
        let fact = crate::fact::task_start::TaskStartedByOwner {
            short: "abc123",
            title: "t",
            session_id: &rec.session_id,
            home_session: "ses_home",
            limit: 0,
            available_before: 0,
            quotes: 0,
            board: false,
            starts_at_ms: None,
            by: "the CLI",
            via: "cli",
        };
        core.store
            .append(&[crate::fact::row(&fact, Some(&rec.session_id), None).unwrap()])
            .unwrap();
    }
    (rec.session_id, exec)
}

/// A task as it stands after its first turn: `turns` run, its execution in `state`.
fn past_first_turn(core: &Core, sid: &str, exec: &str, state: theseus_kernel::ExecState) {
    let mut rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    rec.turns = 1;
    core.store.put_session(sid, &rec).unwrap();
    exec_state(core, exec, state);
}

fn exec_state(core: &Core, id: &str, state: theseus_kernel::ExecState) {
    use theseus_store::Store as _;
    let mut e = core.kernel.execution(id).unwrap().unwrap();
    e.state = state;
    core.store
        .inner()
        .append(&[
            theseus_store::NewRecord::json(theseus_store::kinds::EXECUTION, Some(id), &e).unwrap(),
        ])
        .unwrap();
}

fn hold(core: &Core, sid: &str, profile: &str) {
    let r = core
        .session_set_profile(
            &SessionProfileParams {
                session_id: sid.into(),
                profile: Some(profile.into()),
                author: None,
                discord: None,
            },
            &owner_cli(),
        )
        .unwrap();
    assert_eq!(r.profile.as_deref(), Some(profile), "{r:?}");
}

fn held(core: &Core, sid: &str) -> Value {
    core.store
        .get_meta::<Value>(&format!("session.profile.{sid}"))
        .unwrap()
        .unwrap_or(Value::Null)
}

/// A request over a connection from `surface`: the response, its error kept.
async fn wire(
    core: &Arc<Core>,
    surface: Surface,
    method: &str,
    params: Value,
) -> theseus_protocol::Response {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, server) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(
        core.clone()
            .serve_connection(sr, sw, Client::new("test", surface)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    r
}

async fn pins(core: &Arc<Core>) -> ProfilePinsResult {
    let r = wire(core, Surface::Cli, "profile.pins", Value::Null).await;
    serde_json::from_value(r.result.expect("an answer")).unwrap()
}

async fn unpin(core: &Arc<Core>, all: bool) -> ProfileUnpinResult {
    let r = wire(core, Surface::Cli, "profile.unpin", json!({"all": all})).await;
    serde_json::from_value(
        r.result
            .unwrap_or_else(|| panic!("an answer: {:?}", r.error)),
    )
    .unwrap()
}

/// Two conversations and two tasks held, one of each left alone (and a parent conversation for each task): the read counts the pinned and the
/// totals, and names each pin's kind and profile.
fn rig(core: &Core) -> (Vec<String>, Vec<String>) {
    let (a, b, free) = (session(core), session(core), session(core));
    let (t1, t2, tfree) = (task(core), task(core), task(core));
    hold(core, &a, "glm");
    hold(core, &b, "sonnet");
    hold(core, &t1, "haiku");
    hold(core, &t2, "glm");
    let _ = (free, tfree);
    (vec![a, b], vec![t1, t2])
}

#[tokio::test]
async fn pins_counts_the_held_sessions_and_tasks_beside_their_totals() {
    let core = core();
    let (ses, tasks) = rig(&core);
    let r = pins(&core).await;
    // Each task opens beside a parent conversation, so six conversations and three tasks.
    assert_eq!((r.sessions, r.tasks), (6, 3), "{r:?}");
    assert_eq!((r.pinned_sessions, r.pinned_tasks), (2, 2), "{r:?}");
    assert_eq!(r.pins.len(), 4, "{r:?}");
    for s in &ses {
        let p = r.pins.iter().find(|p| &p.session_id == s).unwrap();
        assert_eq!((p.kind.as_str(), p.configured), ("session", true));
    }
    for t in &tasks {
        let p = r.pins.iter().find(|p| &p.session_id == t).unwrap();
        assert_eq!((p.kind.as_str(), p.configured), ("task", true));
    }
    let on = |name: &str| r.pins.iter().filter(|p| p.profile == name).count();
    assert_eq!((on("glm"), on("sonnet"), on("haiku")), (2, 1, 1));
    assert_eq!(r.live_source, "config");
}

#[tokio::test]
async fn pins_with_nothing_held_still_counts_what_there_is() {
    let core = core();
    session(&core);
    session(&core);
    task(&core);
    let r = pins(&core).await;
    assert_eq!((r.sessions, r.tasks), (3, 1), "{r:?}");
    assert_eq!((r.pinned_sessions, r.pinned_tasks, r.pins.len()), (0, 0, 0));
}

#[tokio::test]
async fn a_released_hold_is_no_longer_counted_and_a_hold_on_a_dropped_profile_is() {
    let core = core();
    let (a, b) = (session(&core), session(&core));
    hold(&core, &a, "glm");
    core.session_set_profile(
        &SessionProfileParams {
            session_id: a.clone(),
            profile: None,
            author: None,
            discord: None,
        },
        &owner_cli(),
    )
    .unwrap();
    // A hold written for a profile the config no longer has: still on record, still a pin.
    core.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&format!("session.profile.{b}")),
            &json!({"profile": "gone"}),
        )
        .unwrap()])
        .unwrap();
    let r = pins(&core).await;
    assert_eq!((r.pinned_sessions, r.pinned_tasks), (1, 0), "{r:?}");
    let p = &r.pins[0];
    assert_eq!(
        (p.session_id.as_str(), p.profile.as_str(), p.configured),
        (b.as_str(), "gone", false)
    );
}

#[tokio::test]
async fn unpin_all_lets_go_of_every_hold_and_writes_a_row_for_each() {
    let core = core();
    let (ses, tasks) = rig(&core);
    let before_rows = rows(&core, "session.profile_set").len();
    assert_eq!(before_rows, 4, "the four holds' own rows");
    let r = unpin(&core, true).await;
    assert!(r.all);
    assert_eq!(
        (r.before.pinned_sessions, r.before.pinned_tasks),
        (2, 2),
        "{r:?}"
    );
    assert_eq!(r.cleared.len(), 4, "{r:?}");
    assert!(r.failed.is_empty(), "{r:?}");
    assert_eq!(
        (
            r.after.pinned_sessions,
            r.after.pinned_tasks,
            r.after.pins.len()
        ),
        (0, 0, 0)
    );
    // The totals did not move: nothing was deleted to get to zero.
    assert_eq!((r.after.sessions, r.after.tasks), (6, 3));
    let all = rows(&core, "session.profile_set");
    let lets_go: Vec<&Value> = all.iter().filter(|d| d["to"].is_null()).collect();
    assert_eq!(all.len() - before_rows, 4, "one row each: {all:?}");
    assert_eq!(lets_go.len(), 4, "{all:?}");
    for sid in ses.iter().chain(tasks.iter()) {
        let row = lets_go
            .iter()
            .find(|d| d["session_id"] == json!(sid))
            .unwrap();
        assert!(
            row["from"].is_string(),
            "the row names what it let go: {row}"
        );
        assert_eq!(held(&core, sid), Value::Null, "{sid} is let go");
    }
    let again = pins(&core).await;
    assert_eq!((again.pinned_sessions, again.pinned_tasks), (0, 0));
}

#[tokio::test]
async fn unpin_without_all_releases_the_live_pin_and_leaves_the_holds() {
    let core = core();
    rig(&core);
    let used = wire(&core, Surface::Cli, "profile.use", json!({"name": "glm"})).await;
    assert!(used.error.is_none());
    let r = unpin(&core, false).await;
    assert!(!r.all);
    assert!(r.released, "the live pin was cleared: {r:?}");
    assert_eq!((r.previous.as_str(), r.live.as_str()), ("glm", "sonnet"));
    assert_eq!(r.before.live_source, "runtime");
    assert_eq!(r.after.live_source, "config");
    assert_eq!(r.cleared.len(), 0);
    assert_eq!(
        (r.before.pins.len(), r.after.pins.len()),
        (4, 4),
        "the holds are still there, and the answer says so"
    );
    let clears = rows(&core, "session.profile_set")
        .into_iter()
        .filter(|d| d["to"].is_null())
        .count();
    assert_eq!(clears, 0);
}

/// The `dq3m1` line. The live pin and the config's `[model] live` are one name (`sonnet`), and a session
/// is held to that same name. `previous == live` and `profile == live`, so only the carried facts tell a
/// clear from nothing: first unpin releases AND clears, the second does neither.
#[tokio::test]
async fn a_pin_on_the_same_name_as_the_config_is_still_cleared_and_said_so() {
    let core = core();
    let s = session(&core);
    hold(&core, &s, "sonnet");
    let used = wire(
        &core,
        Surface::Cli,
        "profile.use",
        json!({"name": "sonnet"}),
    )
    .await;
    assert!(used.error.is_none());
    let first = unpin(&core, true).await;
    assert_eq!(
        (first.previous.as_str(), first.live.as_str()),
        ("sonnet", "sonnet")
    );
    assert!(first.released, "{first:?}");
    assert_eq!(first.cleared.len(), 1, "{first:?}");
    assert_eq!(first.cleared[0].profile, "sonnet");
    assert_eq!(first.before.pins.len(), 1);
    assert_eq!(first.after.pins.len(), 0);
    assert_eq!(held(&core, &s), Value::Null);
    let second = unpin(&core, true).await;
    assert_eq!(
        (second.previous.as_str(), second.live.as_str()),
        ("sonnet", "sonnet")
    );
    assert!(!second.released, "{second:?}");
    assert!(second.cleared.is_empty(), "{second:?}");
    assert_eq!((second.before.sessions, second.before.pins.len()), (1, 0));
}

#[tokio::test]
async fn unpin_all_with_nothing_pinned_writes_nothing() {
    let core = core();
    session(&core);
    task(&core);
    let r = unpin(&core, true).await;
    assert!(!r.released);
    assert!(r.cleared.is_empty() && r.failed.is_empty(), "{r:?}");
    assert_eq!((r.before.sessions, r.before.tasks), (2, 1));
    assert!(rows(&core, "session.profile_set").is_empty());
    assert!(rows(&core, "profile.changed").is_empty());
}

/// A hold on a profile the config dropped is let go like any other.
#[tokio::test]
async fn a_hold_on_a_dropped_profile_is_let_go() {
    let core = core();
    let s = session(&core);
    core.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&format!("session.profile.{s}")),
            &json!({"profile": "gone"}),
        )
        .unwrap()])
        .unwrap();
    let r = unpin(&core, true).await;
    assert_eq!(r.cleared.len(), 1, "{r:?}");
    assert!(!r.cleared[0].configured);
    assert_eq!(held(&core, &s), Value::Null);
    assert_eq!(r.after.pins.len(), 0);
}

/// A person who is not the owner at a private place changes nothing: the holds stay, each refusal is
/// named, and `after` counts what is still pinned (the same as `before`), never a swallowed zero.
#[tokio::test]
async fn a_refused_unpin_reports_the_failures_and_leaves_the_holds() {
    let core = core();
    let (ses, tasks) = rig(&core);
    let (cleared, failed) = core.unpin_holds(&stranger());
    assert!(cleared.is_empty(), "{cleared:?}");
    assert_eq!(failed.len(), 4, "{failed:?}");
    for f in &failed {
        assert!(ses.contains(&f.session_id) || tasks.contains(&f.session_id));
        assert!(!f.error.is_empty());
    }
    let r = core.profile_pins().unwrap();
    assert_eq!((r.pinned_sessions, r.pinned_tasks), (2, 2));
}

/// Over a wire that is not a private place (a Discord channel, an MCP client) the whole `all` is refused
/// per pin: the live half still releases, the answer says five failed (four holds and the task started on a
/// profile, which `unpin_start` judges as `session.profile` does), and the counts hold.
#[tokio::test]
async fn unpin_all_from_a_surface_that_is_not_private_fails_each_hold_visibly() {
    let core = core();
    rig(&core);
    let (started, _) = start_pinned(&core, "opus");
    let used = wire(&core, Surface::Cli, "profile.use", json!({"name": "glm"})).await;
    assert!(used.error.is_none());
    let resp = wire(
        &core,
        Surface::Unnamed,
        "profile.unpin",
        json!({"all": true}),
    )
    .await;
    let r: ProfileUnpinResult = serde_json::from_value(
        resp.result
            .unwrap_or_else(|| panic!("an answer: {:?}", resp.error)),
    )
    .unwrap();
    assert!(r.released);
    assert_eq!((r.cleared.len(), r.failed.len()), (0, 5), "{r:?}");
    assert_eq!(r.after.pins.len(), 4, "{r:?}");
    assert!(
        r.failed.iter().any(|f| f.session_id == started),
        "the start-pin's refusal is named, not silently succeeded: {r:?}"
    );
    assert_eq!(r.after.task_pins.len(), 1, "still pinned: {r:?}");
    assert!(!crate::turn::brief_step::marked(&core.store, &started));
}

/// The exact set no brief or checkpoint routes: a live task with no brief mark and a model of its own, on
/// whatever turn. Each other shape is built beside it and must not be counted, so the count is 1 and not
/// "whatever is not zero".
#[tokio::test]
async fn a_task_started_on_a_profile_is_counted_as_its_own_kind_beside_the_holds() {
    let core = core();
    rig(&core);
    let (pinned, _) = start_pinned(&core, "opus");
    // Not counted: it carries the brief's mark (its model came from the brief, and checkpoints may move it),
    let (marked, _) = start_pinned(&core, "opus");
    core.store
        .append(&[crate::turn::brief_step::mark(&marked).unwrap()])
        .unwrap();
    // or it has ended.
    for state in [
        theseus_kernel::ExecState::Complete,
        theseus_kernel::ExecState::Cancelled,
        theseus_kernel::ExecState::Failed,
    ] {
        let (_, done) = start_pinned(&core, "opus");
        past_first_turn(&core, &_s(&core, &done), &done, state);
    }
    let r = pins(&core).await;
    assert_eq!(r.task_pins.len(), 1, "{r:?}");
    let p = &r.task_pins[0];
    assert_eq!(
        (p.session_id.as_str(), p.kind.as_str(), p.profile.as_str()),
        (pinned.as_str(), "task-start", "opus")
    );
    // Beside the holds, never inside them: nothing held it with `session.profile`.
    assert_eq!((r.pinned_sessions, r.pinned_tasks), (2, 2), "{r:?}");
    assert_eq!(r.pins.len(), 4, "{r:?}");
    assert!(r.pins.iter().all(|p| p.session_id != pinned), "{r:?}");
}

/// The session of an execution, for a test that holds the execution only.
fn _s(core: &Core, exec: &str) -> String {
    core.kernel.execution(exec).unwrap().unwrap().session_id
}

/// N1: Monday he starts a task on opus; it runs a turn and sleeps. The brief and the checkpoints still never
/// ask about it (`checkpoint_step` asks every turn of an unmarked task nothing), so it is a pin, and `pins`
/// counts it: one.
#[tokio::test]
async fn a_task_asleep_after_its_first_turn_is_counted_as_one() {
    let core = core();
    let (t, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &t, &exec, theseus_kernel::ExecState::Waiting);
    let r = pins(&core).await;
    assert_eq!(r.task_pins.len(), 1, "{r:?}");
    assert_eq!(
        (
            r.task_pins[0].session_id.as_str(),
            r.task_pins[0].kind.as_str()
        ),
        (t.as_str(), "task-start")
    );
}

/// N1: a task whose turn is running now is a pin too: its model is the one it was started on, and from its
/// next turn on no checkpoint asks.
#[tokio::test]
async fn a_running_task_started_on_a_profile_is_counted_as_one() {
    let core = core();
    let (t, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &t, &exec, theseus_kernel::ExecState::Running);
    let r = pins(&core).await;
    assert_eq!(r.task_pins.len(), 1, "{r:?}");
    assert_eq!(r.task_pins[0].session_id, t);
}

/// N1 and the owner's words, "unpin all current live profiles and tasks and sessions": `--all` clears a
/// start-pin by writing the brief's mark, one `session.profile_set` row each, so a running task is let go
/// without being cancelled. N before, N rows, 0 after, counted.
#[tokio::test]
async fn unpin_all_clears_a_task_started_on_a_profile_with_a_mark_and_a_row_each() {
    let core = core();
    rig(&core);
    let (pending, _) = start_pinned(&core, "opus");
    let (asleep, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &asleep, &exec, theseus_kernel::ExecState::Waiting);
    let (running, exec) = start_pinned(&core, "sonnet");
    past_first_turn(&core, &running, &exec, theseus_kernel::ExecState::Running);
    let rows_before = rows(&core, "session.profile_set").len();
    assert_eq!(rows_before, 4, "the four holds' own rows");
    let r = unpin(&core, true).await;
    assert_eq!(r.before.task_pins.len(), 3, "{r:?}");
    assert_eq!(
        r.cleared.len(),
        4 + 3,
        "four holds and three start-pins: {r:?}"
    );
    assert!(r.failed.is_empty(), "{r:?}");
    assert_eq!(r.after.task_pins.len(), 0, "{r:?}");
    assert_eq!((r.after.pinned_sessions, r.after.pinned_tasks), (0, 0));
    // The totals did not move.
    assert_eq!(
        (r.after.sessions, r.after.tasks),
        (r.before.sessions, r.before.tasks)
    );
    let all = rows(&core, "session.profile_set");
    assert_eq!(all.len() - rows_before, 7, "one row each: {all:?}");
    for (sid, from) in [(&pending, "opus"), (&asleep, "opus"), (&running, "sonnet")] {
        assert!(
            crate::turn::brief_step::marked(&core.store, sid),
            "{sid} is marked"
        );
        let row = all
            .iter()
            .rev()
            .find(|d| d["session_id"] == json!(sid))
            .unwrap_or_else(|| panic!("a row for {sid}: {all:?}"));
        assert_eq!(
            (row["from"].as_str(), row["to"].is_null()),
            (Some(from), true),
            "{row}"
        );
    }
    // The read after agrees, and a second `--all` has nothing left to do or write.
    let again = unpin(&core, true).await;
    assert_eq!((again.before.task_pins.len(), again.cleared.len()), (0, 0));
    assert_eq!(rows(&core, "session.profile_set").len(), all.len());
}

/// N2: a task Theseus made on a model of its own choosing is counted, under its own kind, and not cleared:
/// nobody named its model and there is no `--profile` to drop. The mark stays absent and no row is written.
#[tokio::test]
async fn unpin_all_names_a_task_theseus_made_and_leaves_it_alone() {
    let core = core();
    let (mine, _) = start_pinned(&core, "opus");
    let (theirs, _) = fixed(&core, "glm");
    let r = unpin(&core, true).await;
    let kind_of = |p: &ProfilePinsResult, sid: &str| {
        p.task_pins
            .iter()
            .find(|t| t.session_id == sid)
            .map(|t| t.kind.clone())
    };
    assert_eq!(r.before.task_pins.len(), 2, "{r:?}");
    assert_eq!(kind_of(&r.before, &mine).as_deref(), Some("task-start"));
    assert_eq!(kind_of(&r.before, &theirs).as_deref(), Some("task-fixed"));
    assert_eq!(r.cleared.len(), 1, "only the owner's: {r:?}");
    assert_eq!(r.cleared[0].session_id, mine);
    assert_eq!(r.after.task_pins.len(), 1, "{r:?}");
    assert_eq!(kind_of(&r.after, &theirs).as_deref(), Some("task-fixed"));
    assert!(!crate::turn::brief_step::marked(&core.store, &theirs));
    assert_eq!(rows(&core, "session.profile_set").len(), 1);
}

/// A path the ledger cannot classify is neither the owner's nor Theseus's: counted, called unknown, and left
/// alone, because writing a mark on a task nobody can name the maker of could undo a check's model.
#[tokio::test]
async fn a_task_whose_maker_the_ledger_cannot_name_is_unknown_and_not_cleared() {
    let core = core();
    let (t, _) = start_pinned(&core, "opus");
    core.store.fail_ledger_pages();
    let r = core.profile_pins().unwrap();
    assert_eq!(r.task_pins.len(), 1, "counted, never hidden: {r:?}");
    assert_eq!(
        (
            r.task_pins[0].session_id.as_str(),
            r.task_pins[0].kind.as_str()
        ),
        (t.as_str(), "task-unknown")
    );
    let (cleared, failed) = core.unpin_holds(&owner_cli());
    assert!(
        cleared.is_empty() && failed.is_empty(),
        "{cleared:?} {failed:?}"
    );
    assert!(!crate::turn::brief_step::marked(&core.store, &t));
}

/// A person who is not the owner clears no start-pin either: the refusal is named, the mark is not written.
#[tokio::test]
async fn a_stranger_cannot_clear_a_task_started_on_a_profile() {
    let core = core();
    let (t, _) = start_pinned(&core, "opus");
    let (cleared, failed) = core.unpin_holds(&stranger());
    assert!(cleared.is_empty(), "{cleared:?}");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].session_id, t);
    assert!(!crate::turn::brief_step::marked(&core.store, &t));
    assert_eq!(core.profile_pins().unwrap().task_pins.len(), 1);
}

#[tokio::test]
async fn unpin_without_all_reports_a_start_pinned_task_too() {
    let core = core();
    let (t, _) = start_pinned(&core, "opus");
    let r = unpin(&core, false).await;
    assert_eq!(r.before.task_pins.len(), 1, "{r:?}");
    assert_eq!(r.after.task_pins.len(), 1, "{r:?}");
    assert_eq!(r.after.task_pins[0].session_id, t);
    assert!(!crate::turn::brief_step::marked(&core.store, &t));
}

/// N5: the `after` read failing must not hide what `--all` did: the message carries how many holds were let
/// go and which were refused.
#[test]
fn a_failed_after_read_still_says_what_was_cleared_and_what_failed() {
    let live = theseus_protocol::ProfileReleaseResult {
        released: false,
        previous: "sonnet".into(),
        live: "sonnet".into(),
        by: "the CLI".into(),
    };
    let pin = |id: &str| theseus_protocol::ops::PinnedSession {
        session_id: id.into(),
        kind: "task".into(),
        profile: "glm".into(),
        configured: true,
        state: None,
    };
    let failed = vec![theseus_protocol::ops::UnpinFailure {
        session_id: "ses_refused".into(),
        error: "not yours".into(),
    }];
    let e = Core::unread(
        &live,
        (&[pin("ses_a"), pin("ses_b")], &failed),
        anyhow::anyhow!("disk"),
    );
    assert!(e.message.contains("2 let go"), "{}", e.message);
    assert!(e.message.contains("1 not let go"), "{}", e.message);
    assert!(e.message.contains("ses_refused"), "{}", e.message);
    assert!(e.message.contains("disk"), "{}", e.message);
}

/// N6: the predicate against what the real paths write, not what we believe they write. A `task.start
/// --profile` (deferred, so it sleeps), a `task.start` with no profile and a model's `task.create` (its child
/// held at the gate, so it runs) are made by their own code; `pins` must find exactly the first and the last,
/// each under its own kind, and `unpin --all` must clear only the first. If `task.start` ever wrote a mark
/// for a profile, or stopped writing `last_target`, or `task.create` began writing the mark, this goes red.
#[tokio::test]
async fn the_read_matches_what_task_start_and_task_create_really_write() {
    use crate::provider::Scripted;
    use crate::tests_tasks::{
        answers_a_call, arranged, first_user, last_user, parent_session, rig, ASK,
    };
    use theseus_protocol::task_start::TaskStartParams;
    let r = rig(
        |req| {
            if first_user(req).contains("CHILD") {
                return Scripted::text("done");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            match last_user(req).as_str() {
                l if l.starts_with("START") => Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: count", "arrangement": arranged()}),
                    )],
                ),
                _ => Scripted::text("ok"),
            }
        },
        |_| {},
    );
    let core = &r.core;
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let later = core.kernel.now_ms() + 86_400_000;
    let named = core
        .task_start(
            &TaskStartParams {
                brief: "Count the boats".into(),
                profile: Some("opus".into()),
                start_at_ms: Some(later),
                ..Default::default()
            },
            owner_cli(),
        )
        .unwrap();
    let briefed = core
        .task_start(
            &TaskStartParams {
                brief: "Count the gulls".into(),
                start_at_ms: Some(later),
                ..Default::default()
            },
            owner_cli(),
        )
        .unwrap();
    let parent = parent_session(core);
    crate::tests_tasks::turn(core, &parent, ASK).await;
    let child = crate::tests_tasks::only_task(core, &parent).session_id;
    let got = core.profile_pins().unwrap();
    let mut seen: Vec<(&str, &str)> = got
        .task_pins
        .iter()
        .map(|p| (p.session_id.as_str(), p.kind.as_str()))
        .collect();
    seen.sort();
    let mut want = vec![
        (named.session_id.as_str(), "task-start"),
        (child.as_str(), "task-fixed"),
    ];
    want.sort();
    assert_eq!(seen, want, "{got:?}");
    assert!(
        got.task_pins
            .iter()
            .all(|p| p.session_id != briefed.session_id),
        "a task started with no profile carries the mark and is not a pin: {got:?}"
    );
    let (cleared, failed) = core.unpin_holds(&owner_cli());
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(
        cleared
            .iter()
            .map(|c| c.session_id.as_str())
            .collect::<Vec<_>>(),
        vec![named.session_id.as_str()]
    );
    let after = core.profile_pins().unwrap();
    assert_eq!(after.task_pins.len(), 1, "{after:?}");
    assert_eq!(after.task_pins[0].session_id, child);
    assert!(crate::turn::brief_step::marked(
        &core.store,
        &named.session_id
    ));
    r.model.gate.add_permits(16);
}

/// One `session.profile.<id>` row that cannot be read: `release` still works, so `unpin` releases the live
/// pin first and names the read that failed, as an internal error, not as invalid params.
#[tokio::test]
async fn an_unreadable_hold_does_not_keep_unpin_from_releasing_the_live_pin() {
    let core = core();
    let bad = session(&core);
    core.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&format!("session.profile.{bad}")),
            &json!("not a hold"),
        )
        .unwrap()])
        .unwrap();
    let used = wire(&core, Surface::Cli, "profile.use", json!({"name": "glm"})).await;
    assert!(used.error.is_none());
    assert_eq!(core.live_profile().1, "runtime", "the live pin is set");
    let r = wire(&core, Surface::Cli, "profile.unpin", json!({"all": false})).await;
    assert!(r.result.is_none(), "no false zero: {r:?}");
    let e = r.error.expect("the read failure is the answer");
    assert_eq!(e.code, theseus_protocol::error_code::INTERNAL, "{e:?}");
    assert!(
        e.message.contains("live pin was released"),
        "it says what it did: {}",
        e.message
    );
    assert_eq!(core.live_profile().1, "config", "the live pin is released");
}

fn state_of(r: &ProfilePinsResult, sid: &str) -> Option<String> {
    r.task_pins
        .iter()
        .find(|p| p.session_id == sid)
        .and_then(|p| p.state.clone())
}

/// R1: what letting go of a model changes depends on where the task is in its life, so the read says it:
/// no turn yet (`pending`: the brief may still choose), past its first turn and idle (`asleep`: the brief
/// never runs again, checkpoints only) and a turn running now (`running`: it finishes on the model it
/// started on). The state rides on the pin before and in `cleared`, where the CLI words each line.
#[tokio::test]
async fn a_pinned_task_says_where_in_its_life_it_is() {
    let core = core();
    rig(&core);
    let (pending, _) = start_pinned(&core, "opus");
    let (asleep, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &asleep, &exec, theseus_kernel::ExecState::Waiting);
    let (running, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &running, &exec, theseus_kernel::ExecState::Running);
    let (queued_again, exec) = start_pinned(&core, "opus");
    past_first_turn(&core, &queued_again, &exec, theseus_kernel::ExecState::Queued);
    let r = unpin(&core, true).await;
    for (sid, want) in [
        (&pending, "pending"),
        (&asleep, "asleep"),
        (&running, "running"),
        (&queued_again, "asleep"),
    ] {
        assert_eq!(state_of(&r.before, sid).as_deref(), Some(want), "{sid}");
        let c = r.cleared.iter().find(|c| &c.session_id == sid).unwrap();
        assert_eq!(c.state.as_deref(), Some(want), "cleared: {c:?}");
    }
    // A hold is not a task's life: it carries none.
    assert!(r.before.pins.iter().all(|p| p.state.is_none()), "{r:?}");
}

/// R2: a `task.started_by_owner` row that is on record but cannot be decoded says nothing sure about who made
/// the task it names. The old read dropped it and called the task Theseus's ("not yours to drop"), which is
/// the wrong direction for an owner's `--profile` task. It is `task-unknown` now: counted, named, and not
/// cleared. A task whose own row reads well stays the owner's.
#[tokio::test]
async fn an_unreadable_started_row_leaves_the_task_unknown_not_theirs() {
    let core = core();
    let (mine, _) = start_pinned(&core, "opus");
    let (blind, _) = fixed(&core, "opus");
    // Tagged by its kind and session (so the index finds it) but with no time: it cannot be decoded.
    core.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &json!({"kind": "task.started_by_owner", "session_id": blind}),
        )
        .unwrap()])
        .unwrap();
    let r = core.profile_pins().unwrap();
    let kind = |sid: &str| {
        r.task_pins
            .iter()
            .find(|p| p.session_id == sid)
            .map(|p| p.kind.clone())
    };
    assert_eq!(kind(&mine).as_deref(), Some("task-start"), "{r:?}");
    assert_eq!(kind(&blind).as_deref(), Some("task-unknown"), "{r:?}");
    let (cleared, failed) = core.unpin_holds(&owner_cli());
    assert_eq!(
        (cleared.len(), failed.len()),
        (1, 0),
        "only the readable one: {cleared:?} {failed:?}"
    );
    assert_eq!(cleared[0].session_id, mine);
    assert!(!crate::turn::brief_step::marked(&core.store, &blind));
}

/// A `task.retried` row as `retry_failed` writes it: the new task's session, `failed_session` in its data.
fn retried(core: &Core, new: &str, failed: &str) {
    let fact = crate::fact::retry::TaskRetried {
        failed_execution: "exe_failed",
        failed_session: failed,
        execution_id: "exe_new",
        session_id: new,
        limit: 0,
        failed_limit: 0,
        note: None,
        by: "the CLI",
    };
    core.store
        .append(&[crate::fact::row(&fact, Some(new), None).unwrap()])
        .unwrap();
}

/// R2: a retry copies the failed task's `last_target` and writes no mark and no `started_by_owner` row, so a
/// retry of the owner's `--profile` task is the owner's choice carried forward and reads as theirs. It
/// inherits "the owner's" through its `task.retried` row, down a chain of retries, and `--all` clears it. A
/// retry of a task Theseus made, or of one the brief chose the model of (marked: the owner never named it),
/// stays Theseus's.
#[tokio::test]
async fn a_retry_of_a_task_started_on_a_profile_is_the_owners_choice_carried_forward() {
    let core = core();
    let (origin, _) = start_pinned(&core, "opus");
    let (first, _) = fixed(&core, "opus");
    retried(&core, &first, &origin);
    let (second, _) = fixed(&core, "opus");
    retried(&core, &second, &first);
    // Not inherited: the failed one was Theseus's, or the brief chose its model.
    let (theirs, _) = fixed(&core, "sonnet");
    let (retry_of_theirs, _) = fixed(&core, "sonnet");
    retried(&core, &retry_of_theirs, &theirs);
    let (briefed, _) = start_pinned(&core, "haiku");
    core.store
        .append(&[crate::turn::brief_step::mark(&briefed).unwrap()])
        .unwrap();
    let (retry_of_briefed, _) = fixed(&core, "haiku");
    retried(&core, &retry_of_briefed, &briefed);
    let r = core.profile_pins().unwrap();
    let kind = |sid: &str| {
        r.task_pins
            .iter()
            .find(|p| p.session_id == sid)
            .map(|p| p.kind.clone())
    };
    assert_eq!(kind(&origin).as_deref(), Some("task-start"));
    assert_eq!(kind(&first).as_deref(), Some("task-start"), "{r:?}");
    assert_eq!(kind(&second).as_deref(), Some("task-start"), "{r:?}");
    assert_eq!(kind(&retry_of_theirs).as_deref(), Some("task-fixed"));
    assert_eq!(kind(&retry_of_briefed).as_deref(), Some("task-fixed"));
    let (cleared, failed) = core.unpin_holds(&owner_cli());
    assert_eq!((cleared.len(), failed.len()), (3, 0), "{cleared:?}");
    for sid in [&origin, &first, &second] {
        assert!(crate::turn::brief_step::marked(&core.store, sid), "{sid}");
    }
    assert!(!crate::turn::brief_step::marked(&core.store, &retry_of_theirs));
}
