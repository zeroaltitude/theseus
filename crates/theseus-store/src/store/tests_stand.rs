//! What `stats` says of the log, read where the writer keeps it, not under its lock (`wal::stand`): the same numbers
//! as the locked reads for a store at rest, through appends, a roll to a new segment and a cut after a failed sync;
//! and an answer while the writer is held inside a frame's write, as a disk that stalls holds it.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::record::kinds;

fn rec(i: usize) -> NewRecord {
    NewRecord::bytes(kinds::LEDGER, None, vec![i as u8; 40])
}

fn small_segments(dir: &Path) -> WalStore {
    let cfg = WalConfig {
        segment_bytes: 200,
        ..WalConfig::default()
    };
    WalStore::open(dir, cfg).unwrap().with_checkpoint_every(0)
}

/// The three numbers `stats` shows, and the locked reads of the same.
fn both(s: &WalStore) -> ((u64, u64, u32), (u64, u64, u32)) {
    let st = s.stats().unwrap();
    let w = &s.inner.wal;
    (
        (st.last_position, st.wal_bytes, st.wal_segments),
        (w.last_position(), w.total_bytes(), w.segment_count()),
    )
}

#[test]
fn stats_say_what_the_locked_reads_say_through_appends_and_a_roll() {
    let dir = tempfile::tempdir().unwrap();
    let s = small_segments(dir.path());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "a store just opened");
    for i in 0..12 {
        s.append(&[rec(i)]).unwrap();
        let (shown, locked) = both(&s);
        assert_eq!(shown, locked, "after append {i}");
    }
    let (shown, _) = both(&s);
    assert!(shown.2 > 1, "the log rolled to a new segment: {shown:?}");
    assert_eq!(shown.0, 12);
    drop(s);
    let s = small_segments(dir.path());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "after a reopen");
    assert_eq!(shown.0, 12);
}

/// A sync that fails cuts its frame back off the log, and the numbers go back with it. (In a segment with room: a
/// frame that rolls fails in the roll's own sync, before anything is written after the last durable frame.)
#[test]
fn stats_go_back_with_a_cut_after_a_failed_sync() {
    let dir = tempfile::tempdir().unwrap();
    let s = WalStore::open(dir.path(), WalConfig::default()).unwrap();
    for i in 0..3 {
        s.append(&[rec(i)]).unwrap();
    }
    let (before, _) = both(&s);
    s.inner.wal.fail_next_sync();
    // The answer comes before the sync (theseus-7yhp), so the append may say Ok: the writer is done when it can be
    // taken for writing, as a checkpoint takes it.
    let _ = s.append(&[rec(99)]);
    drop(s.inner.appending.write().unwrap());
    let (shown, locked) = both(&s);
    assert_eq!(shown, locked, "after a cut");
    assert_eq!(shown, before, "the cut frame is not counted");
}

/// The writer is held inside a frame's write (a gate it waits at, no clock), holding the lock a stalled disk would
/// hold it under: `stats` answers, with the numbers from before the frame. Let go, the frame lands and they move.
/// Without the fix `stats` waits for the lock and the answer never comes in this window (it comes only after the
/// gate is opened, which the test does by itself once the wait is past).
#[test]
fn stats_answer_while_the_writer_is_inside_its_write() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(small_segments(dir.path()));
    s.append(&[rec(0)]).unwrap();
    let gate = s.inner.wal.hold_writes();
    let appended = {
        let s = s.clone();
        std::thread::spawn(move || s.append(&[rec(1)]).unwrap())
    };
    gate.entered();
    let (said, heard) = mpsc::channel();
    {
        let s = s.clone();
        std::thread::spawn(move || {
            let _ = said.send(s.stats().unwrap().last_position);
        });
    }
    let answer = heard.recv_timeout(Duration::from_secs(20));
    gate.release();
    appended.join().unwrap();
    assert_eq!(
        answer,
        Ok(1),
        "stats answered while the write was held, with the position before it"
    );
    assert_eq!(
        s.stats().unwrap().last_position,
        2,
        "and moved when it landed"
    );
}

/// A position asked on a runtime worker while the writer is held in a frame's write (the lock a stalled disk holds)
/// is waited for without holding the worker: on a runtime of ONE worker another task still runs, and when the write
/// lands the caller reads the position after it, the same locked read it always was. The caller says it has begun
/// before it asks, and the lock is held, so it cannot return: the worker is free for another task only if it was
/// handed on, and no task is spawned before the caller has the worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_position_asked_on_the_worker_waits_without_holding_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(small_segments(dir.path()));
    s.append(&[rec(0)]).unwrap();
    let gate = s.inner.wal.hold_writes();
    let appended = {
        let s = s.clone();
        std::thread::spawn(move || s.append(&[rec(1)]).unwrap())
    };
    gate.entered();
    let (begun, began) = mpsc::channel();
    let asked = {
        let s = s.clone();
        tokio::spawn(async move {
            begun.send(()).unwrap();
            s.last_position()
        })
    };
    began.recv().unwrap();
    // Waited for on this thread, not with a timer of the runtime: a worker that was held would hold the timer too.
    let (done, ran) = mpsc::channel();
    tokio::spawn(async move { done.send(7).unwrap() });
    let ran = ran.recv_timeout(Duration::from_secs(20));
    gate.release();
    assert_eq!(
        ran,
        Ok(7),
        "another task ran on the one worker while the position was waited for"
    );
    appended.join().unwrap();
    assert_eq!(
        asked.await.unwrap(),
        2,
        "the position after the write it waited for"
    );
}

#[test]
fn a_failed_roll_never_answers_or_indexes_the_frames_it_cut() {
    for ledger in [false, true] {
        let kind = if ledger { kinds::LEDGER } else { kinds::META };
        let dir = tempfile::tempdir().unwrap();
        let s = small_segments(dir.path());
        let held = s.inner.appending.write().unwrap();
        let mut answers = Vec::new();
        let jobs = (0..3)
            .map(|i| {
                let key = format!("roll_{i}");
                let record = if ledger {
                    NewRecord::json(
                        kind,
                        Some(&key),
                        &serde_json::json!({"kind": "test.row", "data": i}),
                    )
                    .unwrap()
                } else {
                    NewRecord::bytes(kind, Some(&key), vec![7; 100])
                };
                let (answer, answered) = mpsc::sync_channel(1);
                answers.push(answered);
                Job {
                    records: vec![record],
                    projected: vec![(None, None)],
                    tags: vec![vec![]],
                    hidden: None,
                    answer,
                }
            })
            .collect();
        s.inner.queued.fetch_add(3, Ordering::SeqCst);
        s.inner.wal.fail_next_sync();
        s.inner.commit(jobs);
        drop(held);
        for (i, answer) in answers.into_iter().enumerate() {
            assert!(
                answer.recv().unwrap().is_err(),
                "cut or unattempted job {i} succeeded"
            );
            assert!(s
                .latest_by_key(kind, &format!("roll_{i}"))
                .unwrap()
                .is_none());
        }
        assert_eq!(s.inner.wal.last_position(), 0);
        s.append(&[NewRecord::json(
            kinds::LEDGER,
            None,
            &serde_json::json!({"kind": "test.after"}),
        )
        .unwrap()])
            .unwrap();
        assert!(crate::chain_walk::verify(&s, None, None)
            .unwrap()
            .broken
            .is_none());
        s.append(&[NewRecord::bytes(kinds::META, Some("after"), vec![9; 100])])
            .unwrap();
        drop(s);
        let s = small_segments(dir.path());
        assert_eq!(
            s.latest_by_key(kinds::META, "after")
                .unwrap()
                .unwrap()
                .payload,
            vec![9; 100]
        );
        for i in 0..3 {
            assert!(s
                .latest_by_key(kind, &format!("roll_{i}"))
                .unwrap()
                .is_none());
        }
    }
}

#[test]
fn append_errors_distinguish_retained_frames_from_cut_frames() {
    let dir = tempfile::tempdir().unwrap();
    let s = small_segments(dir.path());
    let held = s.inner.appending.write().unwrap();
    let mut answers = Vec::new();
    let mut jobs: Vec<_> = ["retained", "cut"]
        .into_iter()
        .map(|key| {
            let (answer, answered) = mpsc::sync_channel(1);
            answers.push(answered);
            Job {
                records: vec![NewRecord::bytes(kinds::META, Some(key), vec![3; 100])],
                projected: vec![(None, None)],
                tags: vec![vec![]],
                hidden: None,
                answer,
            }
        })
        .collect();
    s.inner.queued.fetch_add(2, Ordering::SeqCst);
    let (written, _) = s.inner.write_batch(&mut jobs);
    assert!(written.iter().all(Result::is_ok));
    s.inner.wal.fail_next_sync();
    assert!(s.inner.sync_batch(true).is_err());
    assert_eq!(
        s.inner.wal.last_position(),
        1,
        "the roll saved only the first frame"
    );
    s.inner.answer_all(
        jobs,
        written,
        &Err(anyhow::anyhow!("index unavailable")),
        std::time::Instant::now(),
    );
    let mut answers = answers.into_iter();
    assert!(answers
        .next()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap_err()
        .is::<RetainedFrame>());
    assert!(!answers
        .next()
        .unwrap()
        .recv()
        .unwrap()
        .unwrap_err()
        .is::<RetainedFrame>());
    drop(held);
    drop(s);
    let s = small_segments(dir.path());
    assert!(s.latest_by_key(kinds::META, "retained").unwrap().is_some());
    assert!(s.latest_by_key(kinds::META, "cut").unwrap().is_none());
}

/// The first roll saves job zero; only the second segment is cut.
#[test]
fn a_later_failed_roll_keeps_the_first_job_and_its_chain() {
    partial_batch(false);
}

/// Exercise the actual commit path, including its refusal latch.
#[test]
fn commit_latches_a_partly_retained_unindexed_batch() {
    partial_batch(true);
}

fn partial_batch(index_fails: bool) {
    let dir = tempfile::tempdir().unwrap();
    let s = small_segments(dir.path());
    let held = s.inner.appending.write().unwrap();
    let count = if index_fails { 2 } else { 3 };
    let mut answers = Vec::new();
    let jobs = (0..count)
        .map(|i| {
            let (answer, answered) = mpsc::sync_channel(1);
            answers.push(answered);
            Job {
                records: vec![NewRecord::json(
                    kinds::LEDGER,
                    Some(&format!("partial_{i}")),
                    &serde_json::json!({"kind": "test.row", "data": i}),
                )
                .unwrap()],
                projected: vec![(None, None)],
                tags: vec![vec![]],
                hidden: None,
                answer,
            }
        })
        .collect();
    s.inner.queued.fetch_add(count, Ordering::SeqCst);
    s.inner.wal.fail_sync_after(1);
    if index_fails {
        s.inner.index_fail.store(3, Ordering::SeqCst);
    }
    s.inner.commit(jobs);
    drop(held);
    for (i, answer) in answers.into_iter().enumerate() {
        let result = answer.recv().unwrap();
        if i == 0 && !index_fails {
            assert!(result.is_ok(), "the first roll saved job zero");
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.is::<RetainedFrame>(), i == 0 && index_fails);
        }
    }
    assert_eq!(
        s.last_position(),
        2,
        "only job zero and its chain-start row survived"
    );
    let later = NewRecord::json(
        kinds::LEDGER,
        Some("later"),
        &serde_json::json!({"kind": "test.after"}),
    )
    .unwrap();
    if index_fails {
        assert_eq!(s.inner.index_fail.load(Ordering::SeqCst), 0);
        assert!(
            s.append(std::slice::from_ref(&later)).is_err(),
            "latched store accepted a frame"
        );
        assert!(
            s.checkpoint().is_err(),
            "latched store accepted a checkpoint"
        );
        assert_eq!(s.last_position(), 2);
    } else {
        s.append(std::slice::from_ref(&later)).unwrap();
        assert!(crate::chain_walk::verify(&s, None, None)
            .unwrap()
            .broken
            .is_none());
    }
    drop(s);
    let s = small_segments(dir.path());
    let rows = s.scan(0, None, 100).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.key.as_deref() == Some("partial_0"))
            .count(),
        1
    );
    for i in 1..count {
        assert!(s
            .latest_by_key(kinds::LEDGER, &format!("partial_{i}"))
            .unwrap()
            .is_none());
    }
    assert!(crate::chain_walk::verify(&s, None, None)
        .unwrap()
        .broken
        .is_none());
    if index_fails {
        s.append(&[later]).unwrap();
        s.checkpoint().unwrap();
    }
    assert!(crate::chain_walk::verify(&s, None, None)
        .unwrap()
        .broken
        .is_none());
}
