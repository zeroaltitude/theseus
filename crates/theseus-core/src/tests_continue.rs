//! CONTINUE (M5 25b; design §3, "25b"): the compiler's candidate signals,
//! pure, one test each beside its near miss; and `continue.v1` in shadow at
//! a compile that appended and fired one, against the fake Jev. No signal,
//! no judgment; a deterministic trigger, no judgment; and the request the
//! same bytes with the judge on or off.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::{TurnSubmitResult, Usage};
use theseus_store::Record;

use crate::bus::EventSink;
use crate::catalog::Catalog;
use crate::compiler::{compile, Compilation, CompileInput, Compiled, Recompile, RequestSpec};
use crate::config::{CacheTtl, Effort, SignalsConfig, ThinkingDisplay};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin};
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::signals::SignalsAt;
use crate::store::Store;
use crate::tests_judge::{rig_with, texts, turn};
use crate::turn::TurnRequest;

// ------------------------------------------------- the signals, pure

const MINUTE: u64 = 60_000;
const T0: u64 = 1_800_000_000_000;

fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-opus-5".into(),
        max_tokens: 1000,
        system_text: "You are Theseus.".into(),
        context_text: String::new(),
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "fs_read", "description": "d", "input_schema": {"type": "object"}}),
        ],
        effort: Some(Effort::High),
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: true,
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        memberships: vec![],
        guidance: vec![],
    }
}

fn user(text: &str, at: u64) -> Node {
    let mut n = Node::user("s", None, "web", text);
    n.created_at_ms = at;
    n
}

/// A task's report or a wake, as the turn writes it.
fn relayed(author: &str, at: u64) -> Node {
    let mut n = Node::relayed("s", None, Origin::Harness, author, "it came back");
    n.created_at_ms = at;
    n
}

/// An answer of compilation `cmp`, with what the provider counted.
fn answer(cmp: &str, cache_read: u64, input: u64, at: u64) -> Node {
    let mut n = Node::assistant(
        "s",
        "t",
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": "Noted."})],
            model: "claude-opus-5".into(),
            provider: "anthropic".into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage {
                input_tokens: input,
                cache_read_input_tokens: cache_read,
                output_tokens: 3,
                ..Usage::default()
            },
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: Some(cmp.into()),
            request_digest: None,
        },
    );
    n.created_at_ms = at;
    n
}

fn config(dormancy_minutes: u64) -> SignalsConfig {
    SignalsConfig {
        dormancy_minutes,
        ..SignalsConfig::default()
    }
}

/// Compile `nodes` (positions from 1) against `current`, at `now`.
fn run(
    nodes: &[Node],
    current: Option<&Compilation>,
    cfg: SignalsConfig,
    window: Option<u64>,
    now: u64,
) -> Compiled {
    let nodes: Vec<(u64, Arc<Node>)> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u64 + 1, Arc::new(n.clone())))
        .collect();
    compile(CompileInput {
        session_id: "s",
        current,
        nodes: &nodes,
        last_position: nodes.len() as u64,
        spec: &spec(),
        catalog: &Catalog::builtin(),
        force: None,
        window_override: window,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        assembled: None,
        signals: Some(SignalsAt {
            config: cfg,
            now_ms: now,
        }),
    })
}

/// A session's first compilation, of its first message at `T0`.
fn first() -> Compilation {
    let c = run(&[user("hello", T0)], None, config(360), None, T0);
    assert!(c.new_compilation);
    c.compilation
}

fn names(c: &Compiled) -> Vec<&str> {
    c.signals.fired.iter().map(|s| s.name.as_str()).collect()
}

/// A dormancy gap: a new input more than `dormancy_minutes` after the node
/// before it fires, with its minutes; one at the limit, or under it, does
/// not.
#[test]
fn a_dormancy_gap_fires_past_its_minutes_and_not_at_them() {
    let c1 = first();
    let at = |gap_minutes: u64| {
        let nodes = [
            user("hello", T0),
            answer(&c1.id, 0, 0, T0 + 1000),
            user("back again", T0 + 1000 + gap_minutes * MINUTE),
        ];
        run(&nodes, Some(&c1), config(360), None, T0 + 2 * 86_400_000)
    };
    let past = at(361);
    assert!(!past.new_compilation, "{:?}", past.trigger);
    assert_eq!(names(&past), ["dormancy"]);
    assert_eq!(past.signals.fired[0].value, 361);
    assert!(past.signals.fired[0].detail.contains("6 hours 1 minutes"));
    for near in [360, 359, 0] {
        assert!(names(&at(near)).is_empty(), "{near} minutes fired");
    }
    // A later loop of the same turn sees its tool traffic, not the input.
    let later = [
        user("hello", T0),
        answer(&c1.id, 0, 0, T0 + 1000),
        user("back again", T0 + 1000 + 400 * MINUTE),
        answer(&c1.id, 0, 0, T0 + 1000 + 401 * MINUTE),
    ];
    assert!(names(&run(&later, Some(&c1), config(360), None, T0)).is_empty());
}

/// The tail crossing its soft band: past half the window with what was
/// written since the last answer fires, with the band it passed; a tail
/// that stays under it does not, nor one that was already past it.
#[test]
fn the_tail_crossing_its_band_fires_and_a_tail_under_it_does_not() {
    let c1 = first();
    let window = 100_000;
    // Claude reads prose at 3.3 bytes a token.
    let of = |percent: u64| "a".repeat((percent * window / 100) as usize * 33 / 10);
    let at = |percent: u64| {
        let nodes = [
            user("hello", T0),
            answer(&c1.id, 0, 0, T0 + 1000),
            user(&of(percent), T0 + 2000),
        ];
        run(&nodes, Some(&c1), config(360), Some(window), T0 + 3000)
    };
    let past = at(55);
    assert!(!past.new_compilation, "{:?}", past.trigger);
    assert_eq!(names(&past), ["tail_band"]);
    assert_eq!(past.signals.fired[0].value, 50);
    assert!(past.signals.tail_tokens >= 55_000, "{:?}", past.signals);
    assert_eq!(past.signals.window, window);
    let near = at(45);
    assert!(names(&near).is_empty(), "{:?}", near.signals);
    assert!(near.signals.tail_tokens >= 45_000 && near.signals.tail_tokens < 50_000);
    // Each further quarter fires again: from past half to past 75 %. The
    // last answer's count keeps the request under the ring's line.
    let further = |percent: u64| {
        let nodes = [
            user("hello", T0),
            answer(&c1.id, 0, 0, T0 + 1000),
            user(&of(60), T0 + 2000),
            answer(&c1.id, 0, 60_000, T0 + 3000),
            user(&of(percent), T0 + 4000),
        ];
        run(&nodes, Some(&c1), config(360), Some(window), T0 + 5000)
    };
    let quarter = further(20);
    assert!(!quarter.new_compilation, "{:?}", quarter.trigger);
    assert_eq!(names(&quarter), ["tail_band"]);
    assert_eq!(quarter.signals.fired[0].value, 75);
    let within = further(5);
    assert!(!within.new_compilation, "{:?}", within.trigger);
    assert!(
        names(&within).is_empty(),
        "already past half: {:?}",
        within.signals
    );
}

/// A task's report or a wake arriving fires, named; one the model already
/// answered does not.
#[test]
fn a_report_or_a_wake_arriving_fires_and_one_already_answered_does_not() {
    let c1 = first();
    let nodes = [
        user("hello", T0),
        answer(&c1.id, 0, 0, T0 + 1000),
        relayed("task:k3f9", T0 + 2000),
        relayed("wake:w7q2", T0 + 2001),
    ];
    let c = run(&nodes, Some(&c1), config(360), None, T0 + 3000);
    assert!(!c.new_compilation);
    assert_eq!(names(&c), ["report", "wake"]);
    assert!(c.signals.fired[0].detail.contains("k3f9"));
    assert_eq!(c.signals.fired[1].value, 1);
    let answered = [
        user("hello", T0),
        relayed("task:k3f9", T0 + 500),
        answer(&c1.id, 0, 0, T0 + 1000),
        user("and then?", T0 + 2000),
    ];
    let c = run(&answered, Some(&c1), config(360), None, T0 + 3000);
    assert!(names(&c).is_empty(), "{:?}", c.signals);
    // An operator's message named like one is not a report.
    let mut forged = user("task:k3f9", T0 + 2000);
    forged.author = Some("task:k3f9".into());
    let nodes = [user("hello", T0), answer(&c1.id, 0, 0, T0 + 1000), forged];
    assert!(names(&run(&nodes, Some(&c1), config(360), None, T0)).is_empty());
}

/// A provider cache miss: the last call read nothing where the one before
/// it, with the same prefix, read some. A cache that was never warm, or a
/// read of another compilation's, does not fire.
#[test]
fn a_cache_miss_under_the_same_prefix_fires_and_a_cold_cache_does_not() {
    let c1 = first();
    let at = |reads: [u64; 2], cmp: &str| {
        let nodes = [
            user("hello", T0),
            answer(cmp, reads[0], 0, T0 + 1000),
            user("more", T0 + 2000),
            answer(&c1.id, reads[1], 0, T0 + 3000),
            user("and more", T0 + 4000),
        ];
        run(&nodes, Some(&c1), config(360), None, T0 + 5000)
    };
    let missed = at([4000, 0], &c1.id);
    assert!(!missed.new_compilation);
    assert_eq!(names(&missed), ["cache_miss"]);
    assert_eq!(missed.signals.fired[0].value, 4000);
    assert_eq!(missed.signals.last_cache_read, 0);
    assert!(names(&at([0, 0], &c1.id)).is_empty(), "never warm");
    assert!(
        names(&at([4000, 0], "cmp_other")).is_empty(),
        "another prefix"
    );
    let warm = at([4000, 3500], &c1.id);
    assert!(names(&warm).is_empty());
    assert_eq!(warm.signals.last_cache_read, 3500);
}

/// The signals decide nothing: a compile's request and decision are the
/// same with them read or not, and a compilation's age is by the clock
/// passed in.
#[test]
fn the_signals_change_no_request_and_read_the_clock_they_are_given() {
    let c1 = first();
    let nodes: Vec<(u64, Arc<Node>)> = [
        user("hello", T0),
        answer(&c1.id, 0, 0, T0 + 1000),
        relayed("wake:w7q2", T0 + 900 * MINUTE),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, n)| (i as u64 + 1, Arc::new(n)))
    .collect();
    let (sp, catalog) = (spec(), Catalog::builtin());
    let sources = crate::recall::render::Sources::default();
    let input = |signals| CompileInput {
        session_id: "s",
        current: Some(&c1),
        nodes: &nodes,
        last_position: 3,
        spec: &sp,
        catalog: &catalog,
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &sources,
        assembled: None,
        signals,
    };
    let none = compile(input(None));
    let now = c1.created_at_ms + 90 * MINUTE;
    let read = compile(input(Some(SignalsAt {
        config: config(360),
        now_ms: now,
    })));
    assert!(none.signals.fired.is_empty());
    assert_eq!(names(&read), ["dormancy", "wake"]);
    assert_eq!(read.digest, none.digest);
    assert_eq!(read.decision(), none.decision());
    assert_eq!(read.signals.compilation_age_minutes, 90);
}

// ---------------------------------------- continue.v1, through the core

/// `scope`'s judgments, decoded.
fn judged(store: &Store, scope: &str) -> Vec<(Record, LedgerRow)> {
    store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            (r, row)
        })
        .collect()
}

async fn until(store: &Store, scope: &str, n: usize) -> Vec<(Record, LedgerRow)> {
    let t0 = Instant::now();
    loop {
        let rows = judged(store, scope);
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} {scope} judgments recorded",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The `context.compiled` rows of `turn`.
fn compiled_rows(core: &Arc<Core>, turn: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == "context.compiled" && r.turn_id.as_deref() == Some(turn))
        .map(|r| r.data)
        .collect()
}

/// The turn's trace's `judge` marks at `compile` (23b's loop.v1 mark at
/// the turn's end, and the other points', are not this point's).
fn marks(core: &Arc<Core>, turn: &str) -> Vec<Value> {
    fn walk(span: &Value, out: &mut Vec<Value>) {
        if span["name"] == "judge" && span["attrs"]["point"] == "compile" {
            out.push(span.clone());
        }
        for c in span["children"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    for (_, r) in core.store.ledger_tail::<LedgerRow>(10_000).unwrap() {
        if r.kind == "turn.trace" && r.turn_id.as_deref() == Some(turn) {
            walk(&r.data, &mut out);
        }
    }
    out
}

/// A second turn in a session after a gap: a short pause past a
/// `dormancy_minutes` of 0.
async fn after_a_gap(core: &Arc<Core>, session: &str, input: &str) -> TurnSubmitResult {
    tokio::time::sleep(Duration::from_millis(30)).await;
    turn(core, Some(session), input).await
}

/// A compile that appended and fired a signal is judged once by
/// `continue.v1` in shadow: its row keyed by the id the turn's trace marks,
/// scoped `judge:continue`, its state (the signals, the sizes, the
/// compilation, the last human message) in a blob; and `context.compiled`
/// carries the signal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signal_and_no_trigger_asks_continue_in_shadow_once() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(2), Some(&jev), |c| c.judge.signals = config(0));
    let first = turn(&r.core, None, "Say done.").await;
    assert!(judged(&r.core.store, "judge:continue").is_empty());
    assert!(marks(&r.core, &first.turn_id).is_empty());
    let res = after_a_gap(&r.core, &first.session_id, "And again.").await;
    let rows = compiled_rows(&r.core, &res.turn_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["decision"], "append");
    assert_eq!(rows[0]["signals"][0]["name"], "dormancy", "{}", rows[0]);
    let rows = until(&r.core.store, "judge:continue", 1).await;
    assert_eq!(rows.len(), 1);
    let (rec, row) = &rows[0];
    let d = &row.data;
    assert_eq!(
        (d["pack"].as_str(), d["mode"].as_str()),
        (Some("continue.v1"), Some("shadow"))
    );
    assert_eq!(d["point"], "compile");
    assert_eq!(row.turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert_eq!(d["context"]["decision"], "append");
    assert_eq!(d["outcome"]["outcome"], "answered", "{d}");
    let m = marks(&r.core, &res.turn_id);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0]["kind"], "mark");
    assert_eq!(m[0]["start_us"], m[0]["end_us"], "zero-length");
    let a = &m[0]["attrs"];
    assert_eq!(
        (a["pack"].as_str(), a["point"].as_str(), a["mode"].as_str()),
        (Some("continue.v1"), Some("compile"), Some("shadow"))
    );
    assert_eq!(
        a["judgment"].as_str(),
        rec.key.as_deref(),
        "the mark names the row"
    );
    assert_eq!(rec.key.as_deref(), d["id"].as_str());
    let digest = d["state"]["sha256"].as_str().unwrap();
    let state = std::fs::read_to_string(r.core.store.blobs().path(digest)).unwrap();
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["signals"][0]["name"], "dormancy", "{state}");
    assert_eq!(state["strategy"], "transcript");
    assert_eq!(state["trigger"], "new_session");
    assert_eq!(state["last_human_message"], "And again.");
    assert!(state["window_tokens"].as_u64().unwrap() > 0);
    // The judge's health names every wired pack (`rig_with` turns the
    // inbound point's off).
    let h = r.core.health().judge.unwrap();
    assert_eq!(
        h.packs,
        [
            "loop.v1: shadow",
            "security.v1: shadow",
            "security.v3: live (owner: decision of 2026-10-04)",
            "classify.v1: off",
            "role.v1: off",
            "route.v1: off (the config's ceiling; on the ladder: live (owner: decision of 2026-10-04))",
            "continue.v1: shadow",
            "categorize.v1: shadow",
            "rerank.v1: live (owner: decision of 2026-10-04)",
            "memory.v1: shadow",
            "attribution.v1: shadow",
            "citation.v1: shadow"
        ]
    );
}

/// No signal, no judgment: a turn soon after the last, under the default
/// thresholds, asks nothing at its compile (`loop.v1` still judges its end).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_signal_no_judgment() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(2), Some(&jev), |_| {});
    let first = turn(&r.core, None, "Say done.").await;
    let res = after_a_gap(&r.core, &first.session_id, "And again.").await;
    until(&r.core.store, "judge:loop", 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(compiled_rows(&r.core, &res.turn_id)[0]
        .get("signals")
        .is_none());
    assert!(judged(&r.core.store, "judge:continue").is_empty());
    assert!(marks(&r.core, &res.turn_id).is_empty());
    assert_eq!(jev.connections(), 2, "the two turns' loop.v1 alone");
}

/// A deterministic trigger, no judgment: an operator's recompile fires its
/// trigger, so the signal that fired beside it is carried and not judged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deterministic_trigger_asks_no_judgment() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(2), Some(&jev), |c| c.judge.signals = config(0));
    let first = turn(&r.core, None, "Say done.").await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&first.session_id)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &rec.session_id, None);
    let res = r
        .core
        .runner
        .run(TurnRequest {
            session: rec,
            input: Some("Start over from the transcript.".into()),
            target,
            sink,
            author: "test".into(),
            recompile: Some(Recompile::Transcript),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            prompt: None,
        })
        .await
        .unwrap();
    let rows = compiled_rows(&r.core, &res.turn_id);
    assert_eq!(rows[0]["decision"], "recompile");
    assert_eq!(rows[0]["trigger"], "manual_transcript");
    assert_eq!(
        rows[0]["signals"][0]["name"], "dormancy",
        "carried: {}",
        rows[0]
    );
    until(&r.core.store, "judge:loop", 2).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(judged(&r.core.store, "judge:continue").is_empty());
    assert!(marks(&r.core, &res.turn_id).is_empty());
}

/// The compiler decides nothing new: with the judge on and judging, every
/// request a session sends is the bytes it sends with the judge off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_requests_are_the_same_with_the_judge_on_or_off() {
    let jev = FakeJev::start().unwrap();
    let on = rig_with(texts(3), Some(&jev), |c| c.judge.signals = config(0));
    let off = rig_with(texts(3), None, |c| c.judge.signals = config(0));
    for r in [&on, &off] {
        let sid = turn(&r.core, None, "one").await.session_id;
        after_a_gap(&r.core, &sid, "two").await;
        after_a_gap(&r.core, &sid, "three").await;
    }
    until(&on.core.store, "judge:continue", 2).await;
    assert!(judged(&off.core.store, "judge:continue").is_empty());
    let requests = |r: &crate::tests_judge::Rig| -> Vec<Value> {
        r.fake
            .requests()
            .iter()
            .map(|q| serde_json::to_value(q).unwrap())
            .collect()
    };
    assert_eq!(requests(&on).len(), 3);
    assert_eq!(requests(&on), requests(&off));
}

/// A judged turn keeps its frame budget: `continue.v1`'s dispatch at its
/// compile writes no frame of the turn's (its row rides in the sink's own
/// frame, after the turn), so a plain turn is still 5.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_judged_at_its_compile_keeps_its_frame_budget() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(3), Some(&jev), |c| c.judge.signals = config(0));
    let sid = turn(&r.core, None, "warm up").await.session_id;
    // The day's first judgments reserve the budget's block: warm it.
    after_a_gap(&r.core, &sid, "and warm").await;
    until(&r.core.store, "judge:continue", 1).await;
    until(&r.core.store, "judge:loop", 2).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let before = r.core.store.stats().unwrap().frames_appended;
    let res = turn(&r.core, Some(&sid), "hi").await;
    let frames = r.core.store.stats().unwrap().frames_appended - before;
    assert_eq!(res.loops, 1);
    assert_eq!(
        marks(&r.core, &res.turn_id).len(),
        1,
        "judged at its compile"
    );
    assert!(
        frames <= 5,
        "a plain turn judged at its compile wrote {frames} frames"
    );
    until(&r.core.store, "judge:continue", 2).await;
}
