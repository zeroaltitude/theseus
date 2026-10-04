use super::*;
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

/// Wait until `n` appends are queued for the writer.
fn until_queued(s: &WalStore, n: i64) {
    let t0 = std::time::Instant::now();
    while s.queued() < n {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(20),
            "{} of {n} appends queued",
            s.queued()
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Group commit by construction (theseus-vni9): the appends that queue
/// while the writer is busy are written back to back and made durable by
/// one fdatasync, each answered once its frame is synced and indexed. The
/// writer is held here as a checkpoint holds it, by `appending`.
#[test]
fn the_writer_commits_every_queued_frame_with_one_sync() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    s.append(&[NewRecord::json(kinds::LEDGER, None, &"first").unwrap()])
        .unwrap();
    let before = s.stats().unwrap();
    let held = s.inner.appending.write().unwrap();
    let appenders: Vec<_> = (0..12)
        .map(|i| {
            let s = s.clone();
            std::thread::spawn(move || {
                let key = format!("k{i}");
                s.append(&[
                    NewRecord::json(kinds::META, Some(&key), &i).unwrap(),
                    NewRecord::json(kinds::LEDGER, None, &i).unwrap(),
                ])
                .unwrap()
            })
        })
        .collect();
    until_queued(&s, 12);
    drop(held);
    let mut positions: Vec<u64> = appenders
        .into_iter()
        .flat_map(|a| a.join().unwrap())
        .collect();
    let after = s.stats().unwrap();
    assert_eq!(after.frames_appended - before.frames_appended, 12);
    assert_eq!(
        after.syncs - before.syncs,
        1,
        "twelve frames queued together are made durable by one fdatasync"
    );
    positions.sort_unstable();
    assert_eq!(positions, (2..=25).collect::<Vec<u64>>());
    // One batch, one mark (theseus-7nfj): the twelve frames each carry
    // position 1, the end of the batch synced before them.
    let seg = std::fs::read(dir.path().join("wal").join("000000001.seg")).unwrap();
    let (mut off, mut next, mut marks) = (0, 1, Vec::new());
    while let crate::wal::FrameRead::Whole {
        end, mark, records, ..
    } = crate::wal::read_frame(&seg, off, 1, 0, next)
    {
        next = records.last().unwrap().0.position + 1;
        marks.push(mark);
        off = end;
    }
    assert_eq!(off, seg.len());
    assert_eq!(marks, [vec![Some(0)], vec![Some(1); 12]].concat());
    for i in 0..12 {
        let r = s.latest_by_key(kinds::META, &format!("k{i}")).unwrap();
        assert_eq!(
            r.unwrap().decode::<i32>().unwrap(),
            i,
            "indexed when answered"
        );
    }
    drop(s);
    let s = open(dir.path());
    assert_eq!(s.last_position(), 25, "every one durable");
}

/// The periodic checkpoint (theseus-avvb) runs on the writer after it has
/// answered the append that crossed the mark: no append's call pays it.
/// A checkpoint held, as a disk under writeback holds one, shows it: the
/// 1,000th record's append returns while the checkpoint is still held, and
/// the checkpoint lands once it is let go. No timing decides it (it was a
/// bound of 500 ms, which a loaded machine's stall could pass): an append
/// that paid the checkpoint would wait on the hold, and fail after 20 s.
#[test]
fn the_append_that_crosses_the_checkpoint_mark_does_not_wait_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = std::sync::Arc::new(
        WalStore::open(
            dir.path(),
            WalConfig {
                fsync: false,
                ..WalConfig::default()
            },
        )
        .unwrap(),
    );
    let (release, hold) = std::sync::mpsc::channel::<()>();
    *s.inner.checkpoint_hold.lock().unwrap() = Some(hold);
    let row = |i: u32| NewRecord::json(kinds::LEDGER, None, &i).unwrap();
    for i in 0..999 {
        s.append(&[row(i)]).unwrap();
    }
    assert_eq!(
        s.stats().unwrap().checkpoint,
        None,
        "no checkpoint before the mark"
    );
    let (done, appended) = std::sync::mpsc::channel();
    let (store, last) = (s.clone(), row(999));
    std::thread::spawn(move || done.send(store.append(&[last]).map(|_| ())));
    if let Ok(answer) = appended.recv_timeout(std::time::Duration::from_secs(20)) {
        answer.unwrap();
    } else {
        drop(release);
        panic!("the 1,000th record's append waited for the checkpoint: not back in 20 s");
    }
    assert_eq!(
        s.stats().unwrap().checkpoint,
        None,
        "the checkpoint is still held when the append returns"
    );
    drop(release);
    // The checkpoint follows, on the writer.
    let t0 = std::time::Instant::now();
    while s.stats().unwrap().checkpoint != Some(1000) {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(20),
            "no checkpoint"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The wait for the writer holds no runtime worker (theseus-vni9): with
/// the writer held, as a long fdatasync holds it, appends from every
/// worker of a two-worker runtime still leave a third task served.
#[test]
fn an_append_that_waits_for_the_disk_holds_no_runtime_worker() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(open(dir.path()));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let held = s.inner.appending.write().unwrap();
    let appends: Vec<_> = (0..4)
        .map(|i| {
            let s = s.clone();
            rt.spawn(async move {
                s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
                    .unwrap()
            })
        })
        .collect();
    until_queued(&s, 4);
    let served = rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::spawn(async { "served" }),
        )
        .await
    });
    drop(held);
    assert!(
        matches!(served, Ok(Ok("served"))),
        "a task waited for a worker while the appends waited for the disk"
    );
    for a in appends {
        rt.block_on(a).unwrap();
    }
}

/// A start at once after a stop (theseus-qa0 F4b): the stopping process
/// still holds the store for a while after its socket is gone. The next
/// open waits for it, and serves what the last holder wrote. The order is
/// proved, not timed (theseus-so1a): the holder closes only once the open
/// has found the store held, and the open returns only after it closed.
#[test]
fn an_open_waits_for_the_last_holder_to_close() {
    use std::sync::atomic::AtomicBool;
    let dir = tempfile::tempdir().unwrap();
    let first = open(dir.path());
    let rec = NewRecord::json(kinds::SESSION, Some("s1"), &serde_json::json!({"turns": 1}));
    first.append(&[rec.unwrap()]).unwrap();
    let closed = Arc::new(AtomicBool::new(false));
    let opener = {
        let (path, closed) = (dir.path().to_path_buf(), closed.clone());
        std::thread::spawn(move || {
            let next = WalStore::open(&path, WalConfig::default());
            (next, closed.load(Ordering::SeqCst))
        })
    };
    let held = || {
        HELD_TRIES
            .lock()
            .unwrap()
            .get(dir.path())
            .copied()
            .unwrap_or(0)
    };
    let t0 = std::time::Instant::now();
    while held() == 0 {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(20),
            "the open never found the store held"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    closed.store(true, Ordering::SeqCst);
    drop(first);
    let (next, after_close) = opener.join().unwrap();
    let next = next.unwrap();
    assert!(
        after_close,
        "the open returned while the first holder held the store"
    );
    let st = next.stats().unwrap();
    assert!(
        st.lock_wait_us > 0,
        "the wait is counted: {}",
        st.lock_wait_us
    );
    assert!(!st.index_repaired, "the last holder closed it");
    let s1 = next.latest_by_key(kinds::SESSION, "s1").unwrap().unwrap();
    assert_eq!(s1.decode::<serde_json::Value>().unwrap()["turns"], 1);
    // An open that found the store free waited for nothing.
    drop(next);
    let tries = held();
    let again = WalStore::open(dir.path(), WalConfig::default()).unwrap();
    assert_eq!(again.stats().unwrap().lock_wait_us, 0);
    assert_eq!(held(), tries, "it never found the store held");
}

/// A second process beside one that is not stopping still fails, once
/// its wait has passed, and says why. The daemon's wait covers a stop
/// whose telemetry flush takes its whole second.
#[test]
fn an_open_beside_a_holder_that_stays_fails_after_the_wait() {
    let dir = tempfile::tempdir().unwrap();
    let _held = open(dir.path());
    let wait = std::time::Duration::from_millis(150);
    let t0 = std::time::Instant::now();
    let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
        .err()
        .expect("the store is held");
    assert!(t0.elapsed() >= wait);
    let msg = format!("{e:#}");
    assert!(msg.contains("is another theseusd serving it?"), "{msg}");
    assert!(RedbIndex::held_elsewhere(&e), "{msg}");
    assert!(LOCK_WAIT >= std::time::Duration::from_secs(2));
}

#[test]
fn redb_store() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let p = s
        .append(&[
            NewRecord::json(kinds::SESSION, Some("s1"), &serde_json::json!({"turns": 0})).unwrap(),
        ])
        .unwrap();
    assert_eq!(p, vec![1]);
    s.append(&[NewRecord::json(kinds::LEDGER, None, &serde_json::json!({"k": "a"})).unwrap()])
        .unwrap();
    s.append(&[
        NewRecord::json(kinds::SESSION, Some("s1"), &serde_json::json!({"turns": 1})).unwrap(),
    ])
    .unwrap();
    let (c, k) = s
        .settle(
            NewRecord::json(
                kinds::COMPLETION,
                Some("act_1"),
                &serde_json::json!({"ok": true}),
            )
            .unwrap(),
            NewRecord::json(
                kinds::EXECUTION,
                Some("ex_1"),
                &serde_json::json!({"state": "runnable"}),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!((c, k), (4, 5));
    let latest = s.latest_by_key(kinds::SESSION, "s1").unwrap().unwrap();
    assert_eq!(latest.position, 3);
    assert_eq!(latest.decode::<serde_json::Value>().unwrap()["turns"], 1);
    assert_eq!(s.latest_of_kind(kinds::SESSION).unwrap().len(), 1);
    assert_eq!(s.tail_of_kind(kinds::LEDGER, 5).unwrap()[0].position, 2);
    assert_eq!(s.count_of_kind(kinds::SESSION).unwrap(), 2);
    assert_eq!(s.scan(2, Some(4), 10).unwrap().len(), 3);
    s.append(&[
        NewRecord::json(kinds::LEDGER, None, &"a")
            .unwrap()
            .scoped("ses_x"),
        NewRecord::json(kinds::LEDGER, None, &"b")
            .unwrap()
            .scoped("ses_y"),
        NewRecord::json(kinds::LEDGER, None, &"c")
            .unwrap()
            .scoped("ses_x"),
    ])
    .unwrap();
    let sx = s.scan_scope("ses_x", 0, 10).unwrap();
    assert_eq!(sx.len(), 2);
    assert_eq!(sx[1].decode::<String>().unwrap(), "c");
    assert_eq!(s.scan_scope("ses_x", sx[0].position, 10).unwrap().len(), 1);
    assert_eq!(s.count_in_scope("ses_y").unwrap(), 1);

    // Checkpoint, append more (index non-durable), reopen: replay rebuilds.
    s.checkpoint().unwrap();
    s.append(&[NewRecord::json(kinds::META, Some("live"), &"glm").unwrap()])
        .unwrap();
    drop(s);
    let s = open(dir.path());
    assert_eq!(s.last_position(), 9);
    assert_eq!(s.count_in_scope("ses_x").unwrap(), 2);
    let st = s.stats().unwrap();
    assert!(st.replayed_into_index <= 9);
    assert_eq!(
        s.latest_by_key(kinds::META, "live")
            .unwrap()
            .unwrap()
            .decode::<String>()
            .unwrap(),
        "glm"
    );
}

/// A toy projection: an execution's payload, a JSON string, is its one
/// term; a session's, a number, is added up as [1, the number].
fn toy_terms(kind: RecordKind, payload: &[u8]) -> Vec<String> {
    if kind != kinds::EXECUTION {
        return Vec::new();
    }
    serde_json::from_slice::<String>(payload)
        .map(|s| vec![s])
        .unwrap_or_default()
}
fn toy_sums(kind: RecordKind, payload: &[u8]) -> Option<Sums> {
    let n: u128 = serde_json::from_slice::<u64>(payload).ok()?.into();
    (kind == kinds::SESSION).then_some([1, n, 0, 0, 0, 0, 0, 0])
}
static TOY: Projection = Projection {
    name: "terms.toy.1",
    kinds: &[kinds::EXECUTION, kinds::SESSION],
    terms: toy_terms,
    sums: toy_sums,
};

fn by_term(s: &WalStore, t: &str) -> Vec<String> {
    s.latest_by_terms(kinds::EXECUTION, t, &format!("{t}\u{1}"))
        .unwrap()
        .expect("a projected store")
        .into_iter()
        .map(|r| r.key.unwrap())
        .collect()
}

/// The terms follow every append and every replay, and survive a reopen
/// (theseus-lv2). A writer that kept none (an open with no projection, as
/// an older build is) leaves them stale at a newer checkpoint: the next
/// projected open reads every record for them until they are built again
/// after serving, a stretch of keys at a time, beside appends.
#[test]
fn the_terms_follow_the_wal_through_reopens_and_a_writer_that_kept_none() {
    let dir = tempfile::tempdir().unwrap();
    let ex = |key: &str, state: &str| NewRecord::json(kinds::EXECUTION, Some(key), &state).unwrap();
    let projected = || {
        WalStore::open_projected(dir.path(), WalConfig::default(), &TOY)
            .unwrap()
            .with_checkpoint_every(0)
    };
    let s = projected();
    assert!(
        !s.stats().unwrap().terms_pending,
        "a new store has none to build"
    );
    s.append(&[ex("e1", "waiting"), ex("e2", "running")])
        .unwrap();
    s.append(&[ex("e3", "waiting")]).unwrap();
    s.append(&[ex("e1", "queued")]).unwrap();
    assert_eq!(by_term(&s, "waiting"), ["e3"]);
    assert_eq!(by_term(&s, "queued"), ["e1"]);
    assert_eq!(
        s.count_by_terms(kinds::EXECUTION, "a", "z").unwrap(),
        Some(3)
    );
    // A kind the projection does not name keeps no terms; a store with no
    // projection, none.
    assert!(s.latest_by_terms(kinds::META, "a", "z").unwrap().is_none());
    s.checkpoint().unwrap();
    // The tail after the checkpoint is replayed with its terms.
    s.append(&[ex("e2", "waiting")]).unwrap();
    drop(s);
    let s = projected();
    let st = s.stats().unwrap();
    assert_eq!((st.replayed_into_index, st.terms_pending), (1, false));
    assert_eq!(by_term(&s, "waiting"), ["e2", "e3"]);
    assert!(by_term(&s, "running").is_empty());
    drop(s);
    // A writer with no projection moves the checkpoint past the terms.
    let plain = open(dir.path());
    assert!(plain
        .latest_by_terms(kinds::EXECUTION, "a", "z")
        .unwrap()
        .is_none());
    plain.append(&[ex("e3", "complete")]).unwrap();
    plain.checkpoint().unwrap();
    drop(plain);
    // Not whole: no terms are answered, and a checkpoint marks none.
    let s = projected();
    assert!(s.stats().unwrap().terms_pending);
    assert!(s
        .latest_by_terms(kinds::EXECUTION, "a", "z")
        .unwrap()
        .is_none());
    s.checkpoint_for_close().unwrap();
    drop(s);
    let s = projected();
    assert!(s.stats().unwrap().terms_pending, "still not whole");
    // The build after serving, two keys a stretch, with an append between
    // its stretches: e1 moves on after the build read it.
    let at = s.build_terms(None, 2).unwrap();
    assert_eq!(at, Some((kinds::EXECUTION, "e2".to_string())));
    s.append(&[ex("e1", "complete"), ex("e4", "waiting")])
        .unwrap();
    assert!(s.build_terms(at, 2).unwrap().is_some(), "e3 and e4");
    assert!(!s.terms_whole());
    let mut at = Some((kinds::EXECUTION, "e4".to_string()));
    while let Some(next) = s.build_terms(at, 2).unwrap() {
        at = Some(next);
    }
    assert!(s.terms_whole() && !s.stats().unwrap().terms_pending);
    assert_eq!(by_term(&s, "waiting"), ["e2", "e4"]);
    assert_eq!(by_term(&s, "complete"), ["e1", "e3"]);
    assert!(
        by_term(&s, "queued").is_empty(),
        "e1's own append replaced it"
    );
    s.checkpoint().unwrap();
    drop(s);
    let s = projected();
    assert!(!s.stats().unwrap().terms_pending, "whole at its checkpoint");
    assert_eq!(by_term(&s, "waiting"), ["e2", "e4"]);
}

/// theseus-byu: a long replay builds the index in key order, a table at a
/// time, and answers every read as the index built record by record does:
/// the same WAL, opened with no index.
#[test]
fn an_index_built_in_bulk_answers_as_one_built_record_by_record() {
    let one_by_one = tempfile::tempdir().unwrap();
    // 1,700 frames: unsynced, which changes nothing a read answers.
    let unsynced = WalConfig {
        fsync: false,
        ..WalConfig::default()
    };
    let s = WalStore::open_projected(one_by_one.path(), unsynced, &TOY)
        .unwrap()
        .with_checkpoint_every(0);
    let states = ["waiting", "running", "queued", "complete"];
    let mut n = 0usize;
    for i in 0..1700u32 {
        let k = format!("e{:04}", (i * 7919) % 900);
        s.append(&[
            NewRecord::json(kinds::EXECUTION, Some(&k), &states[i as usize % 4])
                .unwrap()
                .scoped(&format!("ses_{}", i % 37)),
            NewRecord::json(kinds::LEDGER, None, &i)
                .unwrap()
                .scoped(&format!("ses_{}", i % 37)),
            NewRecord::json(kinds::SESSION, Some(&format!("s{}", i % 300)), &i).unwrap(),
        ])
        .unwrap();
        n += 3;
    }
    assert!(n >= BULK, "a replay long enough for the bulk path");
    s.checkpoint().unwrap();
    drop(s);
    // The same WAL, and no index: the open replays every record.
    let bulk = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(bulk.path().join("wal")).unwrap();
    for seg in crate::wal::list_segments(&one_by_one.path().join("wal")).unwrap() {
        let name = format!("{seg:09}.seg");
        std::fs::copy(
            one_by_one.path().join("wal").join(&name),
            bulk.path().join("wal").join(&name),
        )
        .unwrap();
    }
    let a = WalStore::open_projected(one_by_one.path(), WalConfig::default(), &TOY).unwrap();
    let b = WalStore::open_projected(bulk.path(), WalConfig::default(), &TOY).unwrap();
    assert_eq!(b.stats().unwrap().replayed_into_index, n as u64);
    let keyed = |s: &WalStore, kind| -> Vec<(Option<String>, u64)> {
        s.latest_of_kind(kind)
            .unwrap()
            .into_iter()
            .map(|r| (r.key, r.position))
            .collect()
    };
    for kind in [kinds::EXECUTION, kinds::SESSION, kinds::LEDGER] {
        assert_eq!(keyed(&a, kind), keyed(&b, kind), "kind {kind}");
        assert_eq!(
            a.count_of_kind(kind).unwrap(),
            b.count_of_kind(kind).unwrap()
        );
    }
    for scope in ["ses_0", "ses_5", "ses_36"] {
        let at = |s: &WalStore| -> Vec<u64> {
            s.scan_scope(scope, 0, usize::MAX)
                .unwrap()
                .iter()
                .map(|r| r.position)
                .collect()
        };
        assert_eq!(at(&a), at(&b), "{scope}");
    }
    for state in states {
        assert_eq!(by_term(&a, state), by_term(&b, state), "{state}");
    }
    assert_eq!(
        a.tail_of_kind(kinds::LEDGER, 5).unwrap(),
        b.tail_of_kind(kinds::LEDGER, 5).unwrap()
    );
    // The totals: each session's latest number, added up, either way.
    let latest: u128 = a
        .latest_of_kind(kinds::SESSION)
        .unwrap()
        .iter()
        .map(|r| u128::from(r.decode::<u64>().unwrap()))
        .sum();
    let want = Some([300, latest, 0, 0, 0, 0, 0, 0]);
    assert_eq!(a.totals(kinds::SESSION).unwrap(), want);
    assert_eq!(b.totals(kinds::SESSION).unwrap(), want);
}

/// Keys by prefix and the count of keys come from the key table alone.
#[test]
fn keys_by_prefix_and_their_count() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for (k, v) in [("q_1", 1), ("a_1", 2), ("q_2", 3), ("q_1", 4)] {
        s.append(&[NewRecord::json(kinds::COMPLETION, Some(k), &v).unwrap()])
            .unwrap();
    }
    let q: Vec<(String, i64)> = s
        .latest_with_prefix(kinds::COMPLETION, "q_")
        .unwrap()
        .into_iter()
        .map(|r| (r.key.clone().unwrap(), r.decode().unwrap()))
        .collect();
    assert_eq!(q, [("q_1".to_string(), 4), ("q_2".to_string(), 3)]);
    assert_eq!(s.count_keys(kinds::COMPLETION).unwrap(), 3);
    assert_eq!(s.count_of_kind(kinds::COMPLETION).unwrap(), 4);
}

/// A store this build did not write is refused and left as it was: another
/// format (format 1 is the pre-M2 record layout) or another engine.
#[test]
fn another_format_or_engine_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for (name, manifest, says) in [
        ("format1", r#"{"format": 1, "engine": "redb"}"#, "format 1"),
        (
            "format99",
            r#"{"format": 99, "engine": "redb"}"#,
            "format 99",
        ),
        ("fjall", r#"{"format": 2, "engine": "fjall"}"#, "fjall"),
    ] {
        let store_dir = dir.path().join(name);
        std::fs::create_dir_all(&store_dir).unwrap();
        std::fs::write(store_dir.join("MANIFEST.json"), manifest).unwrap();
        let e = WalStore::open(&store_dir, WalConfig::default())
            .err()
            .unwrap_or_else(|| panic!("{name} opened"));
        assert!(format!("{e:#}").contains(says), "{name}: {e:#}");
        assert!(!store_dir.join("wal").exists(), "{name} was written to");
    }
}

/// Every file under `dir`, with its length and bytes: what "nothing was
/// written" compares.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("MANIFEST.json")).unwrap()).unwrap()
}

/// F4a: a store whose format is newer than this build's is refused, with a
/// message that names both formats and what to do, and nothing in it is
/// written: not the WAL's tail, not the index, not the manifest.
#[test]
fn a_newer_format_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v1").unwrap()])
        .unwrap();
    s.checkpoint().unwrap();
    s.append(&[NewRecord::json(kinds::LEDGER, None, &"after").unwrap()])
        .unwrap();
    drop(s);
    let newer = MANIFEST_FORMAT + 1;
    std::fs::write(
        dir.path().join("MANIFEST.json"),
        format!(r#"{{"format": {newer}, "engine": "redb"}}"#),
    )
    .unwrap();
    // A torn frame too, which an open would cut: it must stay.
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("wal").join("000000001.seg"))
            .unwrap();
        f.write_all(&crate::wal::MAGIC_MARKED.to_le_bytes())
            .unwrap();
    }
    let before = snapshot(dir.path());
    let e = format!(
        "{:#}",
        WalStore::open(dir.path(), WalConfig::default())
            .err()
            .unwrap()
    );
    for says in [
        format!("is format {newer}"),
        format!("reads formats {MANIFEST_OLDEST} to {MANIFEST_FORMAT}"),
        "install the newer theseusd".to_string(),
    ] {
        assert!(e.contains(&says), "{says:?} missing from: {e}");
    }
    assert_eq!(
        snapshot(dir.path()),
        before,
        "the refused store was written to"
    );
}

/// theseus-7hh: the manifest alone says what `open` would, for a store
/// this build reads (a format-3 one too, its per-kind marks left unread),
/// one it is too old for, and a directory with no store yet, and the check
/// writes nothing.
#[test]
fn the_manifest_alone_says_whether_this_build_may_open_a_store() {
    let dir = tempfile::tempdir().unwrap();
    check_manifest(&dir.path().join("none")).unwrap();
    let s = open(dir.path());
    s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v1").unwrap()])
        .unwrap();
    drop(s);
    check_manifest(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("MANIFEST.json"),
        r#"{"format": 3, "engine": "redb", "kinds": [{"kind": 77, "name": "hold", "schema": 9}]}"#,
    )
    .unwrap();
    check_manifest(dir.path()).unwrap();
    let newer = MANIFEST_FORMAT + 1;
    for (manifest, says) in [
        (
            format!(r#"{{"format": {newer}, "engine": "redb"}}"#),
            format!("is format {newer}"),
        ),
        ("{".into(), "reading store manifest".into()),
    ] {
        std::fs::write(dir.path().join("MANIFEST.json"), manifest).unwrap();
        let before = snapshot(dir.path());
        let e = format!("{:#}", check_manifest(dir.path()).err().unwrap());
        assert!(e.contains(&says), "{says:?} missing from: {e}");
        assert_eq!(snapshot(dir.path()), before, "the check wrote");
    }
}

/// F4a, one format number (theseus-ptx1): a store an older build wrote
/// (format 2, and format 3 with its per-kind marks) opens as it is, and
/// stays as it is while this build only reads it, so a rollback still opens
/// it. The writer's first frame moves the manifest to this build's format
/// first, once; a manifest that cannot move fails the append, and nothing
/// reaches the WAL under the old one.
#[test]
fn an_older_store_moves_to_this_builds_format_at_its_first_write() {
    for old in [
        r#"{"format": 2, "engine": "redb"}"#,
        r#"{"format": 3, "engine": "redb", "kinds": [{"kind": 1, "name": "session", "schema": 6}]}"#,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        s.append(&[NewRecord::json(kinds::LEDGER, None, &"a").unwrap()])
            .unwrap();
        drop(s);
        let path = dir.path().join("MANIFEST.json");
        std::fs::write(&path, old).unwrap();

        let s = open(dir.path());
        assert_eq!(s.last_position(), 1);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            old,
            "reading moved it"
        );
        // The rewrite's temporary file can't be made: the append fails.
        let blocker = dir.path().join("MANIFEST.json.tmp");
        std::fs::create_dir(&blocker).unwrap();
        let e = s
            .append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v").unwrap()])
            .unwrap_err();
        assert!(format!("{e:#}").contains("this build's format"), "{e:#}");
        assert_eq!(s.last_position(), 1, "a frame was written under {old}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        std::fs::remove_dir(&blocker).unwrap();

        s.append(&[NewRecord::json(kinds::SESSION, Some("s1"), &"v").unwrap()])
            .unwrap();
        let m = manifest(dir.path());
        assert_eq!(
            m,
            serde_json::json!({"format": MANIFEST_FORMAT, "engine": "redb"})
        );
        let moved = std::fs::metadata(&path).unwrap().modified().unwrap();
        s.append(&[NewRecord::json(kinds::EXECUTION, Some("e1"), &"x").unwrap()])
            .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            moved,
            "moved once per upgrade"
        );
        drop(s);
        // And the moved store opens again under this build.
        let s = open(dir.path());
        assert_eq!(s.last_position(), 3);
        assert_eq!(
            s.latest_by_key(kinds::SESSION, "s1")
                .unwrap()
                .unwrap()
                .decode::<String>()
                .unwrap(),
            "v"
        );
    }
}

/// An open takes no checkpoint after a replay (theseus-ptx1): the WAL is
/// durable, so the start pays no sync for it, and a stop before the next
/// checkpoint only replays the tail again. The replayed tail counts toward
/// the next periodic checkpoint, which the writer takes after serving.
#[test]
fn a_replay_takes_no_checkpoint_and_counts_toward_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.append(&[NewRecord::json(kinds::LEDGER, None, &0).unwrap()])
        .unwrap();
    s.checkpoint().unwrap();
    for i in 1..4 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
            .unwrap();
    }
    drop(s);
    for _ in 0..2 {
        let s = open(dir.path());
        let st = s.stats().unwrap();
        assert_eq!(
            (st.replayed_into_index, st.checkpoint),
            (3, Some(1)),
            "the open replays the tail, and moves no checkpoint"
        );
    }
    let s = WalStore::open(dir.path(), WalConfig::default())
        .unwrap()
        .with_checkpoint_every(4);
    s.append(&[NewRecord::json(kinds::LEDGER, None, &4).unwrap()])
        .unwrap();
    // The writer checkpoints after answering the batch that crossed the
    // mark, and before it takes the next.
    s.append(&[NewRecord::json(kinds::LEDGER, None, &5).unwrap()])
        .unwrap();
    assert_eq!(
        s.stats().unwrap().checkpoint,
        Some(5),
        "3 replayed and 1 written"
    );
}

/// theseus-8ni: open checks only the WAL after the checkpoint. Corrupt
/// an old segment's body and make another unreadable: the store still
/// opens, finds the next position, and cuts a torn tail. Then the
/// history check finds the corrupt segment, and reads from it are
/// refused.
#[test]
fn open_reads_only_the_tail_and_the_history_check_finds_an_old_corrupt_segment() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cfg = WalConfig {
        segment_bytes: 400,
        ..Default::default()
    };
    let s = WalStore::open(dir.path(), cfg.clone())
        .unwrap()
        .with_checkpoint_every(0);
    for i in 0..40u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
            .unwrap();
    }
    let cp = s.checkpoint().unwrap();
    for i in 40..43u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
            .unwrap();
    }
    let segments = s.stats().unwrap().wal_segments;
    assert!(segments >= 4, "{segments} segments");
    drop(s);
    let seg = |n: u32| dir.path().join("wal").join(format!("{n:09}.seg"));
    // Segment 1: a byte of its first record's payload flipped (header
    // 12, mark 8, count 4, record header 28), so the record still decodes
    // and only its frame's crc knows. Segment 2: unreadable.
    let mut b = std::fs::read(seg(1)).unwrap();
    b[12 + 8 + 4 + 28 + 3] ^= 0x01;
    std::fs::write(seg(1), &b).unwrap();
    std::fs::set_permissions(seg(2), std::fs::Permissions::from_mode(0o000)).unwrap();
    // A torn frame at the end of the last segment.
    let last_len = std::fs::metadata(seg(segments)).unwrap().len();
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(seg(segments))
            .unwrap();
        f.write_all(&crate::wal::MAGIC_MARKED.to_le_bytes())
            .unwrap();
        f.write_all(&[9u8; 7]).unwrap();
    }

    let s = WalStore::open(dir.path(), cfg.clone()).unwrap();
    std::fs::set_permissions(seg(2), std::fs::Permissions::from_mode(0o644)).unwrap();
    let st = s.stats().unwrap();
    assert_eq!(st.last_position, 43);
    assert_eq!(st.truncated_bytes, 11, "the torn frame is cut");
    assert_eq!(std::fs::metadata(seg(segments)).unwrap().len(), last_len);
    assert_eq!(st.replayed_into_index, 43 - cp, "only the tail is replayed");
    assert!(st.history_bytes > 0);
    let r = s.recovery();
    assert_eq!(r.records, 43 - cp, "only the tail is checked");
    assert!(r.checked_from.is_some());
    // Appends go on from the next position.
    let p = s
        .append(&[NewRecord::json(kinds::LEDGER, None, &"next").unwrap()])
        .unwrap();
    assert_eq!(p, vec![44]);

    let before = s.get(1);
    assert!(before.is_ok(), "no check has run yet: {before:?}");
    let e = s.verify_history(|_| {}).unwrap_err();
    assert!(
        matches!(e, crate::wal::WalError::Corrupt { segment: 1, .. }),
        "{e}"
    );
    let refused = format!("{:#}", s.get(1).unwrap_err());
    assert!(refused.contains("corrupt frame"), "{refused}");
    assert!(s.get(2).unwrap().is_some(), "the next frame still reads");
    assert!(s.get(cp).unwrap().is_some(), "a later segment still reads");

    // A whole history checks, and `pace` hears of each stretch.
    let clean = tempfile::tempdir().unwrap();
    let s = WalStore::open(clean.path(), cfg.clone())
        .unwrap()
        .with_checkpoint_every(0);
    for i in 0..40u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
            .unwrap();
    }
    s.checkpoint().unwrap();
    drop(s);
    let s = WalStore::open(clean.path(), cfg).unwrap();
    let h = s.verify_history(|_| {}).unwrap();
    assert_eq!(h.records, 40);
    assert!(h.segments >= 3 && !h.checked_at_open, "{h:?}");
}

/// theseus-0dq: a history check starts where the last one ended. Its
/// mark reaches the index with the next checkpoint, and the check after a
/// restart checks the mark's frame again and then only what was written
/// since; a mark whose frame no longer checks, or holds other positions,
/// is not believed, and the whole log is checked, which finds what is
/// wrong.
#[test]
fn a_history_check_starts_at_the_last_checks_mark() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = WalConfig {
        segment_bytes: 400,
        ..Default::default()
    };
    let reopen = || {
        WalStore::open(dir.path(), cfg.clone())
            .unwrap()
            .with_checkpoint_every(0)
    };
    let rows = |s: &WalStore, from: u32, to: u32| {
        for i in from..to {
            s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
                .unwrap();
        }
        s.checkpoint().unwrap()
    };
    let s = reopen();
    rows(&s, 0, 40);
    drop(s);
    // The first check reads the whole history and leaves its mark.
    let s = reopen();
    assert_eq!(s.verified(), None);
    let h = s
        .history_check()
        .from_mark(s.verified())
        .run(|_| {})
        .unwrap();
    assert_eq!((h.records, h.from_position), (40, None));
    let mark = h.verified.expect("a mark");
    assert_eq!(mark.position, 40);
    assert!(mark.full_at_unix_ms > 0, "a whole check sets its time");
    s.verified_slot().set(mark);
    rows(&s, 40, 45);
    drop(s);
    // The next reads the mark's frame and what came after it, and keeps
    // the time of the last whole check.
    let s = reopen();
    assert_eq!(s.verified(), Some(mark), "the checkpoint wrote it");
    let h = s
        .history_check()
        .from_mark(s.verified())
        .run(|_| {})
        .unwrap();
    assert_eq!(h.from_position, Some(40));
    assert_eq!(
        h.records, 6,
        "the mark's frame again, and the five after it"
    );
    let next = h.verified.unwrap();
    assert_eq!(next.position, 45);
    assert_eq!(next.full_at_unix_ms, mark.full_at_unix_ms);
    // A mark that names other positions for its frame is not believed.
    let wrong = Verified {
        first: mark.first + 1,
        ..mark
    };
    let h = s
        .history_check()
        .from_mark(Some(wrong))
        .run(|_| {})
        .unwrap();
    assert_eq!((h.records, h.from_position), (45, None), "the whole log");
    drop(s);
    // Nor one whose frame no longer checks: the whole check that follows
    // finds the corrupt frame.
    let seg = dir
        .path()
        .join("wal")
        .join(format!("{:09}.seg", mark.segment));
    let mut b = std::fs::read(&seg).unwrap();
    b[mark.offset as usize + 12 + 4 + 28 + 3] ^= 0x01;
    std::fs::write(&seg, &b).unwrap();
    let s = reopen();
    let e = s
        .history_check()
        .from_mark(Some(mark))
        .run(|_| {})
        .unwrap_err();
    assert!(
        matches!(e, crate::wal::WalError::Corrupt { segment, .. } if segment == mark.segment),
        "{e}"
    );
}

/// theseus-02k: a stop's checkpoint syncs nothing of its own, and redb's
/// close makes it durable, so the next open replays nothing and repairs
/// nothing, and a history check's mark rides along.
#[test]
fn a_checkpoint_for_close_is_made_durable_by_the_close() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..5u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
            .unwrap();
    }
    s.checkpoint().unwrap();
    s.append(&[NewRecord::json(kinds::LEDGER, None, &"stopping").unwrap()])
        .unwrap();
    let mark = Verified {
        segment: 1,
        offset: 0,
        first: 1,
        position: 5,
        full_at_unix_ms: 7,
    };
    s.verified_slot().set(mark);
    assert_eq!(s.checkpoint_for_close().unwrap(), 6);
    // A durable checkpoint after it is not free: nothing synced 6 yet.
    assert_eq!(s.inner.durable_to.load(Ordering::Relaxed), 5);
    assert_eq!(s.checkpoint_for_close().unwrap(), 6, "and a second is free");
    drop(s);
    let s = open(dir.path());
    let st = s.stats().unwrap();
    assert_eq!(st.checkpoint, Some(6));
    assert_eq!(st.replayed_into_index, 0, "the close made it durable");
    assert!(!st.index_repaired);
    assert_eq!(s.verified(), Some(mark));
}

/// R4 (theseus-15g): one record in a corrupt frame no longer fails every
/// list read that reaches it. Three executions, one in each of the first
/// three frames, then the history check finds the first frame corrupt.
/// Each list read skips that record, and the store counts it once, by
/// position, for health; a read of it alone is still refused.
#[test]
fn list_reads_skip_a_refused_record_and_count_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for k in ["exe_a", "exe_b", "exe_c"] {
        s.append(&[NewRecord::json(kinds::EXECUTION, Some(k), &k)
            .unwrap()
            .scoped("ses_1")])
            .unwrap();
    }
    s.checkpoint().unwrap();
    s.append(&[NewRecord::json(kinds::LEDGER, None, &"after").unwrap()])
        .unwrap();
    drop(s);
    // Frame 1's last byte, its record's payload, flipped: only its crc
    // knows.
    let seg = dir.path().join("wal").join(format!("{:09}.seg", 1));
    let mut b = std::fs::read(&seg).unwrap();
    let body_len = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    b[crate::wal::FRAME_HEADER + body_len - 1] ^= 0x01;
    std::fs::write(&seg, &b).unwrap();
    let s = open(dir.path());
    assert!(
        s.verify_history(|_| {}).is_err(),
        "the history check finds it"
    );

    let keys = |rs: Vec<Record>| -> Vec<String> { rs.into_iter().filter_map(|r| r.key).collect() };
    assert_eq!(
        keys(s.latest_of_kind(kinds::EXECUTION).unwrap()),
        ["exe_b", "exe_c"]
    );
    assert_eq!(
        keys(s.tail_of_kind(kinds::EXECUTION, 10).unwrap()),
        ["exe_b", "exe_c"]
    );
    assert_eq!(
        keys(s.scan_scope("ses_1", 0, 10).unwrap()),
        ["exe_b", "exe_c"]
    );
    assert_eq!(s.scan(1, None, 10).unwrap().len(), 3, "positions 2 to 4");
    let st = s.stats().unwrap();
    assert_eq!(
        (st.refused_records, st.refused_positions),
        (1, vec![1]),
        "one record, counted once over four reads"
    );
    let alone = format!("{:#}", s.get(1).unwrap_err());
    assert!(alone.contains("corrupt frame"), "{alone}");
    assert!(s.latest_by_key(kinds::EXECUTION, "exe_a").is_err());
}

/// When the index's checkpoint does not match the WAL (a WAL copied in
/// under an old index, say), open checks every segment, as it always did,
/// and a read through a stale index entry is refused, never believed.
#[test]
fn a_checkpoint_the_wal_does_not_match_falls_back_to_the_full_check() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..5u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
            .unwrap();
    }
    s.checkpoint().unwrap();
    drop(s);
    // Another WAL of seven longer records: position 5 is elsewhere.
    let other = tempfile::tempdir().unwrap();
    let w = crate::wal::Wal::open(other.path(), WalConfig::default()).unwrap();
    for i in 0..7u32 {
        w.append(&[NewRecord::json(kinds::LEDGER, None, &format!("a longer record {i}")).unwrap()])
            .unwrap();
    }
    drop(w);
    std::fs::copy(
        other.path().join("000000001.seg"),
        dir.path().join("wal").join("000000001.seg"),
    )
    .unwrap();
    let s = open(dir.path());
    assert!(s.recovery().checked_from.is_none(), "every segment checked");
    assert_eq!(s.last_position(), 7);
    assert_eq!(
        s.get(6).unwrap().unwrap().decode::<String>().unwrap(),
        "a longer record 5"
    );
    assert!(s.get(2).is_err(), "a stale entry's read is refused");
}

/// A checkpoint with nothing written since the last one claims the same
/// position and commits nothing (theseus-pfv: a clean stop's last
/// checkpoint, after its own). One after new records claims them, and the
/// next open replays nothing either way.
#[test]
fn a_checkpoint_with_nothing_new_claims_the_same_and_the_next_open_replays_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..5u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
            .unwrap();
    }
    assert_eq!(s.checkpoint().unwrap(), 5);
    assert_eq!(s.checkpoint().unwrap(), 5, "nothing new");
    assert_eq!(s.inner.index.checkpoint().unwrap(), Some(5));
    drop(s);
    let s = open(dir.path());
    assert_eq!(s.stats().unwrap().replayed_into_index, 0);
    assert_eq!(s.checkpoint().unwrap(), 5, "nothing new since the open");
    s.append(&[NewRecord::json(kinds::LEDGER, None, &9u32).unwrap()])
        .unwrap();
    assert_eq!(s.checkpoint().unwrap(), 6);
    assert_eq!(s.inner.index.checkpoint().unwrap(), Some(6));
    drop(s);
    let s = open(dir.path());
    assert_eq!(s.stats().unwrap().replayed_into_index, 0);
    assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 6);
}

#[test]
fn index_loss_is_rebuilt_from_wal() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..50u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &i).unwrap()])
            .unwrap();
    }
    s.append(&[NewRecord::json(kinds::SESSION, Some("s"), &"v1").unwrap()])
        .unwrap();
    drop(s);
    // Destroy the index entirely; the WAL is the truth.
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    let s = open(dir.path());
    assert_eq!(s.last_position(), 51);
    assert_eq!(s.stats().unwrap().replayed_into_index, 51);
    assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 50);
    assert_eq!(
        s.latest_by_key(kinds::SESSION, "s")
            .unwrap()
            .unwrap()
            .decode::<String>()
            .unwrap(),
        "v1"
    );
}

/// Every record a store reads: position, kind, key, scope, and payload.
type Rows = Vec<(u64, RecordKind, Option<String>, Option<String>, Vec<u8>)>;

/// A store of sessions, ledger rows, and scoped nodes, checkpointed and
/// closed, with every record as the store reads it.
fn written(dir: &Path) -> Rows {
    let s = open(dir);
    for i in 0..40u32 {
        let mut batch = vec![NewRecord::json(kinds::LEDGER, None, &i).unwrap()];
        if i % 3 == 0 {
            batch.push(
                NewRecord::json(kinds::SESSION, Some(&format!("ses_{}", i % 4)), &i).unwrap(),
            );
        }
        if i % 5 == 0 {
            batch.push(
                NewRecord::json(kinds::NODE, Some(&format!("n{i}")), &i)
                    .unwrap()
                    .scoped("ses_0"),
            );
        }
        s.append(&batch).unwrap();
    }
    s.checkpoint().unwrap();
    let all = everything(&s);
    assert!(all.len() > 50);
    all
}

fn everything(s: &WalStore) -> Rows {
    s.scan(1, None, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|r| (r.position, r.kind, r.key, r.scope, r.payload))
        .collect()
}

/// The `index.redb.bad-*` files beside the index.
fn moved(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("index.redb.bad-"))
        })
        .collect();
    v.sort();
    v
}

/// A kill inside the store's first open leaves an `index.redb` that is
/// not a redb database (theseus-0b8): a header cut short, bytes that were
/// never a header, or the zeros redb sized the file with before it wrote
/// one. The next open moves the file aside, keeping it byte for byte,
/// builds the index again from the WAL, and reads every record the WAL
/// holds. The open after that finds a good index and moves nothing.
#[test]
fn an_index_that_is_not_a_database_is_moved_aside_and_rebuilt_from_the_wal() {
    type Damage = fn(&[u8]) -> Vec<u8>;
    // Each shape, and why the open says it is not a database.
    let shapes: [(&str, Damage, &str); 3] = [
        (
            "a 37-byte partial header",
            |real| real[..37].to_vec(),
            "the file ends inside redb's header",
        ),
        (
            "a few random bytes",
            |_| {
                vec![
                    0x5c, 0x91, 0x07, 0xee, 0x30, 0x2a, 0xd4, 0x18, 0x66, 0x0b, 0xf3,
                ]
            },
            "the file does not start with redb's magic number",
        ),
        (
            "zeros the length of a new index",
            |real| vec![0u8; real.len()],
            "the file does not start with redb's magic number",
        ),
    ];
    for (shape, damage, says) in shapes {
        let dir = tempfile::tempdir().unwrap();
        let before = written(dir.path());
        let index = dir.path().join("index.redb");
        let bad = damage(&std::fs::read(&index).unwrap());
        std::fs::write(&index, &bad).unwrap();

        let s = open(dir.path());
        let st = s.stats().unwrap();
        let m = st.index_moved_aside.clone().expect(shape);
        let aside = moved(dir.path());
        assert_eq!(aside.len(), 1, "{shape}: {aside:?}");
        assert_eq!(m.path, aside[0].display().to_string(), "{shape}");
        assert_eq!(
            std::fs::read(&aside[0]).unwrap(),
            bad,
            "{shape}: kept as it was"
        );
        assert_eq!(m.bytes, bad.len() as u64);
        assert!(m.why.starts_with(says), "{shape}: {}", m.why);
        assert!(m.why.contains("(redb: "), "{shape}: {}", m.why);
        assert_eq!(st.replayed_into_index, before.len() as u64, "{shape}");
        assert_eq!(everything(&s), before, "{shape}");
        assert_eq!(s.count_of_kind(kinds::LEDGER).unwrap(), 40, "{shape}");
        assert_eq!(s.count_in_scope("ses_0").unwrap(), 8, "{shape}");
        assert_eq!(
            s.latest_by_key(kinds::SESSION, "ses_3")
                .unwrap()
                .unwrap()
                .decode::<u32>()
                .unwrap(),
            39,
            "{shape}"
        );
        // A stop's checkpoint keeps the rebuild: the open takes none
        // (theseus-ptx1).
        s.checkpoint_for_close().unwrap();
        drop(s);

        let s = open(dir.path());
        let st = s.stats().unwrap();
        assert!(st.index_moved_aside.is_none(), "{shape}");
        assert_eq!(
            st.replayed_into_index, 0,
            "{shape}: the rebuild was checkpointed"
        );
        assert_eq!(everything(&s), before, "{shape}");
        assert_eq!(moved(dir.path()).len(), 1, "{shape}");
    }
}

/// A store another process holds is waited for and refused with the
/// message it always had, and nothing is moved (theseus-0b8): the held
/// index is that process's. That holds even while the index is not a
/// database yet, as when its holder is inside its own first open: redb
/// takes its lock before it reads a byte, so the error says held.
#[test]
fn a_held_index_is_refused_and_never_moved_even_before_it_is_a_database() {
    let dir = tempfile::tempdir().unwrap();
    written(dir.path());
    let holder = open(dir.path());
    let wait = std::time::Duration::from_millis(150);
    let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
        .err()
        .expect("the store is held");
    let msg = format!("{e:#}");
    assert!(msg.contains("is another theseusd serving it?"), "{msg}");
    assert!(RedbIndex::held_elsewhere(&e) && !RedbIndex::not_a_database(&e));
    assert!(moved(dir.path()).is_empty());
    drop(holder);

    // A holder inside its first open: the file is zeros, and locked.
    let dir = tempfile::tempdir().unwrap();
    written(dir.path());
    let index = dir.path().join("index.redb");
    let zeros = vec![0u8; 4096];
    std::fs::write(&index, &zeros).unwrap();
    let lock = std::fs::File::open(&index).unwrap();
    lock.try_lock().unwrap();
    let e = WalStore::open_waiting(dir.path(), WalConfig::default(), wait)
        .err()
        .expect("the index is held");
    let msg = format!("{e:#}");
    assert!(msg.contains("is another theseusd serving it?"), "{msg}");
    assert!(moved(dir.path()).is_empty());
    assert_eq!(std::fs::read(&index).unwrap(), zeros, "left as it was");
    drop(lock);
    // Released, it is the holder's leftover: moved aside, and rebuilt.
    let s = open(dir.path());
    assert!(s.stats().unwrap().index_moved_aside.is_some());
    assert_eq!(moved(dir.path()).len(), 1);
}

/// A file that is a redb database but fails some other way (a format
/// newer than redb reads, or a file cut short of its layout) is not this
/// case: it stays where it is, and the open refuses as it did before.
#[test]
fn a_redb_index_that_fails_another_way_is_left_and_refused() {
    type Damage = fn(&mut Vec<u8>);
    let cases: [(&str, Damage); 2] = [
        ("a newer file format", |b| {
            // Each commit slot's first byte is its format version.
            b[64] = 99;
            b[192] = 99;
        }),
        ("a file cut short", |b| b.truncate(b.len() / 2)),
    ];
    for (case, damage) in cases {
        let dir = tempfile::tempdir().unwrap();
        written(dir.path());
        let index = dir.path().join("index.redb");
        let mut bytes = std::fs::read(&index).unwrap();
        damage(&mut bytes);
        std::fs::write(&index, &bytes).unwrap();
        let e = WalStore::open(dir.path(), WalConfig::default())
            .err()
            .expect(case);
        let msg = format!("{e:#}");
        assert!(msg.contains("opening index"), "{case}: {msg}");
        assert!(!RedbIndex::not_a_database(&e), "{case}: {msg}");
        assert!(!RedbIndex::held_elsewhere(&e), "{case}: {msg}");
        assert!(moved(dir.path()).is_empty(), "{case}");
        assert_eq!(std::fs::read(&index).unwrap(), bytes, "{case}: untouched");
    }
}

/// A new store's own name is as durable as its first frame (theseus-gf00).
/// The open makes the store's directory, and the state directories above it
/// when they are new; the manifest's write syncs the store's directory, but
/// nothing syncs the directory that holds its name. The first frame's sync
/// does: the holder of each directory the open made, besides the log's own
/// two (its directory, and the store's). A store opened again made nothing.
#[test]
fn a_new_stores_own_name_is_synced_with_its_first_frame() {
    let root = tempfile::tempdir().unwrap();
    let frame = || [NewRecord::json(kinds::LEDGER, None, &"first").unwrap()];
    let dir_syncs = |s: &WalStore| s.inner.wal.dir_syncs();

    // The store's directory is new, and its parent exists.
    let s = open(&root.path().join("store"));
    assert_eq!(dir_syncs(&s), 0, "nothing is synced before a frame is");
    s.append(&frame()).unwrap();
    assert_eq!(
        dir_syncs(&s),
        3,
        "the log directory (segment 1's name), the store's (the log's own), and the one that holds the store"
    );
    drop(s);

    // The state directory above it is new too: its holder is synced as well.
    let s = open(&root.path().join("state/deeper/store"));
    s.append(&frame()).unwrap();
    assert_eq!(
        dir_syncs(&s),
        5,
        "the holders of the three directories the open made, and the log's two"
    );
    drop(s);

    // A store that exists made no name: its next frame syncs none.
    let s = open(&root.path().join("store"));
    s.append(&frame()).unwrap();
    assert_eq!(dir_syncs(&s), 0);
}

/// theseus-gt12: the frame that holds the index's checkpoint goes bad on
/// disk. The tail-only open cannot find that record, so the open checks
/// every segment and meets the bad frame at the last segment's end. The
/// checkpoint says it was synced, so it is no torn tail: the open refuses
/// before it cuts anything. It used to cut the frame as a torn tail, and
/// only then refuse, as the checkpoint was past the log's end.
#[test]
fn a_checkpointed_frame_gone_bad_is_refused_before_anything_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    for i in 0..5u32 {
        s.append(&[NewRecord::json(kinds::LEDGER, None, &[i; 20]).unwrap()])
            .unwrap();
    }
    assert_eq!(s.checkpoint().unwrap(), 5);
    drop(s);
    let seg = dir.path().join("wal").join(format!("{:09}.seg", 1));
    let mut b = std::fs::read(&seg).unwrap();
    // The last frame's record: its position's low byte (frame header 12,
    // mark 8, count 4).
    let mut last = 0usize;
    while let Some(next) = b
        .get(last + 4..last + 8)
        .map(|n| last + 12 + u32::from_le_bytes(n.try_into().unwrap()) as usize)
        .filter(|&n| n < b.len())
    {
        last = next;
    }
    assert_eq!(b[last..last + 4], crate::wal::MAGIC_MARKED.to_le_bytes());
    b[last + 12 + 8 + 4] ^= 0x40;
    std::fs::write(&seg, &b).unwrap();

    let e = WalStore::open(dir.path(), WalConfig::default())
        .err()
        .expect("a checkpointed frame gone bad is refused");
    let msg = format!("{e:#}");
    assert!(msg.contains("position 5"), "{msg}");
    assert!(msg.contains("theseusd restore --repair"), "{msg}");
    assert_eq!(std::fs::read(&seg).unwrap(), b, "nothing was cut");
}
