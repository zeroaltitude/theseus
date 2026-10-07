//! Health's session counts past an import (theseus-revl): the owner's own
//! sessions, the imported ones still held, and the erased ones are counted
//! apart, through the projection and through the kept-whole fallback; the
//! three add up to the session keys; and a health answer reads the import's
//! tag records, never a session of it.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::SessionKind;
use theseus_store::{records_read_here, Store as _, WalConfig, WalStore};

use super::Core;
use crate::import::{episode, write};
use crate::provider::FakeProvider;
use crate::session::SessionRecord;
use crate::Config;

const TAG: &str = "plover-2026-03";
const LATER: &str = "plover-2026-04";

/// A synthetic episode: one message, no summary, invented text.
fn episode_line(tag: &str, i: usize) -> String {
    let mut v = json!({
        "format": 1, "import_tag": tag, "episode_id": format!("ep_{:064x}", 0x91e7_0000 + i),
        "source": episode::SOURCES[i % episode::SOURCES.len()], "agent": null,
        "place": {"kind": "dm", "name": format!("plover-{}", i % 7)},
        "as_of": {"start": "2026-03-03T09:00:00Z", "end": "2026-03-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": ["plover/count"]},
        "summary": null,
        "messages": [{"idx": 0, "time": "2026-03-03T09:00:00Z", "author": "wren",
            "integrity": "operator", "text": format!("Plover count {i}: twelve on the flats."),
            "unit": format!("unit-{i}"), "sha256": "cd".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

/// Import `n` synthetic episodes of `tag`, numbered from `from`.
fn import(core: &Core, tag: &str, from: usize, n: usize) {
    let lines: Vec<ImportLine> = (from..from + n)
        .map(|i| ImportLine {
            line: i as u64 + 1,
            text: episode_line(tag, i),
        })
        .collect();
    for chunk in lines.chunks(500) {
        let r = write::import_batch(
            &core.store,
            &ImportEpisodesParams {
                file: "plovers.jsonl".into(),
                lines: chunk.to_vec(),
            },
            "test",
        )
        .unwrap();
        assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    }
}

fn core_on(store: crate::store::Store, dir: &std::path::Path) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    Core::build(super::Parts::for_tests(cfg, fake, store)).unwrap()
}

fn open(core: &Core) {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
}

/// What health says of the sessions: own, imported held, erased, turns.
fn counts(core: &Core) -> (u64, u64, u64, u64) {
    let h = core.health();
    (h.sessions, h.imported.sessions, h.imported.erased, h.turns)
}

/// The three counts as a stage of the store should read: the live sessions
/// by their records, the tags' counts through `import.list`, and the keys.
fn check(core: &Core, when: &str, live: u64, held: u64, erased: u64) {
    let (own, imported, gone, _) = counts(core);
    let list = write::list(&core.store).unwrap();
    assert_eq!(
        own,
        core.store.live_sessions::<SessionRecord>().unwrap().len() as u64,
        "{when}: the owner's sessions are the live ones"
    );
    assert_eq!(
        imported,
        list.tags.iter().map(|t| t.sessions - t.erased).sum::<u64>(),
        "{when}: imported held"
    );
    assert_eq!(
        gone,
        list.tags.iter().map(|t| t.erased).sum::<u64>(),
        "{when}: erased"
    );
    assert_eq!(
        own + imported + gone,
        core.store.session_count().unwrap(),
        "{when}: the three add up to the keys"
    );
    assert_eq!((own, imported, gone), (live, held, erased), "{when}");
}

/// Four live sessions, two imports, one tag erased: the counts apart at each
/// stage, by the projection and by the fallback's read of every session.
#[test]
fn health_counts_the_owners_sessions_apart_from_imported_and_erased_ones() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let core = core_on(crate::store::Store::open(&store_dir).unwrap(), dir.path());
    (0..2).for_each(|_| open(&core));
    check(&core, "no import", 2, 0, 0);
    assert_eq!(core.health().imported, Default::default());
    import(&core, TAG, 0, 300);
    open(&core);
    check(&core, "one import", 3, 300, 0);
    import(&core, LATER, 300, 200);
    open(&core);
    check(&core, "two imports", 4, 500, 0);
    write::erase(&core.store, TAG, Some("a test's erase"), "test").unwrap();
    check(&core, "a tag erased", 4, 200, 300);
    // Imported sessions add no turn, token or cost to the projection.
    assert_eq!(counts(&core).3, 0);
    assert_eq!(core.health().cost_usd_total, 0.0);
    assert!(core
        .store
        .inner()
        .totals(theseus_store::kinds::SESSION)
        .unwrap()
        .is_some());
    let by_projection = counts(&core);
    drop(core);

    // A store an older build wrote last: its terms are not whole, so health
    // reads every session record, and says the same three.
    let plain = WalStore::open(&store_dir, WalConfig::default()).unwrap();
    plain
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::LEDGER,
            None,
            &json!({"at": 1}),
        )
        .unwrap()])
        .unwrap();
    plain.checkpoint().unwrap();
    drop(plain);
    let core = core_on(crate::store::Store::open(&store_dir).unwrap(), dir.path());
    assert!(!core.store.inner().terms_whole(), "the terms are not built");
    assert!(core
        .store
        .inner()
        .totals(theseus_store::kinds::SESSION)
        .unwrap()
        .is_none());
    assert_eq!(counts(&core), by_projection, "the fallback's three");
    check(&core, "the fallback", 4, 200, 300);
}

/// Health is polled, so it reads the tags' records and no session: at 21,151
/// imported sessions it reads one record a tag, as it does at 1,000.
#[test]
fn health_reads_the_tag_records_and_never_a_session_of_the_import() {
    let dir = tempfile::tempdir().unwrap();
    let core = core_on(
        crate::store::Store::open_unsynced(&dir.path().join("store")).unwrap(),
        dir.path(),
    );
    (0..4).for_each(|_| open(&core));
    let reads = || {
        core.health();
        let before = records_read_here();
        let h = core.health();
        (
            records_read_here() - before,
            h.sessions,
            h.imported.sessions,
        )
    };
    let (none, ..) = reads();
    import(&core, TAG, 0, 1_000);
    let (small, own, held) = reads();
    assert_eq!((own, held), (4, 1_000));
    import(&core, LATER, 1_000, 20_151);
    let (large, own, held) = reads();
    assert_eq!((own, held), (4, 21_151));
    eprintln!("health reads (records): {none} with no import, {small} at 1,000, {large} at 21,151");
    // One record a tag, never one a session.
    assert_eq!((none, small, large), (0, 1, 2));
}
