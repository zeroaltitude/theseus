//! What has been said about a node since it was written, laid beside its words. A node's words do not change
//! once written (an erase is the one exception), and what is said about it later (a retraction, a label) is
//! a record of its own that points at it. So a window laying an exchange whole shows each node with what has
//! been said since, in the code's own words: a look back's result (`context.history`, `look.work`) by the
//! sources its copy edges name, a task's report by its report edge, and any node by its own label.
//!
//! - **In memory, at lay time.** Nothing here writes or changes a stored node. The act gates read the stored
//!   node (`external.rs`, `task_graph/judged.rs`), so they see exactly what they saw: only the laid copy's
//!   words (`content`, `text`, a text block) gain the line; its tool, mark and meta are as stored.
//! - **Its words.** The line quotes nothing of a source but the gist and the reason its retraction row holds
//!   (`retract.rs`, as its correction note does), with the day. A source erased since lays one fixed line in
//!   place of all the words that showed it ([`ERASED_RESULT`], [`ERASED_REPORT`]).
//! - **Once, then still.** The line depends only on what has been said, so two turns with nothing said
//!   between them lay the same bytes; a correction changes the bytes of the earlier exchange once.
//! - **Its cost.** One edge-prefix read for each laid look back's result or report, and one node read for
//!   each source it names (whether it was erased); none for any other node, whose own label is in the label
//!   set already held in memory. The retraction rows are read once a window, and only when something laid
//!   is labelled.

use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, Store as _};

use crate::graph::{Edge, EdgeKind, VIA_REPORT};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin};
use crate::store::Store;
use crate::stub::{Kind, Stub};

/// What a look back's result is laid as when a source it showed was erased since.
pub const ERASED_RESULT: &str = "[This result showed words that have since been erased.]";
/// What a task's report is laid as when the message it relayed was erased since.
pub const ERASED_REPORT: &str = "[This report showed words that have since been erased.]";

/// The most sources one node's edges are read for.
const SOURCES_READ: usize = 1_000;

/// The author prefix of a task's report relayed into its parent (`task_wait.rs`, `turn/catch_up.rs`).
const REPORT_AUTHOR: &str = "task:";

/// What one window lays beside its nodes: the label set recall reads (`Memory::labeled`) and the store.
pub struct Since<'a> {
    store: &'a Store,
    wrong: &'a BTreeSet<String>,
    retracted: OnceCell<HashMap<String, Retracted>>,
}

/// What a node's `fact.retracted` row says of it.
struct Retracted {
    gist: String,
    why: String,
    at_ms: u64,
}

/// What has been said about one node since.
enum Said {
    /// A source it showed was erased: none of its words are laid.
    Erased,
    /// One line each.
    Lines(Vec<String>),
}

impl<'a> Since<'a> {
    pub fn new(store: &'a Store, wrong: &'a BTreeSet<String>) -> Self {
        Self {
            store,
            wrong,
            retracted: OnceCell::new(),
        }
    }

    /// `nodes`, one exchange laid whole, each with what has been said about it since. A node nothing was
    /// said about is the same stub.
    pub fn lay(&self, nodes: Vec<(u64, Stub)>) -> Result<Vec<(u64, Stub)>> {
        nodes
            .into_iter()
            .map(|(p, n)| {
                Ok(match self.said(&n)? {
                    None => (p, n),
                    Some(said) => (p, Stub::from(laid(&n, said))),
                })
            })
            .collect()
    }

    /// What has been said about `n` since: about a source it showed, then about itself.
    fn said(&self, n: &Stub) -> Result<Option<Said>> {
        if !matches!(
            n.kind,
            Kind::UserMessage | Kind::AssistantMessage | Kind::ToolResult
        ) {
            return Ok(None);
        }
        let mut lines = Vec::new();
        for source in self.sources(n)? {
            if self.erased(&source)? {
                return Ok(Some(Said::Erased));
            }
            if self.wrong.contains(&source) {
                lines.push(self.line(&source, false)?);
            }
        }
        if self.wrong.contains(&n.id) {
            lines.push(self.line(&n.id, true)?);
        }
        lines.dedup();
        Ok((!lines.is_empty()).then_some(Said::Lines(lines)))
    }

    /// The nodes whose words `n` showed: a look back's result's, by its copy edges; a task's report's, by
    /// its report edge. None for any other node, and nothing is read for it.
    fn sources(&self, n: &Node) -> Result<Vec<String>> {
        let route = match &n.body {
            Body::ToolResult { tool, .. } => match super::route(tool) {
                Some(via) => via,
                None => return Ok(Vec::new()),
            },
            Body::UserMessage { .. }
                if n.origin == Origin::Harness
                    && (n.author.as_deref()).is_some_and(|a| a.starts_with(REPORT_AUTHOR)) =>
            {
                VIA_REPORT
            }
            _ => return Ok(Vec::new()),
        };
        let prefix = format!("{}|{}|", EdgeKind::DerivedFrom.as_str(), n.id);
        let mut out = Vec::new();
        for r in (self.store.inner()).first_with_prefix(kinds::EDGE, &prefix, SOURCES_READ)? {
            let e: Edge = r.decode()?;
            if e.via == route && !out.contains(&e.to) {
                out.push(e.to);
            }
        }
        Ok(out)
    }

    /// Whether `id` reads as erased now (its newest record a tombstone).
    fn erased(&self, id: &str) -> Result<bool> {
        Ok((self.store.get_node(id)?).is_some_and(|(_, n)| matches!(n.body, Body::Erased { .. })))
    }

    /// The line for `id`, which is labelled: the node's own (`own`), or a source's.
    fn line(&self, id: &str, own: bool) -> Result<String> {
        let what = match self.retractions()?.get(id) {
            Some(r) => {
                let day = crate::judge::spend::local_day(r.at_ms);
                let who = match (own, r.gist.is_empty()) {
                    (true, _) => "it".to_string(),
                    (false, true) => "a message it showed".to_string(),
                    (false, false) => format!("\u{201c}{}\u{201d}", r.gist),
                };
                format!("{who} was marked wrong on {day}: {}", r.why)
            }
            None if own => "it was labelled wrong or stale".to_string(),
            None => "a message it showed was labelled wrong or stale".to_string(),
        };
        let since = if own { "said" } else { "read" };
        Ok(format!(
            "[Since this was {since}: {what}. The words above are as they were said.]"
        ))
    }

    /// Every node's first retraction row, read once.
    fn retractions(&self) -> Result<&HashMap<String, Retracted>> {
        if let Some(r) = self.retracted.get() {
            return Ok(r);
        }
        let mut out = HashMap::new();
        for r in self.store.scope_after(crate::retract::SCOPE, 0)? {
            if r.kind != kinds::LEDGER {
                continue;
            }
            let row: LedgerRow = r.decode()?;
            if row.kind != LedgerKind::FactRetracted.as_str() {
                continue;
            }
            let text = |k: &str| row.data[k].as_str().unwrap_or_default().to_string();
            out.entry(text("node_id")).or_insert(Retracted {
                gist: text("gist"),
                why: text("why"),
                at_ms: row.data["at_ms"].as_u64().unwrap_or_default(),
            });
        }
        Ok(self.retracted.get_or_init(|| out))
    }
}

/// Whether `n` showed words of a node erased since (a look back's result by its copy edges, a report by
/// its report edge): a replay of a request that laid it would send them again (`own_bench::rebuild`).
pub fn shows_erased(store: &Store, n: &Node) -> Result<bool> {
    let none = BTreeSet::new();
    let s = Since::new(store, &none);
    for source in s.sources(n)? {
        if s.erased(&source)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `n` as laid with `said`: its words, the only part changed. A copy, in memory.
fn laid(n: &Node, said: Said) -> Node {
    let mut node = n.clone();
    match (&mut node.body, said) {
        (Body::ToolResult { content, .. }, Said::Erased) => *content = ERASED_RESULT.into(),
        (Body::UserMessage { text, .. }, Said::Erased) => *text = ERASED_REPORT.into(),
        (Body::ToolResult { content: text, .. } | Body::UserMessage { text, .. }, Said::Lines(l)) => {
            text.push('\n');
            text.push_str(&l.join("\n"));
        }
        (Body::AssistantMessage { blocks, .. }, Said::Lines(l)) => {
            let line = l.join("\n");
            match blocks.iter().rposition(|b| b["type"] == "text") {
                Some(i) => {
                    let text = blocks[i]["text"].as_str().unwrap_or_default();
                    let text = format!("{text}\n\n{line}");
                    blocks[i]["text"] = text.into();
                }
                None => blocks.insert(0, serde_json::json!({"type": "text", "text": line})),
            }
        }
        _ => {}
    }
    node
}

#[cfg(test)]
mod tests;
