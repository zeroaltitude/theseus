//! A kept file's read text, indexed beside its node, once per digest
//! (theseus-lv2u).
//!
//! The file itself and what was read of it live in the store's blobs; the
//! node names them by digest. These tests write the blobs as the core
//! writes them, append the node, and ask the index for a marker that is in
//! the file's text alone.

use std::path::Path;

use theseus_core::node::{Attachment, AttachmentContent, Node};

use crate::proto::QueryParams;
use crate::tender::Tender;
use crate::tests::{settle, Rig};

/// An invented marker, in the fixture's text alone.
const MARKER: &str = "tidewheel-glossary-4417";

/// Write a blob into the store's blobs, where the core's `Blobs` keeps it:
/// `<store>/blobs/<sha256>`.
fn put_blob(store: &Path, bytes: &[u8]) -> String {
    let digest = theseus_core::blobs::digest(bytes);
    let dir = store.join("blobs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(&digest), bytes).unwrap();
    digest
}

/// A PDF kept whole, with its text by page in a blob of its own (a JSON
/// list of strings, as `theseus_core::blobs` writes it).
fn kept_pdf(store: &Path, name: &str, pages: &[String]) -> Attachment {
    let bytes = format!("%PDF-1.7 {name}").into_bytes();
    let size = bytes.len() as u64;
    let digest = put_blob(store, &bytes);
    let text = put_blob(store, &serde_json::to_vec(pages).unwrap());
    Attachment {
        name: name.into(),
        media_type: "application/pdf".into(),
        size,
        content: AttachmentContent::File {
            digest,
            pages: Some(pages.len() as u32),
            text: Some(text),
            parts: Vec::new(),
            unread: None,
        },
    }
}

fn pages() -> Vec<String> {
    vec![
        "The first page says little about the harbour.".into(),
        format!("The second page holds {MARKER} and the rest of the glossary."),
        "The third page is a table of tide times.".into(),
    ]
}

/// Every chunk in the index whose text holds `MARKER`.
fn marked(t: &Tender) -> Vec<(String, u64)> {
    t.shared()
        .engine
        .dump()
        .unwrap()
        .into_iter()
        .filter(|c| c.text.contains(MARKER))
        .map(|c| (c.node_id, c.chunk))
        .collect()
}

/// The defect (theseus-lv2u): a file the owner attaches is kept in the
/// blobs and the node names the blob, so what was read of it never reaches
/// the index. Ask in a later session and recall must still find the file's
/// node, and the hit must say which file and which page it came from.
#[test]
fn a_kept_files_text_is_found_later_and_the_hit_cites_the_file() {
    let rig = Rig::new();
    let a = kept_pdf(&rig.store, "harbour-notes.pdf", &pages());
    let node = Node::user_with(
        "ses_harbour",
        Some("turn_1"),
        "cli",
        "read this one",
        vec![a],
    );
    let id = node.id.clone();
    rig.put(&[node]);
    let mut t = rig.open();
    settle(&mut t);

    let r = t.shared().query(&QueryParams::new(MARKER)).unwrap();
    assert!(!r.hits.is_empty(), "no hit for {MARKER}");
    let hit = &r.hits[0];
    assert_eq!(hit.node_id, id, "the hit is the file's node");
    assert!(hit.text.contains(MARKER), "the hit's text: {:?}", hit.text);
    // What it cites, so a person can find the passage.
    assert!(
        hit.text.contains("harbour-notes.pdf"),
        "it cites the file: {:?}",
        hit.text
    );
    assert!(
        hit.text.contains("page 2"),
        "it cites the page: {:?}",
        hit.text
    );
}

/// Once per digest: the same bytes attached again, in another session, add
/// no second copy of the text. The count is asserted both ways, so an
/// index that took nothing cannot pass.
#[test]
fn the_same_file_attached_twice_is_indexed_once() {
    let rig = Rig::new();
    let first = Node::user_with(
        "ses_harbour",
        Some("turn_1"),
        "cli",
        "read this one",
        vec![kept_pdf(&rig.store, "harbour-notes.pdf", &pages())],
    );
    let first_id = first.id.clone();
    rig.put(&[first]);
    let mut t = rig.open();
    settle(&mut t);
    let one = marked(&t);
    assert_eq!(one.len(), 1, "one copy of the marked page: {one:?}");
    assert_eq!(one[0].0, first_id);

    // The same bytes again, in another session and under another name.
    let again = Node::user_with(
        "ses_jetty",
        Some("turn_2"),
        "cli",
        "and this one",
        vec![kept_pdf(&rig.store, "harbour-notes.pdf", &pages())],
    );
    let again_id = again.id.clone();
    rig.put(&[again]);
    settle(&mut t);

    let two = marked(&t);
    assert_eq!(two, one, "no second copy of the file's text: {two:?}");
    // And the second message itself is indexed, so this is not an empty index.
    assert!(
        t.shared().engine.holds(&again_id).unwrap(),
        "the second message is in the index"
    );
}

/// The cap (`files::MAX_BYTES`) is not implicit: the index stops, the chunk
/// that says so is in the index and findable, and `index.status` counts the
/// file as cut.
#[test]
fn a_file_over_the_cap_stops_there_and_says_what_it_left_out() {
    let rig = Rig::new();
    // Pages of about 4 KB each, 100 of them: 400 KB, well over the cap.
    let page = |n: usize| format!("page {n} of the ledger. {}", "harbourword ".repeat(300));
    let mut long: Vec<String> = (1..=100).map(page).collect();
    // A marker on the last page, which the cap must leave out.
    long.push(format!("the final page holds {MARKER}."));
    let node = Node::user_with(
        "ses_ledger",
        Some("turn_1"),
        "cli",
        "the ledger",
        vec![kept_pdf(&rig.store, "ledger.pdf", &long)],
    );
    rig.put(&[node]);
    let mut t = rig.open();
    settle(&mut t);

    let status = t.shared().status();
    assert_eq!(status.files_indexed, 1, "the file was taken");
    assert_eq!(status.files_cut, 1, "and it was cut");
    assert_eq!(status.files_unread, 0);

    // What it holds stops at the cap, with a little slack for the heads.
    let held: usize = t
        .shared()
        .engine
        .dump()
        .unwrap()
        .iter()
        .filter(|c| c.file.is_some())
        .map(|c| c.text.len())
        .sum();
    assert!(
        held > crate::files::MAX_BYTES / 2 && held < crate::files::MAX_BYTES * 2,
        "{held} bytes against a cap of {}",
        crate::files::MAX_BYTES
    );
    // The page past the cap is not in the index.
    assert!(
        t.shared()
            .query(&QueryParams::new(MARKER))
            .unwrap()
            .hits
            .is_empty(),
        "nothing past the cap is indexed"
    );
    // And the cut is findable and says what to do about it.
    let cut = t
        .shared()
        .engine
        .dump()
        .unwrap()
        .into_iter()
        .find(|c| c.text.contains("is not indexed"))
        .expect("the cut is a chunk of its own");
    assert!(cut.text.contains("ledger.pdf"), "{}", cut.text);
    assert!(cut.text.contains("file_read"), "{}", cut.text);
    assert!(cut.file.is_some(), "the cut is the file's own chunk");
}

/// A text blob that is gone (an old store, a blob never written) leaves the
/// node indexed without it, counted, and fails nothing.
#[test]
fn a_file_whose_text_blob_is_gone_leaves_its_node_indexed() {
    let rig = Rig::new();
    let a = kept_pdf(&rig.store, "missing.pdf", &pages());
    let AttachmentContent::File { text, .. } = &a.content else {
        panic!()
    };
    let text = text.clone().expect("the fixture read its text");
    std::fs::remove_file(rig.store.join("blobs").join(text)).unwrap();
    let node = Node::user_with("ses_gone", Some("turn_1"), "cli", "this one", vec![a]);
    let id = node.id.clone();
    rig.put(&[node]);
    let mut t = rig.open();
    settle(&mut t);

    let status = t.shared().status();
    assert_eq!(status.files_unread, 1);
    assert_eq!(status.files_indexed, 0);
    assert!(
        t.shared().engine.holds(&id).unwrap(),
        "the message itself is still indexed"
    );
    assert!(marked(&t).is_empty());
}

/// What one capped file costs the index on disk (the ripple theseus-lv2u
/// shares with the index's size question). It prints the measure and holds
/// a ceiling, so a change that makes a file far more expensive fails here.
/// The fixture's words are varied, not one word repeated, so the number is
/// not a compression artefact.
#[test]
fn one_capped_file_costs_the_index_under_three_megabytes() {
    let rig = Rig::new();
    let long: Vec<String> = (1..=100).map(|n| prose(n, 4000)).collect();
    let node = Node::user_with(
        "ses_size",
        Some("turn_1"),
        "cli",
        "the ledger",
        vec![kept_pdf(&rig.store, "ledger.pdf", &long)],
    );
    rig.put(&[node]);
    let mut t = rig.open();
    settle(&mut t);
    let bytes = dir_bytes(&t.paths().bm25());
    let chunks = t
        .shared()
        .engine
        .dump()
        .unwrap()
        .iter()
        .filter(|c| c.file.is_some())
        .count();
    println!("one capped file: {chunks} chunks, {bytes} bytes of BM25 index");
    assert!(chunks > 100, "{chunks} chunks");
    assert!(bytes < 3 << 20, "{bytes} bytes of BM25 index for one file");
}

/// About `bytes` of varied invented prose for page `n`: a cheap stand-in
/// for a document's words, so a size measure is not measuring compression.
fn prose(n: usize, bytes: usize) -> String {
    const STEMS: &[&str] = &[
        "tide", "jetty", "lantern", "keel", "quay", "spar", "halyard", "ballast", "cleat",
        "fathom", "gunwale", "mizzen", "bollard", "capstan", "dory", "thwart",
    ];
    let mut out = format!("page {n} of the ledger.");
    let mut x = (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    while out.len() < bytes {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let stem = STEMS[(x % STEMS.len() as u64) as usize];
        out.push_str(&format!(" {stem}{}", x % 9973));
    }
    out
}

fn dir_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}
