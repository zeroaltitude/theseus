//! The place digest is gone (the owner's decision of 2026-11-22: one memory, assembled the same way for
//! every turn). A message in an old thread's own session, which once asked classify.v4's `scope` and, on
//! `place`, carried a digest of the place's other live threads after the request's last cache mark, now
//! waits on no `scope` answer and its request holds no digest block.

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

fn rig_late(jev: &FakeJev, n: usize, total_secs: u64) -> Rig {
    rig_on(texts(n), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = total_secs;
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

/// A message in an old thread's own session, Jev answering `scope` with `place` (what once brought the
/// digest) and late: the turn waits on no `scope` answer, its request holds no digest block anywhere and
/// none of the other threads' cards, and the turn latches nothing as carried. The late answer's latency
/// grows with `bound_factor` as the turn's own work does (1 on a quiet machine: 3 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resumed_old_thread_sessions_request_holds_no_digest_block() {
    use crate::task_graph::judged::carried_in;
    let k = theseus_tools::bound_factor();
    let late = Duration::from_millis(3_000) * k as u32;
    let jev = FakeJev::start().unwrap();
    let r = rig_late(&jev, 2, 5 * k);
    let c = &r.core;
    threads(c);
    scope(&jev, "place");
    jev.set_latency(late, late);
    let t0 = std::time::Instant::now();
    let res = ask(
        c,
        "pr-round",
        "Add an 'also this week' section covering anything else we are shipping.",
    )
    .await;
    assert!(
        t0.elapsed() < late - Duration::from_millis(100),
        "the turn waited on the scope answer: {:?} against an answer at {late:?}",
        t0.elapsed()
    );
    let q = last_request(&r);
    let sent = whole(&q);
    assert!(!sent.contains("[Place digest"), "a digest block: {sent}");
    assert!(
        !sent.contains("from thread \u{ab}"),
        "a digest line: {sent}"
    );
    assert!(
        !carried_in(&c.store, &res.turn_id),
        "nothing latched as carried"
    );
}
