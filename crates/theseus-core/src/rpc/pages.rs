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

/// A page of `session.history` past a cursor when it names no `n`.
pub(super) const HISTORY_PAGE: usize = 200;

/// The tool calls read back at a time while a question's card is looked for.
const CARD_PAGE: usize = 32;

/// One page of a session's nodes (`Core::history_page`), oldest first, with
/// its cursors.
pub(super) struct HistoryPage {
    pub nodes: Vec<(u64, Node)>,
    /// The `after` for the next page, while more may follow.
    pub next: Option<u64>,
    /// The `before` for the next page back, while older nodes remain.
    pub older: Option<u64>,
    /// How many node records the page decoded: a page costs its answer,
    /// whatever the session's length (tests hold it to that).
    #[cfg_attr(not(test), allow(dead_code))]
    pub read: usize,
}

/// A page's bounds on a session read whole, oldest first (the read while
/// the index's shape is built): the nodes between `after` and `before`, the
/// first `n` of them from `after`, else the newest `n`; and whether more lie
/// past them in that direction, as the index's page says.
pub(super) fn bounded(
    mut all: Vec<(u64, Node)>,
    after: Option<u64>,
    before: Option<u64>,
    n: usize,
) -> (Vec<(u64, Node)>, bool) {
    all.retain(|(p, _)| after.is_none_or(|a| *p > a) && before.is_none_or(|b| *p < b));
    let more = all.len() > n;
    if after.is_some() {
        all.truncate(n);
    } else {
        all.drain(..all.len().saturating_sub(n));
    }
    (all, more)
}

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

    /// Every action not settled, however old (theseus-hnof.3), newest first
    /// and cut to `n`, and how many actions there are: the kernel's open
    /// actions, or an execution's by its own term, so the read costs what is
    /// open, never the history.
    pub(super) fn actions_unsettled(
        &self,
        execution: Option<&str>,
        n: usize,
    ) -> anyhow::Result<(Vec<Action>, u64)> {
        let mut actions = match execution {
            Some(x) => self.kernel.unsettled_actions(x)?,
            None => self.kernel.open_actions()?,
        };
        actions.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
        actions.truncate(n);
        Ok((actions, self.store.inner().count_keys(kinds::ACTION)?))
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
    /// none takes a turn. They are skipped by key in the page's one walk of
    /// the births, unread, however long their run (theseus-7087).
    pub(super) fn sessions_paged(
        &self,
        n: usize,
        before: Option<u64>,
    ) -> anyhow::Result<Option<(Vec<SessionRecord>, Option<u64>)>> {
        let Some((born, more)) =
            self.store
                .inner()
                .newest_keys_where(kinds::SESSION, before, n, &|k| {
                    !crate::import::is_imported(k)
                })?
        else {
            return Ok(None);
        };
        let older = born.last().map(|(b, _)| *b).filter(|_| more);
        let recs = born
            .iter()
            .map(|(_, r)| r.decode())
            .collect::<Result<Vec<SessionRecord>, _>>()?;
        Ok(Some((recs, older)))
    }

    /// `session.history`'s nodes (theseus-xo0m, theseus-kym3): with `after`,
    /// the first `n` past it, oldest first, and `next` while more may
    /// follow; with `before`, the newest `n` before it, and `older` while
    /// older nodes remain; with both, those between, from `after`. Each
    /// reads the session's tag, so a page costs its answer; while the
    /// index's shape is built, the session is read whole and the same
    /// bounds applied. Without either, the newest `n`, as before, or the
    /// whole session without `n`.
    pub(super) fn history_page(
        &self,
        session: &str,
        n: Option<usize>,
        after: Option<u64>,
        before: Option<u64>,
    ) -> anyhow::Result<HistoryPage> {
        let whole = |nodes: Vec<(u64, Node)>| HistoryPage {
            read: nodes.len(),
            nodes,
            next: None,
            older: None,
        };
        if after.is_none() && before.is_none() {
            let Some(n) = n else {
                return Ok(whole(self.store.session_nodes(session)?));
            };
            if let Some(mut v) = self.nodes_paged(None, Some(session), n)? {
                v.reverse();
                return Ok(whole(v));
            }
            let mut page = whole(self.store.session_nodes(session)?);
            page.nodes.drain(..page.nodes.len().saturating_sub(n));
            return Ok(page);
        }
        let n = n.unwrap_or(HISTORY_PAGE);
        let q = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(session)],
            after,
            before,
            limit: n,
            ..Page::default()
        };
        let (nodes, more, read) = match self.store.inner().page(&q)? {
            Some(out) => {
                let nodes = out
                    .records
                    .iter()
                    .map(|r| Ok((r.position, r.decode()?)))
                    .collect::<anyhow::Result<Vec<(u64, Node)>>>()?;
                let read = nodes.len();
                (nodes, out.more, read)
            }
            None => {
                let all = self.store.session_nodes(session)?;
                let read = all.len();
                let (nodes, more) = bounded(all, after, before, n);
                (nodes, more, read)
            }
        };
        let (first, last) = (nodes.first().map(|x| x.0), nodes.last().map(|x| x.0));
        Ok(HistoryPage {
            next: after.and(more.then_some(last).flatten()),
            older: before
                .filter(|_| after.is_none())
                .and(more.then_some(first).flatten()),
            nodes,
            read,
        })
    }

    /// The tool-call nodes the cards of `asks` read their gate's record from
    /// (`confirms_of`): the session's tool calls read back from the newest,
    /// a page at a time, until each ask's call is found, so a page of the
    /// history with a question waiting still costs about its answer (the
    /// call that waits is among the newest). The whole session while the
    /// index's shape is built.
    pub(super) fn card_nodes(
        &self,
        session: &str,
        asks: &[Action],
    ) -> anyhow::Result<Vec<(u64, Node)>> {
        // A budget's, an extension's, and a promotion's questions have no
        // call node.
        let own = [
            theseus_kernel::BUDGET_TOOL,
            crate::extend::ACK,
            super::packs::PROMOTE,
        ];
        let mut wanted: std::collections::BTreeSet<&str> = asks
            .iter()
            .filter(|a| !own.contains(&a.tool.as_str()))
            .map(|a| a.correlation_id.as_str())
            .collect();
        let mut found: Vec<(u64, Node)> = Vec::new();
        let mut before = None;
        while !wanted.is_empty() {
            let q = Page {
                kind: kinds::NODE,
                tags: vec![ledger_kind_session("tool_call", session)],
                before,
                limit: CARD_PAGE,
                ..Page::default()
            };
            let Some(out) = self.store.inner().page(&q)? else {
                return self.store.session_nodes(session);
            };
            for r in out.records.iter().rev() {
                let node: Node = r.decode()?;
                if let crate::node::Body::ToolCall {
                    correlation_id: Some(c),
                    ..
                } = &node.body
                {
                    if wanted.remove(c.as_str()) {
                        found.push((r.position, node));
                    }
                }
            }
            match (out.more, out.first) {
                (true, Some(first)) => before = Some(first),
                _ => break,
            }
        }
        found.sort_by_key(|(p, _)| *p);
        Ok(found)
    }
}
