//! `session.fork` (theseus-2qzx, R19): a conversation taken from any moment
//! of another, by reference. The method is `rpc/fork.rs`; what it stands on
//! is here.
//!
//! - **The record.** A fork's `SessionRecord.forked_from` names the session
//!   and the last node it takes (`ForkOrigin`); its first compilation has
//!   `inherits` (`Inherits`), a run, so the frame that opens it is a few
//!   hundred bytes at 1,000 nodes or at 100,000. Nothing of the source is
//!   copied and the source is never written to.
//! - **The transcript.** `Store::transcript` of a fork is the source's
//!   nodes through the fork point (`prefix_records`: itself a fork's, when
//!   the source is one) and then the fork's own; a fork's view lays the
//!   inherited run as its window, under the gate (`fork/window.rs`). A turn
//!   in the fork writes the fork's records alone.
//! - **Reach.** The frame also writes the source's reverse entry: an
//!   `includes` edge from the fork's compilation into the source session
//!   (`EdgeKind::Includes`), one for the run, which `node.reach` reads
//!   (`forks_of`).
//!
//! Hooks for what is not here: R20 (file pre-images, `--worktree`) reads
//! `SessionRecord.forked_from` and the result's `not_restored`; R21's
//! surfaces (the TUI's `f`, Discord's "Fork from here", the Ship's mark)
//! call `session.fork` with a node and print the result's `not_restored`.

pub mod cut;
pub mod window;

use anyhow::Result;
use serde::Deserialize;
use theseus_protocol::ForkOrigin;
use theseus_store::pages::ledger_session;
use theseus_store::{kinds, Page, Record, Store as _};

use crate::node::Node;
use crate::store::Store;

/// The deepest chain of forks followed: a loop in the records, which a
/// build never writes, ends here.
const MAX_DEPTH: usize = 64;

/// What a session record says of its origin, read without the rest.
#[derive(Deserialize)]
struct Peek {
    #[serde(default)]
    forked_from: Option<Box<ForkOrigin>>,
}

/// Where `session` was forked from, if it was.
pub fn origin_of(store: &Store, session: &str) -> Result<Option<ForkOrigin>> {
    Ok(store
        .get_session::<Peek>(session)?
        .and_then(|p| p.forked_from.map(|o| *o)))
}

/// The records a fork's transcript begins with: every node of `origin.session`'s
/// own transcript at or before `origin.position`, in WAL order. A source that
/// is itself a fork brings its own prefix first, cut at the same point.
pub fn prefix_records(store: &Store, origin: &ForkOrigin) -> Result<Vec<Record>> {
    prefix_in(store, origin, 0)
}

fn prefix_in(store: &Store, origin: &ForkOrigin, depth: usize) -> Result<Vec<Record>> {
    let mut out = match origin_of(store, &origin.session)? {
        Some(o) if depth < MAX_DEPTH => {
            let cut = ForkOrigin {
                position: o.position.min(origin.position),
                ..o
            };
            prefix_in(store, &cut, depth + 1)?
        }
        _ => Vec::new(),
    };
    out.extend(own_through(store, &origin.session, origin.position)?);
    Ok(out)
}

/// The node records `session` wrote itself at or before `through`, read a
/// page at a time so a source that has grown since is not read past it.
fn own_through(store: &Store, session: &str, through: u64) -> Result<Vec<Record>> {
    const PAGE: usize = 4096;
    let mut out = Vec::new();
    let mut after = 0;
    loop {
        let page = store.scope_page(session, after, PAGE)?;
        let Some(last) = page.last().map(|r| r.position) else {
            break;
        };
        let full = page.len() == PAGE;
        out.extend(
            page.into_iter()
                .filter(|r| r.kind == kinds::NODE && r.position <= through),
        );
        if !full || last >= through {
            break;
        }
        after = last;
    }
    Ok(out)
}

/// The last node of `session`'s transcript at or before `position`, with its
/// position: the fork point a WAL position names. It reads one record by the
/// index's page, session by session down the chain of forks.
pub fn node_at_or_before(
    store: &Store,
    session: &str,
    position: u64,
) -> Result<Option<(u64, Node)>> {
    let (mut s, mut bound) = (session.to_string(), position);
    for _ in 0..MAX_DEPTH {
        let q = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(&s)],
            before: Some(bound.saturating_add(1)),
            limit: 1,
            ..Page::default()
        };
        let last = match store.inner().page(&q)? {
            Some(out) => out.records.into_iter().next_back(),
            // While the index's shape is built: the session read whole.
            None => own_through(store, &s, bound)?.pop(),
        };
        if let Some(r) = last {
            return Ok(Some((r.position, r.decode()?)));
        }
        match origin_of(store, &s)? {
            Some(o) => {
                bound = bound.min(o.position);
                s = o.session;
            }
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Whether `node` (its session and position) is in `session`'s transcript:
/// its own, or inherited from a source through the fork point.
pub fn in_transcript(
    store: &Store,
    session: &str,
    node_session: &str,
    node_position: u64,
) -> Result<bool> {
    let (mut s, mut bound) = (session.to_string(), u64::MAX);
    for _ in 0..MAX_DEPTH {
        if s == node_session {
            return Ok(node_position <= bound);
        }
        match origin_of(store, &s)? {
            Some(o) => {
                bound = bound.min(o.position);
                s = o.session;
            }
            None => return Ok(false),
        }
    }
    Ok(false)
}

/// The sessions whose transcript holds the node at `node_position` of
/// `node_session`: forks of it cut at or after that position, and forks of
/// those. A verdict that lands on the source after a fork was cut reaches
/// these (theseus-sz6z9): the label follows the item, not the session.
pub fn forks_holding(store: &Store, node_session: &str, node_position: u64) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Forked {
        session_id: String,
        #[serde(default)]
        forked_from: Option<Box<ForkOrigin>>,
    }
    let mut out = Vec::new();
    for f in store.list_sessions::<Forked>()? {
        if f.forked_from.is_some()
            && in_transcript(store, &f.session_id, node_session, node_position)?
        {
            out.push(f.session_id);
        }
    }
    Ok(out)
}

/// A fork's content labels (theseus-9ymz0): the labels whose item the fork
/// carries (the item's node is at or before the fork point; one whose node is
/// unknown, or cannot be placed, is carried too, since fewer labels is never
/// the safe side), and the source's removals. `position` finds a node's WAL
/// position. A fork carries no mark (theseus-gyjyo.7): a turn bound for a model
/// of restricted origin goes on the labels, and an item that entered after the
/// fork point is asked about as it enters.
pub fn data_through(
    source: Option<&crate::content::ContentFlags>,
    point: u64,
    position: impl Fn(&str) -> Option<u64>,
) -> Option<Box<crate::content::ContentFlags>> {
    let s = source?;
    let mut forked = crate::content::ContentFlags::default();
    for l in &s.labels {
        let inside = l
            .node
            .as_deref()
            .and_then(&position)
            .is_none_or(|p| p <= point);
        if inside {
            forked.raise(l.clone());
        }
    }
    forked.removed.clone_from(&s.removed);
    (forked != crate::content::ContentFlags::default()).then(|| Box::new(forked))
}

/// Recover all included item labels, not only the source's first label of each kind.
/// Reads label records rather than the whole transcript, so unlabeled history stays cheap.
/// A record read in neither an item's shape nor its older one (`item_labels::kept`) is read for nothing: one
/// whose key or `node` names an item this fork holds refuses the fork ([`UnreadLabels`]), since a fork is
/// never made with fewer labels than its source may hold; any other is passed over and named in the log.
pub(crate) fn item_data_through(
    store: &Store,
    session: &str,
    point: u64,
    flags: &mut crate::content::ContentFlags,
) -> Result<()> {
    for key in labels_through(store, session, point, flags)? {
        crate::item_labels::kept::passed_over(&key);
    }
    Ok(())
}

/// A record under `labels.` that reads as no item's labels, whose key or `node` names an item a fork holds:
/// the fork is not made. Its words are fixed, and name the record by its key alone.
#[derive(Debug)]
pub struct UnreadLabels(pub String);

impl std::fmt::Display for UnreadLabels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the record under {} could not be read as an item's labels, and it names an item this fork would \
             hold: the fork is not made",
            self.0
        )
    }
}

impl std::error::Error for UnreadLabels {}

/// What [`item_data_through`] reads, with nothing logged: the keys of the records it passes over.
pub(crate) fn labels_through(
    store: &Store,
    session: &str,
    point: u64,
    flags: &mut crate::content::ContentFlags,
) -> Result<Vec<String>> {
    use crate::item_labels::kept::{of_record, Kept};
    let mut bounds = std::collections::BTreeMap::new();
    let (mut sid, mut through) = (session.to_string(), point);
    for _ in 0..MAX_DEPTH {
        if bounds.insert(sid.clone(), through).is_some() {
            break;
        }
        let Some(origin) = origin_of(store, &sid)? else {
            break;
        };
        through = through.min(origin.position);
        sid = origin.session;
    }
    let inside = |node: &str| -> Result<bool> {
        Ok((store.get_node(node)?)
            .is_some_and(|(pos, n)| bounds.get(&n.session_id).is_some_and(|end| pos <= *end)))
    };
    let mut passed = Vec::new();
    for record in store
        .inner()
        .latest_with_prefix(kinds::META, crate::item_labels::PREFIX)?
    {
        let item = match of_record(&record) {
            Kept::Whole(item) | Kept::Older(item) => item,
            Kept::Unread => {
                let key = record.key.unwrap_or_default();
                let named = serde_json::from_slice::<serde_json::Value>(&record.payload)
                    .ok()
                    .and_then(|v| v.get("node")?.as_str().map(str::to_string));
                let by_key = key.strip_prefix(crate::item_labels::PREFIX).map(str::to_string);
                for node in by_key.into_iter().chain(named) {
                    if inside(&node)? {
                        return Err(UnreadLabels(key).into());
                    }
                }
                passed.push(key);
                continue;
            }
        };
        if !item.kind.on_session() || !inside(&item.node)? {
            continue;
        }
        for label in &item.raised {
            if item.stands(label.flag, crate::item_labels::Reading { session, flags }) {
                flags.raise_after(label.clone(), item.entered_ms);
            }
        }
    }
    Ok(passed)
}

/// Whether a position falls in a compilation's inherited run.
pub fn inherited(c: &crate::compiler::Compilation, position: u64) -> bool {
    c.inherits
        .as_ref()
        .is_some_and(|i| position <= i.through_position)
}

/// The fork's title when the caller gives none.
pub fn default_title(source_title: Option<&str>, label: Option<&str>) -> String {
    let base = source_title.or(label).filter(|t| !t.trim().is_empty());
    format!("{} (fork)", base.unwrap_or("session"))
}

/// What a fork does not bring back, in words (R20 brings the files).
pub const NOT_RESTORED: &str =
    "files and what programs did after the fork point (they ran in the original)";
