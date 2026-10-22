//! Compaction obeys the per-request data rule (theseus-a1wvq.10): the rig is
//! `tests_open_request`'s (a restricted `dsflash` on the `zai` fake, the only
//! profile of `trivial`'s `cn_profiles`, the route pack live) on a small
//! window, and `glm` stays of restricted origin (the suite never serves it
//! unrestricted).
//!
//! - an open session's summary of a range its run clears goes to its own
//!   restricted model with a pass, joins the run, and the run is stored;
//! - a restricted summary profile never reads a private session: the
//!   `[model]` default writes the summary and the row says why.
//!
//! The consolidation, keep-warm and replay tests are in their own suites.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::data;
use crate::node::Body;
use crate::provider::ProviderRequest;
use crate::session::SessionRecord;
use crate::test_support::rows_2k;
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
        |_| {},
    )
}

/// Jev: every message `chat` at a low confidence (no detour, no switch), its
/// data `open` at the act band when `open`, else `private`.
fn jev(open: bool) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    let data = if open { "open" } else { "private" };
    jev.script("data", choice(data, 0.95));
    mode(&jev, "chat", 0.2);
    jev
}

fn choice(option: &str, confidence: f64) -> Jev {
    Jev::Choice {
        option: option.into(),
        confidence,
    }
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

/// A session opened as an open task's is (`data.opened`), on `profile`.
async fn open_session(r: &Rig, profile: &str) -> String {
    call(&r.core, "profile.use", json!({"name": profile})).await;
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    crate::tests_open_view::open(&r.core, &rec.session_id, None);
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

fn open_messages() -> Vec<String> {
    (0..12).map(|k| words(&format!("open{k}"), 6_000)).collect()
}

/// T23a, the key test. An open session on `dsflash` with `summary_profile =
/// "session"`, every turn a no-tool answer, so its run is whole. When loop 0
/// rings, the summary call goes to `dsflash` with a pass of its own (nothing
/// is refused), a `Summary` node is written, the compaction's request goes
/// to `dsflash` too, and the turn stores its run through its last node: the
/// next turn is admitted on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_tasks_compaction_carries_its_own_pass() {
    let messages = open_messages();
    let jev = jev(true);
    let r = rig(&jev, |c| c.memory.summary_profile = "session".into());
    let sid = open_session(&r, "dsflash").await;
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
    assert_eq!(blocked(&r.core), 0, "every call carried its pass");
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

    let last = nodes.last().unwrap().1.id.clone();
    let stored = record(&r.core, &sid).data.unwrap();
    assert_eq!(stored.run.as_ref().unwrap().through, last);
    assert!(data::admits_again(Some(&stored), Some(&last)).is_ok());
    assert!(stored.taint.is_none());
}

/// T23b. A DM on a Claude profile, in the private view, whose
/// `summary_profile` is `glm`, of restricted origin: `glm` is sent nothing,
/// the `[model]` default writes the summary, and the row names it and says
/// why. Nothing reaches the chokepoint's refusal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_summary_profile_in_a_private_session_falls_back() {
    let messages: Vec<String> = (0..12).map(|k| words(&format!("dm{k}"), 6_000)).collect();
    let jev = jev(false);
    let r = rig(&jev, |c| c.memory.summary_profile = "glm".into());
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
            .contains("data_gate: private_view"),
        "{row}"
    );
}
