//! The owner's DM against a restricted `dsflash` (theseus-9ymz0): the rig the
//! open view's tests shared (`tests_open_request`, retired with it), kept for
//! the suites that still ride it. `dsflash` is GLM's model on the `zai` fake, of
//! restricted origin, the only profile of `trivial`'s `cn_profiles`; the route
//! pack is live; the owner's DM is bound; the owner's private notes are a
//! context file. A session may use `dsflash` only once its items were judged,
//! which no judge does here: a test that wants a turn on it stands a
//! [`Judging`](crate::content_wait::judging::Judging) behind the runner
//! (`judging`).

use std::sync::Arc;

use serde_json::Value;
use theseus_judge::fake::FakeJev;
use theseus_protocol::standing::{StandingAddParams, StandingKind, StandingScope};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::content_wait::judging::Judging;
use crate::context_files::ContextEntry;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::tests_judge::kinds;
use crate::tests_route::{mode, rig_parts, Rig};
use crate::turn::TurnRequest;
use crate::Core;

pub(crate) const OWNER: u64 = crate::tests_places::OWNER;
/// The owner's private notes, a context file not marked public.
pub(crate) const SECRET: &str = "the vault code is 4417";
/// A standing fact of the owner's.
const FACT: &str = "the owner keeps bees at Hollowmere";
/// A message with a marker.
pub(crate) const PRIVATE: &str = "Remind me what the Acme lease said, marker-7c1q.";
/// Plain messages.
pub(crate) const OPEN_A: &str = "What is the boiling point of water at sea level?";
pub(crate) const OPEN_B: &str = "And how many grams in a kilogram?";

/// The config: `dsflash` (GLM's model on the `zai` fake, of restricted
/// origin) in `trivial`'s `cn_profiles` alone, `trivial`'s own list
/// without Haiku, the owner's DM, the private notes.
pub(crate) fn config(c: &mut crate::Config, notes: &std::path::Path) {
    c.copy_profile("dsflash", "glm");
    c.routing.classes.trivial.profiles = vec!["cheapest".into()];
    c.routing.classes.trivial.cn_profiles = vec!["dsflash".into()];
    c.routing.classes.trivial.cn_after = None;
    c.routing.trivial_context_turns = 6;
    c.places.owner = Some(vec![format!("discord:{OWNER}")]);
    c.context.files = vec![ContextEntry::Path(notes.to_string_lossy().into_owned())];
}

/// A rig as `config` says, its `zai` fake answering by `zai` when given;
/// and the fake that records what reached `dsflash`.
pub(crate) fn rig_with(
    jev: &FakeJev,
    zai: Option<Vec<Scripted>>,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, format!("{SECRET}\n")).unwrap();
    let fake = zai.map(|s| Arc::new(FakeProvider::scripted(s)));
    let mine = fake.clone();
    let r = rig_parts(
        Some(jev),
        8,
        |c| {
            config(c, &notes);
            tweak(c);
        },
        move |p| {
            if let Some(f) = mine {
                p.providers.insert("zai".into(), f);
            }
        },
    );
    r.core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Fact,
                text: FACT.into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let zai = fake.unwrap_or_else(|| r.zai.clone());
    (r, zai, dir)
}

/// `rig_with`, its core's telemetry the given one's.
pub(crate) fn rig_telemetry(
    jev: &FakeJev,
    tel: crate::telemetry::Telemetry,
) -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    rig_telemetry_with(jev, tel, |_| {})
}

/// [`rig_telemetry`], with a tweak of the config.
pub(crate) fn rig_telemetry_with(
    jev: &FakeJev,
    tel: crate::telemetry::Telemetry,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Rig, Arc<FakeProvider>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, format!("{SECRET}\n")).unwrap();
    let r = rig_parts(
        Some(jev),
        8,
        |c| {
            config(c, &notes);
            tweak(c);
        },
        move |p| p.telemetry = Some(tel),
    );
    let zai = r.zai.clone();
    (r, zai, dir)
}

/// Stand a judge that finds nothing behind the rig's runner: every item a
/// turn waits on is judged clean at once.
pub(crate) fn judging(r: &Rig) -> Arc<Judging> {
    judging_with(r, Judging::clean())
}

/// [`judging`], with the given judge.
pub(crate) fn judging_with(r: &Rig, j: Arc<Judging>) -> Arc<Judging> {
    j.attach(&r.core.store);
    r.core.runner.verdicts.set(j.clone());
    j
}

/// Make `sid` a draft's session as the core's own opening does: a task's
/// record with the draft's META record beside it (`draft::is_draft`).
pub(crate) fn mark_draft(core: &Core, sid: &str) {
    let mut rec = record(core, sid);
    rec.task = Some(crate::session::TaskOf {
        parent_session: "ses_parent".into(),
        parent_execution: "exe_parent".into(),
        by: "act_draft".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    core.store.put_session(sid, &rec).unwrap();
    core.store
        .put_meta(
            &crate::draft::meta_key(sid),
            &crate::draft::Draft::default(),
        )
        .unwrap();
}

/// A session on `dsflash`, the live profile: fresh, in no place.
pub(crate) async fn cn_session(r: &Rig) -> String {
    crate::tests_route::call(
        &r.core,
        "profile.use",
        serde_json::json!({"name": "dsflash"}),
    )
    .await;
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// Jev: every message `trivial`.
pub(crate) fn jev(_: &[&str]) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    jev
}

/// A session posting to the owner's DM.
pub(crate) fn dm(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox
        .bind_place(&format!("dm:{OWNER}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// The owner's message in `sid`, on the live profile or `chosen`.
pub(crate) async fn say(
    core: &Arc<Core>,
    sid: &str,
    text: &str,
    chosen: Option<&str>,
) -> TurnSubmitResult {
    try_say(core, sid, text, chosen).await.unwrap()
}

pub(crate) async fn try_say(
    core: &Arc<Core>,
    sid: &str,
    text: &str,
    chosen: Option<&str>,
) -> anyhow::Result<TurnSubmitResult> {
    let rec = record(core, sid);
    let (live, _) = core.live_profile();
    let mut target = core.runner.resolve_target(&live, chosen, None, None)?;
    target.chosen = chosen.map(|p| format!("profile {p}"));
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(text.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

pub(crate) fn record(core: &Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

pub(crate) fn text(req: &ProviderRequest) -> String {
    serde_json::to_string(req).unwrap()
}

pub(crate) fn blocked(core: &Core) -> usize {
    kinds(&core.store, "data_gate.blocked").len()
}

// ------------------------------------------------------------ telemetry

use crate::telemetry::tests::Receiver;
use crate::telemetry::{Telemetry, TelemetryConfig};

/// `theseus.route.data_gate`'s points after a flush: (what, why, count).
pub(crate) async fn gate_counts(core: &Core, rx: &Receiver) -> Vec<(Value, Value, Value)> {
    let flushed = core
        .telemetry()
        .flush(std::time::Duration::from_secs(10))
        .await;
    assert!(flushed);
    let got = rx.at("/v1/metrics");
    let metrics = got
        .last()
        .map(|g| g.body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"].clone())
        .and_then(|m| m.as_array().cloned())
        .unwrap_or_default();
    let Some(m) = metrics
        .iter()
        .find(|m| m["name"] == "theseus.route.data_gate")
    else {
        return vec![];
    };
    let attr = |p: &Value, k: &str| {
        p["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == k)
            .unwrap()["value"]["stringValue"]
            .clone()
    };
    m["sum"]["dataPoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                attr(p, "theseus.data_gate.what"),
                attr(p, "theseus.data_gate.why"),
                p["asInt"].clone(),
            )
        })
        .collect()
}

/// A telemetry receiver and a pipeline that exports to it.
pub(crate) async fn telemetry() -> (Receiver, Telemetry) {
    let rx = Receiver::start(vec![]).await;
    let tel = TelemetryConfig {
        otlp_endpoint: Some(rx.endpoint()),
        ..Default::default()
    };
    let t = Telemetry::from_config(&tel, None).unwrap();
    (rx, t)
}
