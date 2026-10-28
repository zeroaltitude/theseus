//! The place digest (theseus-98bz.9), through the core: classify.v4's `scope`
//! rides the one inbound request of a message in a thread; on `place` that
//! turn's requests carry one labelled line for each other open thread after
//! the last cache mark, and a sealed or seal-pending thread only as a count;
//! `thread`, a late answer, and the switch off give none.

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

/// The words of the sealed and the seal-pending threads that must reach no
/// other thread's request.
const HELD: [&str; 8] = [
    "zircon",
    "ledgerweb",
    "Patch the zircon",
    "zircon-stage-01.lan",
    "opal",
    "homesvc opal",
    "Move opal",
    "opal-stage-01.lan",
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

/// One `turn.submit` addressed to `thread` (`theseus ask --thread`).
async fn ask(c: &Arc<Core>, thread: &str, input: &str) -> TurnSubmitResult {
    let (client, server) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(
        c.clone()
            .serve_connection(sr, sw, Client::new("test", Surface::Cli)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let p = TurnSubmitParams {
        thread: Some(thread.into()),
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
/// one seal-pending: one labelled line for each of the two open other threads
/// and the count line, after the last cache mark; and no byte of the held
/// threads' titles, goals, cards or working sets anywhere in the request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_digest_never_shows_a_sealed_thread() {
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
    assert_eq!(lines.len(), 3, "{d}");
    assert!(lines[0].starts_with("- from thread \u{ab}the"), "{d}");
    assert!(lines[1].starts_with("- from thread \u{ab}the"), "{d}");
    for open in ["the datatidy cleanup", "the quillhn work"] {
        assert!(d.contains(&format!("\u{ab}{open}\u{bb}")), "{d}");
    }
    assert!(
        d.contains("dated: the cutoff is 0.55 since 2026-10-20"),
        "{d}"
    );
    assert_eq!(lines[2], "2 embargoed threads (not shown).]");
    assert!(
        !d.contains("tarnlib PR\u{bb}"),
        "the asking thread has no line: {d}"
    );
    let sent = whole(&q);
    for held in HELD {
        assert!(!sent.contains(held), "{held:?} reached the request");
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

/// The session of `slug`'s thread, which a first turn in it opens.
async fn thread_session(c: &Arc<Core>, jev: &FakeJev, slug: &str) -> String {
    scope(jev, "thread");
    ask(c, slug, "what's left here?").await;
    let board = c.threads();
    let th = board.get(slug).expect("the thread");
    th.session_id.clone().expect("its session")
}

/// A turn whose scope is `thread`, and one whose answer is later than
/// `route_wait_ms`, get no digest; the late one waited no more than the bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_scope_and_a_late_answer_get_no_digest() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4, true);
    let c = &r.core;
    threads(c);
    scope(&jev, "thread");
    ask(c, "pr-round", "what's left on the tarnlib PR?").await;
    assert_eq!(digest_of(&last_request(&r)), None);
    scope(&jev, "place");
    jev.set_latency(Duration::from_millis(3_000), Duration::from_millis(3_000));
    let t0 = std::time::Instant::now();
    ask(c, "pipeline", "What else is going on this week?").await;
    assert_eq!(digest_of(&last_request(&r)), None, "late: no digest");
    assert!(
        t0.elapsed() < Duration::from_millis(2_900),
        "{:?}",
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

/// The other threads' cards are the owner's material, so a draft's request
/// carries no digest (theseus-rcb2u): the same `place` turn in the PR's
/// thread shows one, and shows none once its session is a draft's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_drafts_request_carries_no_place_digest() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4, true);
    let c = &r.core;
    threads(c);
    let pr = thread_session(c, &jev, "pr-round").await;
    scope(&jev, "place");
    ask(c, "pr-round", "What else is going on in this place?").await;
    assert!(digest_of(&last_request(&r)).is_some(), "the control");
    crate::tests_dm_rig::mark_draft(c, &pr);
    scope(&jev, "place");
    ask(c, "pr-round", "And what else is going on in this place?").await;
    assert_eq!(digest_of(&last_request(&r)), None, "a draft's request");
}
