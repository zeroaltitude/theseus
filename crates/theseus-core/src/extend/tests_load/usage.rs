//! A loaded extension's real calls, read back as use (theseus-pw1q.13): the
//! ledger rows a call writes anyway name its tool as the producer, so the
//! counts need no row or counter of their own. A call that answers an error
//! result is a failure; a call that works is not.
//! - three calls (two work, one `isError`) read back as 3 calls, 1 failure;
//! - the calls wrote only the rows a call of any async tool writes: no row
//!   of use's own, so nothing counts on the call path;
//! - `extend.list` with `usage` carries the counts (and a plain one none).

use serde_json::json;
use theseus_protocol::extend::ExtendListParams;

use super::{call, rig, text, TOOL, WC};

const FAIL: &str = "mcp__ext-wordcount__fail";

#[tokio::test]
async fn real_calls_read_back_as_use_with_the_error_result_a_failure() {
    let r = rig();
    r.core.mcp.start();
    let src = r.server("tools/wc", "ok", WC);
    let q = r.propose(&src).await;
    r.ack(&q);
    let sid = r.session(None);
    let kinds_before: Vec<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row.kind)
        .collect();
    r.script(vec![call(TOOL, json!({"text": "one two"})), text()]);
    r.turn(&sid, "count").await;
    r.script(vec![call(FAIL, json!({})), text()]);
    r.turn(&sid, "fail").await;
    r.script(vec![call(TOOL, json!({"text": "three"})), text()]);
    r.turn(&sid, "count again").await;
    let now = theseus_protocol::now_unix_ms() + 1_000;
    let u = r.core.extension_usage("wordcount", now, 14).unwrap();
    assert_eq!((u.calls, u.failures), (3, 1), "{u:?}");
    assert!(!u.truncated && u.p50_ms <= u.p95_ms, "{u:?}");
    let other = r.core.extension_usage("nothing", now, 14).unwrap();
    assert_eq!((other.calls, other.failures), (0, 0));
    // Nothing of use's own was written: every kind the calls added is one a
    // call of any async tool adds.
    let added: std::collections::BTreeSet<String> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, row)| row.kind)
        .filter(|k| !kinds_before.contains(k))
        .collect();
    assert!(
        added
            .iter()
            .all(|k| !k.contains("usage") && !k.contains("graduate")),
        "{added:?}"
    );
    // The surface: only a list asked for usage carries it.
    let plain = r.core.extend_list().unwrap();
    assert!(plain.loaded[0].usage.is_none());
    let list = r
        .core
        .extend_list_with(&ExtendListParams { usage: true })
        .unwrap();
    let shown = list.loaded[0].usage.as_ref().expect("usage");
    assert_eq!((shown.calls, shown.failures), (3, 1), "{shown:?}");
    assert!(shown
        .line()
        .starts_with("used 3 times in 14 days, 1 failure, p50 "));
}
