//! The session lists past an import (theseus-7087): the whole list, a page
//! of `session.list { n }`, and `confirm.list` read no imported session's
//! record, a page steps over the import's run of births in one walk, and
//! every answer equals what the reads they replaced gave (every record
//! decoded and filtered; the births walked `n` at a time), with live
//! sessions opened before, between, and after two imports and an erase.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::{CompilationListParams, SessionKind, SessionListParams};
use theseus_store::{index_rows_here, kinds, records_read_here, Store as _};

use super::Core;
use crate::bus::EventSink;
use crate::import::{episode, is_imported, write};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use crate::Config;

const TAG: &str = "tern-2026-04";
const LATER: &str = "tern-2026-05";

/// A synthetic episode: one message, no summary, invented text.
fn episode_line(tag: &str, i: usize) -> String {
    let mut v = json!({
        "format": 1, "import_tag": tag, "episode_id": format!("ep_{:064x}", 0x7e44_0000 + i),
        "source": episode::SOURCES[i % episode::SOURCES.len()], "agent": null,
        "place": {"kind": "dm", "name": format!("tern-{}", i % 9)},
        "as_of": {"start": "2026-04-03T09:00:00Z", "end": "2026-04-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": ["tern/count"]},
        "summary": null,
        "messages": [{"idx": 0, "time": "2026-04-03T09:00:00Z", "author": "wren",
            "integrity": "operator", "text": format!("Tern count {i}: forty on the north spit."),
            "unit": format!("unit-{i}"), "sha256": "ab".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    serde_json::to_string(&v).unwrap()
}

/// Import `n` synthetic episodes of `tag`, numbered from `from`, in batches.
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
                file: "terns.jsonl".into(),
                lines: chunk.to_vec(),
            },
            "test",
        )
        .unwrap();
        assert!(r.rejected.is_empty(), "{:?}", r.rejected);
        assert_eq!(r.imported as usize, chunk.len());
    }
}

struct Rig {
    core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// A core whose writes wait for the operator, and whose model writes a file
/// once: a turn parks on a question `confirm.list` lists.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Approve;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![Scripted::tools(
        "",
        &[(
            "t1",
            "fs_write",
            json!({"path": "terns.txt", "content": "forty\n"}),
        )],
    )]));
    let core = Core::build(super::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig { core, _dir: dir }
}

fn open(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

/// A session parked on a write's question.
async fn parked(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    let sid = rec.session_id.clone();
    let res = core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("write the tern count".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    sid
}

/// What the whole list answered when it read every session: each record
/// decoded, the imported ones dropped, the most recently active first.
fn whole_as_before(core: &Core) -> Vec<String> {
    let mut recs: Vec<SessionRecord> = core.store.list_sessions().unwrap();
    recs.sort_by(|a, b| {
        b.last_active_ms
            .max(b.created_at_unix_ms)
            .cmp(&a.last_active_ms.max(a.created_at_unix_ms))
    });
    recs.into_iter()
        .filter(|r| r.imported.is_none())
        .map(|r| r.session_id)
        .collect()
}

/// Every session key by birth, newest first: one read, which the pages of
/// one stage are checked against.
fn births(core: &Core) -> Vec<(u64, String)> {
    let (born, more) = core
        .store
        .inner()
        .newest_keys(kinds::SESSION, None, usize::MAX)
        .unwrap()
        .unwrap();
    assert!(!more);
    born.into_iter().map(|(b, r)| (b, r.key.unwrap())).collect()
}

/// What a page answered when it walked the births `n` at a time, skipping
/// imported keys: the live keys born before `before`, newest first, up to
/// `n`, and a cursor at the last when any key, live or not, is older.
/// (theseus-store's `tests_keyed` holds the walk itself to this rule.)
fn page_as_before(
    births: &[(u64, String)],
    n: usize,
    before: Option<u64>,
) -> (Vec<String>, Option<u64>) {
    let older: Vec<&(u64, String)> = births
        .iter()
        .filter(|(b, _)| before.is_none_or(|c| *b < c))
        .collect();
    let mut out = Vec::new();
    for (i, (b, k)) in older.iter().enumerate() {
        if is_imported(k) {
            continue;
        }
        out.push(k.clone());
        if out.len() == n {
            return (out, (i + 1 < older.len()).then_some(*b));
        }
    }
    (out, None)
}

fn page(core: &Core, n: usize, before: Option<u64>) -> (Vec<String>, Option<u64>) {
    let r = core
        .session_list_of(SessionListParams {
            n: Some(n),
            before,
            ..Default::default()
        })
        .unwrap();
    (
        r.sessions.into_iter().map(|s| s.session_id).collect(),
        r.older,
    )
}

/// Every answer equals the one its old read gave: the whole list, and
/// every page of every size, cursor by cursor. Returns how many live
/// sessions the whole list holds.
fn answers_agree(core: &Core, when: &str) -> usize {
    let whole: Vec<String> = core
        .session_list()
        .unwrap()
        .into_iter()
        .map(|s| s.session_id)
        .collect();
    assert_eq!(whole, whole_as_before(core), "{when}: the whole list");
    let births = births(core);
    for n in [1, 2, 3, 4, 20, 1000] {
        let mut cursor = None;
        let mut seen = 0;
        loop {
            let got = page(core, n, cursor);
            assert_eq!(
                got,
                page_as_before(&births, n, cursor),
                "{when}: n {n} from {cursor:?}"
            );
            seen += got.0.len();
            match got.1 {
                Some(b) => cursor = Some(b),
                None => break,
            }
        }
        assert_eq!(
            seen,
            whole.len(),
            "{when}: n {n} pages every live session once"
        );
    }
    whole.len()
}

/// Records `f` reads from the log, once it has run once (a cache warmed).
fn reads<R>(f: impl Fn() -> R) -> u64 {
    f();
    let before = records_read_here();
    f();
    records_read_here() - before
}

/// Index rows `f` visits (theseus-26jo), once it has run once.
fn rows<R>(f: impl Fn() -> R) -> u64 {
    f();
    let before = index_rows_here();
    f();
    index_rows_here() - before
}

/// The index rows the whole list, `confirm.list`, and `compilation.list`
/// visit: each walks the live sessions' keys alone (theseus-26jo).
fn list_rows(core: &Core) -> [u64; 3] {
    [
        rows(|| core.session_list().unwrap()),
        rows(|| core.confirm_list().unwrap()),
        rows(|| {
            core.compilation_list(CompilationListParams::default())
                .unwrap()
        }),
    ]
}

/// The reads of the whole list, a page of 20, the page past the import's
/// run, `confirm.list`, and `compilation.list`, whole and of the session
/// `mine` (theseus-ve34).
fn list_reads(core: &Core, past: Option<u64>, mine: &str) -> [u64; 6] {
    let of = |session_id: Option<&str>| CompilationListParams {
        session_id: session_id.map(str::to_string),
        n: None,
    };
    [
        reads(|| core.session_list().unwrap()),
        reads(|| page(core, 20, None)),
        reads(|| page(core, 20, past)),
        reads(|| core.confirm_list().unwrap()),
        reads(|| core.compilation_list(of(None)).unwrap()),
        reads(|| core.compilation_list(of(Some(mine))).unwrap()),
    ]
}

/// Two imports of 1,000 synthetic sessions each, with live sessions opened
/// before, between, and after them, one parked on a question: each list
/// answers as its old read did, and the second import, and an erase, add
/// not one record to what any list reads. Its old reads read every imported
/// session's record (the whole list, `confirm.list`) or its births' records
/// `n` at a time (a page).
#[tokio::test]
async fn the_session_lists_read_no_imported_session_and_answer_as_before() {
    let r = rig();
    let c = &r.core;
    let early: Vec<String> = (0..3).map(|_| open(c)).collect();
    answers_agree(c, "before any import");
    import(c, TAG, 0, 1_000);
    let waiting = parked(c).await;
    let between = open(c);
    assert_eq!(answers_agree(c, "after the first import"), 5);
    // The page that reaches back past the first import's run.
    let past = page(c, 2, None).1;
    assert!(past.is_some());
    // Past the run, a page visits its 1,000 birth rows and looks up only
    // its live keys: no key-table row per imported key (theseus-26jo).
    let past_rows = rows(|| page(c, 20, past));
    assert!(past_rows < 1_000 + 60, "{past_rows}");
    // One session's compilations are marked from its own record alone
    // (theseus-26jo): the scan of its scope, and one record more.
    let mine = CompilationListParams {
        session_id: Some(waiting.clone()),
        n: None,
    };
    let listed = c.compilation_list(mine.clone()).unwrap().compilations;
    assert_eq!(listed.iter().filter(|x| x.current).count(), 1, "{listed:?}");
    assert_eq!(
        reads(|| c.compilation_list(mine.clone()).unwrap()),
        reads(|| c.store.session_compilations(&waiting).unwrap()) + 1,
        "compilation.list {{session_id}} reads its own session's record alone"
    );
    let asked = c.confirm_list().unwrap();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].session_id, waiting);

    let read_before = list_reads(c, past, &waiting);
    // Its own sessions' records and their executions', not one per imported
    // key or per `n` of them.
    assert!(read_before.iter().all(|r| *r < 60), "{read_before:?}");
    // The live sessions' key rows and their executions' lookups, not one row
    // per imported key: the walks step past the import's run unvisited.
    let rows_before = list_rows(c);
    assert!(rows_before.iter().all(|r| *r < 60), "{rows_before:?}");
    import(c, LATER, 1_000, 1_000);
    assert_eq!(
        list_reads(c, past, &waiting),
        read_before,
        "the second import adds no read to the whole list, a page, the page past the runs, confirm.list, or compilation.list"
    );
    assert_eq!(
        list_rows(c),
        rows_before,
        "the second import adds no index row to the whole list, confirm.list, or compilation.list"
    );
    let late: Vec<String> = (0..2).map(|_| open(c)).collect();
    assert_eq!(answers_agree(c, "after the second import"), 7);
    // The newest page holds the live sessions opened after both runs, and
    // the one before them, past 2,000 imported births.
    let (newest, older) = page(c, 4, None);
    assert_eq!(newest[..2], [late[1].clone(), late[0].clone()]);
    assert_eq!(newest[2], between);
    assert_eq!(newest[3], waiting);
    let (rest, last) = page(c, 4, older);
    assert_eq!(rest, [early[2].clone(), early[1].clone(), early[0].clone()]);
    assert_eq!(last, None);

    // An erase still hides: its tombstones are imported sessions too.
    let read_before = list_reads(c, past, &waiting);
    let rows_before = list_rows(c);
    write::erase(&c.store, TAG, Some("a test's erase"), "test").unwrap();
    assert_eq!(
        list_reads(c, past, &waiting),
        read_before,
        "an erase adds no read"
    );
    assert_eq!(list_rows(c), rows_before, "an erase adds no index row");
    assert_eq!(answers_agree(c, "after an erase"), 7);
    assert_eq!(c.store.session_count().unwrap(), 2_007);
}

/// An import first, then three live sessions (theseus-ve34): the live ones
/// are the newest births, and nothing older is live, so a full page whose
/// older keys are all imported still names its cursor, as the walk `n` at a
/// time did, and the page after it is empty. Every page of every size,
/// cursor by cursor, equals the old walk's.
#[test]
fn the_pages_of_a_store_whose_import_came_first_answer_as_before() {
    let r = rig();
    let c = &r.core;
    import(c, TAG, 0, 300);
    let live: Vec<String> = (0..3).map(|_| open(c)).collect();
    let births = births(c);
    for n in [1, 2, 3, 4, 5, 20, 1000] {
        let mut cursor = None;
        let mut seen = Vec::new();
        loop {
            let got = page(c, n, cursor);
            assert_eq!(
                got,
                page_as_before(&births, n, cursor),
                "n {n} from {cursor:?}"
            );
            seen.extend(got.0);
            match got.1 {
                Some(b) => cursor = Some(b),
                None => break,
            }
        }
        assert_eq!(seen, [live[2].as_str(), &live[1], &live[0]], "n {n}");
    }
    // The case the old tests never met: a full page with only imported
    // keys older keeps its cursor.
    let (three, older) = page(c, 3, None);
    assert_eq!(three.len(), 3);
    assert!(older.is_some());
    assert_eq!(page(c, 3, older), (vec![], None));
}

/// What the lists cost past a full import (theseus-26jo): 21,151 imported
/// sessions between live ones, the whole list and a page of 20 (which
/// crosses the import's run of births) each timed 20 times, with the index
/// rows and records each visits. A measure, not a check: run it with
/// `--ignored --nocapture`, in a debug and a release build.
#[test]
#[ignore = "a measure: 21,151 imported sessions, printed"]
fn the_session_lists_past_a_full_import_timed() {
    let r = rig();
    let c = &r.core;
    for _ in 0..3 {
        open(c);
    }
    import(c, TAG, 0, 21_151);
    for _ in 0..2 {
        open(c);
    }
    let time = |what: &str, f: &dyn Fn()| {
        f();
        let (rows, read) = (index_rows_here(), records_read_here());
        f();
        let (rows, read) = (index_rows_here() - rows, records_read_here() - read);
        let mut ms: Vec<f64> = (0..20)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        println!(
            "{what}: p50 {:.2} ms, max {:.2} ms, {rows} index rows, {read} records",
            ms[10], ms[19]
        );
    };
    time("whole list", &|| drop(c.session_list().unwrap()));
    time("page of 20", &|| drop(page(c, 20, None)));
}
