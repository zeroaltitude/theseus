//! A place's home split into threads, and what a split takes out of its source's compile
//! (theseus-q9b85): the moved range leaves the source's compile for one line, the nodes stay in the
//! store, `thread.unsplit` puts both compiles back byte for byte, a split into an existing thread
//! appends at its tail, and a dry run writes nothing and counts what the census would.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::threads::{
    ThreadNewParams, ThreadSplitParams, ThreadSplitResult, ThreadUnsplitParams,
};
use theseus_protocol::SessionOpenParams;

use super::Core;
use crate::approval::{Answerer, Surface};
use crate::config::MemoryMode;
use crate::node::{Body, Node, ResultStatus};
use crate::provider::Scripted;
use crate::recall::{Ask, AskFuture};
use crate::tests_recall::{rig_with, turn, Rig};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

const PLACE: &str = "dm:owner-tide";

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn rig(script: Vec<Scripted>) -> Rig {
    let r = rig_with(MemoryMode::Off, |_| {});
    r.model.script.lock().unwrap().extend(script);
    r
}

/// An open session bound to a place: a home.
fn home(c: &Core) -> String {
    let sid = c
        .open_session(SessionOpenParams::default())
        .unwrap()
        .session_id;
    c.outbox.bind_place(PLACE, &sid).unwrap();
    sid
}

/// A turn as the loop writes it: the message, a reply that calls a tool (every third turn), the
/// call, its result, and the closing reply. Every text carries `marker-<n>`.
fn add_turn(c: &Core, sid: &str, n: usize) {
    let turn_id = format!("turn_{n:04}");
    let mut recs = vec![Node::user(
        sid,
        Some(&turn_id),
        "owner",
        &format!("Question {n} about marker-{n}"),
    )
    .record()
    .unwrap()];
    let reply = |blocks: Vec<Value>, loop_index: u32| {
        Node::assistant(
            sid,
            &turn_id,
            loop_index,
            Body::AssistantMessage {
                blocks,
                model: "fake".into(),
                provider: "fake".into(),
                stop_reason: Some("end_turn".into()),
                usage: Default::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
                effort: None,
            },
        )
    };
    if n.is_multiple_of(3) {
        let id = format!("tu{n:04}");
        let a = reply(
            vec![
                json!({"type": "text", "text": format!("Looking at marker-{n}")}),
                json!({"type": "tool_use", "id": id, "name": "fs_list", "input": {"path": "/tides"}}),
            ],
            0,
        );
        let call = Node::tool_call(
            sid,
            Some(&turn_id),
            Some(0),
            Body::ToolCall {
                tool_use_id: id.clone(),
                tool: "fs.list".into(),
                wire_name: "fs_list".into(),
                input: json!({"path": "/tides"}),
                assistant_node: a.id.clone(),
                correlation_id: None,
                gate: None,
            },
        );
        let res = Node::tool_result(
            sid,
            Some(&turn_id),
            Some(0),
            Body::ToolResult {
                tool_use_id: id,
                tool: "fs.list".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: format!("gauge.log marker-{n}"),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
                image: None,
                external: None,
            },
        );
        recs.extend([
            a.record().unwrap(),
            call.record().unwrap(),
            res.record().unwrap(),
        ]);
    }
    recs.push(
        reply(
            vec![json!({"type": "text", "text": format!("Answer {n} marker-{n}")})],
            1,
        )
        .record()
        .unwrap(),
    );
    c.store.append(&recs).unwrap();
}

/// The home with `n` turns, then one real turn (scripted), which compiles and freezes them.
async fn compiled_home(r: &Rig, n: usize) -> String {
    let c = &r.core;
    let sid = home(c);
    for i in 0..n {
        add_turn(c, &sid, i);
    }
    turn(c, &sid, "And what is the plan for the neap week?").await;
    sid
}

fn pos(c: &Core, sid: &str, needle: &str) -> u64 {
    c.store
        .transcript(sid)
        .unwrap()
        .iter()
        .find(|(_, n)| crate::arrangement::text_of(n).is_some_and(|t| t.contains(needle)))
        .unwrap_or_else(|| panic!("no node with {needle:?}"))
        .0
}

/// The positions of turns `a` to `b`, whole: the first message through the last reply.
fn span(c: &Core, sid: &str, a: usize, b: usize) -> [u64; 2] {
    [
        pos(c, sid, &format!("Question {a} about")),
        pos(c, sid, &format!("Answer {b} marker")),
    ]
}

/// The next request of a session, as a census compiles it: its messages as text, and how.
fn request(c: &Core, sid: &str) -> (String, &'static str, crate::census::NextRequest) {
    let rec = c.store.get_session(sid).unwrap().unwrap();
    let (live, _) = c.live_profile();
    let next = c.runner.census_request(Some(&rec), "dm", &live).unwrap();
    let text = serde_json::to_string(&next.compiled.request.messages).unwrap();
    (text, next.how, next)
}

fn split(c: &Core, p: ThreadSplitParams) -> Result<ThreadSplitResult, String> {
    c.thread_split(&p, cli()).map_err(|e| e.message)
}

fn params(from: &str, slug: &str, range: [u64; 2]) -> ThreadSplitParams {
    ThreadSplitParams {
        from: from.into(),
        slug: slug.into(),
        range: Some(range),
        ..Default::default()
    }
}

/// Every tool use in the messages has its result, and every result its use.
fn paired(messages: &[Value]) -> bool {
    let mut uses = Vec::new();
    let mut results = Vec::new();
    for m in messages {
        for b in m["content"].as_array().into_iter().flatten() {
            match b["type"].as_str() {
                Some("tool_use") => uses.push(b["id"].as_str().unwrap().to_string()),
                Some("tool_result") => results.push(b["tool_use_id"].as_str().unwrap().to_string()),
                _ => {}
            }
        }
    }
    uses.sort();
    results.sort();
    uses == results
}

fn thread(c: &Core, slug: &str) -> String {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_session(slug, cli()).unwrap().session_id
}

/// theseus-7j00b: a place is typed as `theseus places` prints it (`discord:dm:ID`) as well as
/// as it is stored (`dm:ID`); a place neither names says both forms.
#[tokio::test]
async fn a_place_is_named_as_places_prints_it_or_as_stored() {
    let r = rig(vec![Scripted::text("ok"), Scripted::text("later")]);
    let c = &r.core;
    let sid = compiled_home(&r, 6).await;
    let range = span(c, &sid, 1, 3);
    let res = split(c, params(&format!("home@discord:{PLACE}"), "gauges", range)).unwrap();
    assert!(res.split.starts_with("spl_") && res.moved > 0, "{res:?}");
    let e = split(c, params("home@discord:dm:nobody", "x", range)).unwrap_err();
    assert!(e.contains("discord:dm:ID") && e.contains("dm:ID"), "{e}");
}

/// KEY: a home's range splits into a new thread. The home's next compile lacks every moved node's
/// bytes and holds the moved line in their place; the thread's compile holds the range; tool calls
/// and results stay paired in both.
#[tokio::test]
async fn a_homes_range_leaves_its_compile_for_one_line_and_the_thread_holds_it() {
    let r = rig(vec![Scripted::text("ok"), Scripted::text("later")]);
    let c = &r.core;
    let sid = compiled_home(&r, 12).await;
    let (before, _, _) = request(c, &sid);
    for n in 0..12 {
        assert!(
            before.contains(&format!("marker-{n}")),
            "marker-{n} is in the compile"
        );
    }
    // Turns 3 to 8 move: three of them hold a tool call.
    let range = span(c, &sid, 3, 8);
    let res = split(
        c,
        ThreadSplitParams {
            gist: Some("the neap week's gauge readings".into()),
            ..params(&format!("home@{PLACE}"), "gauges", range)
        },
    )
    .unwrap();
    assert!(
        res.noted && !res.into && res.moved >= 6 * 2 + 2 * 3,
        "{res:?}"
    );
    assert!(res.split.starts_with("spl_"));

    let (after, how, next) = request(c, &sid);
    assert_eq!(
        how, "recompile",
        "the source's prefix changes once, at the split"
    );
    for n in 3..=8 {
        assert!(
            !after.contains(&format!("marker-{n}")),
            "marker-{n} left the home's compile"
        );
    }
    for n in (0..3).chain(9..12) {
        assert!(after.contains(&format!("marker-{n}")), "marker-{n} stays");
    }
    assert_eq!(
        after
            .matches("moved to thread «gauges»: the neap week's gauge readings")
            .count(),
        1,
        "one line in the range's place"
    );
    // The line sits where the range began: after turn 2's reply, before turn 9's message.
    let line = after.find("moved to thread «gauges»").unwrap();
    assert!(after.find("Answer 2 marker-2").unwrap() < line);
    assert!(line < after.find("Question 9 about").unwrap());
    assert!(
        paired(&next.compiled.request.messages),
        "no call without its result in the home"
    );

    // The thread: the range, quoted, in order; calls and results of a quote are text.
    let tsid = c
        .threads()
        .get("gauges")
        .unwrap()
        .session_id
        .clone()
        .unwrap();
    let (theirs, _, tnext) = request(c, &tsid);
    for n in 3..=8 {
        assert!(
            theirs.contains(&format!("marker-{n}")),
            "marker-{n} is in the thread"
        );
    }
    assert!(!theirs.contains("marker-2\"") && !theirs.contains("marker-9\""));
    assert!(paired(&tnext.compiled.request.messages));

    // The nodes are all in the store, and `history` marks them moved with the thread.
    let history = c
        .session_history(theseus_protocol::SessionHistoryParams {
            session_id: sid.clone(),
            ..Default::default()
        })
        .unwrap();
    let marked: Vec<_> = history
        .nodes
        .iter()
        .filter(|n| n.detail["moved_to"] == "gauges")
        .collect();
    assert_eq!(marked.len() as u32, res.moved);
    assert!(history
        .nodes
        .iter()
        .any(|n| n.text.contains("Question 5 about")));
    assert_eq!(crate::tests_day_ceiling::rows(c, "thread.split").len(), 1);
}

/// Into an existing thread: its compile before the split is a byte prefix of its compile after, and
/// the range follows in order.
#[tokio::test]
async fn a_split_into_an_existing_thread_appends_at_its_tail() {
    let r = rig(vec![
        Scripted::text("ok"),
        Scripted::text("the dev answer"),
        Scripted::text("another"),
    ]);
    let c = &r.core;
    let sid = compiled_home(&r, 9).await;
    let dev = thread(c, "dev");
    turn(c, &dev, "How is the harness built?").await;
    let (before, _, _) = request(c, &dev);

    let range = span(c, &sid, 2, 5);
    let res = split(
        c,
        ThreadSplitParams {
            from: sid.clone(),
            into: Some("dev".into()),
            range: Some(range),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(res.into && res.noted);
    let (after, how, _) = request(c, &dev);
    assert_eq!(how, "append", "the thread's prefix is unchanged");
    // The messages array of the compile before is the head of the one after.
    let head = before.trim_end_matches(']');
    assert!(
        after.starts_with(head),
        "the compile before is a byte prefix of the one after"
    );
    let tail = &after[head.len()..];
    let at = |n: usize| {
        tail.find(&format!("marker-{n}"))
            .unwrap_or_else(|| panic!("marker-{n}"))
    };
    assert!(
        at(2) < at(3) && at(3) < at(4) && at(4) < at(5),
        "in their order"
    );
}

/// An unsplit puts both compiles back to their pre-split bytes, and the ledger records it.
#[tokio::test]
async fn an_unsplit_puts_both_compiles_back_byte_for_byte() {
    let r = rig(vec![
        Scripted::text("ok"),
        Scripted::text("dev one"),
        Scripted::text("a"),
        Scripted::text("b"),
    ]);
    let c = &r.core;
    let sid = compiled_home(&r, 9).await;
    let dev = thread(c, "dev");
    turn(c, &dev, "How is the harness built?").await;
    let (home_before, _, _) = request(c, &sid);
    let (dev_before, _, _) = request(c, &dev);

    let into = split(
        c,
        ThreadSplitParams {
            from: sid.clone(),
            into: Some("dev".into()),
            range: Some(span(c, &sid, 1, 3)),
            ..Default::default()
        },
    )
    .unwrap();
    let (_, how, _) = request(c, &sid);
    assert_eq!(how, "recompile");
    // The compiles are frozen as a turn would freeze them.
    turn(c, &sid, "One more about the moon").await;
    turn(c, &dev, "One more about the build").await;
    let u = c
        .thread_unsplit(
            &ThreadUnsplitParams {
                split: into.split.clone(),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(!u.retired && u.restored == into.moved);
    let (home_after, how, _) = request(c, &sid);
    assert_eq!(
        how, "recompile",
        "its prefix changes once more, at its next turn"
    );
    let head = home_before.trim_end_matches(']');
    assert!(
        home_after.starts_with(head),
        "the home's compile is as it was before the split, and the later turn follows"
    );
    assert!(
        !home_after.contains("moved to thread"),
        "no note is left in place"
    );
    let (dev_after, _, _) = request(c, &dev);
    assert!(
        !dev_after.contains("marker-2"),
        "the quoted turns left the thread's compile"
    );
    assert!(dev_after.starts_with(dev_before.trim_end_matches(']')));
    assert_eq!(crate::tests_day_ceiling::rows(c, "thread.unsplit").len(), 1);
    // Nothing was deleted: the nodes are still in the store.
    assert!(c
        .store
        .transcript(&sid)
        .unwrap()
        .iter()
        .any(|(_, n)| { crate::arrangement::text_of(n).is_some_and(|t| t.contains("marker-2")) }));
    // A second unsplit is refused.
    let again = c
        .thread_unsplit(
            &ThreadUnsplitParams {
                split: into.split,
                ..Default::default()
            },
            cli(),
        )
        .unwrap_err();
    assert!(
        again.message.contains("undone already"),
        "{}",
        again.message
    );
}

/// An unsplit of a split that opened a thread ends the thread.
#[tokio::test]
async fn an_unsplit_ends_a_thread_the_split_opened() {
    let r = rig(vec![Scripted::text("ok")]);
    let c = &r.core;
    let sid = compiled_home(&r, 6).await;
    let (home_before, _, _) = request(c, &sid);
    let res = split(c, params(&sid, "gauges", span(c, &sid, 1, 2))).unwrap();
    let u = c
        .thread_unsplit(
            &ThreadUnsplitParams {
                split: res.split,
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(u.retired);
    assert_eq!(
        c.threads().get("gauges").unwrap().state,
        theseus_protocol::threads::ThreadState::Done
    );
    let (home_after, _, _) = request(c, &sid);
    assert_eq!(home_after, home_before);
}

/// A dry run writes nothing, and its "after" is the census of a real split of the same range.
#[tokio::test]
async fn a_dry_run_writes_nothing_and_its_after_is_the_census_after_a_split() {
    let r = rig(vec![Scripted::text("ok")]);
    let c = &r.core;
    let sid = compiled_home(&r, 12).await;
    let range = span(c, &sid, 2, 9);
    let records = c.store.last_position();
    let rows = crate::tests_day_ceiling::rows(c, "thread.split").len();
    let plan = c
        .thread_split_plan(&params(&sid, "gauges", range), cli())
        .unwrap();
    assert_eq!(c.store.last_position(), records, "no record was written");
    assert_eq!(
        crate::tests_day_ceiling::rows(c, "thread.split").len(),
        rows
    );
    assert!(c.threads().get("gauges").is_none());
    assert!(
        plan.nodes >= 8 * 2 && plan.turns == 8 && plan.bytes > 0,
        "{plan:?}"
    );
    assert!(plan.source.after < plan.source.before);
    assert!(plan.target.is_none() && !plan.into);

    split(c, params(&sid, "gauges", range)).unwrap();
    let (_, _, next) = request(c, &sid);
    let real = next.compiled.estimate.tokens as f64;
    let said = plan.source.after as f64;
    assert!(
        (real - said).abs() <= real * 0.05,
        "the dry run said {said} tokens, the census after the split {real}"
    );
}

/// Refusals: half a turn, a thread to split into that does not exist, a part over the cap, a name
/// that is no source, and a split that is not the owner's.
#[tokio::test]
async fn a_split_is_refused_when_it_cuts_a_turn_or_names_nothing() {
    let r = rig(vec![Scripted::text("ok")]);
    let c = &r.core;
    let sid = compiled_home(&r, 6).await;
    // A tool turn (3) cut in half: its message, without its reply.
    let q = pos(c, &sid, "Question 3 about");
    let e = split(c, params(&sid, "half", [q, q])).unwrap_err();
    assert!(e.contains("cuts a turn in half"), "{e}");
    assert!(
        e.contains(&format!("choose @{q} to @")),
        "it says what to type: {e}"
    );
    // A call without its result: from the turn's message through the call, not its result.
    let call = pos(c, &sid, "Looking at marker-3");
    let e = split(c, params(&sid, "half", [q, call])).unwrap_err();
    assert!(
        e.contains("cuts a turn in half") || e.contains("call without its result"),
        "{e}"
    );
    // No thread to split into.
    let range = span(c, &sid, 1, 2);
    let e = split(
        c,
        ThreadSplitParams {
            from: sid.clone(),
            into: Some("nowhere".into()),
            range: Some(range),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(e.contains("no thread nowhere to split into"), "{e}");
    // A name that is neither a thread, a session id nor home@place says what to type.
    let e = split(c, params("tides", "x", range)).unwrap_err();
    assert!(e.contains("home@<place>") && e.contains("ses_"), "{e}");
    let e = split(c, params("home@dm:nobody", "x", range)).unwrap_err();
    assert!(e.contains("theseus places"), "{e}");
    // A part over the cap is told to go in parts.
    let big = home(c);
    let text = "x".repeat(crate::arrangement::PIECE_CHARS);
    for i in 0..30 {
        let t = format!("turn_big{i:03}");
        c.store
            .append(&[Node::user(&big, Some(&t), "owner", &format!("{text}{i}"))
                .record()
                .unwrap()])
            .unwrap();
    }
    let e = split(c, params(&big, "bigger", [1, u64::MAX])).unwrap_err();
    assert!(
        e.contains("split it in parts") && e.contains("--into"),
        "{e}"
    );
    // Over the message count alone (the cap is 400 messages), with short messages.
    let many = home(c);
    for i in 0..401 {
        let t = format!("turn_many{i:04}");
        c.store
            .append(&[Node::user(&many, Some(&t), "owner", "a short one")
                .record()
                .unwrap()])
            .unwrap();
    }
    let e = split(c, params(&many, "most", [1, u64::MAX])).unwrap_err();
    assert!(e.contains("split it in parts"), "{e}");
    // Turns a split moved are not moved twice: the refusal names the split to undo.
    let first = split(c, params(&sid, "once", range)).unwrap();
    let e = split(c, params(&sid, "twice", range)).unwrap_err();
    assert!(
        e.contains("moved to «once» already") && e.contains(&first.split),
        "{e}"
    );
    assert!(c.threads().get("twice").is_none());
    // Not the owner's: a shared place's answerer is turned away.
    let guest = Answerer {
        label: "a guest".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "999".into(),
            channel_id: "chan".into(),
            guild_id: Some("guild".into()),
        }),
        agent: None,
    };
    let e = c
        .thread_split(&params(&sid, "guest", range), guest)
        .unwrap_err();
    assert!(
        !e.message.is_empty() && c.threads().get("guest").is_none(),
        "{}",
        e.message
    );
}

/// A split from a thread elides as well: the source thread's compile holds the line.
#[tokio::test]
async fn a_split_from_a_thread_leaves_the_threads_compile_too() {
    let script: Vec<_> = (0..6)
        .map(|i| Scripted::text(&format!("Answer {i} reply")))
        .collect();
    let r = rig(script);
    let c = &r.core;
    let a = thread(c, "harbour");
    for q in [
        "Tide table marker-a",
        "And Saturday marker-b",
        "The log disk marker-c",
    ] {
        turn(c, &a, q).await;
    }
    let (before, _, _) = request(c, &a);
    assert!(before.contains("marker-b"));
    let range = [
        pos(c, &a, "And Saturday marker-b"),
        pos(c, &a, "Answer 1 reply"),
    ];
    let res = split(c, params("harbour", "saturdays", range)).unwrap();
    assert!(res.noted);
    let (after, how, _) = request(c, &a);
    assert_eq!(
        how, "append",
        "the range was in the tail: the frozen prefix is unchanged"
    );
    // The turn's words are gone; its first message stands in the line, once.
    assert_eq!(after.matches("marker-b").count(), 1, "{after}");
    assert!(after.contains("marker-a") && after.contains("marker-c"));
    assert!(after.contains("moved to thread «saturdays»: And Saturday marker-b"));
}

/// A stand-in index that answers `picks`: (session, the index of its node, the fused score).
fn scored(c: &Arc<Core>, picks: Vec<(String, usize, f64)>) -> Ask {
    let store = c.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = picks
            .iter()
            .enumerate()
            .map(|(i, (sid, k, fused))| {
                let (position, n) = store.session_nodes(sid).unwrap().remove(*k);
                IndexHit {
                    text: crate::recall::text_of(&n),
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "user_message".into(),
                    origin: "operator".into(),
                    author: None,
                    place: None,
                    tool: None,
                    time_ms: 0,
                    external: false,
                    entities_matched: vec![],
                    entities: vec![],
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: *fused,
                }
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// The header of the recalled item in `sid`'s newest `Recall` node.
fn recalled_header(c: &Core, sid: &str) -> String {
    c.store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => items.first().map(|i| i.header.clone()),
            _ => None,
        })
        .expect("a Recall node")
}

/// Recall still reaches a moved turn from anywhere, and credits the thread it moved to; the home,
/// whose compile no longer holds the turn, may recall it again.
#[tokio::test]
async fn recall_still_finds_a_moved_turn_and_credits_its_thread() {
    let r = rig_with(MemoryMode::Canary, |c| c.memory.canary_fraction = 1.0);
    r.model
        .script
        .lock()
        .unwrap()
        .extend((0..5).map(|_| Scripted::text("noted")));
    let c = &r.core;
    let sid = compiled_home(&r, 6).await;
    let dev = thread(c, "dev");
    let ops = thread(c, "ops");
    let k = c
        .store
        .session_nodes(&sid)
        .unwrap()
        .iter()
        .position(|(_, n)| {
            crate::arrangement::text_of(n).is_some_and(|t| t.contains("Question 4 about"))
        })
        .unwrap();
    c.runner
        .memory
        .set_ask(scored(c, vec![(sid.clone(), k, 0.0164)]));
    let res = turn(c, &ops, "What was marker-4?").await;
    assert_eq!(res.recalled, 1, "the hit before the split");
    assert!(
        !recalled_header(c, &ops).contains("from thread"),
        "the home names no thread"
    );

    let range = span(c, &sid, 4, 4);
    split(
        c,
        ThreadSplitParams {
            title: Some("The gauges".into()),
            ..params(&sid, "gauges", range)
        },
    )
    .unwrap();
    let res = turn(c, &dev, "What was marker-4 again?").await;
    assert_eq!(res.recalled, 1, "the hit after the split");
    let header = recalled_header(c, &dev);
    assert!(header.contains("[from thread «The gauges»"), "{header}");
    // In the home itself: out of its compile, so recall brings it back, credited the same.
    let res = turn(c, &sid, "What was marker-4 once more?").await;
    assert_eq!(res.recalled, 1, "a moved turn is not in the home's context");
    assert!(recalled_header(c, &sid).contains("[from thread «The gauges»"));
}
