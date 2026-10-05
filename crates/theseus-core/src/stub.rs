//! Stubs (M6 step 33, theseus-6fn.13; design §2.10): a transcript read keeps
//! what a node's record gives without decoding its payload, and decodes the
//! payload only when a reader touches it.
//!
//! - **What a stub holds.** Its position and id (the record's key), and what
//!   every whole-transcript walk asks of a node: its body's kind, its origin,
//!   its turn, and a summary's range's end. These come from a peek at the
//!   record's bytes that reads those few fields and skips the rest, the
//!   payloads (a tool's output, a message's text) included, without
//!   building them. The bytes stay with the stub until it hydrates; a node
//!   the heat cache holds keeps none.
//! - **Rehydration.** A stub derefs to its `Node`: the first touch takes it
//!   from the heat cache by position, or decodes the bytes it kept (and the
//!   cache keeps it), or, when the cache let it go in between, reads the
//!   record again by position. A reader that only walks the transcript by
//!   kind, id, origin, or turn decodes nothing; a reader that touches a body
//!   decodes that node alone. So a compile decodes what it renders: the
//!   compilation's `includes` and its tail. Everything before a ring's cut
//!   or a compaction's floor stays a stub.
//! - **A failed rehydration** (§6) is logged and counted (health's
//!   `store.node_cache.failed`), and the node reads as a harness message
//!   that says so, where today's read failed whole: a record that decoded
//!   when it was written and does not now is a corrupt store, which the
//!   history check reports.

use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use serde::Deserialize;
use theseus_store::{Record, Store as _, WalStore};

use crate::node::{Body, Node, Origin};
use crate::node_cache::NodeCache;

/// A body's kind, as a stub knows it without the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Recall,
    Arrangement,
    Summary,
    Synthesis,
}

impl Kind {
    pub fn of(b: &Body) -> Kind {
        match b {
            Body::UserMessage { .. } => Kind::UserMessage,
            Body::AssistantMessage { .. } => Kind::AssistantMessage,
            Body::ToolCall { .. } => Kind::ToolCall,
            Body::ToolResult { .. } => Kind::ToolResult,
            Body::Recall { .. } => Kind::Recall,
            Body::Arrangement { .. } => Kind::Arrangement,
            Body::Summary { .. } => Kind::Summary,
            Body::Synthesis { .. } => Kind::Synthesis,
        }
    }
}

/// What a walk over the transcript asks of a node: a stub's own, or a
/// decoded node's, so the compiler's walks take either.
pub trait Shaped {
    fn kind(&self) -> Kind;
    fn node_id(&self) -> &str;
    /// A summary's range's last position.
    fn summary_last(&self) -> Option<u64>;
}

impl Shaped for Node {
    fn kind(&self) -> Kind {
        Kind::of(&self.body)
    }
    fn node_id(&self) -> &str {
        &self.id
    }
    fn summary_last(&self) -> Option<u64> {
        match &self.body {
            Body::Summary { last, .. } => Some(*last),
            _ => None,
        }
    }
}

impl<T: Shaped + ?Sized> Shaped for &T {
    fn kind(&self) -> Kind {
        (**self).kind()
    }
    fn node_id(&self) -> &str {
        (**self).node_id()
    }
    fn summary_last(&self) -> Option<u64> {
        (**self).summary_last()
    }
}

impl Shaped for Stub {
    fn kind(&self) -> Kind {
        self.kind
    }
    fn node_id(&self) -> &str {
        &self.id
    }
    fn summary_last(&self) -> Option<u64> {
        self.summary_last
    }
}

/// The fields a peek reads; serde skips the rest of the record unbuilt.
#[derive(Deserialize)]
struct Peek {
    id: String,
    origin: Origin,
    #[serde(default)]
    turn_id: Option<String>,
    body: PeekBody,
}

#[derive(Deserialize)]
struct PeekBody {
    kind: Kind,
    #[serde(default)]
    last: Option<u64>,
}

/// A node of a transcript: its stub's fields, and the node behind them,
/// decoded at its first touch. Cloning shares the node.
#[derive(Clone)]
pub struct Stub {
    pub id: String,
    pub origin: Origin,
    pub turn_id: Option<String>,
    pub kind: Kind,
    pub summary_last: Option<u64>,
    cell: Arc<Cell>,
}

struct Cell {
    position: u64,
    /// The record's bytes, until the node is decoded from them.
    bytes: Mutex<Option<Vec<u8>>>,
    node: OnceLock<Arc<Node>>,
    cache: Option<Arc<NodeCache>>,
    /// Where to read the record again when neither the bytes nor the cache
    /// hold it.
    wal: Weak<WalStore>,
}

impl std::fmt::Debug for Stub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stub")
            .field("id", &self.id)
            .field("position", &self.cell.position)
            .field("kind", &self.kind)
            .field("hydrated", &self.is_hydrated())
            .finish()
    }
}

impl std::ops::Deref for Stub {
    type Target = Node;
    fn deref(&self) -> &Node {
        self.cell.node.get_or_init(|| self.cell.hydrate(&self.id))
    }
}

impl From<Node> for Stub {
    fn from(n: Node) -> Self {
        Stub::hydrated(0, Arc::new(n))
    }
}

impl From<Arc<Node>> for Stub {
    fn from(n: Arc<Node>) -> Self {
        Stub::hydrated(0, n)
    }
}

impl Stub {
    /// A stub of a node already decoded: nothing to rehydrate.
    pub fn hydrated(position: u64, n: Arc<Node>) -> Self {
        let cell = Cell {
            position,
            bytes: Mutex::new(None),
            node: OnceLock::new(),
            cache: None,
            wal: Weak::new(),
        };
        let stub = Stub {
            id: n.id.clone(),
            origin: n.origin,
            turn_id: n.turn_id.clone(),
            kind: Kind::of(&n.body),
            summary_last: n.summary_last(),
            cell: Arc::new(cell),
        };
        let _ = stub.cell.node.set(n);
        stub
    }

    /// A node record's stub: its fields from the cache's node when it holds
    /// one (the bytes dropped), else from a peek at the bytes, which it keeps.
    /// A record the peek cannot read is decoded whole, so a read fails where
    /// it failed before.
    pub fn of_record(
        r: Record,
        cache: &Arc<NodeCache>,
        wal: &Arc<WalStore>,
    ) -> anyhow::Result<Self> {
        let position = r.position;
        let cell = |bytes: Option<Vec<u8>>| Cell {
            position,
            bytes: Mutex::new(bytes),
            node: OnceLock::new(),
            cache: Some(cache.clone()),
            wal: Arc::downgrade(wal),
        };
        if let Some(n) = cache.peek(position) {
            return Ok(Stub {
                id: n.id.clone(),
                origin: n.origin,
                turn_id: n.turn_id.clone(),
                kind: Kind::of(&n.body),
                summary_last: n.summary_last(),
                cell: Arc::new(cell(None)),
            });
        }
        match serde_json::from_slice::<Peek>(&r.payload) {
            Ok(p) => Ok(Stub {
                id: p.id,
                origin: p.origin,
                turn_id: p.turn_id,
                kind: p.body.kind,
                summary_last: (p.body.kind == Kind::Summary)
                    .then_some(p.body.last)
                    .flatten(),
                cell: Arc::new(cell(Some(r.payload))),
            }),
            Err(_) => {
                let n = cache.node(position, &r.payload)?;
                let mut s = Stub::hydrated(position, n);
                Arc::get_mut(&mut s.cell).expect("new").cache = Some(cache.clone());
                Ok(s)
            }
        }
    }

    pub fn position(&self) -> u64 {
        self.cell.position
    }

    /// Whether its node has been decoded (or taken from the cache).
    pub fn is_hydrated(&self) -> bool {
        self.cell.node.get().is_some()
    }

    /// The node, rehydrated if it is not yet: for a reader that keeps it.
    pub fn node(&self) -> Arc<Node> {
        let _: &Node = self;
        self.cell.node.get().expect("hydrated").clone()
    }

    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}

impl Cell {
    fn hydrate(&self, id: &str) -> Arc<Node> {
        if let Some(n) = self.cache.as_ref().and_then(|c| c.get(self.position)) {
            return n;
        }
        let kept = self
            .bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let read = match kept {
            Some(b) => Ok(b),
            None => self.read_again(id),
        };
        let decoded = read.and_then(|bytes| match &self.cache {
            Some(c) => c.node(self.position, &bytes),
            None => Ok(Arc::new(serde_json::from_slice(&bytes)?)),
        });
        match decoded {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(node_id = id, position = self.position, error = %format!("{e:#}"),
                    "a stub's node could not be read back; it reads as a harness note");
                if let Some(c) = &self.cache {
                    c.failed();
                }
                Arc::new(unreadable(id, self.position, &e))
            }
        }
    }

    /// The record at the stub's position, read again: the cache let it go
    /// between the transcript's read and this touch.
    fn read_again(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        let wal = self
            .wal
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("the store is closed"))?;
        match wal.get(self.position)? {
            Some(r) if r.key.as_deref() == Some(id) => Ok(r.payload),
            _ => anyhow::bail!("position {} no longer holds node {id}", self.position),
        }
    }
}

/// How many of a transcript's nodes have been read past their stub: what a
/// compile's `decoded` and `stubs` are counted from (step 33).
pub fn read(nodes: &[(u64, Stub)]) -> u64 {
    nodes.iter().filter(|(_, n)| n.is_hydrated()).count() as u64
}

/// What a node that cannot be read back reads as.
fn unreadable(id: &str, position: u64, e: &anyhow::Error) -> Node {
    let mut n = Node::user(
        "",
        None,
        "harness",
        &format!("[node {id} at @{position} could not be read: {e:#}]"),
    );
    n.id = id.to_string();
    n.origin = Origin::Harness;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(position: u64, n: &Node) -> Record {
        let r = n.record().unwrap();
        Record {
            position,
            kind: r.kind,
            schema: 0,
            key: r.key,
            scope: r.scope,
            at_unix_ms: 0,
            payload: r.payload,
        }
    }

    /// A peek reads the stub's fields and decodes nothing; the first touch
    /// decodes once, the cache keeps it, and a second stub of the same
    /// position takes it from the cache.
    #[test]
    fn a_stub_decodes_at_its_first_touch_and_only_then() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(WalStore::open(dir.path(), Default::default()).unwrap());
        let cache = Arc::new(NodeCache::default());
        let n = Node::user(
            "ses_tern",
            Some("trn_1"),
            "web",
            &"the tide was low ".repeat(500),
        );
        let s = Stub::of_record(record(9, &n), &cache, &wal).unwrap();
        assert_eq!(
            (s.id.as_str(), s.kind, s.origin, s.turn_id.as_deref()),
            (n.id.as_str(), Kind::UserMessage, n.origin, Some("trn_1"))
        );
        assert!(!s.is_hydrated());
        assert_eq!(cache.decodes(), 0, "a peek is no decode");
        assert_eq!(*s.node(), n);
        assert_eq!(cache.decodes(), 1);
        let again = Stub::of_record(record(9, &n), &cache, &wal).unwrap();
        assert_eq!(again.body, n.body);
        assert_eq!(cache.decodes(), 1, "the cache served it");
    }

    /// A summary's stub knows its range's end; a stub whose bytes are gone
    /// and whose record is not where it was reads as a harness note, counted.
    #[test]
    fn a_summary_knows_its_range_and_a_lost_node_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(WalStore::open(dir.path(), Default::default()).unwrap());
        let cache = Arc::new(NodeCache::default());
        let body: Body = serde_json::from_value(serde_json::json!({
            "kind": "summary", "first": 3, "last": 41, "nodes": 30, "text": "the keeper's log",
            "profile": "session", "model": "m", "header": "[Summary]",
        }))
        .unwrap();
        let sum = Node::summary("ses_tern", "trn_2", body);
        let s = Stub::of_record(record(50, &sum), &cache, &wal).unwrap();
        assert_eq!((s.kind, s.summary_last), (Kind::Summary, Some(41)));
        // Its bytes taken, its position empty: nothing to read back.
        s.cell.bytes.lock().unwrap().take();
        assert_eq!(s.origin, Origin::Harness);
        assert!(
            matches!(&s.body, Body::UserMessage { text, .. } if text.contains("could not be read"))
        );
        assert_eq!(cache.health().failed, 1);
    }
}
