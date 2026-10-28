//! A META key's end (theseus-1kw2v): a `TOMBSTONE` record takes its key out
//! of every read by key, as it is appended, replayed from the log, built in
//! bulk, or built after serving, and a write of the key afterwards starts it
//! afresh. The records the key held stay in the log.

use super::*;
use crate::record::kinds;

fn open(dir: &Path) -> WalStore {
    WalStore::open(dir, WalConfig::default())
        .unwrap()
        .with_checkpoint_every(0)
}

fn meta(key: &str, v: u64) -> NewRecord {
    NewRecord::json(kinds::META, Some(key), &v).unwrap()
}

/// What a store answers about META's keys: each key's value in key order,
/// how many keys there are, those under a prefix and with an ending, and the
/// keys by birth, newest first.
type Answers = (
    Vec<(String, u64)>,
    u64,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

fn answers(s: &WalStore) -> Answers {
    let latest = s
        .latest_of_kind(kinds::META)
        .unwrap()
        .into_iter()
        .map(|r| (r.key.clone().unwrap(), r.decode::<u64>().unwrap()))
        .collect();
    let under = s
        .latest_with_prefix(kinds::META, "a.")
        .unwrap()
        .into_iter()
        .map(|r| r.key.unwrap())
        .collect();
    let (born, _) = s
        .inner
        .index
        .keys_by_birth(kinds::META, None, usize::MAX)
        .unwrap();
    let newest = born.into_iter().map(|(_, key, _)| key).collect();
    (
        latest,
        s.count_keys(kinds::META).unwrap(),
        under,
        s.keys_ending(kinds::META, ".1", 100).unwrap(),
        newest,
    )
}

/// Three keys, one written again, one ended, a key never written ended, an
/// ended one ended again, and an ended one written afresh. Positions 1 to 8.
fn script(s: &WalStore) {
    s.append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    s.append(&[meta("a.2", 20), NewRecord::delete_meta("a.1")])
        .unwrap();
    s.append(&[NewRecord::delete_meta("never.written")])
        .unwrap();
    s.append(&[NewRecord::delete_meta("a.1")]).unwrap();
    s.append(&[meta("a.1", 100)]).unwrap();
    s.append(&[NewRecord::delete_meta("b.1")]).unwrap();
}

/// What the script leaves, in `Answers`' order: `a.1` born again at its last
/// write, `a.2` at its first, and `b.1` gone.
fn want() -> Answers {
    let keys = |ks: &[&str]| ks.iter().map(|k| k.to_string()).collect::<Vec<String>>();
    (
        vec![("a.1".to_string(), 100), ("a.2".to_string(), 20)],
        2,
        keys(&["a.1", "a.2"]),
        keys(&["a.1"]),
        keys(&["a.1", "a.2"]),
    )
}

#[test]
fn an_ended_meta_key_is_gone_from_every_read_by_key_and_a_write_after_starts_it_afresh() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    s.append(&[meta("a.2", 20), NewRecord::delete_meta("a.1")])
        .unwrap();
    assert!(s.latest_by_key(kinds::META, "a.1").unwrap().is_none());
    assert_eq!(s.count_keys(kinds::META).unwrap(), 2);
    let (latest, count, under, ending, _) = answers(&s);
    assert_eq!(
        latest,
        vec![("a.2".to_string(), 20), ("b.1".to_string(), 3)]
    );
    assert_eq!(
        (count, under, ending),
        (2, vec!["a.2".to_string()], vec!["b.1".to_string()])
    );
    // Ending a key never written, or already ended, changes nothing.
    s.append(&[NewRecord::delete_meta("never.written")])
        .unwrap();
    s.append(&[NewRecord::delete_meta("a.1")]).unwrap();
    assert_eq!(s.count_keys(kinds::META).unwrap(), 2);
    // Written again, it is a new key: its birth is the new record's.
    s.append(&[meta("a.1", 100)]).unwrap();
    let (latest, count, _, _, newest) = answers(&s);
    assert_eq!(
        latest,
        [("a.1", 100), ("a.2", 20), ("b.1", 3)].map(|(k, v)| (k.to_string(), v))
    );
    assert_eq!(count, 3);
    assert_eq!(newest, ["a.1", "b.1", "a.2"]);
    // The log keeps what the key held, and the end.
    let history: Vec<RecordKind> = s
        .scan(0, None, 100)
        .unwrap()
        .into_iter()
        .filter(|r| r.key.as_deref() == Some("a.1"))
        .map(|r| r.kind)
        .collect();
    assert_eq!(
        history,
        [kinds::META, kinds::TOMBSTONE, kinds::TOMBSTONE, kinds::META]
    );
}

/// The same answers after every way a store comes back: closed with a
/// checkpoint, closed with none (the whole log replays), its index lost, and
/// a checkpoint in the middle of the script (the tail replays).
#[test]
fn an_ended_meta_key_stays_ended_through_every_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    script(&s);
    let live = answers(&s);
    assert_eq!(live, want());
    s.checkpoint().unwrap();
    drop(s);
    assert_eq!(answers(&open(dir.path())), live, "closed with a checkpoint");

    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    script(&s);
    drop(s);
    assert_eq!(
        answers(&open(dir.path())),
        live,
        "no checkpoint: all replays"
    );

    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    script(&s);
    s.checkpoint().unwrap();
    drop(s);
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    assert_eq!(answers(&open(dir.path())), live, "the index lost");

    // A checkpoint between the writes, and the rest of the script after it.
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.append(&[meta("a.1", 1), meta("a.2", 2), meta("b.1", 3)])
        .unwrap();
    s.checkpoint().unwrap();
    s.append(&[meta("a.2", 20), NewRecord::delete_meta("a.1")])
        .unwrap();
    s.append(&[NewRecord::delete_meta("never.written")])
        .unwrap();
    s.append(&[NewRecord::delete_meta("a.1")]).unwrap();
    s.append(&[meta("a.1", 100)]).unwrap();
    s.append(&[NewRecord::delete_meta("b.1")]).unwrap();
    drop(s);
    let s = open(dir.path());
    assert!(
        s.stats().unwrap().replayed_into_index > 0,
        "the tail replays"
    );
    assert_eq!(answers(&s), live, "the tail after a checkpoint");
}

/// A replay of `BULK` records or more is built in key order, a table at a
/// time; one that holds an end goes in position order instead.
#[test]
fn a_long_replay_that_holds_ends_gives_the_answers_of_the_appends() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let mut alive = std::collections::BTreeMap::new();
    for chunk in 0..60u64 {
        let mut batch = Vec::new();
        for i in 0..100u64 {
            let n = chunk * 100 + i;
            batch.push(meta(&format!("k{n:05}"), n));
            alive.insert(format!("k{n:05}"), n);
        }
        s.append(&batch).unwrap();
    }
    // Every third key ended, every ninth written again afterwards.
    for chunk in 0..20u64 {
        let ends: Vec<NewRecord> = (0..100u64)
            .map(|i| chunk * 300 + i * 3)
            .filter(|n| *n < 6000)
            .map(|n| {
                alive.remove(&format!("k{n:05}"));
                NewRecord::delete_meta(&format!("k{n:05}"))
            })
            .collect();
        s.append(&ends).unwrap();
    }
    for n in (0..6000u64).step_by(9) {
        s.append(&[meta(&format!("k{n:05}"), n + 1)]).unwrap();
        alive.insert(format!("k{n:05}"), n + 1);
    }
    let want: Vec<(String, u64)> = alive.into_iter().collect();
    assert_eq!(answers(&s).0, want);
    assert!(
        s.last_position() as usize > BULK,
        "the replay is long enough to be built in bulk"
    );
    drop(s);
    std::fs::remove_file(dir.path().join("index.redb")).unwrap();
    let s = open(dir.path());
    assert!(s.stats().unwrap().replayed_into_index as usize > BULK);
    let (latest, count, ..) = answers(&s);
    assert_eq!(count, want.len() as u64);
    assert_eq!(latest, want);
}

/// A key the log ended is not born again by the records it held when the
/// index's shape is built after serving.
#[test]
fn a_shape_built_after_serving_does_not_birth_an_ended_key() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    script(&s);
    assert_eq!(answers(&s), want());
    s.checkpoint().unwrap();
    drop(s);
    // As an older build leaves it: no shape mark at its checkpoint, so the
    // open drops the shape's tables and `build_shape` builds them again.
    {
        let db = redb::Database::create(dir.path().join("index.redb")).unwrap();
        let txn = db.begin_write().unwrap();
        {
            let mut meta = txn.open_table(crate::index::META).unwrap();
            meta.remove(crate::index::SHAPE).unwrap();
        }
        txn.commit().unwrap();
    }
    let s = open(dir.path());
    assert!(s.stats().unwrap().shape_pending && !s.shaped());
    let mut at = None;
    loop {
        at = s.build_shape(at, 2).unwrap();
        if at.is_none() {
            break;
        }
    }
    assert!(s.shaped());
    assert_eq!(answers(&s), want());
}
