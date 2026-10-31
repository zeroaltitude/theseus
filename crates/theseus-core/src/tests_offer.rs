//! The task offer (theseus-bh3t.5): `classify.v3`'s `offer_task` answer puts an
//! `offer` on the reply's post when it is yes at the act bar, in the owner's DM,
//! on a turn that is not a trivial or quick detour and made no task itself. The
//! provider is slow here (`PROVIDER_MS`), so the inbound batch has landed by the
//! time the reply is written; a batch held back (`Held`) shows the chip never
//! waits for it.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};

use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Parts;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_inbound::inbound_only;
use crate::tests_route::glm_placements;
use crate::{Config, Core};
use theseus_protocol::SessionKind;

const PROVIDER_MS: u64 = 600;
const LATER: &str = "we should clean up the bench reports later and then re-run the async suite";

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// A core whose classify batch acts and whose provider answers after `PROVIDER_MS`;
/// `routing`: route.v5 asks too, with every detour on the `zai` fake.
fn rig(jev: &FakeJev, script: Vec<Scripted>, routing: bool) -> Rig {
    rig_with(jev, script, routing, |_| {})
}

/// `rig`, with `more` said of the config last.
fn rig_with(
    jev: &FakeJev,
    script: Vec<Scripted>,
    routing: bool,
    more: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg: Config = crate::tests_judge::judge_config(dir.path(), Some(jev));
    inbound_only(&mut cfg);
    if routing {
        cfg.judge.packs.remove(crate::judge::inbound::ROUTE_PACK);
        cfg.routing.max_wait_ms = 5_000;
        glm_placements(&mut cfg);
    }
    more(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut fake = FakeProvider::scripted(script);
    fake.delay_ms = PROVIDER_MS;
    let mut p = Parts::for_tests(cfg, Arc::new(fake), store);
    let mut zai = FakeProvider::scripted(crate::tests_judge::texts(4));
    zai.delay_ms = PROVIDER_MS;
    p.providers.insert("zai".into(), Arc::new(zai));
    p.secrets = crate::tests_judge::board();
    let core = Core::build(p).unwrap();
    crate::tests_judge::warm(&core);
    Rig { core, _dir: dir }
}

/// A session bound to `place`, as the Discord binding binds one; a DM's person
/// is its owner once the binding binds it.
fn bound(core: &Core, place: &str) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(place, &rec.session_id).unwrap();
    if place.starts_with("dm:") {
        core.runner.place_rule.bind_one(crate::places::BoundPlace {
            target: format!("discord:{place}"),
            name: "DM".into(),
            private: false,
            ..Default::default()
        });
    }
    rec.session_id
}

/// Jev's `offer_task` answer for the next message.
fn offer(jev: &FakeJev, p: f64) {
    jev.script("offer_task", Jev::Noul(p));
}

/// The reply post `target` was staged, after one turn of `text`.
async fn reply(r: &Rig, place: &str, text: &str) -> Value {
    let sid = bound(&r.core, place);
    crate::tests_route::turn(&r.core, Some(&sid), text, None).await;
    let posts = r.core.outbox.open_for(&format!("discord:{place}"));
    assert_eq!(posts.len(), 1, "one reply post");
    let body = crate::outbox::body_of(&posts[0]).clone();
    assert_eq!(body["kind"], "reply");
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_yes_at_the_act_bar_puts_the_message_on_the_reply_as_an_offer() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    let r = rig(&jev, crate::tests_judge::texts(1), false);
    let body = reply(&r, "dm:42", LATER).await;
    assert_eq!(body["offer"], json!({"brief": LATER}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_offers_brief_is_the_messages_first_thousand_characters() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    let r = rig(&jev, crate::tests_judge::texts(1), false);
    let long = format!("later: {}", "x".repeat(1_500));
    let body = reply(&r, "dm:42", &long).await;
    let brief = body["offer"]["brief"].as_str().unwrap();
    assert_eq!(brief.chars().count(), 1_000);
    assert!(long.starts_with(brief));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_yes_at_the_confirm_band_or_a_no_offers_nothing() {
    for p in [0.75, 0.05] {
        let jev = FakeJev::start().unwrap();
        offer(&jev, p);
        let r = rig(&jev, crate::tests_judge::texts(1), false);
        let body = reply(&r, "dm:42", LATER).await;
        assert!(body.get("offer").is_none(), "p = {p}: {body}");
    }
}

/// classify.v3 in shadow (the config's ceiling) judges, and offers nothing:
/// "shadow before they act".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_judgment_offers_nothing() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    let r = rig_with(&jev, crate::tests_judge::texts(1), false, |c| {
        c.judge
            .packs
            .entry(crate::judge::inbound::CLASSIFY_PACK.into())
            .or_default()
            .mode = Some(crate::config::PackMode::Shadow);
    });
    let body = reply(&r, "dm:42", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_in_a_shared_place_offers_nothing() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    let r = rig(&jev, crate::tests_judge::texts(1), false);
    let body = reply(&r, "channel:7", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judge_that_has_not_answered_by_the_reply_costs_the_reply_nothing() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig(&jev, crate::tests_judge::texts(1), false);
    let body = reply(&r, "dm:42", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
    jev.release();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_made_a_task_offers_nothing() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    let script = vec![
        Scripted::tools(
            "Starting it.",
            &[("t_task", "task_create", json!({"title": "Bench reports"}))],
        ),
        Scripted::text("Done."),
    ];
    let r = rig(&jev, script, false);
    let body = reply(&r, "dm:42", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
}

/// The posts of `kind` waiting for `place`.
fn posts_of(r: &Rig, place: &str, kind: &str) -> Vec<Value> {
    r.core
        .outbox
        .open_for(&format!("discord:{place}"))
        .iter()
        .map(|a| crate::outbox::body_of(a).clone())
        .filter(|b| b["kind"] == kind)
        .collect()
}

/// theseus-ypnxt.5: the reply goes out without its chip while the batch is
/// slow (Jev's batch answers after the reply, its route question before it);
/// when the batch lands, the core posts the offer to the same place for the
/// binding to add by an edit.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_that_lands_after_the_reply_posts_the_offer_late() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    jev.set_latency(SLOW_BATCH, LATE_BATCH);
    let r = rig_with(&jev, crate::tests_judge::texts(1), false, |c| {
        c.judge.total_secs = 5;
    });
    let body = reply(&r, "dm:42", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
    assert!(posts_of(&r, "dm:42", "offer").is_empty(), "nothing yet");
    let turn = body["turn_id"].as_str().unwrap().to_string();
    for _ in 0..400 {
        if !posts_of(&r, "dm:42", "offer").is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let late = posts_of(&r, "dm:42", "offer");
    assert_eq!(late.len(), 1, "one late offer: {late:?}");
    assert_eq!(late[0]["turn_id"], json!(turn));
    assert_eq!(late[0]["brief"], json!(LATER));
}

/// A turn that made a task itself gets no late offer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_made_a_task_gets_no_late_offer() {
    let jev = FakeJev::start().unwrap();
    offer(&jev, 0.95);
    jev.set_latency(SLOW_BATCH, SLOW_BATCH);
    let script = vec![
        Scripted::tools(
            "Starting it.",
            &[("t_task", "task_create", json!({"title": "Bench reports"}))],
        ),
        Scripted::text("Done."),
    ];
    let r = rig(&jev, script, false);
    let body = reply(&r, "dm:42", LATER).await;
    assert!(body.get("offer").is_none(), "{body}");
    tokio::time::sleep(SLOW_BATCH + std::time::Duration::from_millis(1_000)).await;
    assert!(posts_of(&r, "dm:42", "offer").is_empty());
}

const SLOW_BATCH: std::time::Duration = std::time::Duration::from_millis(850);
/// The late-offer test's batch: past the reply even on a loaded machine (a
/// gate's reply took 1,053 ms beside an 850 ms batch, which then rode it),
/// under that test's 5 s judge budget (the rig's is 1 s).
const LATE_BATCH: std::time::Duration = std::time::Duration::from_millis(2_500);
