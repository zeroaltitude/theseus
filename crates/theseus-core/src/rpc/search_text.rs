//! What a surface that reads no private text is given of a search (theseus-snb3,
//! the w9qv report's lane E). `index.query`, `memory.search` and
//! `memory.recalls` answer with the words of what they found, and none of them
//! took a surface: a connection no listener named, Discord's binding or the MCP
//! server would have been handed an imported session's text as the CLI is, where
//! `import.sessions`, `books.*` and `session.history` already withhold it
//! (`Surface::reads_private`). No such caller reaches them today (the MCP
//! server's allowlist leaves both out and the Discord binding calls neither), so
//! this is defence in depth: the next caller is refused by the method, and not
//! by whoever remembers to leave it off a list.
//!
//! A hit of a session that speaks in a private place, an imported one always
//! (`TurnRunner::place_of`), or in a place that cannot be read, keeps what
//! `session.history` keeps for such a node, who and when and where in the
//! record: its ids, position, kind, origin, author, place, date, sources and
//! scores. It loses its text, and the entities read out of that text. A hit of
//! a session in a shared place is the shared place's own, and is left alone:
//! which shared place the caller speaks for is not something these methods are
//! told, and the place rule (`Place::may_draw_on`) is the pack's.
//!
//! The answer still says that a hit exists, and how well it ranked: a surface
//! that chose the words learns which private sessions hold them. `import.sessions`
//! closes that by dropping the search words (`p.q = None`); closing it here would
//! drop the hits, which the lane's brief did not ask for.

use std::collections::HashMap;

use theseus_memory::recall::Place;
use theseus_protocol::index::{IndexHit, IndexQueryParams, IndexQueryResult};
use theseus_protocol::memory::{
    MemoryRecallsParams, MemoryRecallsResult, MemorySearchParams, RecallItem, RecallManifest,
};

use super::server::RpcFailure;
use super::Core;

/// What stands where a hit's text was.
pub const WITHHELD: &str = "this is the owner's own history, private under the place rule: \
                            its text is not shown on a surface that reads no private text";

/// Which sessions' text a call withholds, each session's place read once.
struct Withholder<'a> {
    core: &'a Core,
    seen: HashMap<String, bool>,
}

impl<'a> Withholder<'a> {
    fn new(core: &'a Core) -> Self {
        Self {
            core,
            seen: HashMap::new(),
        }
    }

    /// Whether `session_id`'s text stays here: it speaks in a shared place.
    /// A private place, an imported session and a place that cannot be read
    /// all withhold, so a miss reads as private.
    fn withholds(&mut self, session_id: &str) -> bool {
        *self
            .seen
            .entry(session_id.to_string())
            .or_insert_with(|| !matches!(self.core.runner.place_of(session_id), Place::Shared(_)))
    }

    fn hit(&mut self, h: &mut IndexHit) {
        if self.withholds(&h.session_id) {
            h.text = WITHHELD.into();
            h.entities.clear();
            h.entities_matched.clear();
        }
    }

    fn item(&mut self, i: &mut RecallItem) {
        if self.withholds(&i.session_id) {
            i.text = Some(WITHHELD.into());
        }
    }
}

impl Core {
    /// `index.query` as a connection asks it: `private` is whether its
    /// surface reads private text, and a surface that does not has each
    /// private hit's text withheld.
    pub(super) async fn index_query_for(
        &self,
        p: IndexQueryParams,
        private: bool,
    ) -> Result<IndexQueryResult, RpcFailure> {
        let mut r = self.index_query(p).await?;
        if !private {
            let mut w = Withholder::new(self);
            r.hits.iter_mut().for_each(|h| w.hit(h));
        }
        Ok(r)
    }

    /// `memory.search`, likewise: the manifest's admitted items.
    pub(super) async fn memory_search_for(
        &self,
        p: MemorySearchParams,
        private: bool,
    ) -> Result<RecallManifest, RpcFailure> {
        let mut m = self.memory_search(p).await?;
        if !private {
            let mut w = Withholder::new(self);
            m.admitted.iter_mut().for_each(|i| w.item(i));
        }
        Ok(m)
    }

    /// `memory.recalls`, likewise: each recall's admitted items.
    pub(super) fn memory_recalls_for(
        &self,
        p: MemoryRecallsParams,
        private: bool,
    ) -> Result<MemoryRecallsResult, RpcFailure> {
        let mut r = self.memory_recalls(p)?;
        if !private {
            let mut w = Withholder::new(self);
            r.recalls
                .iter_mut()
                .flat_map(|m| m.admitted.iter_mut())
                .for_each(|i| w.item(i));
        }
        Ok(r)
    }
}
