use super::*;
use crate::weights::SplitMix;
use std::os::unix::fs::PermissionsExt;

fn stamp(cut: usize, full: usize) -> Stamp {
    Stamp {
        model: "m@1".into(),
        weights: "w".into(),
        tokenizer: "t".into(),
        engine: "e".into(),
        precision: "f32".into(),
        dims: [cut, full],
    }
}

fn random_vector(rng: &mut SplitMix, full: usize, cut: usize) -> Vector {
    let raw: Vec<f32> = (0..full).map(|_| rng.unit()).collect();
    let (f, c) = embedder::finish(&raw, cut);
    Vector {
        full: f,
        cut: c,
        tokens: 1,
        windowed: false,
        truncated: false,
    }
}

/// A table of `n` rows over random vectors, with random positions,
/// sessions, kinds, and external flags; some rows dead, some without a
/// vector.
fn table(dir: &Path, n: usize, seed: u64) -> Table {
    let mut rng = SplitMix(seed);
    let mut t = Table::default();
    let mut cache = Cache::open(dir, &stamp(16, 48)).unwrap();
    let vs: Vec<Vector> = (0..n).map(|_| random_vector(&mut rng, 48, 16)).collect();
    let items: Vec<(u128, &Vector)> = vs.iter().enumerate().map(|(i, v)| (i as u128, v)).collect();
    cache.append(&items).unwrap();
    t.caches.push(cache);
    t.opened = true;
    for i in 0..n {
        let r = rng.next_u64();
        t.add_node(&NodeChunks {
            node_id: format!("nd_{i}"),
            position: r % 1000,
            session: format!("ses_{}", r % 5),
            kind: ["user_message", "tool_result"][(r >> 8) as usize % 2].into(),
            external: (r >> 16).is_multiple_of(3),
            // Every seventh row's text has no vector yet.
            chunks: vec![ChunkKey {
                chunk: 0,
                hash: if i % 7 == 6 {
                    u128::MAX - i as u128
                } else {
                    i as u128
                },
                tokens: 10,
            }],
        });
    }
    for i in (0..n).step_by(11) {
        t.remove_node(&format!("nd_{i}"));
    }
    t
}

#[test]
fn the_flat_scan_matches_a_naive_loop() {
    let tmp = tempfile::tempdir().unwrap();
    let t = table(tmp.path(), 500, 7);
    let mut rng = SplitMix(99);
    let filters = [
        (None, vec![], Filters::default()),
        (Some(500), vec![], Filters::default()),
        (None, vec!["ses_1".to_string()], Filters::default()),
        (
            Some(800),
            vec![],
            Filters {
                sessions: vec!["ses_2".into(), "ses_3".into()],
                kinds: vec!["tool_result".into()],
                external: Some(false),
                ..Filters::default()
            },
        ),
        (
            None,
            vec![],
            Filters {
                sessions: vec!["ses_nowhere".into()],
                ..Filters::default()
            },
        ),
    ];
    for (as_of, exclude, f) in &filters {
        for n in [1, 10, 100, 1000] {
            let q: Vec<i8> = (0..16).map(|_| (rng.next_u64() % 255) as i8).collect();
            let rf = t.filter(*as_of, exclude, f);
            let got = Flat(&t).top(&q, n, &|i| t.admits(&rf, i));
            // The naive loop: every admitted row with a vector, scored,
            // sorted by score and then row.
            let mut want: Vec<(usize, f32)> = (0..t.rows.len())
                .filter(|&i| t.rows[i].alive && t.admits(&rf, i))
                .filter_map(|i| {
                    let (c, e) = t.rows[i].vec()?;
                    let cache = &t.caches[c as usize];
                    Some((i, dot_i8(cache.q(e), &q) as f32 * cache.scale[e as usize]))
                })
                .collect();
            want.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            want.truncate(n);
            assert_eq!(got, want, "as_of {as_of:?}, exclude {exclude:?}, n {n}");
            for &(i, _) in &got {
                let r = &t.rows[i];
                assert!(as_of.is_none_or(|a| r.position < a));
                assert!(!exclude.contains(&t.sessions.name(r.session).to_string()));
            }
        }
    }
    // The unknown session admits nothing; the rest admit something.
    let none = t.filter(None, &[], &filters[4].2);
    assert!(Flat(&t)
        .top(&[1; 16], 10, &|i| t.admits(&none, i))
        .is_empty());
}

#[test]
fn the_re_score_orders_by_the_full_vectors_cosine() {
    let tmp = tempfile::tempdir().unwrap();
    let t = table(tmp.path(), 60, 3);
    let mut rng = SplitMix(5);
    let q = random_vector(&mut rng, 48, 16);
    let rf = t.filter(None, &[], &Filters::default());
    let got = t.search(&q.full, 16, &rf, 30).unwrap();
    // Fewer rows than RESCORE: every one with a vector is re-scored.
    let mut want: Vec<(usize, f64)> = (0..t.rows.len())
        .filter(|&i| t.rows[i].alive)
        .filter_map(|i| Some((i, embedder::cosine(&q.full, &t.full_of(i).unwrap()?))))
        .collect();
    want.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then(t.rows[a.0].position.cmp(&t.rows[b.0].position))
            .then(a.0.cmp(&b.0))
    });
    want.truncate(30);
    assert_eq!(got, want);
}

#[test]
fn a_vector_file_round_trips_and_its_torn_tail_is_cut() {
    let tmp = tempfile::tempdir().unwrap();
    let s = stamp(16, 48);
    let mut rng = SplitMix(1);
    let vs: Vec<Vector> = (0..5).map(|_| random_vector(&mut rng, 48, 16)).collect();
    let mut c = Cache::open(tmp.path(), &s).unwrap();
    let items: Vec<(u128, &Vector)> = vs
        .iter()
        .enumerate()
        .map(|(i, v)| (i as u128 + 10, v))
        .collect();
    assert_eq!(c.append(&items).unwrap(), vec![0, 1, 2, 3, 4]);
    drop(c);
    let path = tmp.path().join(format!("{}.vec", stamp_key(&s)));
    let len = fs::metadata(&path).unwrap().len();
    // A record whose bytes changed fails its check: it and all after go.
    let mut b = fs::read(&path).unwrap();
    let rec = rec_len(16, 48) as u64;
    b[(HEADER + 3 * rec + 30) as usize] ^= 1;
    fs::write(&path, &b).unwrap();
    let c = Cache::open(tmp.path(), &s).unwrap();
    assert_eq!(c.len(), 3);
    assert_eq!(fs::metadata(&path).unwrap().len(), HEADER + 3 * rec);
    for (i, v) in vs.iter().take(3).enumerate() {
        let e = c.get(i as u128 + 10).unwrap();
        let back = c.full(e).unwrap();
        assert!(embedder::cosine(&back, &v.full) > 0.999_99);
        assert_eq!(c.q(e), quantize(&v.cut).0.as_slice());
    }
    assert!(len > HEADER + 3 * rec);
    // A header that is not this format's starts the file over.
    let mut b = fs::read(&path).unwrap();
    b[0] = b'X';
    fs::write(&path, &b).unwrap();
    assert!(Cache::open(tmp.path(), &s).unwrap().is_empty());
    assert_eq!(fs::metadata(&path).unwrap().len(), HEADER);
}

#[test]
fn the_backlog_batches_short_texts_of_one_length_newest_first() {
    let mut b = Backlog::default();
    // (hash, tokens, position, answers)
    for (h, tok, pos, ans) in [
        (1, 20, 10, false),
        (2, 20, 50, false),
        (3, 300, 60, false),
        (4, 100, 40, false),
        (5, 20, 70, true),
        (6, 30, 5, false),
    ] {
        b.add(h, tok, pos, ans);
    }
    // Rows with no vector first, newest first: 3 (300 tokens) goes alone.
    assert_eq!(b.take(), vec![3]);
    // Then 2, with the other short ones of its bucket, newest first.
    assert_eq!(b.take(), vec![2, 1, 6]);
    // Then 4 (100 tokens), the only one of its bucket.
    assert_eq!(b.take(), vec![4]);
    // Last, the one an older stamp's vector answers for.
    assert_eq!(b.take(), vec![5]);
    assert!(b.take().is_empty());
    // Added again with a newer position, it moves up; removed, it goes.
    b.add(1, 20, 1, false);
    b.add(2, 20, 2, false);
    b.add(1, 20, 9, false);
    assert_eq!(b.order.first().map(|s| s.2), Some(1));
    b.remove(1);
    assert_eq!(b.take(), vec![2]);
}

#[test]
fn batches_are_planned_by_length() {
    assert_eq!(
        plan_batches(&[5, 600, 7, 128, 129, 6, 6, 6, 6, 6, 6, 6]),
        vec![vec![0, 5, 6, 7, 8, 9, 10, 11], vec![2, 3], vec![4], vec![1]]
    );
    assert!(plan_batches(&[]).is_empty());
}

/// The scan at the design's scale: 100,000 chunks at 256 int8 dimensions
/// (25.6 MB), and their 768-d vectors on disk. Prints the scan's and the
/// scan-and-re-score's p50 and p95; HNSW waits until the scan's p95
/// passes 10 ms (§2.2). Run in release:
/// `cargo test --release -p theseus-index --lib -- --ignored --nocapture bench_`.
#[test]
#[ignore = "a benchmark: 181 MB of vectors, timed (run in release)"]
fn bench_the_flat_scan_over_100k_chunks() {
    let n = 100_000;
    let tmp = tempfile::tempdir().unwrap();
    let mut rng = SplitMix(2026);
    let mut t = Table::default();
    let mut cache = Cache::open(tmp.path(), &stamp(256, 768)).unwrap();
    for start in (0..n).step_by(1000) {
        let vs: Vec<Vector> = (0..1000)
            .map(|_| random_vector(&mut rng, 768, 256))
            .collect();
        let items: Vec<(u128, &Vector)> = vs
            .iter()
            .enumerate()
            .map(|(i, v)| ((start + i) as u128, v))
            .collect();
        cache.append(&items).unwrap();
    }
    t.caches.push(cache);
    t.opened = true;
    for i in 0..n {
        t.add_node(&NodeChunks {
            node_id: format!("nd_{i}"),
            position: i as u64,
            session: format!("ses_{}", i % 50),
            kind: "tool_result".into(),
            external: false,
            chunks: vec![ChunkKey {
                chunk: 0,
                hash: i as u128,
                tokens: 100,
            }],
        });
    }
    let time = |label: &str, f: &mut dyn FnMut()| {
        let mut ms: Vec<f64> = (0..40)
            .map(|_| {
                let t0 = Instant::now();
                f();
                t0.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        eprintln!(
            "{label}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            ms[ms.len() / 2],
            ms[ms.len() * 95 / 100],
            ms[ms.len() - 1]
        );
    };
    let q = random_vector(&mut rng, 768, 256);
    let (qi, _) = quantize(&q.cut);
    let all = t.filter(None, &[], &Filters::default());
    let half = t.filter(Some(n as u64 / 2), &["ses_7".into()], &Filters::default());
    eprintln!("{n} chunks; int8 cuts {} MB in memory", n * 256 / 1_000_000);
    time("the int8 scan, best 100, no filter", &mut || {
        assert_eq!(
            Flat(&t).top(&qi, RESCORE, &|i| t.admits(&all, i)).len(),
            RESCORE
        );
    });
    time(
        "the int8 scan, best 100, as_of and an excluded session",
        &mut || {
            assert_eq!(
                Flat(&t).top(&qi, RESCORE, &|i| t.admits(&half, i)).len(),
                RESCORE
            );
        },
    );
    time(
        "the scan and the 768-d re-score of its best 100",
        &mut || {
            assert_eq!(t.search(&q.full, 256, &all, 30).unwrap().len(), 30);
        },
    );
}

/// A file of 40 records (hashes 0..40), and the bytes a compaction that
/// keeps the even hashes must leave.
fn forty(dir: &Path) -> (Cache, Vec<u8>, Vec<u8>) {
    let mut rng = SplitMix(64);
    let mut c = Cache::open(dir, &stamp(16, 48)).unwrap();
    let vs: Vec<Vector> = (0..40).map(|_| random_vector(&mut rng, 48, 16)).collect();
    let items: Vec<(u128, &Vector)> = vs.iter().enumerate().map(|(i, v)| (i as u128, v)).collect();
    c.append(&items).unwrap();
    let path = dir.join(format!("{}.vec", stamp_key(&stamp(16, 48))));
    let old = fs::read(&path).unwrap();
    let rec = c.rec;
    let mut new = old[..HEADER as usize].to_vec();
    for e in (0..40).step_by(2) {
        let at = HEADER as usize + e * rec;
        new.extend_from_slice(&old[at..at + rec]);
    }
    (c, old, new)
}

fn even(h: u128) -> bool {
    h.is_multiple_of(2)
}

/// theseus-64x: a compaction stopped at each of its steps, as a crash
/// would stop it, leaves the old file whole (the copy unfinished, or
/// whole but not renamed) or the new one whole (renamed), never a part of
/// either; the next open removes a copy left behind and reads every
/// record of whichever file it finds. Uninterrupted, the file is exactly
/// the kept records, and every record still reads back.
#[test]
fn a_crash_during_the_rewrite_leaves_the_old_file_or_the_new_never_a_torn_one() {
    for (stop, want_new) in [
        (Step::HalfCopied, false),
        (Step::Copied, false),
        (Step::Renamed, true),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let (mut c, old, new) = forty(tmp.path());
        let path = c.paths().0;
        let copy = path.with_extension("vec.tmp");
        let err = c
            .rewrite(&even, &mut |s| {
                if s == stop {
                    Err(io::Error::other("killed (test)"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert!(format!("{err:#}").contains("killed"), "{stop:?}: {err:#}");
        drop(c);
        let on_disk = fs::read(&path).unwrap();
        assert!(
            on_disk == if want_new { new } else { old },
            "{stop:?}: the file is neither the old one nor the new"
        );
        // The copy is left where a crash before the rename leaves it.
        assert_eq!(copy.exists(), !want_new, "{stop:?}");
        let again = Cache::open(tmp.path(), &stamp(16, 48)).unwrap();
        assert!(!copy.exists(), "{stop:?}: the open removes the copy");
        let mut hashes: Vec<u128> = again.hashes.clone();
        hashes.sort_unstable();
        let want: Vec<u128> = (0..40).filter(|h| !want_new || even(*h)).collect();
        assert_eq!(hashes, want, "{stop:?}");
        assert_eq!(fs::read(&path).unwrap(), on_disk, "{stop:?}: nothing cut");
    }

    // Uninterrupted: exactly the kept records, in memory as on disk.
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, old, new) = forty(tmp.path());
    let full_before: Vec<Vec<f32>> = (0..40).map(|e| c.full(e).unwrap()).collect();
    let done = c.compact(&even).unwrap();
    assert_eq!(
        done,
        Compacted {
            files: 1,
            dropped: 20,
            bytes_before: old.len() as u64,
            bytes_after: new.len() as u64,
        }
    );
    let path = c.paths().0;
    assert_eq!(fs::read(&path).unwrap(), new);
    assert!(!path.with_extension("vec.tmp").exists());
    for h in (0..40u128).step_by(2) {
        let e = c.get(h).unwrap();
        assert_eq!(c.full(e).unwrap(), full_before[h as usize]);
    }
    assert!((1..40u128).step_by(2).all(|h| c.get(h).is_none()));
    // The copy was made 0600, as the file is private.
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // Keeping everything rewrites nothing; an append after a compaction
    // goes to the new file.
    assert_eq!(c.compact(&|_| true).unwrap(), Compacted::default());
    let mut rng = SplitMix(5);
    let v = random_vector(&mut rng, 48, 16);
    c.append(&[(1000, &v)]).unwrap();
    let again = Cache::open(tmp.path(), &stamp(16, 48)).unwrap();
    assert_eq!(again.len(), 21);
    assert!(again.get(1000).is_some());
}

/// A text that loses its last row is dead at once, in every file that
/// holds it, and alive again when a row holds it again; a recount from
/// the rows agrees with the marks.
#[test]
fn a_record_is_dead_once_no_row_holds_its_text() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = table(tmp.path(), 60, 3);
    t.reconciled = true;
    t.caught_up = true;
    t.recount();
    let marks = |t: &Table| -> Vec<bool> { t.caches[0].dead.clone() };
    let counted = marks(&t);
    // Rows removed by `table` (every 11th) left their texts dead.
    for e in (0..60).step_by(11) {
        assert!(counted[e], "{e}");
    }
    let (n, dead) = t.records();
    assert_eq!((n, dead), (60, counted.iter().filter(|d| **d).count()));
    // A node removed now: its record dies at once.
    assert!(!t.caches[0].dead[5]);
    t.remove_node("nd_5");
    assert!(t.caches[0].dead[5]);
    // Its text said again by another node: alive again.
    t.add_node(&NodeChunks {
        node_id: "nd_again".into(),
        position: 1,
        session: "ses_1".into(),
        kind: "user_message".into(),
        external: false,
        chunks: vec![ChunkKey {
            chunk: 0,
            hash: 5,
            tokens: 10,
        }],
    });
    assert!(!t.caches[0].dead[5]);
    // One chunk of a node goes alone.
    assert_eq!(t.remove_chunk("nd_again", 0), Some(5));
    assert!(t.caches[0].dead[5] && !t.by_node.contains_key("nd_again"));
    assert_eq!(t.remove_chunk("nd_none", 0), None);
    // Incremental marks and a recount from the rows agree.
    let incremental = marks(&t);
    t.recount();
    assert_eq!(marks(&t), incremental);
}

/// theseus-64x at the design's scale: a file of 100,000 records at
/// [256, 768] (181.6 MB) compacted with a quarter of them dead, which
/// holds the table's write lock (a vector query waits that long); and
/// the open that reads it. Records are written raw (each with its CRC),
/// so building the file costs no model. Prints the times. Run with
/// `cargo nextest run --workspace --run-ignored only --no-capture -E
/// 'test(bench_a_compaction)'` (a debug build: the copy is I/O).
#[test]
#[ignore = "a benchmark: 182 MB written, compacted, timed"]
fn bench_a_compaction_of_100k_records() {
    let tmp = tempfile::tempdir().unwrap();
    let st = stamp(256, 768);
    let n = 100_000usize;
    {
        let c = Cache::open(tmp.path(), &st).unwrap();
        let mut rng = SplitMix(11);
        let mut w = io::BufWriter::with_capacity(1 << 20, &c.file);
        let mut rec = vec![0u8; c.rec];
        for i in 0..n {
            rec[..16].copy_from_slice(&(i as u128).to_le_bytes());
            for b in rec[16..c.rec - 4].iter_mut() {
                *b = (rng.next_u64() & 0x3f) as u8;
            }
            let at = c.rec - 4;
            let crc = crc32fast::hash(&rec[..at]);
            rec[at..].copy_from_slice(&crc.to_le_bytes());
            w.write_all(&rec).unwrap();
        }
        w.flush().unwrap();
    }
    let t0 = Instant::now();
    let mut c = Cache::open(tmp.path(), &st).unwrap();
    let open = t0.elapsed();
    assert_eq!(c.len(), n);
    let t1 = Instant::now();
    let done = c.compact(&|h| !h.is_multiple_of(4)).unwrap();
    let compact = t1.elapsed();
    assert_eq!(done.dropped, n as u64 / 4);
    eprintln!(
        "{n} records, {:.1} MB: the open {:.0} ms; a compaction dropping a quarter {:.0} ms, {:.1} MB left",
        done.bytes_before as f64 / 1e6,
        open.as_secs_f64() * 1e3,
        compact.as_secs_f64() * 1e3,
        done.bytes_after as f64 / 1e6,
    );
}
