//! The place digest (theseus-98bz.9), through the core: classify.v4's `scope`
//! rides the one inbound request of a message in a thread; on `place` that
//! turn's requests carry one labelled line for each other live thread after
//! the last cache mark, a sealed or seal-pending one too (anything goes,
//! 2026-11-22); `thread`, a late answer, and the switch off give none.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams, ThreadSealParams, WorkingSet};
use theseus_protocol::{method, Id, Request, TurnSubmitParams, TurnSubmitResult};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::Core;
use crate::approval::{Answerer, Client, Surface};
use crate::provider::ProviderRequest;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{rig_on, texts, Rig};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// What each thread holds: slug, title, goal, card, a working-set host.
const THREADS: [(&str, &str, &str, &[&str], &str); 5] = [
    (
        "pr-round",
        "the tarnlib PR",
        "Answer four review comments and land the PR.",
        &["merge by 2026-11-19", "all four comments answered"],
        "tarn-docs-01.lan",
    ),
    (
        "pipeline",
        "the datatidy cleanup",
        "Clean three CSVs and write a per-group report.",
        &[
            "the cutoff is 0.55 since 2026-10-20",
            "the report regenerated",
        ],
        "tidy-stage-01.lan",
    ),
    (
        "quillhn",
        "the quillhn work",
        "A CLI flag and a follow-up bug.",
        &["--min-len landed"],
        "quill-stage-01.lan",
    ),
    (
        "embargo",
        "the ledgerweb zircon fix",
        "Patch the zircon traversal before disclosure.",
        &["zircon disclosure on 2026-11-02"],
        "zircon-stage-01.lan",
    ),
    (
        "pending",
        "the homesvc opal migration",
        "Move opal's secrets to the new vault.",
        &["opal window closes 2026-11-13"],
        "opal-stage-01.lan",
    ),
];

fn rig(jev: &FakeJev, n: usize, digest: bool) -> Rig {
    rig_on(texts(n), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.place_digest = digest;
    })
}

/// The five threads, `embargo` sealed and `pending` with a seal proposed.
fn threads(c: &Core) {
    for (slug, title, goal, card, host) in THREADS {
        c.thread_new(
            &ThreadNewParams {
                slug: slug.into(),
                title: Some(title.into()),
                goal: Some(goal.into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
        c.thread_edit(
            &ThreadEditParams {
                slug: slug.into(),
                card: Some(card.iter().map(|s| (*s).to_string()).collect()),
                working_set: Some(WorkingSet {
                    hosts: vec![host.into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    }
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            reason: Some("before disclosure".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    assert!(c
        .thread_recommend_seal("pending", "it names the vault")
        .unwrap());
}

fn scope(jev: &FakeJev, option: &str) {
    jev.script(
        "scope",
        Jev::Choice {
            option: option.into(),
            confidence: 0.95,
        },
    );
}

/// One `turn.submit` to `thread`'s session (a `turn.submit` names no thread any more: threads are
/// gone, and an address runs where it was written).
async fn ask(c: &Arc<Core>, thread: &str, input: &str) -> TurnSubmitResult {
    let session = c.thread_session(thread, cli()).unwrap().session_id;
    let (client, server) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(
        c.clone()
            .serve_connection(sr, sw, Client::new("test", Surface::Cli)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let p = TurnSubmitParams {
        session_id: Some(session),
        ..serde_json::from_value(serde_json::json!({"input": input})).unwrap()
    };
    let mut line =
        serde_json::to_string(&Request::new(Id::Num(1), method::TURN_SUBMIT, p)).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    loop {
        let l = tokio::time::timeout(Duration::from_secs(60), lines.next_line())
            .await
            .expect("a response")
            .unwrap()
            .expect("a line");
        let v: Value = serde_json::from_str(&l).unwrap();
        if v["id"] == 1 {
            return serde_json::from_value(v["result"].clone())
                .unwrap_or_else(|_| panic!("a turn result in {v}"));
        }
    }
}

fn last_request(r: &Rig) -> ProviderRequest {
    r.fake.requests().pop().expect("a request")
}

fn whole(q: &ProviderRequest) -> String {
    serde_json::to_string(&(&q.system, &q.messages, &q.tools)).unwrap()
}

/// The digest block of a request: the text after its last cache mark.
fn digest_of(q: &ProviderRequest) -> Option<String> {
    let blocks = q.messages.last()?["content"].as_array()?;
    blocks
        .iter()
        .filter_map(|b| b["text"].as_str())
        .find(|t| t.starts_with("[Place digest"))
        .map(str::to_string)
}

/// A turn whose scope is `place`, in a place of five threads, one sealed and
/// one seal-pending: one labelled line for each of the four other threads,
/// after the last cache mark, the sealed and the seal-pending ones too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_digest_shows_a_sealed_thread_like_any_other() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 2, true);
    let c = &r.core;
    threads(c);
    scope(&jev, "place");
    ask(
        c,
        "pr-round",
        "Add an 'also this week' section covering anything else we are shipping.",
    )
    .await;
    let q = last_request(&r);
    let d = digest_of(&q).expect("a digest");
    let lines: Vec<&str> = d.lines().skip(1).collect();
    assert_eq!(lines.len(), 4, "{d}");
    for l in &lines {
        assert!(l.starts_with("- from thread \u{ab}the"), "{d}");
    }
    for open in [
        "the datatidy cleanup",
        "the quillhn work",
        "the ledgerweb zircon fix",
        "the homesvc opal migration",
    ] {
        assert!(d.contains(&format!("\u{ab}{open}\u{bb}")), "{d}");
    }
    assert!(
        d.contains("dated: the cutoff is 0.55 since 2026-10-20"),
        "{d}"
    );
    assert!(d.ends_with(']'), "{d}");
    assert_eq!(d.matches("embargoed").count(), 0, "no count line: {d}");
    assert!(
        !d.contains("tarnlib PR\u{bb}"),
        "the asking thread has no line: {d}"
    );
    let sent = whole(&q);
    for held in ["ledgerweb", "Patch the zircon", "homesvc opal", "Move opal"] {
        assert!(sent.contains(held), "{held:?} did not reach the request");
    }
    // After the last cache mark: the block before it carries the mark.
    let blocks = q.messages.last().unwrap()["content"].as_array().unwrap();
    let at = blocks
        .iter()
        .position(|b| b["text"].as_str() == Some(d.as_str()))
        .unwrap();
    assert!(blocks[at].get("cache_control").is_none());
    assert!(at > 0 && blocks[at - 1].get("cache_control").is_some());
    assert!(q.cache_control.is_none(), "the mark moved onto the block");
}

/// A turn whose scope is `thread`, and one whose answer is later than
/// `route_wait_ms`, get no digest; the late one waited no more than the bound.
/// The bound sits 100 ms under the late answer's own latency, so it tells a
/// turn that waited for the answer from one that did not; on a machine that
/// gives each thread a third of a core the turn's own work stretches, so the
/// latency and the bound grow together, by what `bound_factor` measured (at
/// most 10; 1 on a quiet machine, where they are 3 s and 2.9 s exactly).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_scope_and_a_late_answer_get_no_digest() {
    let k = theseus_tools::bound_factor();
    let late = Duration::from_millis(3_000) * k as u32;
    let jev = FakeJev::start().unwrap();
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5 * k;
        c.threads.place_digest = true;
    });
    let c = &r.core;
    threads(c);
    scope(&jev, "thread");
    ask(c, "pr-round", "what's left on the tarnlib PR?").await;
    assert_eq!(digest_of(&last_request(&r)), None);
    scope(&jev, "place");
    jev.set_latency(late, late);
    let t0 = std::time::Instant::now();
    ask(c, "pipeline", "What else is going on this week?").await;
    assert_eq!(digest_of(&last_request(&r)), None, "late: no digest");
    assert!(
        t0.elapsed() < late - Duration::from_millis(100),
        "{:?} against an answer at {late:?}",
        t0.elapsed()
    );
}

/// The digest moves no cached byte: the system, the tools and every message
/// block before it are the same with and without it (the conversation's mark
/// aside, which the tail moves onto the block it closes).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cached_prefix_is_the_same_bytes_with_and_without_a_digest() {
    let (with, without) = (
        sent_with(true, "place").await,
        sent_with(true, "thread").await,
    );
    let (a, b) = (strip(&with), strip(&without));
    assert!(digest_of(&with).is_some() && digest_of(&without).is_none());
    assert_eq!(normal(&a), normal(&b));
    assert_eq!(normal(&with.system), normal(&without.system));
}

/// With `place_digest = false` the request is byte for byte the one a turn
/// without a digest sends, whatever Jev answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_digest_off_the_request_is_the_one_before() {
    let off = sent_with(false, "place").await;
    let none = sent_with(true, "thread").await;
    assert_eq!(digest_of(&off), None);
    assert_eq!(normal(&whole(&off)), normal(&whole(&none)));
    assert_eq!(off.cache_control, none.cache_control);
}

/// The model's request of one turn in `pr-round`, Jev answering `scope`.
async fn sent_with(digest: bool, answer: &str) -> ProviderRequest {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 2, digest);
    threads(&r.core);
    scope(&jev, answer);
    ask(&r.core, "pr-round", "What else is going on this week?").await;
    last_request(&r)
}

/// The request's messages without the digest block and without cache marks.
fn strip(q: &ProviderRequest) -> String {
    let mut m = q.messages.clone();
    for msg in &mut m {
        if let Some(blocks) = msg["content"].as_array_mut() {
            blocks.retain(|b| {
                !b["text"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("[Place digest"))
            });
            for b in blocks {
                if let Some(o) = b.as_object_mut() {
                    o.remove("cache_control");
                }
            }
        }
    }
    serde_json::to_string(&(&m, &q.tools)).unwrap()
}

/// Ids and times differ between two cores: each id and digit run is `#`.
fn normal(v: &impl serde::Serialize) -> String {
    let s = serde_json::to_string(v).unwrap();
    let mut out = String::with_capacity(s.len());
    let mut last_hash = false;
    for ch in s.chars() {
        if ch.is_ascii_hexdigit() && !ch.is_ascii_alphabetic() || ch.is_ascii_digit() {
            if !last_hash {
                out.push('#');
            }
            last_hash = true;
        } else {
            out.push(ch);
            last_hash = false;
        }
    }
    out
}

/// Issue 44 (a change in the owner's words): a turn that carried the digest
/// latches it for itself as it takes it, and the latch is what the change's
/// checks read. The thread's session retired and the thread opened again
/// (the old session gone from the board, as a rebind mid-turn would leave
/// it), the turn still reads as having carried it. A turn whose scope is
/// `thread` carried none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_carried_digest_is_latched_for_its_turn_and_outlives_a_rebind() {
    use crate::task_graph::judged::carried_in;
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4, true);
    let c = &r.core;
    threads(c);
    scope(&jev, "place");
    let res = ask(c, "pr-round", "Add an 'also this week' section.").await;
    assert!(
        digest_of(&last_request(&r)).is_some(),
        "the digest was carried"
    );
    assert!(carried_in(&c.store, &res.turn_id), "and latched");
    c.retire_session(&res.session_id, &cli()).unwrap();
    let again = c.thread_session("pr-round", cli()).unwrap();
    assert_ne!(again.session_id, res.session_id);
    let board = c.runner.threads.held().expect("the board");
    assert!(
        board.of_session(&res.session_id).is_none(),
        "the old session left the board"
    );
    assert!(carried_in(&c.store, &res.turn_id), "the latch outlives it");
    scope(&jev, "thread");
    let plain = ask(c, "quillhn", "One small question about the flag.").await;
    assert!(digest_of(&last_request(&r)).is_none(), "no digest");
    assert!(!carried_in(&c.store, &plain.turn_id), "and no latch");
}
