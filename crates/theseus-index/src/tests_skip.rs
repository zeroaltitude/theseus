//! A frame that passes its checksum but is wrong (issue 6): the core serves on past it, so the
//! tender skips it, says so in `index.status` (and so in health), and indexes what follows,
//! where it used to stall for good with the same error at every step.

use std::os::unix::fs::FileExt;

use theseus_store::wal;

use crate::proto::QueryParams;
use crate::tests::{settle, user, Rig};

/// Rewrite the `n`th frame of segment 1 (from 0) as one that checks and is wrong: its length
/// kept, its one record's bytes garbage, its crc recomputed.
fn plant_wrong_frame(rig: &Rig, n: usize) {
    let path = wal::segment_path(&rig.store.join("wal"), 1);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut at = 0usize;
    for _ in 0..n {
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        at += wal::FRAME_HEADER + len;
    }
    let layout = wal::Layout::at(&bytes, at).unwrap();
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
    let body = at + wal::FRAME_HEADER..at + wal::FRAME_HEADER + len;
    let mut wrong = bytes[body.clone()].to_vec();
    let count_at = layout.count_at();
    wrong[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    for b in &mut wrong[count_at + 4..] {
        *b = 0xFF;
    }
    file.write_all_at(&wrong, body.start as u64).unwrap();
    file.write_all_at(&layout.crc(&wrong).to_le_bytes(), at as u64 + 8)
        .unwrap();
}

fn found(t: &crate::tender::Tender, text: &str) -> Vec<String> {
    t.shared()
        .query(&QueryParams::new(text))
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.node_id)
        .collect()
}

/// KEY: the middle frame is wrong; the nodes before and after it are indexed, the tender is not
/// stalled, and the status counts the skip and says where and why.
#[test]
fn a_frame_that_checks_but_is_wrong_is_skipped_and_said() {
    let rig = Rig::new();
    let nodes = [
        user("ses_a", "the alpha tide table"),
        user("ses_b", "the bravo shed key"),
        user("ses_c", "the charlie reef chart"),
    ];
    rig.put(&nodes);
    plant_wrong_frame(&rig, 1);
    let mut t = rig.open();
    settle(&mut t);
    assert!(found(&t, "alpha tide").contains(&nodes[0].id));
    assert!(
        found(&t, "charlie reef").contains(&nodes[2].id),
        "read on past it"
    );
    assert!(
        found(&t, "bravo shed").is_empty(),
        "the wrong frame's node is not there"
    );
    let status = t.shared().status();
    assert_ne!(status.state, "stalled", "{status:?}");
    assert_eq!(status.frames_skipped, 1);
    let said = status.last_frame_skipped.expect("it says where and why");
    assert!(said.starts_with("segment 1, offset "), "{said}");
    assert!(said.contains("did not decode"), "{said}");
    assert_eq!(status.last_error, None, "no error: it went on");
}
