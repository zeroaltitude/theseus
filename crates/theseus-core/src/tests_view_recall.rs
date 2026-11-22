//! Recall on a view turn (issue 44, issue 46): the turn keeps its recall's bookkeeping as a transcript turn
//! does (the `Recall` node, its rows and edges), and the view never lays the node as well as the items.

use crate::config::MemoryMode;
use crate::node::Body;
use crate::test_support::recall_rows as rows;
use crate::tests_recall::{admitted, index_of, recalls, rig_with, session, turn, Rig};

const HERON: &str = "Remember: the grey heron nests by the old weir at Millbrook.";

fn view_rig() -> Rig {
    rig_with(MemoryMode::Live, |_| {})
}

/// What the rig's model was sent, request by request, as text.
fn sent(r: &Rig) -> Vec<String> {
    let all = r.model.requests.lock().unwrap().clone();
    all.iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect()
}

/// A view turn keeps its recall's bookkeeping as a transcript turn does (issue 44, issue 46): the `Recall`
/// node is written with its rows, so the reply counts what it recalled and citations, reach and retraction
/// see it; no `recall.withheld` row is written. The view lays the items beside the message, never the node
/// as well: the request carries the note at most once, and a later turn's window never shows the node.
#[tokio::test]
async fn a_view_turn_writes_its_recall_node_and_never_lays_it_twice() {
    let r = view_rig();
    let c = &r.core;
    let a = session(c, None, &[HERON]);
    let b = session(c, None, &[]);
    c.runner.memory.set_ask(index_of(c, vec![]));
    turn(c, &b, "warm up").await;
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let before = sent(&r).len();
    let res = turn(c, &b, "Where does the grey heron nest?").await;
    let m = recalls(c, &b).pop().expect("a recall.ran row");
    assert_eq!(admitted(&m).len(), 1, "recall admitted the note");
    assert_eq!(res.recalled, 1, "the reply counts it");
    let written = c.store.session_nodes(&b).unwrap();
    let written = written
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::Recall { .. }));
    assert_eq!(written.count(), 1, "one `Recall` node in the session");
    assert!(
        rows(c, &b, "recall.withheld").is_empty(),
        "nothing withheld"
    );
    let asked = sent(&r)[before].clone();
    assert!(
        asked.matches("grey heron nests by the old weir").count() <= 1,
        "{asked}"
    );
    c.runner.memory.set_ask(index_of(c, vec![]));
    let later = sent(&r).len();
    turn(c, &b, "and the otters?").await;
    let next = sent(&r)[later].clone();
    assert!(!next.contains("grey heron nests by the old weir"), "{next}");
}
