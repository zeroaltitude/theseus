//! `session.history` paged both ways (theseus-xo0m, theseus-kym3): `after`
//! walks forward and `before` walks back, each node once while nodes are
//! written between the pages; a question's card stays on every page; and a
//! page costs its answer, whatever the session's length.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{SessionHistoryParams, SessionHistoryResult, SessionKind};
use theseus_store::NewRecord;

use super::pages::bounded;
use super::tests::test_core;
use super::Core;
use crate::bus::EventSink;
use crate::node::Node;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::Config;

fn open(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// `count` messages into `session`, `per` to a frame; their positions.
fn write(core: &Core, session: &str, count: u64, per: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut left = count;
    while left > 0 {
        let k = left.min(per);
        let batch: Vec<NewRecord> = (0..k)
            .map(|i| {
                Node::user(session, None, "operator", &format!("message {i}"))
                    .record()
                    .unwrap()
            })
            .collect();
        out.extend(core.store.append(&batch).unwrap());
        left -= k;
    }
    out
}

fn history(
    core: &Core,
    session: &str,
    n: Option<usize>,
    after: Option<u64>,
    before: Option<u64>,
) -> SessionHistoryResult {
    core.session_history(SessionHistoryParams {
        session_id: session.into(),
        n,
        after,
        before,
    })
    .unwrap()
}

fn positions(h: &SessionHistoryResult) -> Vec<u64> {
    h.nodes.iter().map(|x| x.position).collect()
}

fn stored(core: &Core, session: &str) -> Vec<u64> {
    core.store
        .session_nodes(session)
        .unwrap()
        .iter()
        .map(|(p, _)| *p)
        .collect()
}

/// A walk forward from 0 by `next` reads every node of the session once,
/// oldest first, with another session's nodes between them and nodes
/// written between the pages; at the end, `next` is absent, and a poll past
/// the newest is an empty page at once.
#[tokio::test]
async fn a_walk_forward_by_next_reads_each_node_once() {
    let core = test_core("ok");
    let (a, b) = (open(&core), open(&core));
    for _ in 0..100 {
        write(&core, &a, 2, 2);
        write(&core, &b, 1, 1);
    }
    let mut seen = Vec::new();
    let mut after = 0;
    let mut pages = 0;
    loop {
        let h = history(&core, &a, Some(37), Some(after), None);
        assert!(h.nodes.len() <= 37, "a page holds at most n");
        assert_eq!(h.older, None, "a page forward has no `older`");
        seen.extend(positions(&h));
        pages += 1;
        assert!(pages < 50, "the walk ends: {seen:?}");
        if pages <= 3 {
            write(&core, &a, 5, 1);
            write(&core, &b, 3, 1);
        }
        match h.next {
            Some(p) => {
                assert_eq!(Some(p), h.nodes.last().map(|x| x.position));
                after = p;
            }
            None => break,
        }
    }
    assert_eq!(seen, stored(&core, &a), "each node once, in order");
    assert_eq!(seen.len(), 215);
    let newest = *seen.last().unwrap();
    let poll = history(&core, &a, Some(37), Some(newest), None);
    assert!(poll.nodes.is_empty() && poll.next.is_none(), "{poll:?}");
}

/// A walk back from past the newest by `older` reads every node the
/// session had once, newest page first; nodes written meanwhile are newer
/// than every page and appear on none.
#[tokio::test]
async fn a_walk_back_by_older_reads_each_node_once() {
    let core = test_core("ok");
    let (a, b) = (open(&core), open(&core));
    for _ in 0..100 {
        write(&core, &a, 3, 3);
        write(&core, &b, 1, 1);
    }
    let had = stored(&core, &a);
    let mut pages: Vec<Vec<u64>> = Vec::new();
    let mut before = u64::MAX;
    loop {
        let h = history(&core, &a, Some(41), None, Some(before));
        assert!(h.nodes.len() <= 41);
        assert_eq!(h.next, None, "a page back has no `next`");
        pages.push(positions(&h));
        assert!(pages.len() < 50, "the walk ends: {pages:?}");
        write(&core, &a, 2, 1);
        match h.older {
            Some(p) => {
                assert_eq!(Some(p), h.nodes.first().map(|x| x.position));
                before = p;
            }
            None => break,
        }
    }
    let seen: Vec<u64> = pages.into_iter().rev().flatten().collect();
    assert_eq!(seen, had, "each node once, in order");
    assert_eq!(stored(&core, &a).len(), had.len() + 2 * 8);
}

/// Neither cursor: today's answer, byte for byte: the newest `n`, or the
/// whole session, and no `next` or `older` on the wire.
#[tokio::test]
async fn without_a_cursor_the_answer_is_todays() {
    let core = test_core("ok");
    let a = open(&core);
    write(&core, &a, 30, 7);
    let all = stored(&core, &a);
    let h = history(&core, &a, Some(10), None, None);
    assert_eq!(positions(&h), all[20..].to_vec());
    let whole = history(&core, &a, None, None, None);
    assert_eq!(positions(&whole), all);
    for h in [h, whole] {
        let v = serde_json::to_value(&h).unwrap();
        assert!(v.get("next").is_none() && v.get("older").is_none(), "{v}");
    }
    // With a cursor and no `n`, a page is 200.
    write(&core, &a, 300, 50);
    let h = history(&core, &a, None, Some(0), None);
    assert_eq!(h.nodes.len(), 200);
    assert!(h.next.is_some());
}

/// While the index's shape is built, the session is read whole and the
/// page's bounds applied (`bounded`): the same nodes and the same cursors
/// as the index's page, for every kind of question.
#[tokio::test]
async fn the_read_while_the_shape_is_built_bounds_as_the_index_does() {
    let core = test_core("ok");
    let (a, b) = (open(&core), open(&core));
    for i in 0..120u64 {
        write(&core, &a, 1 + i % 3, 2);
        write(&core, &b, 1, 1);
    }
    let all = core.store.session_nodes(&a).unwrap();
    let first = all[0].0;
    let last = all.last().unwrap().0;
    let mut cuts = vec![0, first - 1, first, first + 1, last - 1, last, last + 1];
    cuts.extend((0..20).map(|i| first + (last - first) * i / 20));
    for &n in &[1usize, 7, 50, 1000] {
        for &c in &cuts {
            for (after, before) in [(Some(c), None), (None, Some(c)), (Some(c), Some(c + 90))] {
                let h = core.history_page(&a, Some(n), after, before).unwrap();
                let (want, more) = bounded(all.clone(), after, before, n);
                let want: Vec<u64> = want.iter().map(|(p, _)| *p).collect();
                let have: Vec<u64> = h.nodes.iter().map(|(p, _)| *p).collect();
                assert_eq!(have, want, "n {n}, after {after:?}, before {before:?}");
                let next = after.and(more.then(|| want.last().copied()).flatten());
                assert_eq!(h.next, next, "n {n}, after {after:?}, before {before:?}");
                let older = before
                    .filter(|_| after.is_none())
                    .and(more.then(|| want.first().copied()).flatten());
                assert_eq!(h.older, older, "n {n}, after {after:?}, before {before:?}");
            }
        }
    }
}

/// A page costs its answer: a page past a position, or back from one,
/// decodes `n` nodes in a session of 100 and in one of 10,000, and the
/// transcript is never read whole.
#[tokio::test]
async fn a_page_reads_the_same_at_a_hundred_nodes_and_at_ten_thousand() {
    let core = test_core("ok");
    let mut reads = Vec::new();
    for size in [100u64, 10_000] {
        let s = open(&core);
        let at = write(&core, &s, size, 500);
        let mid = at[at.len() / 2];
        let whole = core.store.transcript_reads();
        let fwd = core.history_page(&s, Some(20), Some(mid), None).unwrap();
        let back = core.history_page(&s, Some(20), None, Some(mid)).unwrap();
        let newest = core.history_page(&s, Some(20), None, None).unwrap();
        assert_eq!(core.store.transcript_reads(), whole, "no whole read");
        assert_eq!((fwd.nodes.len(), back.nodes.len()), (20, 20));
        reads.push((fwd.read, back.read, newest.read));
    }
    assert_eq!(reads, vec![(20, 20, 20), (20, 20, 20)]);
}

fn rig(script: Vec<Scripted>) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Approve;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    (core, dir)
}

/// A question waiting in the session keeps its card on every page, the gate's
/// reason read from its call's node however far back the page is, as the
/// whole read gives it, and no page reads the transcript whole.
#[tokio::test]
async fn a_pending_confirms_card_is_on_every_page() {
    let (core, _dir) = rig(vec![Scripted::tools(
        "",
        &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
    )]);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let r = core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("write a.txt".into()),
            target,
            sink: EventSink::new(core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
    let corr = r.awaiting_confirm.expect("the write waits");
    let early = stored(&core, &sid);
    write(&core, &sid, 300, 50);
    let all = history(&core, &sid, None, None, None);
    let card = serde_json::to_value(&all.pending_confirms).unwrap();
    assert_eq!(all.pending_confirms.len(), 1);
    assert_eq!(all.pending_confirms[0].correlation_id, corr);
    assert!(
        !all.pending_confirms[0].reason.is_empty(),
        "the gate's reason"
    );
    let newest = *stored(&core, &sid).last().unwrap();
    let whole = core.store.transcript_reads();
    for (n, after, before) in [
        (Some(5), None, None),
        (Some(5), None, Some(u64::MAX)),
        (Some(5), Some(newest), None),
        (Some(5), Some(0), None),
        (Some(3), None, Some(early[1])),
    ] {
        let h = history(&core, &sid, n, after, before);
        assert_eq!(
            serde_json::to_value(&h.pending_confirms).unwrap(),
            card,
            "{n:?} {after:?} {before:?}"
        );
        assert_eq!(h.session.attention, all.session.attention);
    }
    assert_eq!(
        core.store.transcript_reads(),
        whole,
        "no page read it whole"
    );
}
