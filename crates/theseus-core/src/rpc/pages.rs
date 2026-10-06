//! The lists that clients poll, read through the store's index
//! (theseus-96w2, theseus-vm3n.5): each costs about its answer, never the
//! history before it. Each falls back to the read it replaced while the
//! index's shape is built after serving (`WalStore::build_shape`).

use theseus_kernel::Action;
use theseus_store::pages::{action_execution, ledger_kind, ledger_kind_session, ledger_session};
use theseus_store::{kinds, Page, Store as _};

use super::Core;
use crate::node::Node;
use crate::session::SessionRecord;

/// The node tags a `node.list` filter reads: its kind, its session, or the
/// two together (`theseus_store::pages::tags_of`).
fn node_tags(kind: Option<&str>, session: Option<&str>) -> Vec<String> {
    match (kind, session) {
        (Some(k), Some(s)) => vec![ledger_kind_session(k, s)],
        (Some(k), None) => vec![ledger_kind(k)],
        (None, Some(s)) => vec![ledger_session(s)],
        (None, None) => Vec::new(),
    }
}

impl Core {
    /// The newest `n` actions by when each was planned, and how many there
    /// are, read through the index: the newest `n` keys by birth (an
    /// action's first record is its plan), or an execution's actions by
    /// their tag. `None` while the index's shape is built.
    pub(super) fn actions_paged(
        &self,
        execution: Option<&str>,
        n: usize,
    ) -> anyhow::Result<Option<(Vec<Action>, u64)>> {
        let store = self.store.inner();
        let mut actions: Vec<Action> = match execution {
            Some(x) => {
                let page = Page {
                    kind: kinds::ACTION,
                    tags: vec![action_execution(x)],
                    limit: usize::MAX / 2,
                    ..Page::default()
                };
                let Some(out) = store.page(&page)? else {
                    return Ok(None);
                };
                // Each action's latest record: the last of its key's.
                let mut latest = std::collections::BTreeMap::new();
                for r in out.records {
                    if let Some(k) = r.key.clone() {
                        latest.insert(k, r);
                    }
                }
                latest
                    .values()
                    .map(|r| r.decode())
                    .collect::<anyhow::Result<_>>()?
            }
            None => {
                let Some((born, _)) = store.newest_keys(kinds::ACTION, None, n)? else {
                    return Ok(None);
                };
                born.iter()
                    .map(|(_, r)| r.decode())
                    .collect::<anyhow::Result<_>>()?
            }
        };
        actions.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
        actions.truncate(n);
        Ok(Some((actions, store.count_keys(kinds::ACTION)?)))
    }

    /// The newest `n` nodes of a kind, a session, or both, newest first,
    /// read through the index's tags; `None` while its shape is built.
    pub(super) fn nodes_paged(
        &self,
        kind: Option<&str>,
        session: Option<&str>,
        n: usize,
    ) -> anyhow::Result<Option<Vec<(u64, Node)>>> {
        let page = Page {
            kind: kinds::NODE,
            tags: node_tags(kind, session),
            limit: n,
            ..Page::default()
        };
        let Some(out) = self.store.inner().page(&page)? else {
            return Ok(None);
        };
        let mut nodes = out
            .records
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect::<anyhow::Result<Vec<(u64, Node)>>>()?;
        nodes.reverse();
        Ok(Some(nodes))
    }

    /// The newest `n` sessions by when each was opened, before `before`,
    /// newest first, and the cursor to the next page back; `None` while the
    /// index's shape is built. Imported sessions (theseus-0lrr.6) are not
    /// listed: an import writes thousands at once, the newest births, and
    /// none takes a turn. They are skipped by key, undecoded.
    pub(super) fn sessions_paged(
        &self,
        n: usize,
        before: Option<u64>,
    ) -> anyhow::Result<Option<(Vec<SessionRecord>, Option<u64>)>> {
        let mut recs = Vec::new();
        let mut cursor = before;
        loop {
            let Some((born, more)) = self.store.inner().newest_keys(kinds::SESSION, cursor, n)?
            else {
                return Ok(None);
            };
            for (b, r) in &born {
                if r.key.as_deref().is_some_and(crate::import::is_imported) {
                    continue;
                }
                recs.push(r.decode()?);
                if recs.len() == n {
                    let last = born.last().map(|(l, _)| *l) == Some(*b);
                    return Ok(Some((recs, (more || !last).then_some(*b))));
                }
            }
            match born.last() {
                Some((b, _)) if more => cursor = Some(*b),
                _ => return Ok(Some((recs, None))),
            }
        }
    }
}
