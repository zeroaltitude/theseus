//! `session.profile` (theseus-jid7t.2): a model the owner holds a session to.
//! The floor's refusal and what it leaves unwritten, the hold across turns, a
//! turn's own profile over it, the revert and its undo, a mark taken after the
//! hold, a task's next turn, and the pure refusal over every (mark x origin).

use serde_json::json;
use theseus_protocol::ops::{DataMark, SessionProfileParams, SessionProfileResult};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::provider::Scripted;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_judge::kinds;
use crate::tests_route::{call, rig, Rig};
use crate::Config;

use crate::test_support::{owner_cli as cli, stranger};

/// A config with Google on (its mark is read) and, with `eu`, `glm53` a
/// Claude served from a host vouched EU.
fn google_on(eu: bool) -> impl FnOnce(&mut Config) {
    move |c| {
        c.google.account = "owner@example.test".into();
        c.secrets.insert(
            "google_client".into(),
            "op://vault/google client/notesPlain".into(),
        );
        c.secrets.insert(
            "google_refresh_token".into(),
            "file:/tmp/unused-google-refresh-token".into(),
        );
        if eu {
            c.serve_glm_unrestricted();
            c.vouch_zai(crate::catalog::ModelOrigin::Eu);
        }
    }
}

pub(crate) fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// Label `sid` `confidential`, as a Google read does.
pub(crate) fn mark(core: &Core, sid: &str) {
    crate::test_support::label_session(core, sid, crate::content::Flag::Confidential);
}

fn params(sid: &str, profile: Option<&str>) -> SessionProfileParams {
    SessionProfileParams {
        session_id: sid.into(),
        profile: profile.map(str::to_string),
        author: None,
        discord: None,
    }
}

fn hold(core: &Core, sid: &str, profile: Option<&str>) -> SessionProfileResult {
    core.session_set_profile(&params(sid, profile), &cli())
        .unwrap()
}

/// What nothing written looks like: no hold record, no row.
fn untouched(r: &Rig, sid: &str) {
    let held = r
        .core
        .store
        .get_meta::<serde_json::Value>(&format!("session.profile.{sid}"))
        .unwrap();
    assert!(held.is_none(), "no hold was written: {held:?}");
    assert!(kinds(&r.core.store, "session.profile_set").is_empty());
}

/// A turn through the wire, as a client sends it.
async fn submit(r: &Rig, sid: &str, extra: serde_json::Value) -> TurnSubmitResult {
    let mut p = json!({"session_id": sid, "input": "go on"});
    p.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(call(&r.core, "turn.submit", p).await).unwrap()
}

fn model_of(r: &Rig, profile: &str) -> String {
    r.core.cfg.profile(profile).unwrap().model.clone()
}

#[tokio::test]
async fn a_cn_profile_on_a_private_session_is_refused_with_nothing_written() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    mark(&r.core, &sid);
    let got = hold(&r.core, &sid, Some("glm"));
    let why = got.refused.expect("refused");
    assert!(why.contains("restricted origin"), "{why}");
    assert_eq!((got.profile, got.previous), (None, None));
    untouched(&r, &sid);
}

#[tokio::test]
async fn an_eu_profile_on_a_labelled_session_is_set_as_a_us_one_is() {
    let r = rig(None, 1, google_on(true));
    let sid = session(&r.core);
    mark(&r.core, &sid);
    let eu = hold(&r.core, &sid, Some("glm53"));
    assert_eq!(
        eu.refused, None,
        "a label keeps only a restricted model off"
    );
    let set = hold(&r.core, &sid, Some("opus"));
    assert_eq!(set.refused, None);
    assert_eq!(set.profile.as_deref(), Some("opus"));
    assert_eq!(set.note, None, "a US profile has nothing to caveat");
}

#[tokio::test]
async fn a_cn_profile_on_an_open_session_is_set_and_the_floor_decides_per_turn() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    assert_eq!(
        crate::content_wait::mark_of(&r.core.store, &sid),
        DataMark::Open
    );
    let got = hold(&r.core, &sid, Some("glm"));
    assert_eq!(got.refused, None);
    assert_eq!(got.profile.as_deref(), Some("glm"));
    let note = got.note.expect("the answer says so");
    assert!(note.contains("the floor decides each turn"), "{note}");
}

#[tokio::test]
async fn an_unknown_profile_is_refused() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    let got = hold(&r.core, &sid, Some("no-such-profile"));
    assert!(got.refused.unwrap().contains("not a configured profile"));
    untouched(&r, &sid);
    // An unknown session is an error, not an answer.
    let e = r
        .core
        .session_set_profile(&params("ses_missing", Some("opus")), &cli())
        .unwrap_err();
    assert!(format!("{e:#}").contains("no session"), "{e:#}");
}

#[tokio::test]
async fn only_the_owner_from_a_private_place_holds_a_session_to_a_model() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    let e = r
        .core
        .session_set_profile(&params(&sid, Some("opus")), &stranger())
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("owner") || said.contains("private"), "{said}");
    untouched(&r, &sid);
}

#[tokio::test]
async fn the_hold_stands_across_turns_and_a_turns_own_profile_beats_it_once() {
    let r = rig(None, 4, google_on(false));
    let sid = session(&r.core);
    let (opus, sonnet) = (model_of(&r, "opus"), model_of(&r, "sonnet"));
    assert_ne!(opus, sonnet);
    hold(&r.core, &sid, Some("opus"));
    for _ in 0..2 {
        let res = submit(&r, &sid, json!({})).await;
        assert_eq!(
            (res.profile.as_str(), res.model.as_str()),
            ("opus", opus.as_str())
        );
    }
    // The turn's own profile (not a pane's carried one) wins for that turn.
    let own = submit(&r, &sid, json!({"profile": "sonnet"})).await;
    assert_eq!(own.profile, "sonnet");
    // The next is held again, and so is one that carries the last turn's.
    let back = submit(&r, &sid, json!({})).await;
    assert_eq!(back.profile, "opus");
    let carried = submit(&r, &sid, json!({"profile": "sonnet", "carried": true})).await;
    assert_eq!(
        carried.profile, "opus",
        "a pane's carried profile does not beat it"
    );
    let sent: Vec<String> = r.claude.requests().into_iter().map(|q| q.model).collect();
    assert_eq!(
        sent,
        vec![
            opus.clone(),
            opus.clone(),
            sonnet.clone(),
            opus.clone(),
            opus.clone()
        ]
    );
}

#[tokio::test]
async fn null_reverts_and_previous_is_the_undo() {
    let r = rig(None, 3, google_on(false));
    let sid = session(&r.core);
    let (live, _) = r.core.live_profile();
    let one = hold(&r.core, &sid, Some("opus"));
    assert_eq!((one.previous, one.profile.as_deref()), (None, Some("opus")));
    let two = hold(&r.core, &sid, Some("haiku"));
    assert_eq!(two.previous.as_deref(), Some("opus"));
    let off = hold(&r.core, &sid, None);
    assert_eq!(
        (off.previous.as_deref(), off.profile),
        (Some("haiku"), None)
    );
    assert_eq!(
        submit(&r, &sid, json!({})).await.profile,
        live,
        "back to the default"
    );
    // The undo is one more call with `previous`.
    let undo = hold(&r.core, &sid, off.previous.as_deref());
    assert_eq!(undo.profile.as_deref(), Some("haiku"));
    assert_eq!(submit(&r, &sid, json!({})).await.profile, "haiku");
    // Letting go of nothing, and holding the same twice, write nothing more.
    let rows = kinds(&r.core.store, "session.profile_set").len();
    hold(&r.core, &sid, Some("haiku"));
    let fresh = session(&r.core);
    assert_eq!(hold(&r.core, &fresh, None).profile, None);
    assert_eq!(kinds(&r.core.store, "session.profile_set").len(), rows);
}

#[tokio::test]
async fn the_ledger_row_names_the_session_the_change_and_who() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    hold(&r.core, &sid, Some("opus"));
    hold(&r.core, &sid, Some("haiku"));
    let rows = kinds(&r.core.store, "session.profile_set");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(
        rows[0].data,
        json!({"session_id": sid, "from": null, "to": "opus", "by": "the CLI"})
    );
    assert_eq!(rows[1].data["from"], "opus");
    assert_eq!(rows[1].data["to"], "haiku");
}

/// A request through the wire: the response, its error kept.
async fn raw(
    core: &std::sync::Arc<Core>,
    method: &str,
    params: serde_json::Value,
) -> theseus_protocol::Response {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, "test".into()));
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

/// A mark taken after the hold, with no judge: the turn's base is the held
/// profile, the provider's floor refuses it before a byte is sent.
#[tokio::test]
async fn a_mark_taken_after_the_hold_still_cannot_reach_the_model() {
    let r = rig(None, 2, google_on(false));
    let sid = session(&r.core);
    hold(&r.core, &sid, Some("glm53"));
    mark(&r.core, &sid);
    let res = raw(
        &r.core,
        "turn.submit",
        json!({"session_id": sid, "input": "go on"}),
    )
    .await;
    assert!(r.zai.requests().is_empty(), "nothing went to it: {res:?}");
}

/// The same with the route on: the floor moves the refused base as it moves
/// any, to the next usable profile, and the turn says where from.
#[tokio::test]
async fn a_mark_taken_after_the_hold_moves_a_routed_turn_off_it() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_route::mode(&jev, "routine_coding", 0.95);
    let r = crate::tests_route::rig_parts(Some(&jev), 2, google_on(false), |_p| {
    });
    let sid = session(&r.core);
    hold(&r.core, &sid, Some("glm53"));
    mark(&r.core, &sid);
    let res = submit(
        &r,
        &sid,
        json!({"input": "Rename the call sites in the billing module."}),
    )
    .await;
    assert_eq!(res.profile, "sonnet", "{}", res.output);
    assert!(
        r.zai.requests().is_empty(),
        "nothing went to the held profile"
    );
    let route = res.route.expect("the move is told");
    assert_eq!(route.from, "glm53", "{route:?}");
}

#[tokio::test]
async fn no_verb_posts_anything_to_a_place() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    let before = r.core.kernel.outbox_actions().unwrap().len();
    hold(&r.core, &sid, Some("opus"));
    hold(&r.core, &sid, None);
    assert_eq!(r.core.kernel.outbox_actions().unwrap().len(), before);
}

/// The wire reaches it: the method has its arm, and a connection no listener
/// named is refused in the owner's act's words, with the session as it was.
#[tokio::test]
async fn the_method_has_its_arm_and_refuses_an_unnamed_connection() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    let res = raw(
        &r.core,
        "session.profile",
        json!({"session_id": sid, "profile": "opus"}),
    )
    .await;
    let e = res.error.expect("refused");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(e.message.contains("is never a private place"), "{e:?}");
    untouched(&r, &sid);
}

/// A running task keeps the model of the call it is making and of the rest of
/// its turn; its next turn (what `continue_execution` builds) takes the hold,
/// and a revert puts back where it ran before.
#[tokio::test]
async fn a_running_task_takes_the_hold_at_its_next_turn_not_mid_turn() {
    use crate::tests_tasks::{
        answers_a_call, arranged, exec, first_user, last_user, only_task, parent_session,
        rig as task_rig, turn, until, ASK,
    };
    let mut r = task_rig(
        |req| {
            let (first, last) = (first_user(req), last_user(req));
            if first.contains("CHILD:") {
                return if answers_a_call(req) {
                    Scripted::text("Counted to three.")
                } else {
                    Scripted::tools("Looking.", &[("c1", "proc_run", json!({"argv": ["true"]}))])
                };
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            if last.starts_with("START") {
                return Scripted::tools(
                    "Starting it.",
                    &[(
                        "t_task",
                        "task_create",
                        json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                               "arrangement": arranged()}),
                    )],
                );
            }
            Scripted::text("Fine.")
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    turn(&r.core, &parent, ASK).await;
    let task = only_task(&r.core, &parent);
    r.entered.recv().await.unwrap(); // its first call is in flight
    let set = hold(&r.core, &task.session_id, Some("opus"));
    assert_eq!(set.profile.as_deref(), Some("opus"));
    r.model.gate.add_permits(1);
    r.entered.recv().await.unwrap(); // its second loop, in the same turn
    r.model.gate.add_permits(1);
    until("the task completes", || {
        exec(&r.core, &task.id).state == theseus_kernel::ExecState::Complete
    })
    .await;
    let (sonnet, opus) = (
        model_of_core(&r.core, "sonnet"),
        model_of_core(&r.core, "opus"),
    );
    let child: Vec<String> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| first_user(q).contains("CHILD:"))
        .map(|q| q.model.clone())
        .collect();
    assert_eq!(
        child,
        vec![sonnet.clone(), sonnet.clone()],
        "the turn kept its model"
    );
    // Its next turn runs on the hold; letting go puts back what it ran on.
    let rec: SessionRecord = r.core.store.get_session(&task.session_id).unwrap().unwrap();
    let (live, _) = r.core.live_profile();
    let next = r.core.runner.target_for_session(&rec, &live).unwrap();
    assert_eq!(
        (next.profile.as_str(), next.model.as_str()),
        ("opus", opus.as_str())
    );
    hold(&r.core, &task.session_id, None);
    let rec: SessionRecord = r.core.store.get_session(&task.session_id).unwrap().unwrap();
    let back = r.core.runner.target_for_session(&rec, &live).unwrap();
    assert_eq!(back.model, sonnet, "it goes back to where it ran");
}

fn model_of_core(c: &Core, profile: &str) -> String {
    c.cfg.profile(profile).unwrap().model.clone()
}

/// A turn that ran under the hold leaves its target on the session; letting
/// go puts back the one it ran on before the hold, so a continuation does not
/// stay on a model the owner let go of.
#[tokio::test]
async fn a_revert_puts_back_the_target_it_ran_on_before_the_hold() {
    use crate::session::TargetRef;
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    let (live, _) = r.core.live_profile();
    let on = |profile: &str| {
        let t = r
            .core
            .runner
            .resolve_target(&live, Some(profile), None, None)
            .unwrap();
        TargetRef::from(&t)
    };
    let write = |t: TargetRef| {
        let mut rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
        rec.last_target = Some(t);
        r.core.store.put_session(&sid, &rec).unwrap();
    };
    write(on("sonnet"));
    hold(&r.core, &sid, Some("opus"));
    write(on("opus")); // a turn ran under the hold
    hold(&r.core, &sid, None);
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.last_target.unwrap().profile, "sonnet");
}

#[tokio::test]
async fn a_retired_session_takes_no_hold_but_lets_one_go() {
    let r = rig(None, 1, google_on(false));
    let sid = session(&r.core);
    hold(&r.core, &sid, Some("opus"));
    r.core
        .retire_session(&sid, &crate::approval::Answerer::from("cli"))
        .unwrap();
    let before = kinds(&r.core.store, "session.profile_set").len();
    let held = r
        .core
        .store
        .get_meta::<serde_json::Value>(&format!("session.profile.{sid}"))
        .unwrap();
    let e = r
        .core
        .session_set_profile(&params(&sid, Some("glm")), &cli())
        .unwrap_err();
    let said = format!("{e:#}");
    assert!(
        said.contains("is retired: `theseus sessions reopen` it first"),
        "{said}"
    );
    assert_eq!(
        r.core
            .store
            .get_meta::<serde_json::Value>(&format!("session.profile.{sid}"))
            .unwrap(),
        held,
        "no META written"
    );
    assert_eq!(kinds(&r.core.store, "session.profile_set").len(), before);
    // A let-go still goes, so a hold can always be cleared.
    let gone = hold(&r.core, &sid, None);
    assert_eq!(
        (gone.profile, gone.previous.as_deref()),
        (None, Some("opus"))
    );
}
