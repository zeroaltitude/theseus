//! The vector table over its mapped files (issue 21 of 1.0.7): the rows in a
//! file with no name (`rows.rs`) and the records' cuts in a scan file
//! (`scan.rs`) answer exactly as the same table held in memory does, what
//! changes after they are written is kept, and an index written before them
//! opens and is converted by itself.

use std::os::unix::fs::MetadataExt;

use super::*;
use crate::weights::SplitMix;

pub(super) const CUT: usize = 16;
pub(super) const FULL: usize = 48;

pub(super) fn stamp() -> Stamp {
    Stamp {
        model: "m@1".into(),
        weights: "w".into(),
        tokenizer: "t".into(),
        engine: "e".into(),
        precision: "f32".into(),
        dims: [CUT, FULL],
    }
}

pub(super) fn vector(rng: &mut SplitMix) -> Vector {
    let raw: Vec<f32> = (0..FULL).map(|_| rng.unit()).collect();
    let (full, cut) = embedder::finish(&raw, CUT);
    Vector {
        full,
        cut,
        tokens: 1,
        windowed: false,
        truncated: false,
    }
}

/// Node `i`: two or three chunks, a text in every fifth shared with another node's, and
/// positions, sessions, kinds and flags that vary.
pub(super) fn node(i: u64) -> NodeChunks {
    NodeChunks {
        node_id: format!("nd_{i:08}"),
        position: i * 3 % 1000,
        time_ms: 1_000 * (i * 7 % 500),
        session: format!("ses_{}", i % 9),
        kind: ["user_message", "tool_result", "assistant_message"][(i % 3) as usize].into(),
        external: i.is_multiple_of(4),
        chunks: (0..2 + i % 2)
            .map(|c| ChunkKey {
                chunk: c as u32,
                hash: if i.is_multiple_of(5) && c == 0 {
                    u128::from(i % 50)
                } else {
                    u128::from(i) << 8 | u128::from(c) | 1 << 100
                },
                tokens: 40,
            })
            .collect(),
    }
}

pub(super) fn nodes(n: u64) -> Vec<NodeChunks> {
    (0..n).map(node).collect()
}

/// The vector file of every text in `nodes` but each tenth's.
pub(super) fn write_file(dir: &Path, nodes: &[NodeChunks], seed: u64) {
    let mut rng = SplitMix(seed);
    let mut cache = Cache::open(dir, &stamp()).unwrap();
    let mut seen = HashSet::new();
    let mut items: Vec<(u128, Vector)> = Vec::new();
    for (k, n) in nodes.iter().enumerate() {
        for c in &n.chunks {
            if k % 10 != 7 && seen.insert(c.hash) {
                items.push((c.hash, vector(&mut rng)));
            }
        }
    }
    for part in items.chunks(500) {
        let refs: Vec<(u128, &Vector)> = part.iter().map(|(h, v)| (*h, v)).collect();
        cache.append(&refs).unwrap();
    }
}

/// The table a tender with no directory to write in keeps: every row in memory.
fn in_memory(dir: &Path, nodes: &[NodeChunks]) -> Table {
    let mut t = Table::default();
    t.caches.push(Cache::open(dir, &stamp()).unwrap());
    t.opened = true;
    for n in nodes {
        t.add_node(n);
    }
    t
}

/// The table a start makes: the vector file's cuts from a scan file, the rows streamed into a
/// file of their own.
pub(super) fn mapped(dir: &Path, nodes: &[NodeChunks]) -> Table {
    let mut t = Table {
        dir: Some(dir.to_path_buf()),
        ..Table::default()
    };
    t.caches.push(Cache::open_scanned(dir, &stamp()).unwrap());
    t.opened = true;
    t.start_rows(
        nodes.len(),
        nodes.iter().map(|n| n.chunks.len()).sum(),
        true,
    );
    assert!(t.is_building(), "a file for the rows");
    for part in nodes.chunks(40) {
        t.write_nodes(&mut part.to_vec()).unwrap();
    }
    t.finish_rows().unwrap();
    t
}

/// What `t` answers to `n` queries under each filter, as lines: node, chunk, position, and the
/// score's bits.
fn answers(t: &Table, n: u64) -> Vec<String> {
    let mut rng = SplitMix(5);
    let filters: Vec<(Option<u64>, Vec<String>, Filters)> = vec![
        (None, vec![], Filters::default()),
        (Some(500), vec![], Filters::default()),
        (None, vec!["ses_3".into()], Filters::default()),
        (
            None,
            vec![],
            Filters {
                sessions: vec!["ses_1".into(), "ses_4".into()],
                kinds: vec!["tool_result".into(), "user_message".into()],
                external: Some(false),
                ..Filters::default()
            },
        ),
    ];
    let mut out = Vec::new();
    for q in 0..n {
        let raw: Vec<f32> = (0..FULL).map(|_| rng.unit()).collect();
        let full = embedder::l2_normalize(&raw);
        let (as_of, exclude, f) = &filters[q as usize % filters.len()];
        let rf = t.filter(*as_of, exclude, f);
        for (i, score) in t.search(&full, CUT, &rf, 25).unwrap() {
            let r = t.row(i as u32);
            out.push(format!(
                "q{q} {} {} {} {:016x}",
                r.node,
                r.chunk,
                r.position,
                score.to_bits()
            ));
        }
    }
    out
}

fn counts(t: &Table) -> (usize, usize, usize) {
    let (records, dead) = t.records();
    (t.alive_vecs().count(), records, dead)
}

/// The same texts, the same rows in the same order: a table over its files answers every
/// query, under every filter, to the last bit of the score, as the table in memory does.
#[test]
fn the_files_answer_as_the_table_in_memory_does() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(900);
    write_file(tmp.path(), &all, 3);
    let want = answers(&in_memory(tmp.path(), &all), 24);
    let t = mapped(tmp.path(), &all);
    assert!(
        t.base.is_some() && t.rows.is_empty(),
        "every row is in the file"
    );
    assert!(t.caches[0].scan.is_some(), "the cuts are in a scan file");
    assert!(want.len() > 400, "the fixture answers: {}", want.len());
    assert_eq!(answers(&t, 24), want);
    let mem = in_memory(tmp.path(), &all);
    assert_eq!(counts(&t).0, counts(&mem).0);
}

/// What a text's holders, a node's rows and a row's whole are, from the file.
#[test]
fn the_files_look_up_as_the_table_in_memory_does() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    write_file(tmp.path(), &all, 4);
    let mem = in_memory(tmp.path(), &all);
    let t = mapped(tmp.path(), &all);
    for n in &all {
        assert_eq!(
            t.rows_of(&n.node_id),
            mem.rows_of(&n.node_id),
            "{}",
            n.node_id
        );
    }
    assert!(t.rows_of("nd_absent").is_empty());
    for h in [0u128, 5, 49, 50, 1 << 100 | 1 << 8, u128::MAX] {
        assert_eq!(t.holders(h), mem.holders(h), "{h}");
        assert_eq!(t.holds(h), mem.holds(h));
    }
    for i in 0..mem.row_count() as u32 {
        let (a, b) = (t.row(i), mem.row(i));
        assert_eq!(
            (&a.node, a.chunk, a.position, a.session, a.kind, a.external, a.hash, a.tokens),
            (&b.node, b.chunk, b.position, b.session, b.kind, b.external, b.hash, b.tokens)
        );
        assert_eq!(a.vec(), b.vec());
    }
}

/// A node that arrives after the file was written (a commit) is in the tail and answers; one
/// written again replaces the file's rows; a forget takes a file's node and a file's chunk.
#[test]
fn what_changes_after_the_file_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(400);
    write_file(tmp.path(), &all, 5);
    let (mut t, mut m) = (mapped(tmp.path(), &all), in_memory(tmp.path(), &all));
    let cache_both = |t: &mut Table, hashes: &[u128], rng: &mut SplitMix| {
        let vs: Vec<Vector> = hashes.iter().map(|_| vector(rng)).collect();
        let items: Vec<(u128, &Vector)> = hashes.iter().copied().zip(&vs).collect();
        let es = t.caches[0].append(&items).unwrap();
        for (h, e) in hashes.iter().zip(es) {
            for i in t.holders(*h) {
                t.set_row_vec(i, Some((0, e)));
            }
        }
    };
    for t in [&mut t, &mut m] {
        let mut fresh = node(1000);
        fresh.chunks[0].hash = 77 << 8 | 1 << 101;
        t.add_node(&fresh);
        let mut again = node(10);
        again.chunks[0].hash = 78 << 8 | 1 << 101;
        t.add_node(&again);
        t.remove_node("nd_00000050");
        t.remove_chunk("nd_00000060", 1);
    }
    assert!(t
        .rows_of("nd_00001000")
        .iter()
        .all(|&i| t.in_tail(i).is_some()));
    assert!(t.rows_of("nd_00000050").is_empty());
    assert_eq!(
        t.rows_of("nd_00000060").len(),
        m.rows_of("nd_00000060").len()
    );
    // The new texts get vectors: the rows that hold them answer.
    let new = [77u128 << 8 | 1 << 101, 78 << 8 | 1 << 101];
    cache_both(&mut t, &new, &mut SplitMix(6));
    cache_both(&mut m, &new, &mut SplitMix(6));
    assert_eq!(answers(&t, 20), answers(&m, 20));
    assert_eq!(counts(&t).0, counts(&m).0);
}

/// Sixteen thousand rows in the tail are written into the file, and the table answers the same
/// before and after, with the dead rows gone and every row's vector kept.
#[test]
fn a_long_tail_is_written_into_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    write_file(tmp.path(), &all, 7);
    let mut t = mapped(tmp.path(), &all);
    t.remove_node("nd_00000100");
    let before_base = t.base_len();
    let mut next = 5_000u64;
    while t.rows.len() < rows::FOLD_ROWS - 3 {
        t.add_node(&node(next));
        next += 1;
    }
    assert_eq!(t.base_len(), before_base, "not yet");
    let mem = {
        let mut m = in_memory(tmp.path(), &all);
        m.remove_node("nd_00000100");
        for i in 5_000..next {
            m.add_node(&node(i));
        }
        m
    };
    let want = answers(&mem, 12);
    assert_eq!(answers(&t, 12), want, "with a long tail");
    let mut end = next;
    while t.base_len() == before_base {
        t.add_node(&node(end));
        end += 1;
    }
    assert!(t.rows.is_empty(), "the tail was written into the file");
    assert!(t.base_len() > before_base);
    let mut mem = mem;
    for i in next..end {
        mem.add_node(&node(i));
    }
    assert_eq!(answers(&t, 12), answers(&mem, 12), "after it");
    assert_eq!(counts(&t).0, counts(&mem).0);
    assert!(t.rows_of("nd_00000100").is_empty());
}

/// The same in memory per chunk as nothing like the table in memory: the glibc heap a table over
/// its files takes is a few bytes a chunk, where the table in memory took five hundred.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn the_files_hold_almost_nothing_on_the_heap() {
    fn heap() -> usize {
        // SAFETY: no pointers.
        let m = unsafe { libc::mallinfo2() };
        m.uordblks + m.hblkhd
    }
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(20_000);
    write_file(tmp.path(), &all, 8);
    let chunks: usize = all.iter().map(|n| n.chunks.len()).sum();
    let before = heap();
    let t = mapped(tmp.path(), &all);
    let used = heap().saturating_sub(before);
    let per = used as f64 / chunks as f64;
    eprintln!("{chunks} chunks, {used} bytes of heap: {per:.1} bytes a chunk");
    assert!(t.base.is_some());
    assert!(per < 60.0, "{per:.1} bytes of heap a chunk");
    let in_mem = {
        let before = heap();
        let m = in_memory(tmp.path(), &all);
        let used = heap().saturating_sub(before);
        drop(m);
        used as f64 / chunks as f64
    };
    assert!(
        in_mem > 5.0 * per,
        "{in_mem:.1} in memory, {per:.1} over files"
    );
}

/// A scan file made from the records, then used as it is by the next open; a cache opened without
/// one answers the same.
#[test]
fn a_scan_file_is_made_once_and_used_again() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(tmp.path(), &nodes(500), 9);
    let plain = Cache::open(tmp.path(), &stamp()).unwrap();
    assert!(plain.scan.is_none());
    let first = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert!(
        first.scan.is_some() && first.scale.is_empty(),
        "every record is in the file"
    );
    let path = first.scan_path();
    let made = fs::metadata(&path).unwrap();
    drop(first);
    let again = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().ino(),
        made.ino(),
        "the file was used, not made again"
    );
    assert_eq!(again.len(), plain.len());
    for e in 0..plain.len() as u32 {
        assert_eq!(again.hash_of(e), plain.hash_of(e));
        assert_eq!(again.scale_of(e).to_bits(), plain.scale_of(e).to_bits());
        assert_eq!(again.q(e), plain.q(e));
        assert_eq!(again.get(plain.hash_of(e)), plain.get(plain.hash_of(e)));
    }
    assert_eq!(again.get(u128::MAX - 1), None);
}

/// An index written before scan files existed has none: it opens, and the file is made from the
/// records, with nothing embedded again.
#[test]
fn an_index_from_before_opens_and_is_converted() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    write_file(tmp.path(), &all, 10);
    let vec_bytes = fs::read(tmp.path().join(format!("{}.vec", stamp_key(&stamp())))).unwrap();
    assert!(!tmp
        .path()
        .join(format!("{}.scan", stamp_key(&stamp())))
        .exists());
    let want = answers(&in_memory(tmp.path(), &all), 8);
    let t = mapped(tmp.path(), &all);
    assert_eq!(answers(&t, 8), want);
    assert!(tmp
        .path()
        .join(format!("{}.scan", stamp_key(&stamp())))
        .exists());
    assert_eq!(
        fs::read(tmp.path().join(format!("{}.vec", stamp_key(&stamp())))).unwrap(),
        vec_bytes,
        "the vector file was not touched"
    );
}

/// What was appended after the scan file was made is read into memory and answers, and is written
/// into a new scan file once there are enough.
#[test]
fn records_appended_after_the_scan_file_are_kept_and_folded() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(tmp.path(), &nodes(200), 11);
    let mut c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    let base = c.len();
    let mut rng = SplitMix(12);
    let vs: Vec<Vector> = (0..50).map(|_| vector(&mut rng)).collect();
    let items: Vec<(u128, &Vector)> = vs
        .iter()
        .enumerate()
        .map(|(i, v)| (u128::from(i as u64) | 1 << 110, v))
        .collect();
    let es = c.append(&items).unwrap();
    assert_eq!(es[0] as usize, base);
    assert_eq!(c.scale.len(), 50, "in memory");
    drop(c);
    // A reopen finds them after the scan file's own.
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert_eq!(
        (c.len(), c.scan.as_ref().map(|s| s.len())),
        (base + 50, Some(base))
    );
    assert_eq!(c.get(1 << 110), Some(base as u32));
    assert_eq!(c.q(base as u32), quantize(&vs[0].cut).0.as_slice());
    // Enough of them, and they are written into a new scan file.
    let mut c = c;
    let ino = fs::metadata(c.scan_path()).unwrap().ino();
    let more: Vec<Vector> = (0..FOLD).map(|_| vector(&mut rng)).collect();
    let items: Vec<(u128, &Vector)> = more
        .iter()
        .enumerate()
        .map(|(i, v)| (u128::from(i as u64) | 1 << 111, v))
        .collect();
    c.append(&items).unwrap();
    assert!(
        c.scale.is_empty(),
        "written out: {} in memory",
        c.scale.len()
    );
    assert_eq!(c.scan.as_ref().map(|s| s.len()), Some(base + 50 + FOLD));
    assert_ne!(
        fs::metadata(c.scan_path()).unwrap().ino(),
        ino,
        "a new file, not the old one written over"
    );
    assert_eq!(c.get(1 << 111 | 3), Some((base + 50 + 3) as u32));
}

/// A scan file that is not the one these records made is made again: the vector file replaced by
/// another's (a compaction a crash cut short), a scan file cut short, one of other dimensions.
#[test]
fn a_scan_file_that_is_not_theirs_is_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(200);
    write_file(tmp.path(), &all, 13);
    let key = stamp_key(&stamp());
    let scan = tmp.path().join(format!("{key}.scan"));
    drop(Cache::open_scanned(tmp.path(), &stamp()).unwrap());
    let good = fs::read(&scan).unwrap();
    // Cut short.
    fs::write(&scan, &good[..good.len() - 100]).unwrap();
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert!(c.scan.is_some() && c.scale.is_empty());
    assert_eq!(fs::read(&scan).unwrap().len(), good.len());
    drop(c);
    // Another vector file under the same name: its records are not the scan file's.
    let other = tempfile::tempdir().unwrap();
    write_file(other.path(), &nodes(200), 14);
    fs::copy(
        other.path().join(format!("{key}.vec")),
        tmp.path().join(format!("{key}.vec")),
    )
    .unwrap();
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    let plain = Cache::open(tmp.path(), &stamp()).unwrap();
    for e in 0..plain.len() as u32 {
        assert_eq!(c.q(e), plain.q(e), "entry {e}");
        assert_eq!(c.hash_of(e), plain.hash_of(e));
    }
    // Garbage where the header is.
    drop(c);
    fs::write(&scan, vec![7u8; good.len()]).unwrap();
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert!(c.scan.is_some());
    assert_eq!(c.get(plain.hash_of(5)), Some(5));
}

/// A search that is running keeps the file it opened: the scan file replaced by name (a rename
/// over it, as a fold and a compaction do) does not change what a held cache answers, and a fold
/// writes a new file rather than into the one that is mapped.
#[test]
fn a_file_replaced_under_a_cache_does_not_change_its_answers() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    write_file(tmp.path(), &all, 15);
    let mut t = mapped(tmp.path(), &all);
    let want = answers(&t, 8);
    let scan = t.caches[0].scan_path();
    let junk = tmp.path().join("junk");
    fs::write(&junk, vec![0xAAu8; 4096]).unwrap();
    fs::rename(&junk, &scan).unwrap();
    assert_eq!(
        answers(&t, 8),
        want,
        "the mapped file is the one that was opened"
    );
    t.caches[0].fold_scan();
    assert_eq!(answers(&t, 8), want, "after a fold");
    assert!(
        fs::metadata(&scan).unwrap().len() > 4096,
        "the fold's file has its name"
    );
}

/// Compacting a file that has a scan file: the records kept are in a new scan file, in order, and
/// the table answers as it did over what is left.
#[test]
fn a_compaction_keeps_the_scan_file_in_step() {
    let tmp = tempfile::tempdir().unwrap();
    let all = nodes(300);
    write_file(tmp.path(), &all, 16);
    let mut t = mapped(tmp.path(), &all);
    for i in (0..300).step_by(3) {
        t.remove_node(&format!("nd_{i:08}"));
    }
    t.recount();
    let (_, records, dead) = counts(&t);
    assert!(dead > 0, "{records} records, {dead} dead");
    let ino = fs::metadata(t.caches[0].scan_path()).unwrap().ino();
    let before = answers(&t, 10);
    let r = Vectors::compact_files(&mut t, tmp.path(), &|t, h| !t.holds(h)).unwrap();
    assert!(r.dropped > 0);
    assert_eq!(counts(&t).1, records - dead);
    assert_eq!(answers(&t, 10), before);
    assert_ne!(fs::metadata(t.caches[0].scan_path()).unwrap().ino(), ino);
    // And a reopen reads the new scan file whole.
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert_eq!((c.len(), c.scale.len()), (records - dead, 0));
}

/// A forget compacts every stamp's file, another space's too (opened plain, not scanned): the
/// records it drops are in no scan file beside it afterwards.
#[test]
fn a_forgotten_record_is_in_no_scan_file() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(tmp.path(), &nodes(200), 17);
    let key = stamp_key(&stamp());
    let (vec, scan) = (
        tmp.path().join(format!("{key}.vec")),
        tmp.path().join(format!("{key}.scan")),
    );
    drop(Cache::open_scanned(tmp.path(), &stamp()).unwrap());
    let plain = Cache::open(tmp.path(), &stamp()).unwrap();
    let records = plain.len();
    let gone = plain.hash_of(7).to_le_bytes();
    let has = |path: &Path| {
        fs::read(path)
            .map(|b| b.windows(16).any(|w| w == gone))
            .unwrap_or(false)
    };
    assert!(has(&vec) && has(&scan), "both files hold it to begin with");
    let mut c = Cache::open(tmp.path(), &stamp()).unwrap();
    let r = c.compact(&|h| h.to_le_bytes() != gone).unwrap();
    assert_eq!(r.dropped, 1);
    assert!(!has(&vec), "dropped from the vector file");
    assert!(!has(&scan), "and from the scan file beside it");
    // Opened scanned again, the scan file is made from what is left.
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert!(c.scan.is_some());
    assert_eq!(c.len(), records - 1);
    assert!(!has(&scan));
}

/// A scan file whose entry table names an entry the file does not have is made again, not trusted.
#[test]
fn a_scan_file_with_a_bad_entry_table_is_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    write_file(tmp.path(), &nodes(200), 18);
    let key = stamp_key(&stamp());
    let scan = tmp.path().join(format!("{key}.scan"));
    drop(Cache::open_scanned(tmp.path(), &stamp()).unwrap());
    let mut bytes = fs::read(&scan).unwrap();
    // The entry table is the file's last section; its last slot gets an entry past the file's.
    let end = bytes.len();
    bytes[end - 4..].copy_from_slice(&1_000_000u32.to_le_bytes());
    fs::write(&scan, &bytes).unwrap();
    let c = Cache::open_scanned(tmp.path(), &stamp()).unwrap();
    assert!(c.scan.is_some() && c.scale.is_empty());
    let plain = Cache::open(tmp.path(), &stamp()).unwrap();
    for e in 0..plain.len() as u32 {
        assert_eq!(c.get(plain.hash_of(e)), plain.get(plain.hash_of(e)));
    }
    assert_ne!(fs::read(&scan).unwrap(), bytes, "made again");
}
