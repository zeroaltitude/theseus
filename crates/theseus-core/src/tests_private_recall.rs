//! The Google `private` mark follows recalled nodes (theseus-39dj0): a turn
//! whose recall admits a note of a session that holds the mark puts private
//! content in its request, so the asking session is marked, and the provider
//! floor refuses its requests to a model of restricted origin even for a
//! session with no content label. A note of an unmarked session leaves the asker unmarked, and with no
//! `[google]` nothing is read.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use crate::config::MemoryMode;
use crate::google::private;
use crate::provider::{FakeProvider, Provider, ProviderRequest};
use crate::tests_private_background::{google_on, mark};
use crate::tests_recall::{index_of, rig_with, session, turn, Rig};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";
const ASK: &str = "Where does the grey heron nest?";

/// Live recall for every session, `[google]` bound when `google`.
fn canary(google: bool) -> Rig {
    rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        if google {
            google_on(c);
        }
    })
}

fn private_now(r: &Rig, sid: &str) -> bool {
    private::session(&r.core.store, &r.core.cfg, sid)
}

/// A request to GLM (China origin): the floor sends it for a plain session
/// whose items were judged, and refuses it for a private one.
fn glm_request() -> ProviderRequest {
    ProviderRequest {
        model: "glm-5.3-flash".into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": "and now?"}]})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// `sid`'s next call to GLM through the floor, as a background step makes it
/// (`private::for_session`), once a judge that finds nothing has reached its
/// items: Ok when sent, else the refusal's class.
async fn glm_call(r: &Rig, sid: &str) -> Result<(), String> {
    glm_call_on(&r.core, sid).await
}

/// [`glm_call`], for any rig's core.
pub(crate) async fn glm_call_on(core: &crate::Core, sid: &str) -> Result<(), String> {
    let judge = crate::content_wait::judging::Judging::clean();
    judge.attach(&core.store);
    judge.land(sid);
    let fake: Arc<dyn Provider> = Arc::new(FakeProvider::default());
    let gated = crate::data_gate::guard(
        BTreeMap::from([("zai".to_string(), fake)]),
        &core.cfg,
        &core.catalog,
        &core.store,
        &Default::default(),
    );
    let p = private::for_session(gated["zai"].clone(), &core.store, &core.cfg, sid);
    let mut sink = |_: crate::provider::Delta<'_>| {};
    match p.stream_message(&glm_request(), &mut sink).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e
            .downcast_ref::<crate::provider::ProviderError>()
            .map_or("", |p| p.class())
            .to_string()),
    }
}

/// Session B recalls a note of private session A: B is marked, and B's next
/// request to a model of restricted origin is refused by the floor, though
/// it has no content label.
#[tokio::test]
async fn a_session_that_recalls_a_private_note_becomes_private() {
    let r = canary(true);
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    mark(c, &a);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    assert!(!private_now(&r, &b));
    assert_eq!(glm_call(&r, &b).await, Ok(()), "a plain session is sent it");
    let res = turn(c, &b, ASK).await;
    assert_eq!(res.recalled, 1, "the note of A was admitted");
    assert!(private_now(&r, &b), "the mark followed the recalled node");
    assert_eq!(glm_call(&r, &b).await, Err("data_gate".to_string()));
}

/// The same recall of an unmarked session's note leaves the asker unmarked.
#[tokio::test]
async fn recalling_a_plain_note_leaves_the_asker_unmarked() {
    let r = canary(true);
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let res = turn(c, &b, ASK).await;
    assert_eq!(res.recalled, 1);
    assert!(!private_now(&r, &b));
    assert_eq!(glm_call(&r, &b).await, Ok(()));
}

/// With no `[google]` nothing is read: A's mark is in the store, yet B's
/// recall of A's note writes no mark for B (a read would have found A's).
#[tokio::test]
async fn with_no_google_the_recall_reads_no_mark() {
    let r = canary(false);
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    mark(c, &a);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let res = turn(c, &b, ASK).await;
    assert_eq!(res.recalled, 1);
    let stored = c.store.get_meta::<String>(&private::meta_key(&b)).unwrap();
    assert_eq!(stored, None, "no mark written for B");
}

/// `memory.lookup` is a recall the model asks for: its answer from a marked
/// session's messages is marked, so the frame that writes the result marks
/// the asker (`private::record`); from a plain session's it is not.
#[tokio::test]
async fn a_lookup_of_a_private_sessions_messages_is_marked() {
    let r = canary(true);
    let c = &r.core;
    let (a, plain) = (session(c, None, &[HERON]), session(c, None, &[HERON]));
    mark(c, &a);
    let b = session(c, None, &[]);
    let out = crate::memory_lookup::call(c.clone(), &b, json!({"session": a}))
        .await
        .unwrap();
    assert!(out.text.contains("old weir"), "{}", out.text);
    assert!(private::marked(&out.meta), "{}", out.meta);
    let out = crate::memory_lookup::call(c.clone(), &b, json!({"session": plain}))
        .await
        .unwrap();
    assert!(!private::marked(&out.meta), "{}", out.meta);
}

/// The turn that recalls the note is private itself, before its frame writes
/// the mark: with the default served from a host vouched EU (no restricted
/// origin, only the private mark can refuse it), the request that holds A's
/// note is never sent; a plain note's is.
#[tokio::test]
async fn the_recalling_turn_itself_is_held_by_the_floor() {
    for marked in [false, true] {
        let r = rig_with(MemoryMode::Canary, |c| {
            c.memory.canary_fraction = 1.0;
            crate::tests_private_background::private_on(c);
        });
        let c = &r.core;
        let a = session(c, None, &[HERON]);
        if marked {
            mark(c, &a);
        }
        let b = session(c, None, &[]);
        c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
        let _ = try_turn(c, &b, ASK).await;
        let sent = r.model.requests.lock().unwrap().clone();
        let held_note = sent.iter().any(|q| {
            serde_json::to_string(&q.messages)
                .unwrap()
                .contains("old weir")
        });
        assert_eq!(
            held_note,
            !marked,
            "marked={marked}: {} requests",
            sent.len()
        );
    }
}

async fn try_turn(
    core: &Arc<crate::Core>,
    sid: &str,
    input: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: crate::session::SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
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
}
