//! Moved ranges (theseus-q9b85): what a split takes out of its source's compile.
//!
//! A split (`thread_split.rs`) quotes turns of a session into a thread. The source keeps every node in
//! the store, and writes one `Body::Moved` node naming them: the compiler reads that record and leaves
//! the nodes out of the source's compile, one line in the range's place (`line`). Nothing is deleted:
//! `history` lists the nodes marked moved, recall finds them and credits the thread, and
//! `thread.unsplit` writes a second `Moved` node, `undoes` the first, after which the compile reads the
//! range again, in its own place.
//!
//! The compiler's half (pure): [`read`] takes the records from the stubs (it decodes only the `Moved`
//! ones), [`Moves::shows`] filters a recompile's nodes, [`Moves::stale`] says a frozen compilation
//! still holds what has left (or lacks what came back), and [`in_place`] puts a note where its range
//! began. The source's prefix changes once at a split: its next turn writes its cache again.

use std::collections::HashSet;

use crate::compiler::Compilation;
use crate::node::{Body, Node};
use crate::stub::{Kind, Stub};

/// The author of a `Moved` node.
pub const AUTHOR: &str = "harness:moved";

/// The line a range's place holds in the source's compile.
pub fn line(thread: &str, gist: &str) -> String {
    format!("moved to thread «{thread}»: {gist}")
}

/// The ranges a session has moved, as its stubs say.
#[derive(Debug, Default)]
pub struct Moves {
    /// Every node a live record moved.
    pub hidden: HashSet<String>,
    /// The ids of the live records: they render a note.
    pub live: HashSet<String>,
    /// The ids of every other `Moved` node (an undone record, an undo): they render nothing.
    pub dead: HashSet<String>,
}

/// The session's moves: one walk over the stubs, decoding the `Moved` nodes alone.
pub fn read(nodes: &[(u64, Stub)]) -> Moves {
    let moved: Vec<&Node> = nodes
        .iter()
        .filter(|(_, n)| n.kind == Kind::Moved)
        .map(|(_, n)| &**n)
        .collect();
    let mut out = Moves::default();
    if moved.is_empty() {
        return out;
    }
    let undone: HashSet<&str> = moved
        .iter()
        .filter_map(|n| match &n.body {
            Body::Moved {
                undoes: Some(s), ..
            } => Some(s.as_str()),
            _ => None,
        })
        .collect();
    for n in moved {
        match &n.body {
            Body::Moved {
                undoes: None,
                split,
                nodes,
                ..
            } if !undone.contains(split.as_str()) => {
                out.hidden.extend(nodes.iter().cloned());
                out.live.insert(n.id.clone());
            }
            _ => {
                out.dead.insert(n.id.clone());
            }
        }
    }
    out
}

impl Moves {
    /// Nothing moved: every other method answers as if the records did not exist.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty() && self.dead.is_empty()
    }

    /// Whether a recompile selects `n`: not a moved node, and no record that is not live.
    pub fn shows(&self, n: &Stub) -> bool {
        !self.hidden.contains(&n.id) && !self.dead.contains(&n.id)
    }

    /// Whether `c`, frozen, holds what has since moved, or a record since undone.
    pub fn stale(&self, c: &Compilation) -> bool {
        !self.is_empty()
            && c.includes
                .iter()
                .any(|id| self.hidden.contains(id) || self.dead.contains(id))
    }
}

/// `prefix` with each live note put before the first node after its range began, not at its own
/// position (the tail of the session when it was written).
pub fn in_place(prefix: Vec<(u64, &Node)>) -> Vec<(u64, &Node)> {
    let is_note = |n: &Node| matches!(n.body, Body::Moved { .. });
    if !prefix.iter().any(|(_, n)| is_note(n)) {
        return prefix;
    }
    let (mut notes, mut rest): (Vec<_>, Vec<_>) = prefix.into_iter().partition(|(_, n)| is_note(n));
    notes.sort_by_key(|(p, n)| match &n.body {
        Body::Moved { first, .. } => (*first, *p),
        _ => (0, *p),
    });
    // Later notes first, so an earlier insertion never moves a later one's index.
    for (p, n) in notes.into_iter().rev() {
        let first = match &n.body {
            Body::Moved { first, .. } => *first,
            _ => p,
        };
        let at = rest
            .iter()
            .position(|(q, _)| *q > first)
            .unwrap_or(rest.len());
        rest.insert(at, (p, n));
    }
    rest
}

/// The META key of a split's record, which `thread.unsplit` reads.
pub fn split_key(id: &str) -> String {
    format!("thread.split.{id}")
}

/// The META key of the thread a node moved to: recall and `history` read it for one node without
/// reading a transcript. Its value is a [`MovedTo`], or null once the split is undone.
pub fn moved_of_key(node: &str) -> String {
    format!("moved.of.{node}")
}

/// Where a moved node went.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MovedTo {
    pub thread: String,
    pub split: String,
}

/// The id prefix of a claim (theseus-98bz.6, `rpc/home_claim.rs`): a [`MovedTo`] whose `split` is
/// `claim_…` keys a home turn's node to the thread it served. Recall reads it as a split's; nothing
/// left the home's compile, and a split may take the node over.
pub const CLAIM: &str = "claim";

impl MovedTo {
    /// Whether this is a claim's value, not a split's.
    pub fn is_claim(&self) -> bool {
        self.split.starts_with("claim_")
    }
}

/// What a split left, for its undo.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SplitRecord {
    pub id: String,
    /// How the source was named, and the session it is.
    pub from: String,
    pub source_session: String,
    /// The `Moved` node in the source.
    pub moved_node: String,
    /// The thread, its session, and whether the split opened it.
    pub thread: String,
    pub target_session: String,
    pub opened: bool,
    /// The brief and the arrangement written into the target's session.
    pub target_nodes: Vec<String>,
    pub moved: u32,
    pub first: u64,
    pub last: u64,
    pub gist: String,
    pub at_ms: u64,
    #[serde(default)]
    pub undone: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stub::Stub;

    fn note(sid: &str, split: &str, nodes: &[&str], first: u64, undoes: Option<&str>) -> Node {
        Node::harness_body(
            "mov",
            sid,
            AUTHOR,
            Body::Moved {
                split: split.into(),
                undoes: undoes.map(str::to_string),
                thread: "gauges".into(),
                to_session: "ses_g".into(),
                nodes: nodes.iter().map(|s| s.to_string()).collect(),
                first,
                last: first + 2,
                gist: "tides".into(),
                quiet: false,
            },
        )
    }

    fn stubbed(nodes: Vec<Node>) -> Vec<(u64, Stub)> {
        nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| {
                (
                    i as u64 + 1,
                    Stub::hydrated(i as u64 + 1, std::sync::Arc::new(n)),
                )
            })
            .collect()
    }

    #[test]
    fn a_record_hides_its_nodes_until_an_undo_names_it() {
        let a = Node::user("ses_h", Some("t1"), "owner", "a");
        let b = Node::user("ses_h", Some("t2"), "owner", "b");
        let m = note("ses_h", "spl_1", &[&a.id], 1, None);
        let live = read(&stubbed(vec![a.clone(), b.clone(), m.clone()]));
        assert!(live.hidden.contains(&a.id) && !live.hidden.contains(&b.id));
        assert!(live.live.contains(&m.id) && live.dead.is_empty());
        let undo = note("ses_h", "spl_1", &[], 0, Some("spl_1"));
        let undone = read(&stubbed(vec![a.clone(), b, m.clone(), undo.clone()]));
        assert!(undone.hidden.is_empty() && undone.live.is_empty());
        assert!(undone.dead.contains(&m.id) && undone.dead.contains(&undo.id));
        assert!(read(&stubbed(vec![a])).is_empty());
    }

    #[test]
    fn a_note_goes_before_the_first_node_after_its_range_began() {
        let n = |t: &str| Node::user("ses_h", None, "owner", t);
        let (a, b, c) = (n("a"), n("b"), n("c"));
        let m1 = note("ses_h", "spl_1", &[], 2, None);
        let m2 = note("ses_h", "spl_2", &[], 5, None);
        let prefix = vec![(1, &a), (4, &b), (6, &c), (7, &m1), (8, &m2)];
        let ordered: Vec<&str> = in_place(prefix)
            .iter()
            .map(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => text.as_str(),
                Body::Moved { split, .. } => split.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(ordered, ["a", "spl_1", "b", "spl_2", "c"]);
    }
}
