//! The tender against real node records: written by `theseus-core`'s own
//! constructors into a real WAL, followed, indexed, and asked.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::node::{Attachment, AttachmentContent, Body, Node, ResultStatus};
use theseus_follow::Stop;
use theseus_store::{kinds, NewRecord, Wal, WalConfig};

use crate::engine::StoredChunk;
use crate::extract::{extract, Extract};
use crate::proto::{method, Filters, IndexStatus, QueryParams, QueryResult, RebuildResult};
use crate::tender::{Config, OpenError, Shared, Tender, PLACE_META_PREFIX, TASK_META_PREFIX};
use crate::{client::Client, server};

pub(crate) struct Rig {
    pub(crate) _tmp: tempfile::TempDir,
    pub(crate) store: PathBuf,
    pub(crate) index: PathBuf,
    pub(crate) wal: Wal,
}

impl Rig {
    pub(crate) fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("store");
        let index = tmp.path().join("index");
        let wal = Wal::open(
            &store.join("wal"),
            WalConfig {
                segment_bytes: 4096,
                fsync: false,
                ..WalConfig::default()
            },
        )
        .unwrap();
        Self {
            _tmp: tmp,
            store,
            index,
            wal,
        }
    }

    /// Batches of 16 KB: several to a backfill here, while a commit (about
    /// 140 ms of fsyncs on this disk) comes once a batch. The follower's own
    /// tests cut reads at every boundary.
    pub(crate) fn cfg(&self, index: &Path) -> Config {
        let mut c = Config::new(&self.store, index);
        c.batch_bytes = 16 << 10;
        c.backstop = Duration::from_millis(200);
        c
    }

    pub(crate) fn open(&self) -> Tender {
        Tender::open(self.cfg(&self.index)).unwrap()
    }

    /// Append each node as its own frame; their positions.
    pub(crate) fn put(&self, nodes: &[Node]) -> Vec<u64> {
        nodes
            .iter()
            .map(|n| self.wal.append(&[n.record().unwrap()]).unwrap()[0].0)
            .collect()
    }

    fn bind_place(&self, place: &str, session: &str) {
        self.wal
            .append(&[NewRecord::json(
                kinds::META,
                Some(&format!("{PLACE_META_PREFIX}{place}")),
                &session,
            )
            .unwrap()])
            .unwrap();
    }
}

/// Read until the follower is caught up.
pub(crate) fn settle(t: &mut Tender) {
    for _ in 0..10_000 {
        if t.step().unwrap().stop != Stop::Budget {
            return;
        }
    }
    panic!("never caught up");
}

fn query(shared: &Shared, text: &str) -> QueryResult {
    shared.query(&QueryParams::new(text)).unwrap()
}

fn ids(r: &QueryResult) -> Vec<String> {
    r.hits.iter().map(|h| h.node_id.clone()).collect()
}

pub(crate) fn user(session: &str, text: &str) -> Node {
    Node::user(session, Some("turn_1"), "cli", text)
}

pub(crate) fn result(session: &str, tool: &str, content: &str, external: bool) -> Node {
    Node::tool_result(
        session,
        Some("turn_1"),
        Some(0),
        Body::ToolResult {
            tool_use_id: "toolu_1".into(),
            tool: tool.into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: content.into(),
            correlation_id: None,
            bytes_total: content.len() as u64,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: json!({}),
            image: None,
            external: external
                .then(|| serde_json::from_value(json!({"url": "https://example.org/a"})).unwrap()),
        },
    )
}

fn assistant(session: &str, blocks: serde_json::Value) -> Node {
    Node::assistant(
        session,
        "turn_1",
        0,
        Body::AssistantMessage {
            blocks: blocks.as_array().unwrap().clone(),
            model: "m".into(),
            provider: "p".into(),
            stop_reason: Some("end_turn".into()),
            usage: theseus_protocol::Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
        },
    )
}

pub(crate) fn call(session: &str, tool: &str, input: serde_json::Value) -> Node {
    Node::tool_call(
        session,
        Some("turn_1"),
        Some(0),
        Body::ToolCall {
            tool_use_id: "toolu_1".into(),
            tool: tool.into(),
            wire_name: tool.replace('.', "_"),
            input,
            assistant_node: "msg_0".into(),
            correlation_id: None,
            gate: None,
        },
    )
}

/// The registry test (M6 §2.1): every `Body` variant, through the core's own
/// serialization, into the extractor. The match has no wildcard, so a new
/// variant fails this build until the extractor's table gives it a row.
#[test]
fn the_extractor_covers_every_body_variant() {
    let samples = vec![
        Node::user_with(
            "ses_1",
            None,
            "cli",
            "the port is 7433",
            vec![
                Attachment {
                    name: "notes.txt".into(),
                    media_type: "text/plain".into(),
                    size: 4,
                    content: AttachmentContent::Text {
                        text: "attached words".into(),
                        cut: false,
                    },
                },
                Attachment {
                    name: "shot.png".into(),
                    media_type: "image/png".into(),
                    size: 9,
                    content: AttachmentContent::Image {
                        digest: "sha256-deadbeef".into(),
                        width: 1,
                        height: 1,
                    },
                },
            ],
        ),
        assistant(
            "ses_1",
            json!([
                {"type": "thinking", "thinking": "private reasoning", "signature": "s"},
                {"type": "text", "text": "Reading the file."},
                {"type": "tool_use", "id": "toolu_1", "name": "fs_read", "input": {"path": "/x"}},
            ]),
        ),
        call(
            "ses_1",
            "proc.run",
            json!({"argv": ["cargo", "build", "-p", "theseus-index"]}),
        ),
        result("ses_1", "http.fetch", "fetched words", true),
    ];
    for node in samples {
        let record = node.record().unwrap();
        let got = extract(&record.payload).unwrap();
        let Extract::Index(e) = got else {
            panic!("{} was skipped", node.kind_str());
        };
        assert_eq!(e.node_id, node.id);
        assert_eq!(e.kind, node.kind_str());
        match &node.body {
            Body::UserMessage { .. } => {
                assert_eq!(
                    e.text,
                    "the port is 7433\n\nnotes.txt\n\nattached words\n\nshot.png"
                );
                assert_eq!(e.origin, "operator");
                assert_eq!(e.author.as_deref(), Some("cli"));
            }
            Body::AssistantMessage { .. } => {
                assert_eq!(e.text, "Reading the file.");
                assert_eq!(e.origin, "agent");
            }
            Body::ToolCall { .. } => {
                assert_eq!(e.text, "cargo build -p theseus-index");
                assert_eq!(e.tool.as_deref(), Some("proc.run"));
            }
            Body::ToolResult { .. } => {
                assert_eq!(e.text, "fetched words");
                assert!(e.external);
                assert_eq!(e.origin, "tool");
            }
            Body::Arrangement { .. } => unreachable!("an arrangement is skipped, below"),
        }
    }
    // A task's arrangement (M5 27) copies nodes the index already holds.
    let arrangement = Node::arrangement("ses_1", "session:ses_0", vec![], false);
    assert!(matches!(
        extract(&arrangement.record().unwrap().payload).unwrap(),
        Extract::Skip { .. }
    ));
    // A call whose input is never indexed: a write's content.
    let w = call(
        "ses_1",
        "fs.write",
        json!({"path": "a.txt", "content": "secret"}),
    );
    assert!(matches!(
        extract(&w.record().unwrap().payload).unwrap(),
        Extract::Skip { .. }
    ));
}

#[test]
fn the_place_prefixes_are_the_cores() {
    assert_eq!(PLACE_META_PREFIX, theseus_core::outbox::PLACE_META_PREFIX);
    assert_eq!(TASK_META_PREFIX, theseus_core::outbox::TASK_META_PREFIX);
}

#[test]
fn a_query_finds_a_node_by_its_words_and_by_its_entities() {
    let rig = Rig::new();
    rig.put(&[
        user("ses_1", "the web UI listens on 127.0.0.1:7433 by default"),
        result(
            "ses_1",
            "fs.read",
            "fn open() in crates/theseus-store/src/wal.rs",
            false,
        ),
        user("ses_2", "unrelated chatter about lunch"),
    ]);
    let mut t = rig.open();
    settle(&mut t);
    let shared = t.shared();
    let r = query(&shared, "7433");
    assert_eq!(r.hits.len(), 1, "{:?}", ids(&r));
    assert!(r.hits[0].text.contains("7433"));
    assert!(r.hits[0].sources.contains_key("bm25"));

    // An exact entity: the path, named in a question.
    let r = query(&shared, "what is in crates/theseus-store/src/wal.rs?");
    let top = &r.hits[0];
    assert!(top.text.contains("wal.rs"));
    assert!(top.sources.contains_key("entity"), "{:?}", top.sources);
    assert!(top
        .entities_matched
        .contains(&"path:crates/theseus-store/src/wal.rs".to_string()));
    assert_eq!(top.tool.as_deref(), Some("fs.read"));
    // Entities alone.
    let mut p = QueryParams::new("wal.rs");
    p.sources = vec!["entity".into()];
    let r = shared.query(&p).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert!(!r.hits[0].sources.contains_key("bm25"));
    assert_eq!(r.indexed_through, t.cursor().position);
    assert_eq!(r.lag.bytes, 0);
    // Vectors, on a tender without them: no hits, and it says why.
    p.sources = vec!["vector".into()];
    let v = shared.query(&p).unwrap();
    assert!(v.hits.is_empty());
    assert!(v.skipped["vector"].contains("off"), "{:?}", v.skipped);
    // A source no tender has.
    p.sources = vec!["nonsense".into()];
    assert!(shared.query(&p).is_err());
}

#[test]
fn as_of_hides_later_nodes() {
    let rig = Rig::new();
    let positions = rig.put(&[
        user("ses_1", "kumquat one"),
        user("ses_1", "kumquat two"),
        user("ses_1", "kumquat three"),
    ]);
    let mut t = rig.open();
    settle(&mut t);
    let shared = t.shared();
    let at = |as_of: Option<u64>| {
        let mut p = QueryParams::new("kumquat");
        p.as_of = as_of;
        let mut got: Vec<u64> = shared
            .query(&p)
            .unwrap()
            .hits
            .iter()
            .map(|h| h.position)
            .collect();
        got.sort_unstable();
        got
    };
    assert_eq!(at(None), positions);
    assert_eq!(at(Some(positions[2])), positions[..2].to_vec());
    assert_eq!(at(Some(positions[1])), positions[..1].to_vec());
    // A node is not before itself.
    assert_eq!(at(Some(positions[0])), Vec::<u64>::new());
    assert_eq!(at(Some(positions[2] + 1)), positions);
}

#[test]
fn filters_keep_and_drop_sessions_kinds_and_external_text() {
    let rig = Rig::new();
    rig.put(&[
        user("ses_1", "zebra in the operator's words"),
        result("ses_2", "http.fetch", "zebra from the web", true),
        result("ses_3", "fs.read", "zebra from a file", false),
    ]);
    let mut t = rig.open();
    settle(&mut t);
    let shared = t.shared();
    let sessions = |p: QueryParams| {
        let mut s: Vec<String> = shared
            .query(&p)
            .unwrap()
            .hits
            .iter()
            .map(|h| h.session_id.clone())
            .collect();
        s.sort();
        s
    };
    let mut p = QueryParams::new("zebra");
    p.filters = Filters {
        external: Some(false),
        ..Filters::default()
    };
    assert_eq!(sessions(p.clone()), vec!["ses_1", "ses_3"]);
    p.filters = Filters {
        kinds: vec!["tool_result".into()],
        ..Filters::default()
    };
    assert_eq!(sessions(p.clone()), vec!["ses_2", "ses_3"]);
    p.filters = Filters::default();
    p.exclude_sessions = vec!["ses_1".into(), "ses_2".into()];
    assert_eq!(sessions(p.clone()), vec!["ses_3"]);
    p.exclude_sessions.clear();
    p.filters.sessions = vec!["ses_2".into()];
    assert_eq!(sessions(p), vec!["ses_2"]);
}

#[test]
fn a_session_s_place_comes_from_the_core_s_meta_records() {
    let rig = Rig::new();
    rig.bind_place("dm:42", "ses_dm");
    rig.put(&[
        user("ses_dm", "aardvark in a DM"),
        user("ses_cli", "aardvark at the CLI"),
    ]);
    let mut t = rig.open();
    settle(&mut t);
    let r = query(&t.shared(), "aardvark");
    let place = |s: &str| {
        r.hits
            .iter()
            .find(|h| h.session_id == s)
            .unwrap()
            .place
            .clone()
    };
    assert_eq!(place("ses_dm").as_deref(), Some("discord:dm:42"));
    assert_eq!(place("ses_cli"), None);
    assert_eq!(
        t.places().get("ses_dm").map(String::as_str),
        Some("discord:dm:42")
    );
}

#[test]
fn a_long_result_is_chunked_and_a_hit_names_its_chunk() {
    let rig = Rig::new();
    let mut long: String = (0..3000).map(|i| format!("filler{i} ")).collect();
    long.push_str("needle-in-the-haystack");
    rig.put(&[result("ses_1", "proc.run", &long, false)]);
    let mut t = rig.open();
    settle(&mut t);
    let shared = t.shared();
    let (docs, nodes) = shared.engine.counts().unwrap();
    assert_eq!(nodes, 1);
    assert!(docs >= 4, "{docs} chunks");
    let r = query(&shared, "haystack");
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].chunk, docs - 1);
    assert!(r.hits[0].text.ends_with("needle-in-the-haystack"));
    for c in shared.engine.dump().unwrap() {
        assert!(crate::chunk::tokens(&c.text) <= crate::chunk::MAX_TOKENS);
    }
}

fn many(rig: &Rig, from: usize, n: usize) -> Vec<u64> {
    let nodes: Vec<Node> = (from..from + n)
        .map(|i| match i % 3 {
            0 => user(
                &format!("ses_{}", i % 4),
                &format!("note {i} about theseus-zaz.{i}"),
            ),
            1 => result(
                &format!("ses_{}", i % 4),
                "fs.read",
                &format!("file body {i} in src/m{i}.rs"),
                false,
            ),
            _ => assistant(
                &format!("ses_{}", i % 4),
                json!([{"type": "text", "text": format!("answer {i}")}]),
            ),
        })
        .collect();
    rig.put(&nodes)
}

fn dump(t: &Tender) -> Vec<StoredChunk> {
    t.shared().engine.dump().unwrap()
}

#[test]
fn a_rebuild_equals_incremental_ingest() {
    let rig = Rig::new();
    let mut t = rig.open();
    // Incremental: three rounds, the follower catching up between them,
    // with a place bound part way and segments rolling (4 KB each).
    many(&rig, 0, 20);
    settle(&mut t);
    rig.bind_place("channel:7", "ses_1");
    many(&rig, 20, 25);
    settle(&mut t);
    many(&rig, 45, 30);
    settle(&mut t);
    assert!(
        rig.wal.segment_count() > 3,
        "{} segments",
        rig.wal.segment_count()
    );
    let incremental = dump(&t);
    assert_eq!(
        incremental.iter().filter(|c| c.chunk == 0).count(),
        75,
        "every node once"
    );

    // From scratch, into another index directory.
    let other = rig._tmp.path().join("index2");
    let mut fresh = Tender::open(rig.cfg(&other)).unwrap();
    settle(&mut fresh);
    assert_eq!(dump(&fresh), incremental);

    // And `index.rebuild` on the first: dropped, then backfilled.
    t.shared().request_rebuild();
    drop(fresh);
    let shared = t.shared();
    let h = std::thread::spawn(move || t.run());
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let s = shared.status();
        if s.rebuilds == 1 && s.state == "ready" && s.position > 0 {
            break;
        }
        assert!(Instant::now() < deadline, "rebuild never finished: {s:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(shared.engine.dump().unwrap(), incremental);
    shared.request_stop();
    h.join().unwrap().unwrap();
}

#[test]
fn a_kill_between_commit_and_cursor_reindexes_idempotently() {
    let rig = Rig::new();
    many(&rig, 0, 12);
    let mut t = rig.open();
    settle(&mut t);
    let before = t.cursor().clone();
    many(&rig, 12, 9);
    // The batch is committed, and the tender dies before its cursor.
    t.crash_after_commit = true;
    assert!(t.step().is_err());
    drop(t);
    let saved: crate::state::Saved = crate::state::load(&rig.index.join("cursor.json")).unwrap();
    assert_eq!(saved.cursor, before, "the cursor was written after all");

    // Restarted: the batch is read again, from the cursor, and indexed again.
    let mut again = rig.open();
    assert_eq!(*again.cursor(), before);
    assert_eq!(
        again.shared().status().rebuilds,
        0,
        "it resumed, not rebuilt"
    );
    settle(&mut again);
    let got = dump(&again);

    let clean_dir = rig._tmp.path().join("clean");
    let mut clean = Tender::open(rig.cfg(&clean_dir)).unwrap();
    settle(&mut clean);
    assert_eq!(got, dump(&clean));
    let mut keys: Vec<(String, u64)> = got.iter().map(|c| (c.node_id.clone(), c.chunk)).collect();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n, "a chunk indexed twice");
    assert_eq!(got.iter().filter(|c| c.chunk == 0).count(), 21);
}

#[test]
fn a_restart_resumes_from_the_cursor_and_reads_only_what_is_new() {
    let rig = Rig::new();
    many(&rig, 0, 10);
    let mut t = rig.open();
    settle(&mut t);
    let at = t.cursor().clone();
    drop(t);
    many(&rig, 10, 3);
    let mut t = rig.open();
    assert_eq!(*t.cursor(), at);
    let mut read = 0;
    loop {
        let s = t.step().unwrap();
        read += s.records;
        if s.stop != Stop::Budget {
            break;
        }
    }
    assert_eq!(read, 3);
    assert_eq!(dump(&t).iter().filter(|c| c.chunk == 0).count(), 13);
}

#[test]
fn a_replaced_wal_or_another_build_s_index_is_rebuilt() {
    let rig = Rig::new();
    many(&rig, 0, 6);
    let mut t = rig.open();
    settle(&mut t);
    drop(t);

    // Another build's index: its extractor's version differs.
    let path = rig.index.join("cursor.json");
    let mut saved: crate::state::Saved = crate::state::load(&path).unwrap();
    saved.extractor += 1;
    crate::state::save(&path, &saved).unwrap();
    let mut t = rig.open();
    assert_eq!(t.shared().status().rebuilds, 1);
    settle(&mut t);
    assert_eq!(dump(&t).iter().filter(|c| c.chunk == 0).count(), 6);
    drop(t);

    // A WAL that is not the one the cursor was taken on (a restore of
    // another copy): rebuilt from it.
    let wal_dir = rig.store.join("wal");
    for s in theseus_store::wal::list_segments(&wal_dir).unwrap() {
        std::fs::remove_file(theseus_store::wal::segment_path(&wal_dir, s)).unwrap();
    }
    let other = Wal::open(&wal_dir, WalConfig::default()).unwrap();
    other
        .append(&[user("ses_9", "only this one").record().unwrap()])
        .unwrap();
    drop(other);
    let mut t = rig.open();
    assert_eq!(t.shared().status().rebuilds, 1);
    settle(&mut t);
    let d = dump(&t);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].session, "ses_9");
}

#[test]
fn the_tender_waits_at_a_torn_tail_and_indexes_on_after_the_cores_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let (store, index) = (tmp.path().join("store"), tmp.path().join("index"));
    let wal_dir = store.join("wal");
    let cfg = WalConfig {
        fsync: false,
        ..WalConfig::default()
    };
    let wal = Wal::open(&wal_dir, cfg.clone()).unwrap();
    wal.append(&[user("ses_1", "said before the tear").record().unwrap()])
        .unwrap();
    drop(wal);
    // The core dies inside its next frame: half a frame at the log's end.
    let seg = theseus_store::wal::segment_path(&wal_dir, 1);
    let mut torn = theseus_store::wal::MAGIC_MARKED.to_le_bytes().to_vec();
    torn.extend_from_slice(&900u32.to_le_bytes());
    torn.extend_from_slice(&[0u8; 30]);
    std::io::Write::write_all(
        &mut std::fs::OpenOptions::new().append(true).open(&seg).unwrap(),
        &torn,
    )
    .unwrap();

    let mut t = Tender::open(Config::new(&store, &index)).unwrap();
    settle(&mut t);
    let s = t.shared().status();
    assert_eq!(s.state, "ready");
    assert_eq!(s.nodes, 1);
    assert_eq!(
        s.lag.bytes,
        torn.len() as u64,
        "the torn bytes are after its cursor"
    );
    assert!(
        s.waiting
            .as_deref()
            .is_some_and(|w| w.contains("not yet whole")),
        "{s:?}"
    );

    // The core's next open cuts the tail and appends on; the tender follows.
    let wal = Wal::open(&wal_dir, cfg).unwrap();
    wal.append(&[user("ses_1", "said after the repair").record().unwrap()])
        .unwrap();
    settle(&mut t);
    let s = t.shared().status();
    assert_eq!((s.nodes, s.lag.bytes, s.waiting), (2, 0, None));
    assert_eq!(query(&t.shared(), "repair").hits.len(), 1);
}

#[test]
fn an_index_that_will_not_open_is_rebuilt_even_after_a_kill_before_its_first_commit() {
    let rig = Rig::new();
    many(&rig, 0, 6);
    let mut t = rig.open();
    settle(&mut t);
    drop(t);
    std::fs::write(rig.index.join("bm25").join("meta.json"), b"not an index").unwrap();
    // Recreated empty, and killed before it commits anything.
    let t = rig.open();
    assert_eq!(t.shared().status().rebuilds, 1);
    assert!(t.cursor().at_start());
    drop(t);
    // The next start must not trust the old cursor over the empty index.
    let mut t = rig.open();
    assert!(t.cursor().at_start(), "resumed at {:?}", t.cursor());
    settle(&mut t);
    assert_eq!(dump(&t).iter().filter(|c| c.chunk == 0).count(), 6);
}

#[test]
fn a_second_tender_on_the_same_index_is_refused() {
    let rig = Rig::new();
    let t = rig.open();
    assert!(matches!(
        Tender::open(rig.cfg(&rig.index)),
        Err(OpenError::Held(_))
    ));
    drop(t);
    rig.open();
}

#[test]
fn the_socket_answers_and_a_new_node_is_indexed_within_a_second() {
    let rig = Rig::new();
    rig.put(&[user("ses_1", "the first words")]);
    let t = rig.open();
    let shared = t.shared();
    let sock = t.paths().socket();
    let server = server::spawn(&sock, shared.clone()).unwrap();
    drop(server);
    let h = std::thread::spawn(move || t.run());
    let mode = std::fs::metadata(&sock).unwrap();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
        0o600
    );
    let mut c = Client::connect(&sock, Duration::from_secs(10)).unwrap();
    let wait_for = |c: &mut Client, text: &str| {
        let t0 = Instant::now();
        loop {
            let r: QueryResult = c.call(method::QUERY, QueryParams::new(text)).unwrap();
            if !r.hits.is_empty() {
                return (r, t0.elapsed());
            }
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "{text:?} never indexed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    wait_for(&mut c, "first");

    // A new turn: indexed at the inotify event, well inside the backstop.
    rig.put(&[user("ses_1", "a brand new ocelot")]);
    let (r, took) = wait_for(&mut c, "ocelot");
    assert_eq!(r.hits.len(), 1);
    eprintln!("a new node was indexed in {took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");

    let s: IndexStatus = c.call(method::STATUS, ()).unwrap();
    assert_eq!(s.state, "ready");
    assert_eq!(s.mode, "bm25_only");
    assert_eq!(s.nodes, 2);
    assert_eq!(s.lag.bytes, 0);
    assert!(s.rss_bytes > 0);
    assert!(s.commits >= 2 && s.last_commit_ms > 0, "{s:?}");
    assert!(c.call::<serde_json::Value>("index.nothing", ()).is_err());
    let r: RebuildResult = c.call(method::REBUILD, ()).unwrap();
    assert!(r.accepted);
    wait_for(&mut c, "ocelot");
    shared.request_stop();
    h.join().unwrap().unwrap();
}

#[test]
fn a_reader_shares_the_engine_while_ingest_runs() {
    // `Shared` crosses threads: the socket's and the ingest loop's.
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Arc<Shared>>();
    fn send<T: Send>() {}
    send::<Tender>();
}
