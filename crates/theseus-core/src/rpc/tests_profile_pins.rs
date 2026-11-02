//! `profile.pins` and `profile.unpin` (theseus-h66v4.2): the owner's "unpin all current live profiles and
//! tasks and sessions". A session or a task is pinned when `session.profile` holds it to a profile (META
//! `session.profile.<id>`; a task's own session is its task). Every test counts the positive: N pinned
//! before, N rows written, 0 after, beside the totals that make "0 pinned" more than "0 sessions".
//! The live profile's own pin is `profile.release`'s, and the same-name tests below hold the `dq3m1` line:
//! `released` and `cleared` are carried, never read off two names that happen to match.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::ops::SessionProfileParams;
use theseus_protocol::{ProfilePinsResult, ProfileUnpinResult, SessionKind};

use super::Core;
use crate::rpc::Parts;
use crate::approval::{Client, Surface};
use crate::provider::FakeProvider;
use crate::session::{SessionRecord, TaskOf};
use crate::store::Store;
use crate::test_support::{owner_cli, rows, stranger};
use crate::Config;

fn core_on(store: Store) -> Arc<Core> {
    let mut c = Config::example();
    c.serve_glm_unrestricted();
    Core::build(Parts::for_tests(c, Arc::new(FakeProvider::default()), store)).unwrap()
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
    rec.session_id
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
    serde_json::from_value(r.result.unwrap_or_else(|| panic!("an answer: {:?}", r.error))).unwrap()
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
        (r.after.pinned_sessions, r.after.pinned_tasks, r.after.pins.len()),
        (0, 0, 0)
    );
    // The totals did not move: nothing was deleted to get to zero.
    assert_eq!((r.after.sessions, r.after.tasks), (6, 3));
    let all = rows(&core, "session.profile_set");
    let lets_go: Vec<&Value> = all.iter().filter(|d| d["to"].is_null()).collect();
    assert_eq!(all.len() - before_rows, 4, "one row each: {all:?}");
    assert_eq!(lets_go.len(), 4, "{all:?}");
    for sid in ses.iter().chain(tasks.iter()) {
        let row = lets_go.iter().find(|d| d["session_id"] == json!(sid)).unwrap();
        assert!(row["from"].is_string(), "the row names what it let go: {row}");
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
    let used = wire(&core, Surface::Cli, "profile.use", json!({"name": "sonnet"})).await;
    assert!(used.error.is_none());
    let first = unpin(&core, true).await;
    assert_eq!((first.previous.as_str(), first.live.as_str()), ("sonnet", "sonnet"));
    assert!(first.released, "{first:?}");
    assert_eq!(first.cleared.len(), 1, "{first:?}");
    assert_eq!(first.cleared[0].profile, "sonnet");
    assert_eq!(first.before.pins.len(), 1);
    assert_eq!(first.after.pins.len(), 0);
    assert_eq!(held(&core, &s), Value::Null);
    let second = unpin(&core, true).await;
    assert_eq!((second.previous.as_str(), second.live.as_str()), ("sonnet", "sonnet"));
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
/// per hold: the live half still releases, the answer says four failed, and the counts hold at four.
#[tokio::test]
async fn unpin_all_from_a_surface_that_is_not_private_fails_each_hold_visibly() {
    let core = core();
    rig(&core);
    let used = wire(&core, Surface::Cli, "profile.use", json!({"name": "glm"})).await;
    assert!(used.error.is_none());
    let resp = wire(&core, Surface::Unnamed, "profile.unpin", json!({"all": true})).await;
    let r: ProfileUnpinResult = serde_json::from_value(
        resp.result
            .unwrap_or_else(|| panic!("an answer: {:?}", resp.error)),
    )
    .unwrap();
    assert!(r.released);
    assert_eq!((r.cleared.len(), r.failed.len()), (0, 4), "{r:?}");
    assert_eq!(r.after.pins.len(), 4, "{r:?}");
}
