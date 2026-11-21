//! The view contract's fold traits over the store (`crate::view::fold`): the
//! ladder's read ([`StoreLadder`]) and the fold writer ([`StoreFolds`]). The
//! nodes are this module's records, shown as the view's `FoldNode`; the
//! window is the ledger's exchanges, as `ExchangeRef`s.

use anyhow::anyhow;

use super::fold::{self, Flags, FoldNode};
use super::read::{self, Step};
use super::writer::Writer;
use super::N;
use crate::exchange::{self, Exchange};
use crate::store::Store;
use crate::view::fold::{FoldNode as ViewNode, FoldWriter, Ladder, LadderRead, Writer as By};
use crate::view::timeline::ExchangeRef;
use crate::view::{Handle, Pos};

/// A node as the view reads it.
pub fn shown(n: &FoldNode) -> ViewNode {
    let pos = |at_ms, id: Option<&String>| Pos {
        at_ms,
        id: id.cloned().unwrap_or_default(),
    };
    ViewNode {
        level: n.level,
        place: n.place.clone(),
        first: pos(n.first_ms, n.exchanges.first()),
        last: pos(n.last_ms, n.exchanges.last()),
        exchanges: n.exchanges.len() as u32,
        title: n.title.clone(),
        his_words: n.his.iter().map(|h| h.node.clone()).collect(),
        his_counted: n.counted,
        account: n.account.clone(),
        marks: n.marks.clone(),
        handle: Handle(n.id.clone()),
        writer: By {
            by: n.model.clone().unwrap_or_else(|| "code".into()),
            version: n.writer.to_string(),
        },
    }
}

/// An exchange as the window lists it.
pub fn listed(e: &Exchange) -> ExchangeRef {
    ExchangeRef {
        turn_id: e.turn_id.clone(),
        session_id: e.session_id.clone(),
        at_ms: e.start_ms,
    }
}

/// The ladder's read over a store.
pub struct StoreLadder<'a>(pub &'a Store);

impl Ladder for StoreLadder<'_> {
    /// Every level's standing nodes, the highest first (so oldest first), and
    /// the window. The grain is [`N`]: a read at another `n` is refused.
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        if n != N {
            return Err(anyhow!("the ladder is folded at N = {N}, not {n}"));
        }
        let l = read::ladder(self.0, place, since.map_or(0, |p| p.at_ms))?;
        Ok(LadderRead {
            nodes: l
                .rungs
                .iter()
                .flat_map(|r| r.nodes.iter().map(shown))
                .collect(),
            window: l.window.iter().map(listed).collect(),
        })
    }
}

/// The fold writer over a store: a first-level node by the writer's model
/// (its frame written as the fold step writes it), a node above by code.
pub struct StoreFolds<'a> {
    pub store: &'a Store,
    pub writer: &'a dyn Writer,
    pub flags: &'a dyn Flags,
    pub hidden: &'a (dyn Fn(&str) -> bool + Sync),
}

impl StoreFolds<'_> {
    /// [`FoldWriter::fold`], for a caller on the runtime.
    pub async fn fold_async(
        &self,
        place: &str,
        exchanges: &[ExchangeRef],
    ) -> anyhow::Result<ViewNode> {
        let mut found = Vec::new();
        for r in exchanges {
            let e = exchange::exchange(self.store, &r.turn_id)?
                .ok_or_else(|| anyhow!("no exchange {}", r.turn_id))?;
            if e.place != place || e.end_ms.is_none() {
                return Err(anyhow!("{} is not an ended exchange of {place}", r.turn_id));
            }
            found.push(e);
        }
        let step = Step {
            writer: self.writer,
            flags: self.flags,
            hidden: self.hidden,
            cap_usd: None,
        };
        let _one = super::WRITING.lock().await;
        match read::first(self.store, &found, &step, None).await? {
            Ok((node, _)) => Ok(shown(&node)),
            Err((why, _)) => Err(anyhow!(why)),
        }
    }
}

impl FoldWriter for StoreFolds<'_> {
    /// Writes and returns the node. Its call is a network wait: on the
    /// daemon's multi-threaded runtime the worker's role is handed on first
    /// (`block_in_place`); with no runtime one is made for it. A
    /// current-thread runtime's caller uses [`StoreFolds::fold_async`].
    fn fold(&self, place: &str, exchanges: &[ExchangeRef]) -> anyhow::Result<ViewNode> {
        use tokio::runtime::{Handle as Rt, RuntimeFlavor};
        let fut = self.fold_async(place, exchanges);
        match Rt::try_current() {
            Ok(rt) if rt.runtime_flavor() == RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| rt.block_on(fut))
            }
            Ok(_) => Err(anyhow!("on a current-thread runtime, call fold_async")),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(fut),
        }
    }

    fn lift(&self, level: u8, nodes: &[ViewNode]) -> anyhow::Result<ViewNode> {
        let mut children = Vec::new();
        for v in nodes {
            let n = fold::node(self.store, &v.handle.0)?
                .ok_or_else(|| anyhow!("no node {}", v.handle.0))?;
            children.push(n);
        }
        if children.iter().any(|c| c.level + 1 != level) {
            return Err(anyhow!(
                "a node of level {level} is built from nodes of level {}",
                level.saturating_sub(1)
            ));
        }
        let node = fold::above(&children).ok_or_else(|| anyhow!("no nodes to lift"))?;
        let recs = fold::records(&node)?;
        theseus_store::blocking(|| self.store.append(&recs))?;
        Ok(shown(&node))
    }
}
