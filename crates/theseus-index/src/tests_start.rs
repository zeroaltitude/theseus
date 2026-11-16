//! The start reads each chunk's fingerprint and where it lives, never its text (issue 21 of 1.0.7):
//! the vector side's rows are rebuilt from the index's columns, so the stored text stays on disk.

use std::collections::BTreeMap;
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::engine::{chunk_key, open_or_create, Engine, Writer};
use crate::extract::Extracted;
use crate::vectors::{NodeChunks, Texts};

fn node(id: &str, session: &str, kind: &str, external: bool, text: &str) -> Extracted {
    Extracted {
        node_id: id.into(),
        session_id: session.into(),
        kind: kind.into(),
        origin: "operator".into(),
        author: None,
        created_at_ms: 1_700_000_000_000,
        external,
        tool: None,
        text: text.into(),
        files: Vec::new(),
    }
}

/// A text of about `words` words, each its own token, so it takes several chunks when long.
fn words(seed: &str, n: usize) -> String {
    (0..n)
        .map(|i| format!("{seed}{i}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A writer and an engine over one directory.
fn open(dir: &Path) -> (Writer, Engine) {
    let (index, fields) = open_or_create(dir).unwrap();
    (
        Writer::new(&index, fields).unwrap(),
        Engine::new(index, fields).unwrap(),
    )
}

fn commit(w: &mut Writer, e: &Engine) {
    w.commit().unwrap();
    e.reload().unwrap();
}

/// What a start used to compute: every stored document read whole, grouped by node.
fn from_the_text(e: &Engine) -> Vec<NodeChunks> {
    let mut by_node: BTreeMap<String, NodeChunks> = BTreeMap::new();
    for c in e.dump().unwrap() {
        by_node
            .entry(c.node_id.clone())
            .or_insert_with(|| NodeChunks {
                node_id: c.node_id.clone(),
                position: c.position,
                time_ms: c.time,
                session: c.session.clone(),
                kind: c.kind.clone(),
                external: c.external,
                chunks: Vec::new(),
            })
            .chunks
            .push(chunk_key(c.chunk, &c.text));
    }
    let mut out: Vec<NodeChunks> = by_node.into_values().collect();
    for n in &mut out {
        n.chunks.sort_by_key(|c| c.chunk);
    }
    out
}

/// Overwrite the stored text of every segment with garbage: what is left readable is the columns,
/// the terms and the footers.
fn ruin_the_stored_text(dir: &Path) {
    let mut ruined = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|x| x.to_str()) != Some("store") {
            continue;
        }
        let len = std::fs::metadata(&path).unwrap().len();
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all_at(&vec![0xFFu8; (len / 2) as usize], 0)
            .unwrap();
        ruined += 1;
    }
    assert!(ruined > 0, "the index has stored text to ruin");
}

fn fixture(w: &Writer, base: u64) {
    for i in 0..12u64 {
        let long = words(&format!("w{i}x"), if i % 4 == 0 { 1300 } else { 40 });
        let e = node(
            &format!("nd_{i:04}"),
            &format!("ses_{}", i % 3),
            if i % 2 == 0 {
                "user_message"
            } else {
                "assistant_message"
            },
            i % 5 == 0,
            &long,
        );
        w.replace(base + i, None, &e, &[]).unwrap();
    }
}

/// The start's table is what the stored text gives: the same nodes, in the same order, with
/// the same chunks, hashes and token counts, where the node lives, and whether it is external.
#[test]
fn the_start_reads_what_the_text_gave() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut w, e) = open(tmp.path());
    fixture(&w, 100);
    commit(&mut w, &e);
    let got = e.all_nodes().unwrap();
    assert_eq!(got.len(), 12);
    assert!(
        got.iter().any(|n| n.chunks.len() > 1),
        "a node of several chunks"
    );
    assert_eq!(got, from_the_text(&e));
}

/// The same across segments, a node written again with other text, a node and a chunk deleted:
/// only what is alive is read, and a changed text has its new hash.
#[test]
fn the_start_reads_only_what_is_alive_across_segments() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut w, e) = open(tmp.path());
    fixture(&w, 100);
    commit(&mut w, &e);
    let before = e.all_nodes().unwrap();
    // Node 3 again, with other words; node 5 gone; node 8's second chunk gone.
    w.replace(
        500,
        None,
        &node(
            "nd_0003",
            "ses_0",
            "user_message",
            false,
            "words that are new",
        ),
        &[],
    )
    .unwrap();
    w.delete_node("nd_0005");
    w.delete_chunk("nd_0008", 1).unwrap();
    commit(&mut w, &e);
    let got = e.all_nodes().unwrap();
    assert_eq!(got, from_the_text(&e));
    assert_eq!(got.len(), 11);
    let find = |v: &[NodeChunks], id: &str| v.iter().find(|n| n.node_id == id).cloned();
    assert!(find(&got, "nd_0005").is_none());
    let (old3, new3) = (
        find(&before, "nd_0003").unwrap(),
        find(&got, "nd_0003").unwrap(),
    );
    assert_ne!(old3.chunks[0].hash, new3.chunks[0].hash);
    assert_eq!(new3.position, 500);
    let (old8, new8) = (
        find(&before, "nd_0008").unwrap(),
        find(&got, "nd_0008").unwrap(),
    );
    assert_eq!(new8.chunks.len(), old8.chunks.len() - 1);
    assert!(new8.chunks.iter().all(|c| c.chunk != 1));
}

/// A start that does not read the stored text is not troubled when it cannot: with every
/// segment's text overwritten, it still gives the table the text gave.
#[test]
fn the_start_does_not_touch_the_stored_text() {
    let tmp = tempfile::tempdir().unwrap();
    let want = {
        let (mut w, e) = open(tmp.path());
        fixture(&w, 100);
        commit(&mut w, &e);
        from_the_text(&e)
    };
    ruin_the_stored_text(tmp.path());
    let (_w, e) = open(tmp.path());
    assert_eq!(e.all_nodes().unwrap(), want);
}
