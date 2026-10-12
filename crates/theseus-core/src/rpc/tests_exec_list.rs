//! `execution.list` (theseus-0jet): the newest `n` by birth with a cursor,
//! only the executions `ids` names, and the answer to no params that every
//! client older than the paged forms still reads.

use std::sync::Arc;

use serde_json::Value;
use theseus_kernel::{Authority, SessionKind};
use theseus_protocol::ExecutionListParams;

use super::server::{or_empty, parse};
use super::tests::test_core;
use super::Core;

/// `count` executions, oldest first, by id.
fn open(core: &Core, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            let authority = Authority {
                principal: crate::turn::OPERATOR.into(),
                ..Default::default()
            };
            let sid = format!("ses_{}", theseus_kernel::new_id("x"));
            core.kernel
                .open_execution(&sid, SessionKind::Conversation, authority, None, None)
                .unwrap()
                .id
        })
        .collect()
}

fn ids(r: &theseus_protocol::ExecutionListResult) -> Vec<String> {
    r.executions
        .iter()
        .map(|e| e.execution_id.clone())
        .collect()
}

/// Pages back from the newest give each execution once, newest first, with a
/// cursor while older ones remain and none at the last page; one opened
/// between two pages is newer than the cursor, so no page back holds it.
#[tokio::test]
async fn execution_list_pages_back_from_the_newest_born() {
    let core: Arc<Core> = test_core("ok");
    let mut opened = open(&core, 23);
    let mut seen = Vec::new();
    let mut before = None;
    for pages in 0.. {
        assert!(pages < 20, "the cursor never reached the oldest: {seen:?}");
        let r = core
            .execution_list(ExecutionListParams {
                n: Some(5),
                before,
                ..Default::default()
            })
            .unwrap();
        assert!(r.executions.len() <= 5);
        seen.extend(ids(&r));
        open(&core, 1);
        match r.older {
            Some(b) => before = Some(b),
            None => break,
        }
    }
    opened.reverse();
    assert_eq!(seen, opened, "each execution once, the newest first");
    let all = core.execution_list(ExecutionListParams::default()).unwrap();
    assert_eq!(all.executions.len(), 23 + 5);
    assert_eq!(all.older, None, "no cursor without `n`");
}

/// `{ids}` answers the executions it names, in that order, and leaves an
/// unknown one out.
#[tokio::test]
async fn execution_list_by_ids_answers_only_those() {
    let core: Arc<Core> = test_core("ok");
    let opened = open(&core, 6);
    let want = vec![
        opened[4].clone(),
        "exe_unknown".to_string(),
        opened[1].clone(),
    ];
    let r = core
        .execution_list(ExecutionListParams {
            ids: Some(want),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ids(&r), vec![opened[4].clone(), opened[1].clone()]);
    assert!(r.executions.iter().all(|e| e.attention.is_some()));
}

/// The reader rule: a client of before the paged forms sends no params, or
/// `{}`, and is answered every execution, as it always was.
#[tokio::test]
async fn a_bare_execution_list_answers_every_execution_as_before() {
    let core: Arc<Core> = test_core("ok");
    let opened = open(&core, 7);
    for bare in [Value::Null, serde_json::json!({})] {
        let p: ExecutionListParams = parse(or_empty(bare)).unwrap();
        let r = core.execution_list(p).unwrap();
        let mut got = ids(&r);
        got.sort();
        let mut want = opened.clone();
        want.sort();
        assert_eq!(got, want);
        assert_eq!(r.older, None);
    }
}
