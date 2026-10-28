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
//!   the source is one) and then the fork's own, so the compiler reads one
//!   list, and its `selected` takes the run from `Inherits`
//!   (`inherited`). A turn in the fork writes the fork's records alone.
//! - **Reach.** The frame also writes the source's reverse entry: an
//!   `includes` edge from the fork's compilation into the source session
//!   (`EdgeKind::Includes`), one for the run, which `node.reach` reads
//!   (`forks_of`).
//!
//! Hooks for what is not here: R20 (file pre-images, `--worktree`) reads
//! `SessionRecord.forked_from` and the result's `not_restored`; R21's
//! surfaces (the TUI's `f`, Discord's "Fork from here", the Ship's mark)
//! call `session.fork` with a node and print the result's `not_restored`.

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

/// A fork's content labels (theseus-9ymz0): the labels whose item the fork
/// carries (the item's node is at or before the fork point; one whose node is
/// unknown, or cannot be placed, is carried too, since fewer labels is never
/// the safe side), and how far the fork is judged: through the point when the
/// source was judged that far, else as far as it was. `position` finds a
/// node's WAL position; a `judged_through` it cannot place is dropped (the
/// fork is judged from nothing, fail closed).
pub fn data_through(
    source: Option<&crate::content::ContentFlags>,
    point: (u64, &str),
    position: impl Fn(&str) -> Option<u64>,
) -> Option<Box<crate::content::ContentFlags>> {
    let s = source?;
    let mut forked = crate::content::ContentFlags::default();
    for l in &s.labels {
        let inside = l
            .node
            .as_deref()
            .and_then(&position)
            .is_none_or(|p| p <= point.0);
        if inside {
            forked.raise(l.clone());
        }
    }
    forked.removed.clone_from(&s.removed);
    forked.judged_through =
        s.judged_through
            .as_deref()
            .and_then(&position)
            .map(|p| match p >= point.0 {
                true => point.1.to_string(),
                false => s.judged_through.clone().unwrap_or_default(),
            });
    (forked != crate::content::ContentFlags::default()).then(|| Box::new(forked))
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
