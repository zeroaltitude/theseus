//! A record under the tags' prefix that does not read as a tag's counts (issue 46): `import.list` and
//! `import.sessions` complete, pass it over and say so, and leave it as it is; the first sync's start, which
//! a tag's counts decide, is not planned past it, and the refusal names its key alone.

use serde_json::{json, Value};
use theseus_protocol::import::{ImportListResult, ImportSessionsResult};
use theseus_protocol::method;
use theseus_store::Store as _;

use super::tag_key;
use super::tests::{call, fixture, import, lines, live};
use crate::rpc::Core;

/// Words the planted record holds where a decoder's error would quote them.
pub(crate) const TAG_WORDS: &str = "TAG-WORDS-p5";

/// A record under the tags' prefix whose session count is words: its key.
pub(crate) fn unread_tag(c: &Core) -> String {
    let key = tag_key("odd");
    let odd = json!({"tag": "odd", "sessions": TAG_WORDS, "nodes": 1, "updated_ms": 1});
    c.store.put_meta(&key, &odd).unwrap();
    key
}

fn held(c: &Core, key: &str) -> Option<(u64, Vec<u8>)> {
    let r = (c.store.inner()).latest_by_key(theseus_store::kinds::META, key);
    r.unwrap().map(|r| (r.position, r.payload))
}

/// `import.list` lists every tag that reads and names the one that does not by its key; `import.sessions`
/// answers with every session of the tags that read and counts the one passed over. The record is left as it
/// is.
#[tokio::test]
async fn a_tag_record_that_does_not_read_is_passed_over_and_named() {
    let r = live();
    let c = &r.core;
    import(c, lines(&fixture())).await;
    let key = unread_tag(c);
    let before = held(c, &key);
    let list = call(c, method::IMPORT_LIST, Value::Null).await;
    let list: ImportListResult = serde_json::from_value(list.expect("the list completes")).unwrap();
    assert_eq!(list.tags.len(), 1, "{list:?}");
    assert_eq!(list.unread, [key.clone()]);
    let page = call(c, method::IMPORT_SESSIONS, json!({"limit": 1})).await;
    let page: ImportSessionsResult =
        serde_json::from_value(page.expect("the sessions complete")).unwrap();
    assert_eq!(page.tags_unread, Some(1));
    assert!(page.all > 0, "{page:?}");
    assert_eq!(held(c, &key), before, "left as it is");
}

/// Where the first sync starts is the newest end over the tags' sessions: a tag record that does not read
/// may hold it, so the plan is refused, naming the record's key and nothing it holds.
#[tokio::test]
async fn the_first_sync_is_not_planned_past_a_tag_record_that_does_not_read() {
    let r = live();
    let c = &r.core;
    import(c, lines(&fixture())).await;
    let key = unread_tag(c);
    let before = held(c, &key);
    let got = call(c, method::IMPORT_SYNC_PLAN, Value::Null).await;
    let (_, message) = got.expect_err("the plan is refused");
    assert!(message.contains(&key), "{message}");
    assert!(message.contains("nothing is planned"), "{message}");
    for quoted in [TAG_WORDS, "invalid type", "expected"] {
        assert!(!message.contains(quoted), "{message}");
    }
    assert_eq!(held(c, &key), before, "left as it is");
}

/// The import's list names a record it cannot read by its key alone in the log, never by what it holds.
#[tokio::test]
async fn a_tag_record_that_does_not_read_is_named_by_its_key_alone() {
    let r = live();
    let c = &r.core;
    let key = unread_tag(c);
    let log = crate::test_support::Log::default();
    let read = log.capture(|| super::write::list(&c.store));
    assert_eq!(read.unwrap().unread, [key.clone()]);
    let text = log.text();
    assert_eq!(text.matches(key.as_str()).count(), 1, "{text}");
    assert!(!text.contains(TAG_WORDS), "the record was quoted: {text}");
}
