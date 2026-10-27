//! Compaction obeys the content labels (theseus-a1wvq.10, theseus-9ymz0): the
//! rig is `tests_dm_rig`'s style (a restricted `dsflash` on the `zai` fake, the
//! only profile of `trivial`'s `cn_profiles`, the route pack live) on a small
//! window, and `glm` stays of restricted origin (the suite never serves it
//! unrestricted).
//!
//! - a session with no label, its items judged, has its summary written by its
//!   own restricted model;
//! - a restricted summary profile never reads a session labelled
//!   `confidential`: the `[model]` default writes the summary and the row says
//!   why.
//!
//! The consolidation, keep-warm and replay tests are in their own suites.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::content::Flag;
use crate::content_wait::judging::Judging;
use crate::node::Body;
use crate::provider::ProviderRequest;
use crate::session::SessionRecord;
use crate::test_support::rows_2k;
use crate::tests_dm_rig::{gate_counts, judging_with, telemetry};
use crate::tests_judge::kinds;
use crate::tests_route::{call, mode, rig_parts, Rig};
use crate::turn::TurnRequest;
use crate::Core;

const OWNER: u64 = crate::tests_places::OWNER;
/// The session's model's window and output cap in this suite: the kept turns
/// leave the summary room (`tests_compaction`'s).
const WINDOW: u64 = 48_000;
const OUTPUT: u32 = 2_000;
/// What a summary call's system says.
const SUMMARIZES: &str = "You summarize the earlier part of a conversation";

/// A message of about `tokens` tokens of prose, its first word `tag`.
fn words(tag: &str, tokens: usize) -> String {
    let mut s = format!("{tag} ");
    while s.len() < tokens * 4 {
        s.push_str("the keeper logged the tide and the wind before the lamp was lit. ");
    }
    s
}

/// The config: `dsflash` of restricted origin alone in `trivial`'s
/// `cn_profiles`, the owner's DM, and a small window for both models.
fn rig(jev: &FakeJev, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    rig_with(jev, tweak, |_| {})
}

/// `rig`, its parts as `parts` leaves them (a scripted `zai`).
fn rig_with(
    jev: &FakeJev,
    tweak: impl FnOnce(&mut crate::Config),
    parts: impl FnOnce(&mut crate::rpc::Parts),
) -> Rig {
    rig_parts(
        Some(jev),
        60,
        |c| {
            c.copy_profile("dsflash", "glm");
            c.routing.classes.trivial.profiles = vec!["cheapest".into()];
            c.routing.classes.trivial.cn_profiles = vec!["dsflash".into()];
            c.routing.classes.trivial.cn_after = None;
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
            for model in ["glm-5.3-flash", "claude-sonnet-5-5"] {
                c.catalog.insert(
                    model.into(),
                    crate::catalog::CatalogRow {
                        context_window: Some(WINDOW),
                        max_output_tokens: Some(OUTPUT),
                        ..Default::default()
                    },
                );
            }
            tweak(c);
        },
        parts,
    )
}

/// Jev: every message `chat` at a low confidence (no detour, no switch).
fn jev() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.2);
    jev
}

/// A session posting to the owner's DM.
fn dm(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox
        .bind_place(&format!("dm:{OWNER}"), &r.session_id)
        .unwrap();
    r.session_id
}

/// A fresh session, `profile` the live one.
async fn session_on(r: &Rig, profile: &str) -> String {
    call(&r.core, "profile.use", json!({"name": profile})).await;
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

async fn say(core: &Arc<Core>, sid: &str, text: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
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
        .unwrap()
}

/// Turns of about 6,000 tokens until one's compilation is not the
/// transcript: the turn that rang.
async fn until_rang(r: &Rig, sid: &str, messages: &[String]) -> TurnSubmitResult {
    for m in messages {
        let res = say(&r.core, sid, m).await;
        let compiled = rows_2k(&r.core, "context.compiled");
        if compiled
            .last()
            .is_some_and(|c| c["strategy"] != "transcript")
        {
            return res;
        }
    }
    panic!("never rang");
}

fn record(core: &Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

fn blocked(core: &Core) -> usize {
    kinds(&core.store, "data_gate.blocked").len()
}

fn summary_calls(sent: &[ProviderRequest]) -> usize {
    sent.iter()
        .filter(|q| {
            serde_json::to_string(&q.system)
                .unwrap()
                .contains(SUMMARIZES)
        })
        .count()
}

fn compacted(core: &Core) -> Vec<Value> {
    rows_2k(core, "context.compacted")
}

fn messages(tag: &str) -> Vec<String> {
    (0..12)
        .map(|k| words(&format!("{tag}{k}"), 6_000))
        .collect()
}

/// T23a, the key test. A session on `dsflash` with `summary_profile =
/// "session"`, every turn a no-tool answer, no label on it, its items judged.
/// When loop 0 rings, the summary call goes to `dsflash` (nothing is refused),
/// a `Summary` node is written, and the compaction's request goes to `dsflash`
/// too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cn_sessions_compaction_goes_to_its_own_model() {
    let messages = messages("cn");
    let jev = jev();
    let r = rig(&jev, |c| c.memory.summary_profile = "session".into());
    judging_with(&r, Judging::clean());
    let sid = session_on(&r, "dsflash").await;
    let res = until_rang(&r, &sid, &messages).await;
    assert_eq!(res.profile, "dsflash", "{:?}", res.route);

    let sent = r.zai.requests();
    assert_eq!(
        summary_calls(&sent),
        1,
        "the summary call reached dsflash: {:?}",
        compacted(&r.core)
    );
    assert!(r.claude.requests().is_empty(), "nothing went elsewhere");
    assert_eq!(blocked(&r.core), 0, "nothing was refused");
    let nodes = r.core.store.transcript(&sid).unwrap();
    assert!(
        nodes
            .iter()
            .any(|(_, n)| matches!(n.body, Body::Summary { .. })),
        "a summary was written"
    );
    let rows = compacted(&r.core);
    let row = rows.last().unwrap();
    assert_eq!(
        (&row["outcome"], &row["profile"]),
        (&json!("compaction"), &json!("dsflash")),
        "{row}"
    );
    let compiled = rows_2k(&r.core, "context.compiled");
    assert_eq!(compiled.last().unwrap()["strategy"], "compaction");
    assert!(
        !sent
            .last()
            .is_some_and(|q| summary_calls(std::slice::from_ref(q)) == 1),
        "the compaction's own request followed the summary's"
    );
    let stored = record(&r.core, &sid).data.unwrap();
    assert!(stored.labels.is_empty(), "{stored:?}");
    assert!(stored.judged_through.is_some(), "its items were judged");
}

/// T23b. A DM on a Claude profile whose `summary_profile` is `glm`, of
/// restricted origin, and whose session the judge labelled `confidential`:
/// `glm` is sent nothing, the `[model]` default writes the summary, and the row
/// names it and says why. Nothing reaches the chokepoint's refusal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_summary_profile_in_a_labelled_session_falls_back() {
    let messages = messages("dm");
    let jev = jev();
    let r = rig(&jev, |c| c.memory.summary_profile = "glm".into());
    judging_with(&r, Judging::with(|_, _| vec![Flag::Confidential]));
    call(&r.core, "profile.use", json!({"name": "sonnet"})).await;
    let sid = dm(&r.core);
    let res = until_rang(&r, &sid, &messages).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);

    assert!(r.zai.requests().is_empty(), "glm was sent nothing");
    assert_eq!(summary_calls(&r.claude.requests()), 1, "default wrote it");
    assert_eq!(blocked(&r.core), 0);
    let rows = compacted(&r.core);
    let row = rows.last().unwrap();
    assert_eq!(
        (&row["outcome"], &row["profile"]),
        (&json!("compaction"), &json!("default")),
        "{row}"
    );
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("data_gate: confidential"),
        "{row}"
    );
}

/// theseus-4h85l: each background path that refuses a restricted-origin model
/// counts it on `theseus.route.data_gate`, by caller and why: a compaction
/// (the flag's word), a consolidation, a keep-warm read and a replay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_background_refusals_are_counted() {
    let (rx, tel) = telemetry().await;
    let messages = messages("dm");
    let jev = jev();
    let r = rig_with(
        &jev,
        |c| c.memory.summary_profile = "glm".into(),
        move |p| p.telemetry = Some(tel),
    );
    judging_with(&r, Judging::with(|_, _| vec![Flag::Confidential]));
    call(&r.core, "profile.use", json!({"name": "sonnet"})).await;
    let sid = dm(&r.core);
    until_rang(&r, &sid, &messages).await;

    let mut report = theseus_protocol::memory::SynthesisReport::default();
    assert_eq!(
        r.core.unrestricted("glm".into(), &mut report).as_deref(),
        Some("default")
    );
    let target = r
        .core
        .runner
        .resolve_target("sonnet", Some("glm"), None, None);
    assert!(r.core.refuse_restricted(&target.unwrap()).is_err());
    call(&r.core, "profile.use", json!({"name": "dsflash"})).await;
    let kept = dm(&r.core);
    assert!(r.core.runner.keep_warm_prepare(&kept, "dsflash").is_err());

    let mut got: Vec<(String, String)> = gate_counts(&r.core, &rx)
        .await
        .into_iter()
        .map(|(what, why, _)| {
            (
                what.as_str().unwrap().to_string(),
                why.as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        got,
        vec![
            pair("compaction", "confidential"),
            pair("consolidation", "restricted_origin"),
            pair("keep_warm", "restricted_origin"),
            pair("replay", "restricted_origin"),
        ]
    );
}
