//! theseus-64x against the tender: a node that leaves the index never
//! answers a query or `index.neighbours` again; on `index.forget` its
//! vector's bytes leave every vector file at once; a file a quarter dead, or
//! a rebuild, is compacted; and the socket carries `index.forget`. The tiny
//! seeded model of `vtests`, so nothing is downloaded.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::node::Node;

use crate::client::Client;
use crate::proto::{
    method, ChunkRef, ForgetParams, ForgetResult, NeighboursParams, QueryParams, QueryResult, Task,
};
use crate::server;
use crate::tender::{Shared, Tender};
use crate::tests::{call, settle, user};
use crate::vectors::{text_hash, NodeChunks, Texts};
use crate::vtests::{settle_all, status, VRig, TEXTS};

/// Every node any source returns for any of `questions` (k = 100, so all of
/// them), by default sources, vectors alone, and BM25 and entities.
fn answers(s: &Shared, questions: &[&str]) -> Vec<String> {
    let mut seen = Vec::new();
    for q in questions {
        for sources in [&[][..], &["vector"][..], &["bm25", "entity"][..]] {
            let mut p = QueryParams::new(q);
            p.k = 100;
            p.wait_ms = 10_000;
            p.sources = sources.iter().map(|x| x.to_string()).collect();
            let r = s.query(&p).unwrap();
            assert!(r.skipped.is_empty(), "{:?}", r.skipped);
            seen.extend(r.hits.into_iter().map(|h| h.node_id));
        }
    }
    seen
}

fn neighbours(s: &Shared, id: &str) -> Result<Vec<String>, String> {
    s.neighbours(&NeighboursParams {
        node_id: id.to_string(),
        k: 100,
        as_of: None,
    })
    .map(|r| r.neighbours.into_iter().map(|n| n.node_id).collect())
    .map_err(|e| e.to_string())
}

/// Every byte of every file in the vector directory.
fn file_bytes(dir: &Path) -> Vec<u8> {
    let mut all = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        all.extend(std::fs::read(e.unwrap().path()).unwrap());
    }
    all
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn forget_nodes(t: &mut Tender, nodes: &[&Node]) -> ForgetResult {
    t.forget(&ForgetParams {
        nodes: nodes.iter().map(|n| n.id.clone()).collect(),
        texts: Vec::new(),
    })
    .unwrap()
}

/// A node leaves the index three ways: forgotten; written again with
/// nothing to index (as an erased payload will be); or written again with
/// another text, when only its old text leaves. Then no question, by any
/// source, returns it (or its old text), `index.neighbours` knows it no more,
/// and it is no other node's neighbour.
#[test]
fn a_node_that_leaves_the_index_never_answers_again() {
    let v = VRig::new();
    let nodes: Vec<Node> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let s = t.shared();
    for (n, text) in nodes.iter().zip(TEXTS) {
        assert!(answers(&s, &[text]).contains(&n.id));
    }
    assert_eq!(neighbours(&s, &nodes[11].id).unwrap().len(), 11);

    // 1. Forgotten.
    let forgot = &nodes[0];
    let r = forget_nodes(&mut t, &[forgot]);
    assert_eq!((r.nodes, r.chunks, r.vectors_dropped), (1, 1, 1), "{r:?}");
    // 2. Written again as a call whose input is never indexed.
    let erased = &nodes[1];
    let mut again = call(
        "ses_1",
        "fs.write",
        json!({"path": "a.txt", "content": "x"}),
    );
    again.id = erased.id.clone();
    v.rig.put(&[again]);
    settle(&mut t);
    // 3. Written again with another text.
    let replaced = &nodes[2];
    let mut new = user("ses_1", "an entirely different sentence about lighthouses");
    new.id = replaced.id.clone();
    v.rig.put(&[new]);
    settle_all(&mut t);

    let questions: Vec<&str> = TEXTS
        .iter()
        .copied()
        .chain(["lighthouses", "a feline on a rug", "the sea"])
        .collect();
    let seen = answers(&s, &questions);
    for gone in [forgot, erased] {
        assert!(!seen.contains(&gone.id), "{} answered", gone.id);
        let e = neighbours(&s, &gone.id).unwrap_err();
        assert!(e.contains("no node"), "{e}");
    }
    for n in &nodes[2..] {
        let near = neighbours(&s, &n.id).unwrap();
        assert!(
            !near.contains(&forgot.id) && !near.contains(&erased.id),
            "{near:?}"
        );
        assert_eq!(near.len(), 9);
    }
    // The node written again answers with its new text, never its old one.
    let mut p = QueryParams::new(TEXTS[2]);
    p.k = 100;
    p.wait_ms = 10_000;
    let r = s.query(&p).unwrap();
    assert!(r.hits.iter().all(|h| h.text != TEXTS[2]));
    assert!(r
        .hits
        .iter()
        .any(|h| h.node_id == replaced.id && h.text.contains("lighthouses")));
    // The vector side: ten chunks; the forgotten text's record gone from the
    // file; the erased and the replaced texts' records dead, not yet
    // compacted (2 of 12, under a quarter).
    let st = status(&s);
    assert_eq!(
        (st.chunks, st.vectors, st.records, st.dead),
        (10, 10, 12, 2),
        "{st:?}"
    );
}

/// After `index.forget`, a forgotten text's vector is in no vector file:
/// neither its hash nor its full vector's bytes. A text another chunk still
/// holds keeps its vector, and the answer says which chunk; forgetting the
/// text itself takes every chunk that holds it, and the vector. A restart
/// reads the rewritten file whole, and a forget that comes before the
/// embedding thread has read the files takes the record all the same.
#[test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn after_forget_no_vector_file_holds_its_bytes() {
    let v = VRig::new();
    let mut nodes: Vec<Node> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    // TEXTS[3], said twice: by nodes[3] and by nodes[12].
    nodes.push(user("ses_2", TEXTS[3]));
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let s = t.shared();
    let dir = v.rig.index.join("vectors");
    let emb = s.vectors.model(Duration::ZERO).unwrap();
    // What a record of `text` holds: its hash, and its full vector at f16.
    let marks = |text: &str| -> [Vec<u8>; 2] {
        let v = emb.embed(Task::SearchDocument, &[text]).unwrap().remove(0);
        [
            text_hash(text).to_le_bytes().to_vec(),
            v.full
                .iter()
                .flat_map(|x| half::f16::from_f32(*x).to_bits().to_le_bytes())
                .collect(),
        ]
    };
    let present = |text: &str| -> [bool; 2] {
        let all = file_bytes(&dir);
        marks(text).map(|m| has(&all, &m))
    };
    assert_eq!(present(TEXTS[0]), [true, true]);
    assert_eq!(present(TEXTS[3]), [true, true]);
    assert_eq!(status(&s).records, 12);

    let r = forget_nodes(&mut t, &[&nodes[0]]);
    assert_eq!((r.nodes, r.vectors_dropped, r.files), (1, 1, 1), "{r:?}");
    assert!(r.bytes_after < r.bytes_before && r.still_held.is_empty());
    assert_eq!(present(TEXTS[0]), [false, false]);

    // Held by another chunk: it stays, and the answer says where.
    let r = forget_nodes(&mut t, &[&nodes[3]]);
    assert_eq!((r.nodes, r.vectors_dropped), (1, 0), "{r:?}");
    assert_eq!(
        r.still_held,
        [ChunkRef {
            node_id: nodes[12].id.clone(),
            chunk: 0
        }]
    );
    assert_eq!(present(TEXTS[3]), [true, true]);
    // The text itself: every chunk that holds it leaves, with the vector.
    let r = t
        .forget(&ForgetParams {
            nodes: Vec::new(),
            texts: vec![TEXTS[3].to_string()],
        })
        .unwrap();
    assert_eq!((r.nodes, r.chunks, r.vectors_dropped), (0, 1, 1), "{r:?}");
    assert_eq!(present(TEXTS[3]), [false, false]);
    let seen = answers(&s, &[TEXTS[0], TEXTS[3]]);
    for gone in [&nodes[0], &nodes[3], &nodes[12]] {
        assert!(!seen.contains(&gone.id));
    }
    // Nothing named that the index holds: nothing changes.
    let r = t
        .forget(&ForgetParams {
            nodes: vec!["nd_none".into()],
            texts: vec!["no chunk says this".into()],
        })
        .unwrap();
    assert_eq!(
        (r.nodes, r.chunks, r.vectors_dropped, r.files),
        (0, 0, 0, 0)
    );
    let st = status(&s);
    assert_eq!(
        (st.chunks, st.records, st.dead, st.compactions.count),
        (10, 10, 0, 2),
        "{st:?}"
    );
    assert_eq!(st.compactions.last_why, "forget");
    drop(s);

    // A restart: every record left reads back, nothing is embedded again.
    drop(t);
    let mut t = v.open();
    settle_all(&mut t);
    let st = status(&t.shared());
    assert_eq!(
        (st.vectors, st.pending, st.backfill.texts, st.records),
        (10, 0, 0, 10),
        "{st:?}"
    );

    // A forget before the embedding thread has read the files: the ingest
    // thread has caught up, the rows are not yet reconciled.
    drop(t);
    let mut t = v.open();
    settle(&mut t);
    let s = t.shared();
    // The index as a reconcile that started before the forget read it.
    let before = Stale(s.engine.all_nodes().unwrap());
    assert!(before.0.iter().any(|n| n.node_id == nodes[5].id));
    let r = forget_nodes(&mut t, &[&nodes[5]]);
    assert_eq!((r.nodes, r.vectors_dropped), (1, 1), "{r:?}");
    assert_eq!(present(TEXTS[5]), [false, false]);
    // That reconcile does not bring the node back.
    s.vectors.reconcile(&before).unwrap();
    assert!(neighbours(&s, &nodes[5].id).is_err());
    let st = status(&s);
    assert_eq!((st.chunks, st.pending, st.dead), (9, 0, 0), "{st:?}");
    drop(s);
    settle_all(&mut t);
    let s = t.shared();
    let st = status(&s);
    assert_eq!(
        (
            st.chunks,
            st.vectors,
            st.pending,
            st.backfill.texts,
            st.records
        ),
        (9, 9, 0, 0, 9),
        "{st:?}"
    );
    assert!(!answers(&s, &[TEXTS[5]]).contains(&nodes[5].id));
}

/// A `Texts` whose view of the index is older than a forget.
struct Stale(Vec<NodeChunks>);

impl Texts for Stale {
    fn chunk_text(&self, _: &str, _: u32) -> Option<String> {
        None
    }

    fn all_nodes(&self) -> anyhow::Result<Vec<NodeChunks>> {
        Ok(self.0.clone())
    }
}

/// The file is compacted once dead records pass a quarter of it (not at a
/// fifth), by the embedding thread; and once a rebuild has caught up, of any
/// dead record. Every text a chunk holds still answers, none embedded again.
#[test]
fn a_quarter_dead_or_a_rebuild_compacts_the_vector_file() {
    let v = VRig::new();
    let nodes: Vec<Node> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let s = t.shared();
    let replace = |i: usize, t: &mut Tender| {
        let mut n = user("ses_1", &format!("replacement number {i} of the text"));
        n.id = nodes[i].id.clone();
        v.rig.put(&[n]);
        settle_all(t);
    };
    // Three written again: 3 of 15 records dead, a fifth.
    for i in 0..3 {
        replace(i, &mut t);
    }
    let st = status(&s);
    assert_eq!(
        (st.records, st.dead, st.compactions.count),
        (15, 3, 0),
        "{st:?}"
    );
    // A fourth: 4 of 15 before its new text is embedded, past a quarter.
    replace(3, &mut t);
    let st = status(&s);
    assert_eq!(
        (
            st.records,
            st.dead,
            st.compactions.count,
            st.chunks,
            st.vectors
        ),
        (12, 0, 1, 12, 12),
        "{st:?}"
    );
    assert_eq!(st.compactions.last_why, "dead");
    assert_eq!(st.compactions.dropped, 4);
    assert!(st.compactions.last_bytes_after < st.compactions.last_bytes_before);
    let embedded = st.backfill.texts;

    // One more dead record, under a quarter: it waits for the rebuild.
    replace(4, &mut t);
    assert_eq!((status(&s).records, status(&s).dead), (13, 1));
    s.request_rebuild();
    let h = std::thread::spawn(move || t.run());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let x = s.status();
        if x.rebuilds == 1 && x.state == "ready" && x.position > 0 && x.nodes == 12 {
            break;
        }
        assert!(Instant::now() < deadline, "rebuild never finished: {x:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    s.vectors.settle(&s.engine).unwrap();
    s.vectors.work_once(&s.engine);
    let st = status(&s);
    assert_eq!(
        (
            st.records,
            st.dead,
            st.compactions.count,
            st.vectors,
            st.pending
        ),
        (12, 0, 2, 12, 0),
        "{st:?}"
    );
    assert_eq!(st.compactions.last_why, "rebuild");
    // Nothing embedded again: the rebuilt chunks found their vectors.
    assert_eq!(st.backfill.texts, embedded + 1);
    s.request_stop();
    h.join().unwrap().unwrap();
}

/// `index.forget` through the socket, while the ingest thread runs: the
/// answer comes when it is done, and a query after it never finds the node.
#[test]
fn the_socket_forgets_while_the_tender_runs() {
    let v = VRig::new();
    let nodes: Vec<Node> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let s = t.shared();
    let sock = t.paths().socket();
    drop(server::spawn(&sock, s.clone()).unwrap());
    let h = std::thread::spawn(move || t.run());
    let mut c = Client::connect(&sock, Duration::from_secs(30)).unwrap();
    let r: ForgetResult = c
        .call(method::FORGET, json!({"nodes": [nodes[7].id]}))
        .unwrap();
    assert_eq!((r.nodes, r.chunks, r.vectors_dropped), (1, 1, 1), "{r:?}");
    let q: QueryResult = c
        .call(
            method::QUERY,
            json!({"text": TEXTS[7], "k": 100, "wait_ms": 10000}),
        )
        .unwrap();
    assert!(q.hits.iter().all(|h| h.node_id != nodes[7].id));
    assert_eq!(q.hits.len(), 11);
    // Not a forget: a malformed one is refused before it is queued.
    assert!(c
        .call::<ForgetResult>(method::FORGET, json!({"nodes": "one"}))
        .is_err());
    s.request_stop();
    h.join().unwrap().unwrap();
}
