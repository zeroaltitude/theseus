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
use crate::provider::binding::Binding;
use crate::provider::{FakeProvider, ProviderError, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::view::record::Room;
use crate::{Config, Core};

const OUTPUT: u32 = 2_000;
const WINDOW: u64 = 80_000;
/// The lines of each log, after its first: about 10,000 characters, a result well over what clearing leaves
/// whole.
const LINES: usize = 150;
const ASK: &str = "Read every log under logs/ and tell me whether the keepers agree.";
const DONE: &str = "All the logs agree with the tide table.";
const LINE: &str = "the keeper logged the tide at the north light and the table agreed.\n";

#[test]
fn the_steps_clear_first_then_lower_slot_g_to_none() {
    let at = |keep: u64, here: Option<u64>| Room {
        made: 1,
        keep: Some(keep),
        here,
        held: 8_000,
        reserve: here.is_some(),
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

/// Log `k`: its first line, [`LINES`] lines of the keeper's words, and its last line, which only a result
/// sent whole holds.
fn log(k: usize) -> String {
    format!("log {k}\n{}{}", LINE.repeat(LINES), end(k))
}

fn end(k: usize) -> String {
    format!("end of log {k}\n")
}

/// A core in a place set to `view` whose model reads `calls` logs of [`LINES`] lines, one a loop, then
/// answers [`DONE`]; `extra` is answered in place of the read at its index. Its window is `window`, its
/// reply's room [`OUTPUT`].
fn rig(
    calls: usize,
    extra: Option<(usize, Scripted)>,
    binding: Option<Binding>,
    window: u64,
) -> Rig {
    let mut script: Vec<Scripted> = (0..calls).map(reads).collect();
    if let Some((at, s)) = extra {
        script.insert(at, s);
    }
    script.push(Scripted::text(DONE));
    rig_answering(calls, script, binding, window)
}

/// [`rig`]'s core with `calls` logs to read, its model answering `script` in order, and an ordinary tail
/// on every request (the date and the git line).
fn rig_answering(
    calls: usize,
    script: Vec<Scripted>,
    binding: Option<Binding>,
    window: u64,
) -> Rig {
    rig_tweaked(calls, script, (binding, window), |_| {})
}

/// [`rig_answering`], with `tweak` applied to its settings last.
fn rig_tweaked(
    calls: usize,
    script: Vec<Scripted>,
    (binding, window): (Option<Binding>, u64),
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("logs")).unwrap();
    for k in 0..calls {
        let name = format!("logs/keeper-{k:03}.txt");
        std::fs::write(root.join(name), log(k)).unwrap();
    }
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let root = root.canonicalize().unwrap();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.project.tail_facts = Some(true);
    cfg.catalog.insert(
        "claude-sonnet-5-5".into(),
        crate::catalog::CatalogRow {
            context_window: Some(window),
            max_output_tokens: Some(OUTPUT),
            ..Default::default()
        },
    );
    tweak(&mut cfg);
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
    turn_in(core, &rec.session_id, ASK).await
}

/// A turn of `sid` asking `input`.
async fn turn_in(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// Every request the provider was sent is within the limit the turn compiles against, as sent (its tails
/// included): the window less the reply's room and the compiler's headroom.
fn every_send_within(r: &Rig, window: u64) {
    let limit = window - u64::from(OUTPUT) - 4_096;
    for (i, q) in asked(r).iter().enumerate() {
        let tokens = q.estimate_tokens();
        assert!(
            tokens <= limit,
            "request {i} was sent at {tokens} tokens over {limit}"
        );
    }
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
    let r = rig(CALLS, None, Some(Binding::Refuse), WINDOW);
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
    assert!(
        newest.contains(end(CALLS - 1).trim()),
        "the newest result is whole"
    );
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
    every_send_within(&r, WINDOW);
    // Within the window as the compiler counts: every compilation the turn made fits its limit.
    let comps = r.core.store.session_compilations(&res.session_id).unwrap();
    for c in comps.iter().filter_map(|c| c.budget.as_ref()) {
        assert!(c.overage.is_none(), "{c:?}");
        assert!(c.used_tokens <= c.limit_tokens, "{c:?}");
    }
}

/// A provider that refuses a request of the turn for its size: the retry is compiled under its stated
/// window (its maximum, read at the estimate), with room made, and it answers; the turn goes on to its end.
/// On a window the turn never had to make room in before the refusal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_for_size_is_retried_under_the_providers_number_and_succeeds() {
    const CALLS: usize = 28;
    const AT: usize = 24;
    let refused = Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: "invalid_request_error: prompt is too long: 80000 tokens > 70000 maximum".into(),
    });
    let r = rig(CALLS, Some((AT, refused)), None, 200_000);
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
        result_of(retry, &format!("toolu_{:03}", AT - 1)).contains(end(AT - 1).trim()),
        "the newest result is whole"
    );
    let made = rooms(&r.core, &res.turn_id);
    assert!(
        made.iter().any(|m| m.window.is_some_and(|w| w <= 70_000)),
        "the record says the provider's window: {made:?}"
    );
    every_send_within(&r, 200_000);
}

/// A conversation of four earlier exchanges, an ordinary tail on every request, and a first bare size
/// refusal (no numbers) of the fifth turn: the retry is measured as it is sent, it is strictly smaller than
/// the refused request (the window's oldest exchange, past the floor, goes), and it answers on one retry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bare_refusal_after_four_exchanges_shrinks_and_answers_on_one_retry() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let asked_now = AtomicU32::new(0);
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.project.tail_facts = Some(true);
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                crate::catalog::CatalogRow {
                    // Room for four large exchanges: the refusal, not the window, is what this case is about.
                    context_window: Some(200_000),
                    max_output_tokens: Some(OUTPUT),
                    ..Default::default()
                },
            );
        },
        |_| {},
        move |q| {
            if q.tools.is_empty() || !crate::tests_places::last_user(q).contains(ASK) {
                return Scripted::text("Noted.");
            }
            match asked_now.fetch_add(1, Ordering::SeqCst) {
                0 => Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: "invalid_request_error: prompt is too long".into(),
                }),
                _ => Scripted::text(DONE),
            }
        },
    );
    let sid = crate::tests_places::session(&r.core, None);
    for k in 0..4 {
        // About 7,000 tokens each: the oldest, past the window's floor of three, is what the retry cuts.
        let said = format!("earlier ({k}): {}", LINE.repeat(400));
        // Marked as a person's message, as a turn's is, so each exchange is in the place's window.
        crate::tests_places::turn(&r.core, &sid, &said).await;
    }
    let res = crate::tests_places::turn(&r.core, &sid, ASK).await;
    assert_eq!(res.output, DONE, "{res:?}");
    let sent: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && crate::tests_places::last_user(q).contains(ASK))
        .collect();
    assert_eq!(sent.len(), 2, "the refused call and its one retry");
    let (refused, retry) = (&sent[0], &sent[1]);
    assert!(
        retry.estimate_tokens() < refused.estimate_tokens(),
        "the retry as sent is smaller"
    );
    assert!(
        texts(refused).contains("earlier (0)"),
        "the oldest exchange was laid"
    );
    assert!(!texts(retry).contains("earlier (0)"), "and made room for");
    assert!(crate::test_support::rows(&r.core, "context.overage").is_empty());
}

/// The turn (`turn k:`) a request is of, and how many tool results it already holds after that message.
fn turn_and_results(q: &ProviderRequest) -> Option<(usize, usize)> {
    let at = (q.messages.iter()).rposition(|m| {
        m["role"] == "user"
            && (m["content"].as_array().into_iter().flatten()).any(|b| {
                b["type"] == "text" && b["text"].as_str().is_some_and(|t| t.contains("turn "))
            })
    })?;
    let text = crate::tests_places::last_user(&ProviderRequest {
        messages: vec![q.messages[at].clone()],
        ..q.clone()
    });
    let k = text
        .split("turn ")
        .last()?
        .split(':')
        .next()?
        .trim()
        .parse()
        .ok()?;
    let results = (q.messages[at + 1..].iter())
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .count();
    Some((k, results))
}

/// A conversation of tool-heavy turns (three results of about 8,000 characters each a turn): between two
/// clearings, each turn's first request begins with the previous turn's front and window, byte for byte (the
/// cache's prefix holds), and a clearing pass runs at most once per 40,000 tokens of new results.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clearing_is_sticky_and_the_prefix_holds_between_clearings() {
    const TURNS: usize = 24;
    const READS: usize = 3;
    let r = crate::tests_places::rig_setup_scripted(
        |_| {},
        |_| {},
        |q| {
            let Some((k, done)) = (!q.tools.is_empty()).then(|| turn_and_results(q)).flatten()
            else {
                return Scripted::text("Noted.");
            };
            match done < READS {
                true => Scripted::tools(
                    &format!("Reading part {done} of turn {k}."),
                    &[(
                        &format!("toolu_{k:02}_{done}"),
                        "fs_read",
                        json!({"path": format!("logs/t{k:02}-{done}.txt")}),
                    )],
                ),
                false => Scripted::text(&format!("Turn {k} read.")),
            }
        },
    );
    std::fs::create_dir_all(r.root.join("logs")).unwrap();
    for k in 0..TURNS {
        for n in 0..READS {
            let name = r.root.join(format!("logs/t{k:02}-{n}.txt"));
            std::fs::write(name, format!("part {n} of {k}\n{}", LINE.repeat(110))).unwrap();
        }
    }
    let sid = crate::tests_private_view::session_in(&r.core, None, &r.root);
    let mut firsts: Vec<ProviderRequest> = Vec::new();
    let mut cleared_at: Vec<usize> = Vec::new();
    for k in 0..TURNS {
        let before = crate::test_support::rows(&r.core, "context.cleared").len();
        crate::tests_places::turn(&r.core, &sid, &format!("turn {k}: read the three parts")).await;
        if crate::test_support::rows(&r.core, "context.cleared").len() > before {
            cleared_at.push(k);
        }
        let first = (r.requests().into_iter())
            .find(|q| !q.tools.is_empty() && turn_and_results(q) == Some((k, 0)))
            .expect("the turn's first request");
        firsts.push(first);
    }
    assert!(!cleared_at.is_empty(), "the conversation cleared");
    // About 2,000 tokens a result, three a turn: at most one pass per 40,000 tokens of new results.
    let new_tokens = (TURNS * READS * 2_000) as u64;
    assert!(
        cleared_at.len() as u64 <= new_tokens / 40_000 + 1,
        "clearing ran at turns {cleared_at:?}"
    );
    let plain = |q: &ProviderRequest| -> Vec<Value> {
        q.messages
            .iter()
            .map(crate::provider::binding::unmarked)
            .collect()
    };
    for k in 1..TURNS {
        if cleared_at.contains(&k) {
            continue;
        }
        let (was, now) = (plain(&firsts[k - 1]), plain(&firsts[k]));
        // The previous turn's front and window: every message but its last (the beside block and the ask).
        let held = was.len() - 1;
        let same = (0..held)
            .take_while(|i| now.get(*i) == Some(&was[*i]))
            .count();
        assert_eq!(
            same, held,
            "turn {k} (cleared at {cleared_at:?}): message {same} of {held} changed from the turn before:\nwas {}\nnow {}",
            short_json(&was[same]),
            now.get(same).map(short_json).unwrap_or_default()
        );
    }
}

/// A message, its first 600 characters.
fn short_json(v: &Value) -> String {
    let s = v.to_string();
    s.chars().take(600).collect()
}

/// One result alone larger than the window: before the overage, it is laid as its head with one line
/// saying how much is shown and how to read the rest; the turn answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_result_larger_than_the_window_is_laid_as_its_head_and_the_turn_answers() {
    let script = vec![reads(0), Scripted::text(DONE)];
    let r = rig_tweaked(1, script, (None, 200_000), |c| {
        c.context.working_window_tokens = Some(40_000);
    });
    let root = r.core.runner.cfg.tools.projects_dir.clone().unwrap();
    let path = std::path::Path::new(&root).join("logs/keeper-000.txt");
    std::fs::write(path, format!("log 0\n{}{}", LINE.repeat(1_300), end(0))).unwrap();
    let res = turn(&r.core).await.expect("the turn answers");
    assert_eq!(res.output, DONE, "{res:?}");
    let last = asked(&r).pop().unwrap();
    let result = result_of(&last, "toolu_000");
    assert!(result.contains("log 0"), "the head is laid: {result}");
    assert!(
        result.contains("[Only the first"),
        "with its line: {result}"
    );
    assert!(!result.contains(end(0).trim()), "not the whole");
    every_send_within(&r, 40_000);
}

/// Two room episodes in one real turn: the window's large earlier exchanges make the first call over, and
/// slot G is lowered; then the turn's own large results pass the limit again, and clearing makes the room. The
/// second pass's clearing is not charged the first pass's reservation: the turn answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_that_lowered_slot_g_then_needs_clearing_still_answers() {
    const CALLS: usize = 24;
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                crate::catalog::CatalogRow {
                    context_window: Some(WINDOW),
                    max_output_tokens: Some(OUTPUT),
                    ..Default::default()
                },
            );
        },
        |_| {},
        |q| match (!q.tools.is_empty()).then(|| turn_and_results(q)).flatten() {
            Some((99, done)) if done < CALLS => Scripted::tools(
                &format!("Reading log {done}."),
                &[(
                    &format!("toolu_{done:03}"),
                    "fs_read",
                    json!({"path": format!("logs/keeper-{done:03}.txt")}),
                )],
            ),
            Some((99, _)) => Scripted::text(DONE),
            _ => Scripted::text("Noted."),
        },
    );
    std::fs::create_dir_all(r.root.join("logs")).unwrap();
    for k in 0..CALLS {
        std::fs::write(r.root.join(format!("logs/keeper-{k:03}.txt")), log(k)).unwrap();
    }
    let sid = crate::tests_private_view::session_in(&r.core, None, &r.root);
    // Eight earlier exchanges of about 5,500 tokens each: more than the view's whole, so the window itself
    // makes the first call over, and slot G is what gives; the floor (the newest three) still fits.
    for k in 0..8 {
        let said = format!("turn {k}: earlier, {}", LINE.repeat(300));
        crate::tests_places::turn(&r.core, &sid, &said).await;
    }
    let res = crate::tests_places::turn(&r.core, &sid, "turn 99: read every log").await;
    assert!(res.output.ends_with(DONE), "{res:?}");
    assert!(crate::test_support::rows(&r.core, "context.overage").is_empty());
    let made = rooms(&r.core, &res.turn_id);
    assert!(
        made.iter().any(|m| m.here.is_some()),
        "slot G was lowered: {made:?}"
    );
    assert!(
        made.iter().any(|m| m.cleared > 0),
        "and results were cleared: {made:?}"
    );
}

/// A turn that answered only by a head cut does not strand the next one: the person's short follow-up in
/// the same session answers too, and the old result is in its request as its receipt, not whole (the newest
/// previous exchange's words are the floor, its tool results are not).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_a_head_cut_the_next_short_message_answers_with_the_old_result_as_its_receipt() {
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.context.working_window_tokens = Some(40_000);
            c.catalog.insert(
                "claude-sonnet-5-5".into(),
                crate::catalog::CatalogRow {
                    context_window: Some(200_000),
                    max_output_tokens: Some(OUTPUT),
                    ..Default::default()
                },
            );
        },
        |_| {},
        |q| match (!q.tools.is_empty()).then(|| turn_and_results(q)).flatten() {
            Some((1, 0)) => Scripted::tools(
                "Reading the log.",
                &[(
                    "toolu_000",
                    "fs_read",
                    json!({"path": "logs/keeper-000.txt"}),
                )],
            ),
            Some((1, _)) => Scripted::text(DONE),
            _ => Scripted::text("Noted."),
        },
    );
    std::fs::create_dir_all(r.root.join("logs")).unwrap();
    std::fs::write(
        r.root.join("logs/keeper-000.txt"),
        format!("log 0\n{}{}", LINE.repeat(1_300), end(0)),
    )
    .unwrap();
    let sid = crate::tests_private_view::session_in(&r.core, None, &r.root);
    let first = crate::tests_places::turn(&r.core, &sid, "turn 1: read the keeper's log").await;
    assert!(first.output.ends_with(DONE), "{first:?}");
    let second = crate::tests_places::turn(&r.core, &sid, "turn 2: thank you").await;
    assert_eq!(second.output, "Noted.", "{second:?}");
    assert!(crate::test_support::rows(&r.core, "context.overage").is_empty());
    let last = (r.requests().into_iter())
        .rfind(|q| !q.tools.is_empty() && texts(q).contains("turn 2: thank you"))
        .expect("the follow-up was sent");
    let sent = texts(&last);
    assert!(
        sent.contains("turn 1: read the keeper"),
        "the previous words stay"
    );
    assert!(
        sent.contains("[Result cleared to save context:"),
        "the old result as its receipt: {}",
        &sent[..sent.len().min(2_000)]
    );
    assert!(!sent.contains(end(0).trim()), "not whole");
}
