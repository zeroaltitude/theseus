//! `/why` (theseus-ighf, `runtime/why.rs`): registered with the others,
//! answered ephemerally with the place's last turn (its tool call and the
//! gate's posture for it), written nowhere, and fitted to Discord's limit; a
//! shared place is told that a private or imported source was there, and
//! shown nothing of it.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::Scripted;
use theseus_core::Core;
use theseus_protocol::context::ContextExplainResult;
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
use tokio::sync::mpsc;

use super::tests::{place_for_tests, slash, OWNER};
use super::why::fenced;
use super::{commands, shared_for_tests, Control, PlaceMsg};
use crate::rpc_client::RpcClient;
use theseus_core::approval::{Client, Surface};

/// A core whose model answers from `script` and whose tools ask first, with
/// its Discord REST at the fake.
fn core_at(dir: &std::path::Path, rest: &str, script: Vec<Scripted>) -> Arc<Core> {
    let mut cfg = theseus_core::Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = theseus_core::policy::Posture::Approve;
    // The memory pass writes its own frame a moment after a turn (`QUIET`,
    // `WINDOW`), which a read's "no frame, no row" would count as the read's
    // when a loaded host takes that long to get from the snapshot to the check.
    cfg.memory.mode = theseus_core::config::MemoryMode::Off;
    cfg.discord.rest_proxy = Some(rest.to_string());
    cfg.discord.gateway_proxy = Some("ws://127.0.0.1:9".into());
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
    let fake: Arc<dyn theseus_core::provider::Provider> =
        Arc::new(theseus_core::provider::FakeProvider::scripted(script));
    let providers = [(cfg.model.provider.clone(), fake)].into_iter().collect();
    Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets: theseus_core::secrets::SecretBoard::empty(),
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("test"),
        toollets: vec![],
        cpu_cores: None,
    })
    .unwrap()
}

/// A session of `core`'s, bound to the DM place, whose one turn called
/// `fs_write` and is waiting for its confirm; its id.
async fn turn_that_asks(core: &Arc<Core>) -> String {
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    core.outbox.bind_place("dm:42", &sid).unwrap();
    // A bound place's calls ask first (the place rule).
    core.bind_places(vec![theseus_core::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    }]);
    let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new("test", Surface::Cli));
    let first: TurnSubmitResult = rpc
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                surface: None,
                carried: false,
                prompt: None,
                session_id: Some(sid.clone()),
                input: "write a file".into(),
                profile: None,
                provider: None,
                model: None,
                author: Some("test".into()),
                attachments: vec![],
                reply_to: None,
                opened_from: None,
                dir: None,
                thread: None,
                speaking: None,
                in_reply_to: None,
                route_at_ms: None,
                deadline_secs: None,
                via: None,
            },
        )
        .await
        .unwrap();
    assert!(first.awaiting_confirm.is_some(), "its call asks first");

    sid
}

#[test]
fn why_is_registered_with_the_other_commands() {
    let names: Vec<String> = commands().into_iter().map(|c| c.name).collect();
    assert!(names.iter().any(|n| n == "why"), "{names:?}");
    assert_eq!(names.iter().filter(|n| *n == "why").count(), 1);
}

/// The press: acknowledged at once with the ephemeral flag, then edited with
/// the text of the last turn (its tool and the gate's posture), within
/// Discord's limit, and the core's store gains no frame and no row.
#[tokio::test]
async fn why_answers_ephemerally_with_the_last_turn_and_writes_nothing() {
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let core = core_at(
        d.path(),
        &fake.addr,
        vec![
            Scripted::tools(
                "Writing.",
                &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
            ),
            Scripted::text("Done."),
        ],
    );
    let sid = turn_that_asks(&core).await;
    let shared = shared_for_tests(&core);
    let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    let (place, _rx) = place_for_tests(&core, &sid);

    let (frames, last) = (
        core.store.stats().unwrap().frames_appended,
        core.store.last_position(),
    );
    let h = tokio::spawn(
        shared
            .clone()
            .on_interaction(slash(None, 444, OWNER, "why")),
    );
    let got = tokio::time::timeout(Duration::from_secs(10), dm_rx.recv())
        .await
        .unwrap();
    let Some(PlaceMsg::Control { cmd, reply, .. }) = got else {
        panic!("not a control");
    };
    assert_eq!(cmd, Control::Why);
    let said = place.why().await;
    let _ = reply.send(said.clone());
    h.await.unwrap();

    assert!(
        said.chars().count() <= 2000,
        "{} chars",
        said.chars().count()
    );
    assert!(
        said.starts_with("```\n") && said.ends_with("\n```"),
        "{said}"
    );
    assert!(said.contains(&format!("session {sid}")), "{said}");
    assert!(
        (said.contains("fs.write") || said.contains("fs_write")) && said.contains("approve"),
        "{said}"
    );
    let replies = fake.replies();
    let callback = replies
        .iter()
        .find(|r| r.kind == "callback")
        .expect("acknowledged");
    assert_eq!(callback.response_type, Some(5), "a deferred message");
    assert!(callback.ephemeral, "only the presser sees it: {callback:?}");
    let edit = replies
        .iter()
        .find(|r| r.kind == "original")
        .expect("edited");
    assert_eq!(edit.content.as_deref(), Some(said.as_str()));
    assert_eq!(
        (
            core.store.stats().unwrap().frames_appended,
            core.store.last_position()
        ),
        (frames, last),
        "a read: no frame, no row"
    );
}

/// A shared place's `/why`: an imported or another session's note is said to
/// have been there, with none of its words, and a long turn still fits in
/// one message with the rest counted.
#[test]
fn a_shared_place_hides_a_private_source_and_the_text_fits_the_limit() {
    let admitted: Vec<_> = (0..40)
        .map(|k| {
            json!({"node_id": format!("nod_{k}"), "chunk": 0, "session_id": "ses_ep9", "position": k,
                "kind": "message", "rank": k + 1, "fused": 0.5, "tokens": 100})
        })
        .collect();
    let parts: Vec<_> = (0..30)
        .map(|k| json!({"block": "context", "name": format!("docs/file-{k}.md"), "bytes": 10, "tokens": 5}))
        .collect();
    let x: ContextExplainResult = serde_json::from_value(json!({
        "session_id": "ses_reef42", "kind": "conversation", "class": "shared", "place": "#pier",
        "turns": [], "turn_id": "trn_k3m9x2", "profile": "deep", "model": "glm-x",
        "parts": parts, "digest_now": "d", "ms": 1.0,
        "recalls": [{"recall_id": "rcl_1", "mode": "shadow", "science": "baseline@ab", "outcome": "ran",
            "place": "shared:pier", "query_chars": 4, "query_digest": "00", "candidates": 40,
            "admitted": admitted, "dropped": [], "budget_tokens": 1, "used_tokens": 1,
            "timings": {"index_ms": 1.0, "pack_ms": 1.0, "total_ms": 1.0, "deadline_ms": 250}}],
        "sources": {"ses_ep9": {"place": "CLI", "imported": {
            "session_id": "ses_ep9", "episode_id": "ep9", "tag": "chat", "source": "chat export",
            "place_kind": "chat", "place_name": "tide office", "start_ms": 1772582400000u64,
            "end_ms": 1772668800000u64, "sensitivity": "personal", "topics": [], "messages": 3, "summary": true, "file": "chat.jsonl", "line": 1, "imported_at_ms": 1u64}}}
    }))
    .unwrap();
    let said = fenced(&x, None, None, &[]);
    assert!(
        said.chars().count() <= 2000,
        "{} chars",
        said.chars().count()
    );
    for secret in ["tide office", "chat export", "ses_ep9"] {
        assert!(
            !said.contains(secret),
            "{secret} reached a shared place:\n{said}"
        );
    }
    assert!(
        said.contains("an imported source (not shown in a shared place)"),
        "{said}"
    );
    assert!(said.contains("… and 37 more"), "{said}");

    // A title far past the limit has its line clipped, and the sections
    // after it still say what they have, in one message.
    let mut long = x;
    long.title = Some("x".repeat(3000));
    long.session_id = "ses_a".repeat(300);
    let said = fenced(&long, None, None, &[]);
    assert!(
        said.chars().count() <= 2000,
        "{} chars",
        said.chars().count()
    );
    assert!(
        said.contains("…") && said.contains("recalled: 40 of"),
        "{said}"
    );
}
