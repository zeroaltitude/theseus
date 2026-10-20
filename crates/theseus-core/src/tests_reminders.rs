//! Reminders through the whole core (theseus-8ux6x): a reminder always
//! reaches the owner. Set with no session it lives in Home, the session the
//! owner's DM runs on, or a "Reminders" conversation when no DM is bound; a
//! `Remind` wake set in a placeless session posts to the DM when it fires;
//! with no place at all every surface still hears `wake.noticed`, with its four
//! answers; and none of it takes a turn or calls a model. A snooze, from a
//! button or the CLI, sets it again; the horizon is `[kernel]
//! max_wake_ahead_days`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{Message, Request, WakeNoticed};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::test_support::{rows, until_within as until};
use crate::{Config, Core};

const DM: &str = "dm:42";
const TARGET: &str = "discord:dm:42";

/// A stand-in model that counts its requests, and sets a say-only reminder for
/// the owner when asked (`REMIND <after>: <note>`).
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let last = req
                .messages
                .iter()
                .rev()
                .find(|m| m["role"] == "user")
                .map(|m| match &m["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(b) => b
                        .iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let answers_a_call = req
                .messages
                .last()
                .and_then(|m| m["content"].as_array())
                .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
            let next = if answers_a_call {
                Scripted::text("Set.")
            } else if let Some(rest) = last.strip_prefix("REMIND ") {
                let (after, note) = rest.split_once(": ").unwrap();
                Scripted::tools(
                    "Setting it.",
                    &[(
                        "t_remind",
                        "wake_at",
                        json!({"after": after, "note": note, "for": "owner", "say_only": true}),
                    )],
                )
            } else {
                Scripted::text("Hello.")
            };
            FakeProvider::scripted(vec![next])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    events: tokio::sync::mpsc::UnboundedReceiver<Message>,
    driver: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

/// A core with the driver running (so the due scan ticks), a DM bound when
/// `dm`, and a tap on everything it publishes.
fn rig(dm: bool, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    tweak(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    if dm {
        core.bind_places(vec![crate::places::BoundPlace {
            target: TARGET.into(),
            name: "DM @owner".into(),
            private: true,
            ..Default::default()
        }]);
    }
    let (tx, events) = tokio::sync::mpsc::unbounded_channel();
    *core.bus.tap.lock().unwrap() = Some(tx);
    let driver = tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        events,
        driver,
        _dir: dir,
    }
}

fn open(core: &Arc<Core>) -> String {
    core.open_session(theseus_protocol::SessionOpenParams {
        dir: None,
        kind: None,
        label: None,
        opened_from: None,
        persona: None,
    })
    .unwrap()
    .session_id
}

/// The owner's DM runs on a session.
fn dm_session(core: &Arc<Core>) -> String {
    let sid = open(core);
    core.outbox.bind_place(DM, &sid).unwrap();
    sid
}

fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

/// One request on a connection of the CLI's surface, and its result or error.
async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, cli()));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// The reminders the outbox holds: (target, body).
fn posts(core: &Core) -> Vec<(String, Value)> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "reminder")
        .map(|a| {
            (
                crate::outbox::target_of(&a).to_string(),
                crate::outbox::body_of(&a).clone(),
            )
        })
        .collect()
}

/// The first `wake.noticed` to come, waited for: the notification follows the frame that
/// wrote the post, so a reader that finds the post may be ahead of it (theseus-1b2zm).
async fn next_noticed(r: &mut Rig) -> WakeNoticed {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), r.events.recv())
            .await
            .expect("a wake.noticed within 20 s")
            .expect("the events stay open");
        if let Message::Notification(n) = m {
            if n.method == "wake.noticed" {
                return serde_json::from_value(n.params).unwrap();
            }
        }
    }
}

fn noticed(r: &mut Rig) -> Vec<WakeNoticed> {
    let mut out = vec![];
    while let Ok(m) = r.events.try_recv() {
        if let Message::Notification(n) = m {
            if n.method == "wake.noticed" {
                out.push(serde_json::from_value(n.params).unwrap());
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reminder_with_no_session_lives_in_the_dm_and_is_posted_there_with_no_model_call() {
    let r = rig(true, |_| {});
    let dm = dm_session(&r.core);
    let made = rpc(
        &r.core,
        "wake.create",
        json!({"when": "2s", "note": "stretch"}),
    )
    .await
    .unwrap();
    assert_eq!(made["home"], true, "{made}");
    assert_eq!(made["wake"]["session_id"], dm, "it lives where the DM runs");
    assert_eq!(made["wake"]["purpose"], "remind");
    assert_eq!(made["wake"]["run"], "notice");
    until("the reminder's post", 10, || !posts(&r.core).is_empty()).await;
    let (target, body) = posts(&r.core).remove(0);
    assert_eq!(target, TARGET);
    assert_eq!(body["text"], "stretch");
    assert_eq!(body["answers"].as_array().unwrap().len(), 4);
    assert!(
        r.model.requests.lock().unwrap().is_empty(),
        "a reminder is no model call"
    );
    let fired = rows(&r.core, "wake.fired");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0]["notice"], true);
    // It left, and it fires once.
    let list = rpc(&r.core, "wake.list", json!({})).await.unwrap();
    assert_eq!(list["wakes"].as_array().unwrap().len(), 0);
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(posts(&r.core).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reminder_in_a_placeless_session_posts_to_the_owners_dm_when_it_fires() {
    let mut r = rig(true, |_| {});
    // A CLI session: no place. The DM has no session yet.
    let cli_session = open(&r.core);
    let made = rpc(
        &r.core,
        "wake.create",
        json!({"when": "2s", "note": "stretch", "session_id": cli_session}),
    )
    .await
    .unwrap();
    assert_eq!(made["wake"]["session_id"], cli_session);
    assert_eq!(
        made["home"].as_bool(),
        None,
        "not Home: it lives where it was set"
    );
    until("the reminder's post", 10, || !posts(&r.core).is_empty()).await;
    let (target, _) = posts(&r.core).remove(0);
    assert_eq!(target, TARGET, "0 silent: the DM hears it");
    let n = next_noticed(&mut r).await;
    assert!(noticed(&mut r).is_empty(), "heard once");
    assert_eq!(n.posted_to.as_deref(), Some(TARGET));
    assert!(n.home, "it was Home's DM that took it");
    assert!(r.model.requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_dm_it_lives_in_a_reminders_conversation_and_every_surface_hears_it() {
    let mut r = rig(false, |_| {});
    let made = rpc(
        &r.core,
        "wake.create",
        json!({"when": "2s", "note": "stretch"}),
    )
    .await
    .unwrap();
    let home = made["wake"]["session_id"].as_str().unwrap().to_string();
    let rec: crate::session::SessionRecord = r.core.store.get_session(&home).unwrap().unwrap();
    assert_eq!(rec.label.as_deref(), Some("Reminders"));
    // A second reminder lives in the same Home.
    let again = rpc(
        &r.core,
        "wake.create",
        json!({"when": "3s", "note": "water"}),
    )
    .await
    .unwrap();
    assert_eq!(again["wake"]["session_id"], home, "one Home, made once");
    until("the first notice", 10, || {
        rows(&r.core, "wake.fired").len() == 1
    })
    .await;
    let n = noticed(&mut r);
    assert_eq!(n.len(), 1, "{n:?}");
    assert_eq!(n[0].note, "stretch");
    assert_eq!(n[0].purpose, "remind");
    assert_eq!(n[0].posted_to, None, "no place hears it");
    let ids: Vec<&str> = n[0].answers.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(
        ids,
        ["Done", "Snooze 10 min", "Snooze 1 h", "Tomorrow 9:00"],
        "the four answers"
    );
    assert!(posts(&r.core).is_empty(), "nothing to post to");
    assert!(r.model.requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snooze_from_a_button_sets_a_fired_reminder_again_and_it_fires_again() {
    let mut r = rig(true, |_| {});
    dm_session(&r.core);
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "2s", "note": "stretch"}),
    )
    .await
    .unwrap();
    until("the first post", 10, || posts(&r.core).len() == 1).await;
    let id = posts(&r.core)[0].1["wake_id"].as_str().unwrap().to_string();
    assert_eq!(next_noticed(&mut r).await.wake_id, id);
    // The button's call: wake.snooze with the whole id, fired, for 2 s.
    let again = rpc(
        &r.core,
        "wake.snooze",
        json!({"wake": id, "when": "2s", "fired": true}),
    )
    .await
    .unwrap();
    assert_eq!(again["wake"]["note"], "stretch");
    assert_eq!(again["wake"]["purpose"], "remind");
    until("the second post", 10, || posts(&r.core).len() == 2).await;
    assert_eq!(rows(&r.core, "wake.snoozed").len(), 1);
    // Done settles it: a row, and nothing pending.
    let done = rpc(&r.core, "wake.done", json!({"wake": id}))
        .await
        .unwrap();
    assert_eq!(done["wake_id"], id);
    assert_eq!(rows(&r.core, "wake.done").len(), 1);
    assert!(r.model.requests.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_horizon_is_the_configured_number_of_days() {
    let r = rig(true, |c| c.kernel.max_wake_ahead_days = 400);
    dm_session(&r.core);
    let ok = rpc(
        &r.core,
        "wake.create",
        json!({"when": "399d", "note": "far"}),
    )
    .await;
    assert!(ok.is_ok(), "{ok:?}");
    let err = rpc(
        &r.core,
        "wake.create",
        json!({"when": "401d", "note": "too far"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("at most 400 days"), "{err}");
    let err = rpc(
        &r.core,
        "wake.create",
        json!({"when": "whenever", "note": "n"}),
    )
    .await
    .unwrap_err();
    assert!(err.contains("not a time I can read"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_models_say_only_reminder_for_the_owner_is_a_notice_and_takes_no_turn() {
    let mut r = rig(false, |_| {});
    let sid = open(&r.core);
    // The model sets it in a placeless session.
    let rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = crate::bus::EventSink::new(r.core.bus.clone(), &sid, None);
    r.core
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("REMIND 2s: stretch".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
    let before = r.model.requests.lock().unwrap().len();
    let list = rpc(&r.core, "wake.list", json!({"session_id": sid}))
        .await
        .unwrap();
    let w = &list["wakes"][0];
    assert_eq!(
        (w["purpose"].as_str(), w["run"].as_str()),
        (Some("remind"), Some("notice"))
    );
    until("the notice", 10, || !rows(&r.core, "wake.fired").is_empty()).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        before,
        "the notice took no turn and made no model call"
    );
    let n = next_noticed(&mut r).await;
    assert!(noticed(&mut r).is_empty(), "heard once");
    assert_eq!(n.note, "stretch");
    assert_eq!(n.session_id, sid);
}

/// A reminder is answered through `work.answer` (theseus-ds5os): a fired one
/// by its whole id, a pending one by a unique end of it; Done and the
/// snoozes are the wake owner's verbs, with their rows, and an option a
/// reminder lacks is refused with the four it has.
#[tokio::test(flavor = "multi_thread")]
async fn a_reminder_is_answered_through_work_answer() {
    let mut r = rig(true, |_| {});
    dm_session(&r.core);
    rpc(
        &r.core,
        "wake.create",
        json!({"when": "2s", "note": "stretch"}),
    )
    .await
    .unwrap();
    until("the first post", 10, || posts(&r.core).len() == 1).await;
    let id = posts(&r.core)[0].1["wake_id"].as_str().unwrap().to_string();
    let _ = noticed(&mut r);
    let answer = |id: String, option: &'static str| {
        let core = r.core.clone();
        async move {
            rpc(
                &core,
                "work.answer",
                json!({"question_id": id, "option": option}),
            )
            .await
        }
    };
    let e = answer(id.clone(), "approve")
        .await
        .expect_err("not a reminder's");
    assert!(e.contains("done, snooze_10m, snooze_1h, tomorrow_9"), "{e}");
    // Fired: snoozed by its whole id; it is pending again.
    let moved = answer(id.clone(), "snooze_1h").await.unwrap();
    assert_eq!(
        (moved["option"].as_str(), moved["question_id"].as_str()),
        (Some("snooze_1h"), Some(id.as_str()))
    );
    assert_eq!(rows(&r.core, "wake.snoozed").len(), 1);
    let list = rpc(&r.core, "wake.list", json!({})).await.unwrap();
    assert_eq!(list["wakes"].as_array().unwrap().len(), 1, "{list}");
    let due = |list: &Value| list["wakes"][0]["due_at_ms"].as_u64().unwrap();
    let now = theseus_protocol::now_unix_ms();
    assert!(
        due(&list) > now + 50 * 60_000,
        "an hour, not ten minutes: {list}"
    );
    // Pending: named by a unique end, set again for ten minutes.
    let end = id[id.len() - 6..].to_string();
    answer(end.clone(), "snooze_10m").await.unwrap();
    assert_eq!(rows(&r.core, "wake.snoozed").len(), 2);
    let list = rpc(&r.core, "wake.list", json!({})).await.unwrap();
    let now = theseus_protocol::now_unix_ms();
    assert!(due(&list) < now + 11 * 60_000, "ten minutes: {list}");
    // A connection that is not the Discord binding never answers as a
    // Discord user: refused, and the reminder is as it was.
    let posed = rpc(
        &r.core,
        "work.answer",
        json!({"question_id": end, "option": "done",
            "discord": {"user_id": "271828182845904523", "channel_id": "900000000000000007"}}),
    )
    .await
    .expect_err("only the binding names a Discord user");
    assert!(posed.contains("only the Discord binding"), "{posed}");
    assert!(rows(&r.core, "wake.done").is_empty());
    // Done settles it.
    let done = answer(end, "done").await.unwrap();
    assert_eq!(done["option"], "done");
    let settled = rows(&r.core, "wake.done");
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0]["wake_id"], id.as_str());
}
