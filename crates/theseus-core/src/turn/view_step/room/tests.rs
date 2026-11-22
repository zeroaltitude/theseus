//! The view makes its own room (`room.rs`): its steps and the provider's stated window, and real turns in a
//! place set to `view` (`AssemblySetting::All(Assembly::View)`) on a small window, with a scripted model that
//! reads large logs one a loop, each answer thinking first.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use super::{stated_window, steps};
use crate::bus::EventSink;
use crate::compiler::view::KEEP_TOKENS;
use crate::compiler::Overflowed;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::provider::binding::Binding;
use crate::provider::{FakeProvider, ProviderError, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::view::record::Room;
use crate::{Config, Core};

const OUTPUT: u32 = 2_000;
const WINDOW: u64 = 80_000;
const CHARS: usize = 8_000;
const ASK: &str = "Read every log under logs/ and tell me whether the keepers agree.";
const DONE: &str = "All the logs agree with the tide table.";
const LINE: &str = "the keeper logged the tide at the north light and the table agreed. ";

#[test]
fn the_steps_clear_first_then_lower_slot_g_to_none() {
    let at = |keep: u64, here: Option<u64>| Room {
        made: 1,
        keep: Some(keep),
        here,
        held: 8_000,
        ..Room::default()
    };
    let now = Room {
        held: 8_000,
        ..Room::default()
    };
    assert_eq!(
        steps(now, 1_000_000),
        vec![
            at(KEEP_TOKENS, None),
            at(0, None),
            at(0, Some(4_000)),
            at(0, Some(2_000)),
            at(0, Some(1_000)),
            at(0, Some(0)),
        ]
    );
    // A small budget keeps a quarter of it whole, so the next room is a while off.
    assert_eq!(steps(now, 40_000)[0].keep, Some(10_000));
    // A turn that made room before clears again, and halves from the allowance it lowered.
    let lowered = Room {
        made: 3,
        keep: Some(0),
        here: Some(1_000),
        held: 900,
        ..Room::default()
    };
    let next = steps(lowered, 1_000_000);
    assert_eq!(next[0].keep, Some(KEEP_TOKENS));
    assert!(next.iter().all(|r| r.made == 4));
    assert_eq!(next.last().unwrap().here, Some(0));
    // Nothing in slot G: clearing is all there is to try.
    assert_eq!(steps(Room::default(), 1_000_000).len(), 2);
}

#[test]
fn the_stated_window_is_read_at_the_estimate() {
    let o = |counted, maximum| Overflowed {
        counted,
        estimated: 150_000,
        maximum,
        retrying: None,
    };
    assert_eq!(stated_window(&o(None, Some(200_000))), Some(200_000));
    assert_eq!(
        stated_window(&o(Some(100_000), Some(200_000))),
        Some(200_000)
    );
    // The provider counted more than the estimate: its window, at the estimate, is that much less.
    assert_eq!(
        stated_window(&o(Some(250_000), Some(200_000))),
        Some(120_000)
    );
    assert_eq!(stated_window(&o(Some(250_000), None)), None);
}

struct Rig {
    core: Arc<Core>,
    model: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

/// A read of log `k`, after a thought.
fn reads(k: usize) -> Scripted {
    Scripted::Blocks {
        blocks: vec![
            json!({"type": "thinking", "thinking": format!("read log {k}")}),
            json!({"type": "tool_use", "id": format!("toolu_{k:03}"), "name": "fs_read",
                   "input": {"path": format!("logs/keeper-{k:03}.txt")}}),
        ],
        stop_reason: "tool_use".into(),
    }
}

/// The body every log holds after its first line.
fn body() -> String {
    LINE.repeat(CHARS / LINE.len() + 1)[..CHARS].to_string()
}

/// A core in a place set to `view` whose model reads `calls` logs of [`CHARS`] characters, one a loop, then
/// answers [`DONE`]; `extra` is answered in place of the read at its index. Its window is [`WINDOW`], its
/// reply's room [`OUTPUT`].
fn rig(calls: usize, extra: Option<(usize, Scripted)>, binding: Option<Binding>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("logs")).unwrap();
    let mut script: Vec<Scripted> = (0..calls).map(reads).collect();
    for k in 0..calls {
        let name = format!("logs/keeper-{k:03}.txt");
        std::fs::write(root.join(name), format!("log {k}\n{}", body())).unwrap();
    }
    if let Some((at, s)) = extra {
        script.insert(at, s);
    }
    script.push(Scripted::text(DONE));
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let root = root.canonicalize().unwrap();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.context.assembly = Some(AssemblySetting::All(Assembly::View));
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(WINDOW),
            max_output_tokens: Some(OUTPUT),
            ..Default::default()
        },
    );
    let model = Arc::new(FakeProvider {
        binding,
        ..FakeProvider::scripted(script)
    });
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    Rig {
        core,
        model,
        _dir: dir,
    }
}

async fn turn(core: &Arc<Core>) -> anyhow::Result<TurnSubmitResult> {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(ASK.into()),
            target,
            sink: EventSink::new(core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// The turn's own requests (a request with tools), in the order sent.
fn asked(r: &Rig) -> Vec<ProviderRequest> {
    (r.model.requests().into_iter())
        .filter(|q| !q.tools.is_empty())
        .collect()
}

fn texts(q: &ProviderRequest) -> String {
    serde_json::to_string(&q.messages).unwrap()
}

/// The block of the result of call `id` in `q`, as sent.
fn result_of(q: &ProviderRequest, id: &str) -> String {
    (q.messages.iter())
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == id)
        .map(Value::to_string)
        .unwrap_or_default()
}

/// Every call in a request has its result in the next message.
fn closed(q: &ProviderRequest) {
    for (i, m) in q.messages.iter().enumerate() {
        let calls = (m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == "tool_use")
            .filter_map(|b| b["id"].as_str());
        for id in calls {
            let next = &q.messages[i + 1]["content"];
            let answered = (next.as_array().into_iter().flatten())
                .any(|b| b["type"] == "tool_result" && b["tool_use_id"] == id);
            assert!(answered, "call {id} has its result after it");
        }
    }
}

/// The room the turn's views say it made.
fn rooms(core: &Core, turn: &str) -> Vec<Room> {
    (crate::view::record::of_turn(&core.store, turn)
        .unwrap()
        .into_iter())
    .filter_map(|v| v.room)
    .collect()
}

/// A turn of many large results keeps going past the budget: no fallback, no overage, every request within
/// the window, the newest result whole and the oldest its stand-in, and the answer arrives. Every room is a
/// new chain, so under a provider that refuses a changed prefix under signed thinking no request is refused
/// (none was repeated with the provider's remedy).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_of_many_large_results_makes_its_own_room_and_answers() {
    const CALLS: usize = 36;
    let r = rig(CALLS, None, Some(Binding::Refuse));
    let res = turn(&r.core).await.expect("the turn answers");
    assert_eq!(res.output, DONE, "{res:?}");
    let rows = |kind: &str| crate::test_support::rows(&r.core, kind);
    assert!(rows("context.view_fallback").is_empty());
    assert!(rows("context.overage").is_empty());
    let sent = asked(&r);
    assert_eq!(sent.len(), CALLS + 1, "one request a loop, none repeated");
    for q in &sent {
        closed(q);
        let bound = q.thinking.as_ref().and_then(|t| t.get("block_binding"));
        assert!(bound.is_none(), "no request was refused and repeated");
    }
    let last = sent.last().unwrap();
    let newest = result_of(last, &format!("toolu_{:03}", CALLS - 1));
    assert!(newest.contains(&body()), "the newest result is whole");
    assert!(
        texts(last).contains("[Result cleared to save context: fs_read logs/keeper-000.txt"),
        "the oldest result is its stand-in"
    );
    let made = rooms(&r.core, &res.turn_id);
    assert!(
        made.iter()
            .any(|m| m.made >= 1 && m.cleared >= 1 && m.keep.is_some()),
        "the view's record says what was cleared: {made:?}"
    );
    // Within the window as the compiler counts: every compilation the turn made fits its limit.
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    for c in comps.iter().filter_map(|c| c.budget.as_ref()) {
        assert!(c.overage.is_none(), "{c:?}");
        assert!(c.used_tokens <= c.limit_tokens, "{c:?}");
    }
}

/// A provider that refuses a request of the turn for its size: the retry is compiled under its stated
/// window (its maximum, read at the estimate), with room made, and it answers; the turn goes on to its end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_for_size_is_retried_under_the_providers_number_and_succeeds() {
    const CALLS: usize = 16;
    const AT: usize = 12;
    let refused = Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: "invalid_request_error: prompt is too long: 80000 tokens > 70000 maximum".into(),
    });
    let r = rig(CALLS, Some((AT, refused)), None);
    let res = turn(&r.core).await.expect("the turn answers");
    assert_eq!(res.output, DONE, "{res:?}");
    assert!(crate::test_support::rows(&r.core, "context.view_fallback").is_empty());
    assert!(crate::test_support::rows(&r.core, "context.overage").is_empty());
    let sent = asked(&r);
    assert_eq!(sent.len(), CALLS + 2, "the refused call and its one retry");
    let (refused, retry) = (&sent[AT], &sent[AT + 1]);
    assert_ne!(refused.digest(), retry.digest());
    assert!(
        retry.estimate_tokens() < refused.estimate_tokens(),
        "the retry is smaller"
    );
    assert!(texts(retry).contains("[Result cleared to save context: fs_read logs/keeper-000.txt"));
    assert!(
        result_of(retry, &format!("toolu_{:03}", AT - 1)).contains(&body()),
        "the newest result is whole"
    );
    let made = rooms(&r.core, &res.turn_id);
    assert!(
        made.iter().any(|m| m.window.is_some_and(|w| w < 70_000)),
        "the record says the provider's window: {made:?}"
    );
}
