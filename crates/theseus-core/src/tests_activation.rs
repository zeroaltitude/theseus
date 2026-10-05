//! Spreading activation's wire-in (M6 step 32b; §3.2's 32b row): the
//! adjacency projection's edges on a small store (positions, tool pairs,
//! each route, `supersedes` both ways, shared entities with their `df`), a
//! recall's edges weighing zero, and the same projection and spread whether
//! built whole or kept current.

use std::collections::BTreeSet;

use serde_json::json;
use theseus_memory::{spread, Adjacency, EdgeKind, SpreadParams};
use theseus_protocol::LedgerKind;
use theseus_store::Store as _;
use theseus_store::{kinds, NewRecord};

use crate::graph::{self, Edge};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, RecalledRef, ResultStatus};
use crate::recall::adjacency::{cap, mapped, Mapped, Projection};
use crate::store::Store;

pub(crate) fn user(sid: &str, text: &str) -> Node {
    Node::user(sid, None, "test", text)
}

fn call(sid: &str, use_id: &str) -> Node {
    Node::tool_call(
        sid,
        None,
        None,
        Body::ToolCall {
            tool_use_id: use_id.into(),
            tool: "fs.read".into(),
            wire_name: "fs_read".into(),
            input: json!({"path": "notes/heron.md"}),
            assistant_node: String::new(),
            correlation_id: None,
            gate: None,
        },
    )
}

fn result(sid: &str, use_id: &str, text: &str) -> Node {
    Node::tool_result(
        sid,
        None,
        None,
        Body::ToolResult {
            tool_use_id: use_id.into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: text.into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: serde_json::Value::Null,
            image: None,
            external: None,
        },
    )
}

fn recall_node(sid: &str, sources: &[&Node]) -> Node {
    let items = sources
        .iter()
        .map(|n| RecalledRef {
            node_id: n.id.clone(),
            session_id: n.session_id.clone(),
            position: 0,
            chunk: (0, 0),
            header: String::new(),
            tokens: 1,
        })
        .collect();
    Node::recall(sid, "turn_kestrel", "rcl_kestrel", "baseline", items)
}

pub(crate) fn put(store: &Store, nodes: &[&Node]) {
    let records: Vec<NewRecord> = nodes.iter().map(|n| n.record().unwrap()).collect();
    store.append(&records).unwrap();
}

pub(crate) fn edge(store: &Store, kind: graph::EdgeKind, from: &Node, to: &Node, via: &str) {
    store
        .append(&[Edge::new(kind, &from.id, &to.id, via).record().unwrap()])
        .unwrap();
}

/// A `memory.labeled` row naming `n`'s entities, as the memory pass writes.
pub(crate) fn label(store: &Store, n: &Node, about: &[&str]) {
    let row = LedgerRow::new(
        LedgerKind::MemoryLabeled,
        Some(&n.session_id),
        None,
        json!({"node_id": n.id, "position": 0, "body": n.kind_str(), "about": about,
               "kind": "fact", "durability": "medium", "volatile": false, "trust": "own",
               "correction": false}),
    );
    let r = NewRecord::json(kinds::LEDGER, None, &row)
        .unwrap()
        .scoped(&crate::fact::memory::scope(&n.session_id));
    store.append(&[r]).unwrap();
}

/// Every edge out of `n` in `p`, as (neighbour, kind), sorted.
fn out(p: &Projection, n: &Node) -> Vec<(String, EdgeKind)> {
    let mut v = Vec::new();
    p.view(&SpreadParams::default(), None, &[])
        .edges(&n.id, &mut v);
    v.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(format!("{:?}", a.1).cmp(&format!("{:?}", b.1)))
    });
    v
}

fn sorted(mut v: Vec<(String, EdgeKind)>) -> Vec<(String, EdgeKind)> {
    v.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(format!("{:?}", a.1).cmp(&format!("{:?}", b.1)))
    });
    v
}

fn to(n: &Node, kind: EdgeKind) -> (String, EdgeKind) {
    (n.id.clone(), kind)
}

/// A small store with one of each edge, and what it builds.
struct Small {
    _dir: tempfile::TempDir,
    store: Store,
    /// Session Wren: a message, a call, its result, a recall, a reply.
    ask: Node,
    call: Node,
    result: Node,
    recall: Node,
    reply: Node,
    /// Session Heron: notes linked to Wren's by the EDGEs.
    notes: Vec<Node>,
    /// What the recall renders, alone in its session.
    source: Node,
}

fn small() -> Small {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let (wren, heron) = ("ses_wren", "ses_heron");
    let notes: Vec<Node> = (0..10)
        .map(|i| user(heron, &format!("Heron note {i}.")))
        .collect();
    put(&store, &notes.iter().collect::<Vec<_>>());
    let ask = user(wren, "Where is the Kestrel relay's config?");
    let call = call(wren, "toolu_wren1");
    let result = result(wren, "toolu_wren1", "relay = kestrel-3");
    // The recall's source is alone in a session of its own.
    let source = user("ses_dunlin", "Dunlin's only note.");
    put(&store, &[&source]);
    let recall = recall_node(wren, &[&source]);
    let reply = user(wren, "It is in notes/heron.md.");
    put(&store, &[&ask, &call]);
    put(&store, &[&result]);
    put(&store, &[&recall]);
    edge(
        &store,
        graph::EdgeKind::DerivedFrom,
        &recall,
        &source,
        graph::VIA_RECALL,
    );
    put(&store, &[&reply]);
    let routes = [
        graph::VIA_REPORT,
        graph::VIA_BRIEF,
        graph::VIA_PUBLISH,
        graph::VIA_ARRANGEMENT,
        graph::VIA_CLAIM,
        graph::VIA_GLIDE,
    ];
    for (i, via) in routes.iter().enumerate() {
        edge(&store, graph::EdgeKind::DerivedFrom, &notes[i], &ask, via);
    }
    // A route this build does not know (31b's): it spreads nothing.
    edge(
        &store,
        graph::EdgeKind::DerivedFrom,
        &notes[6],
        &ask,
        "synthesis",
    );
    edge(
        &store,
        graph::EdgeKind::SameEntity,
        &notes[7],
        &reply,
        graph::VIA_MEMORY,
    );
    // notes[8] corrects the reply: from the newer to the older.
    edge(
        &store,
        graph::EdgeKind::Supersedes,
        &notes[8],
        &reply,
        graph::VIA_MEMORY,
    );
    label(&store, &ask, &["commit:3f9a2c1", "path:notes/heron.md"]);
    label(&store, &notes[0], &["commit:3f9a2c1"]);
    label(
        &store,
        &notes[1],
        &["commit:3f9a2c1", "path:notes/heron.md"],
    );
    Small {
        _dir: dir,
        store,
        ask,
        call,
        result,
        recall,
        reply,
        notes,
        source,
    }
}

/// Each edge the projection holds, from a small store: positions, the tool
/// pair, each route of `derived_from`, `same_entity`, `supersedes` both
/// ways, and shared entities with their `df`; a recall is no one's
/// neighbour, and its edge weighs nothing.
#[test]
fn the_projection_holds_each_edge_on_a_small_store() {
    let s = small();
    let p = Projection::build(&s.store).unwrap();
    let n = &s.notes;
    let (d2, d3) = (
        EdgeKind::SharedEntity { df: 2 },
        EdgeKind::SharedEntity { df: 3 },
    );
    // The ask: its neighbour by position, every copying route, and the
    // nodes it shares an entity with (the commit's df is 3, the path's 2).
    assert_eq!(
        out(&p, &s.ask),
        sorted(vec![
            to(&s.call, EdgeKind::Neighbour),
            to(&n[0], EdgeKind::DerivedFrom),
            to(&n[0], d3),
            to(&n[1], EdgeKind::DerivedFrom),
            to(&n[1], d3),
            to(&n[1], d2),
            to(&n[2], EdgeKind::DerivedFrom),
            to(&n[3], EdgeKind::DerivedFrom),
            to(&n[4], EdgeKind::DerivedFrom),
            to(&n[5], EdgeKind::DerivedFrom),
        ])
    );
    // The call and its result, by `tool_use_id`, and by position.
    assert_eq!(
        out(&p, &s.call),
        sorted(vec![
            to(&s.ask, EdgeKind::Neighbour),
            to(&s.result, EdgeKind::Neighbour),
            to(&s.result, EdgeKind::ToolResult),
        ])
    );
    // The recall is skipped in the chain: the result's next is the reply.
    assert_eq!(
        out(&p, &s.result),
        sorted(vec![
            to(&s.call, EdgeKind::Neighbour),
            to(&s.call, EdgeKind::ToolResult),
            to(&s.reply, EdgeKind::Neighbour),
        ])
    );
    assert_eq!(out(&p, &s.recall), vec![to(&s.source, EdgeKind::Recall)]);
    assert_eq!(out(&p, &s.source), vec![to(&s.recall, EdgeKind::Recall)]);
    // `same_entity` both ways; `supersedes` 1.0 toward the newer note, 0.2
    // back to the older reply.
    assert_eq!(
        out(&p, &s.reply),
        sorted(vec![
            to(&s.result, EdgeKind::Neighbour),
            to(&n[7], EdgeKind::SameEntity),
            to(&n[8], EdgeKind::ToNewer),
        ])
    );
    assert!(out(&p, &n[8]).contains(&to(&s.reply, EdgeKind::ToOlder)));
    assert!(out(&p, &n[7]).contains(&to(&s.reply, EdgeKind::SameEntity)));
    // The unknown route is counted, and spreads nothing.
    assert!(!out(&p, &n[6]).iter().any(|(id, _)| *id == s.ask.id));
    let st = p.stats();
    assert_eq!(st.unmapped, 1);
    assert_eq!(st.nodes, 16);
    assert_eq!((st.entities, st.memberships), (2, 5));
    assert_eq!(p.df("commit:3f9a2c1"), 3);
    assert!(st.bytes > 0 && st.through == s.store.last_position());
    let unknown = Edge::new(graph::EdgeKind::DerivedFrom, "a", "b", "synthesis");
    assert_eq!(mapped(&unknown), Mapped::Unmapped);
}

/// A node reachable only through a `Recall` node is never activated: the
/// recall's edge weighs zero by construction, at any weight in the data.
#[test]
fn a_node_reachable_only_through_a_recall_is_never_activated() {
    let s = small();
    let p = Projection::build(&s.store).unwrap();
    let params = SpreadParams::default();
    let v = p.view(&params, None, &[]);
    let got = spread(&v, &[(s.recall.id.clone(), 1.0)], &params);
    assert!(got.is_empty(), "{got:?}");
    // From the reply, the recall's source is two hops away only through the
    // recall; and the reply's spread reaches its neighbours.
    let got = spread(&v, &[(s.reply.id.clone(), 1.0)], &params);
    assert!(!got.iter().any(|(k, _)| *k == s.source.id), "{got:?}");
    assert!(got.iter().any(|(k, _)| *k == s.notes[7].id), "{got:?}");
    // Nor from the result, the recall's neighbour by position before the
    // chain skipped it.
    let got = spread(&v, &[(s.result.id.clone(), 1.0)], &params);
    assert!(
        !got.iter()
            .any(|(k, _)| *k == s.source.id || *k == s.recall.id),
        "{got:?}"
    );
}

/// The same store gives the same projection and the same spread, whether
/// built whole or kept current a frame at a time.
#[test]
fn a_projection_kept_current_equals_one_built_whole() {
    let s = small();
    let whole = Projection::build(&s.store).unwrap();
    // Folded again, a few records at a time, from a fresh store written in
    // the same order: refresh after each write.
    let dir = tempfile::tempdir().unwrap();
    let copy = Store::open(&dir.path().join("store")).unwrap();
    let mut kept = Projection::default();
    let mut after = 0;
    loop {
        let rs = s.store.inner().scan(after + 1, None, 3).unwrap();
        if rs.is_empty() {
            break;
        }
        after = rs.last().unwrap().position;
        let batch: Vec<NewRecord> = rs
            .iter()
            .filter(|r| [kinds::NODE, kinds::EDGE, kinds::LEDGER].contains(&r.kind))
            .map(|r| NewRecord {
                kind: r.kind,
                key: r.key.clone(),
                scope: r.scope.clone(),
                payload: r.payload.clone(),
            })
            .collect();
        if !batch.is_empty() {
            copy.append(&batch).unwrap();
        }
        kept.refresh(&copy).unwrap();
    }
    let params = SpreadParams::default();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    ids.extend(s.notes.iter().map(|n| n.id.clone()));
    ids.extend([&s.ask, &s.call, &s.result, &s.reply, &s.recall, &s.source].map(|n| n.id.clone()));
    for id in &ids {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        whole.view(&params, None, &[]).edges(id, &mut a);
        kept.view(&params, None, &[]).edges(id, &mut b);
        assert_eq!(sorted(a), sorted(b), "{id}");
        let seeds = [(id.clone(), 1.0)];
        assert_eq!(
            spread(&whole.view(&params, None, &[]), &seeds, &params),
            spread(&kept.view(&params, None, &[]), &seeds, &params),
            "{id}"
        );
    }
    let (a, b) = (whole.stats(), kept.stats());
    assert_eq!(
        (a.nodes, a.edges, a.entities, a.memberships, a.unmapped),
        (b.nodes, b.edges, b.entities, b.memberships, b.unmapped)
    );
}

/// A common entity's bound: at the defaults, an entity in more than 1,095
/// nodes is not expanded, since one such edge cannot carry a seed of 1.0
/// over the threshold alone; one in fewer is.
#[test]
fn a_common_entity_is_bounded_where_it_could_carry_nothing() {
    let p = SpreadParams::default();
    assert_eq!(cap(&p), 1095);
    let w = |df| p.weights.weight(EdgeKind::SharedEntity { df }) * p.decay;
    assert!(w(cap(&p)) >= p.threshold && w(cap(&p) + 1) < p.threshold);
    let none = SpreadParams {
        threshold: 0.0,
        ..p
    };
    assert_eq!(cap(&none), u32::MAX);
    let high = SpreadParams {
        threshold: 0.9,
        ..p
    };
    assert_eq!(cap(&high), 0);

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let nodes: Vec<Node> = (0..1100)
        .map(|i| user("ses_plover", &format!("plover {i}")))
        .collect();
    put(&store, &nodes.iter().collect::<Vec<_>>());
    for n in &nodes[..1096] {
        label(&store, n, &["host:plover.example"]);
    }
    for n in &nodes[..1095] {
        label(&store, n, &["host:dunlin.example"]);
    }
    let proj = Projection::build(&store).unwrap();
    let mut e = Vec::new();
    proj.view(&p, None, &[]).edges(&nodes[0].id, &mut e);
    let shared: Vec<_> = e
        .iter()
        .filter(|(_, k)| matches!(k, EdgeKind::SharedEntity { .. }))
        .collect();
    assert_eq!(shared.len(), 1094, "only the 1,095-node entity expands");
    assert!(shared
        .iter()
        .all(|(_, k)| *k == EdgeKind::SharedEntity { df: 1095 }));
}
