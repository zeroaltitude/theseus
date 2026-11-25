//! A stored record that cannot be decoded is named in the log by its key or id, never by the decoder's error,
//! which can quote what the record holds (issue 46). One reader of each kind: a reader of a whole prefix, a
//! reader of one key, a record's own decode (an execution), and a file the daemon keeps.

use theseus_protocol::{CARD_OBS_PREFIX, CARD_PROPOSAL_PREFIX};

use crate::config::Config;
use crate::provider::FakeProvider;
use crate::rpc::Core;
use crate::store::Store;
use crate::test_support::Log;

/// Words a planted record holds where a decoder's error would quote them.
const WORDS: &str = "STORED-WORDS-z9";

/// A record of `WORDS` alone: a JSON string, where each reader asks for an object.
fn words() -> serde_json::Value {
    serde_json::Value::String(WORDS.into())
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

/// What `f` logged; it must name each of `named` and quote nothing of a record.
fn logged<T>(named: &[&str], f: impl FnOnce() -> T) -> T {
    let log = Log::default();
    let out = log.capture(f);
    let text = log.text();
    for n in named {
        assert!(text.contains(n), "{n} is named: {text}");
    }
    assert!(!text.contains(WORDS), "a record was quoted: {text}");
    out
}

/// The readers of a whole prefix that leave out a record they cannot read: a card proposal, a card
/// observation and a standing proposal.
#[test]
fn a_prefix_reader_names_a_record_it_cannot_read_by_its_key_alone() {
    let (_dir, s) = store();
    let keys = [
        format!("{CARD_PROPOSAL_PREFIX}odd"),
        format!("{CARD_OBS_PREFIX}odd"),
        format!("{}odd", crate::standing_proposals::PREFIX),
    ];
    for k in &keys {
        s.put_meta(k, &words()).unwrap();
    }
    let named: Vec<&str> = keys.iter().map(String::as_str).collect();
    let (proposals, observations, standing) = logged(&named, || {
        (
            crate::card::state::proposals(&s).unwrap(),
            crate::card::state::observations(&s, None).unwrap(),
            crate::standing_proposals::all(&s).unwrap(),
        )
    });
    assert!(proposals.is_empty() && observations.is_empty() && standing.is_empty());
}

/// A reader of one key: an MCP server's stored tool list.
#[test]
fn a_key_reader_names_a_record_it_cannot_read_by_its_server_alone() {
    let (_dir, s) = store();
    let key = format!("{}harbour", crate::mcp::STORED_PREFIX);
    s.put_meta(&key, &words()).unwrap();
    let got = logged(&["harbour"], || crate::mcp::read_stored(&s, "harbour"));
    assert!(got.is_none());
}

/// A record's own decode: an execution the listing leaves out, named by its position.
#[tokio::test]
async fn an_execution_that_does_not_decode_is_named_by_its_position_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = std::sync::Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let payload = serde_json::to_vec(&words()).unwrap();
    let (read, skipped) = logged(&["position=4711"], || {
        core.kernel
            .decode_executions_skipping([(4711, payload.as_slice())])
    });
    assert_eq!((read.len(), skipped), (0, 1));
}

/// A file the daemon keeps: the durability cursor.
#[test]
fn a_kept_file_that_does_not_parse_is_named_by_no_part_of_it() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::aws::durable::cursor::Paths::for_store(&dir.path().join("store"));
    std::fs::create_dir_all(&paths.state).unwrap();
    std::fs::write(paths.cursor(), serde_json::to_vec(&words()).unwrap()).unwrap();
    let got = logged(&["the cursor does not parse"], || {
        crate::aws::durable::cursor::load(&paths)
    });
    assert!(got.is_none());
}
