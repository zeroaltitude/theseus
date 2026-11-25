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
//!   (`retract.rs`, as its correction note does), with the day, and only of a source the kept text showed
//!   (`copies::shows`, asked with whether a source is the one of its session the copy links; a report shows the
//!   one message it relays): a copy's edges are a conservative superset
//!   (a runtime cut keeps a link to words it cut), and the line must not bring the words of a message the
//!   conversation never saw. A source erased since lays one fixed line in place of all the words that may
//!   have shown it, whether or not they did ([`ERASED_RESULT`], [`ERASED_REPORT`]).
//! - **A result kept by reference** (`quotes`) says exactly what it showed, and its one renderer
//!   (`quotes::render`) lays an erased quote as one fixed line in place of its words alone, and a quoted source
//!   marked wrong with the since line, once a result. So here its edges are not read: only its own mark is
//!   said, after the rendering a reader laid in `content`.
//! - **Once, then still.** The line depends only on what has been said, so two turns with nothing said
//!   between them lay the same bytes; a correction changes the bytes of the earlier exchange once.
//! - **Fail closed.** A source that cannot be found is treated as erased, as recall treats a source it cannot
//!   read (`recall/render.rs`, `source`). A node with more edges than are read ([`SOURCES_READ`]) is laid
//!   as one fixed line too ([`UNCHECKED_RESULT`], [`UNCHECKED_REPORT`]): its sources were not all checked.
//! - **Its cost.** One edge-prefix read for each laid look back's result or report, and one node read for
//!   each source it names (whether it was erased); none for any other unlabelled node. A node in the label
//!   set (held in memory) costs the retraction rows' one read a window.

use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap, HashSet};

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
/// What a look back's result is laid as when it has more edges than are read: its sources were not all
/// checked.
pub const UNCHECKED_RESULT: &str =
    "[This result showed more messages than can be checked now; its words are not laid.]";
/// The same for a task's report.
pub const UNCHECKED_REPORT: &str =
    "[This report showed more messages than can be checked now; its words are not laid.]";

/// The most edges one node's sources are read from; a node with more is laid as [`UNCHECKED_RESULT`].
pub const SOURCES_READ: usize = 1_000;

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
    /// A source it showed was erased, or cannot be found: none of its words are laid.
    Erased,
    /// It names more sources than are read: none of its words are laid.
    Unchecked,
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
        let (via, sources) = match by_reference(n) {
            true => ("", Vec::new()),
            false => match self.sources(n)? {
                Some(s) => s,
                None => return Ok(Some(Said::Unchecked)),
            },
        };
        let mut read = Vec::new();
        for source in sources {
            let Some(src) = self.readable(&source)? else {
                return Ok(Some(Said::Erased));
            };
            read.push(src);
        }
        for src in &read {
            // A session's handle shows `src` only when no other source `n` links shares its session.
            let alone = || {
                (read.iter())
                    .filter(|o| o.session_id == src.session_id)
                    .count()
                    == 1
            };
            if self.wrong.contains(&src.id) && (via == VIA_REPORT || super::shows(n, src, alone)) {
                lines.push(self.line(&src.id, false)?);
            }
        }
        if self.wrong.contains(&n.id) {
            lines.push(self.line(&n.id, true)?);
        }
        // Each line once, in the order first said: two sources labelled with no row say the same line.
        let mut seen = HashSet::new();
        lines.retain(|l| seen.insert(l.clone()));
        Ok((!lines.is_empty()).then_some(Said::Lines(lines)))
    }

    /// The nodes whose words `n` showed, and their route: a look back's result's, by its copy edges; a
    /// task's report's, by its report edge. Empty for any other node, and nothing is read for it; none when
    /// `n` has more edges than are read ([`SOURCES_READ`]), so they cannot all be checked.
    fn sources(&self, n: &Node) -> Result<Option<(&'static str, Vec<String>)>> {
        let route = match &n.body {
            Body::ToolResult { tool, .. } => match super::route(tool) {
                Some(via) => via,
                None => return Ok(Some(("", Vec::new()))),
            },
            Body::UserMessage { .. }
                if n.origin == Origin::Harness
                    && (n.author.as_deref()).is_some_and(|a| a.starts_with(REPORT_AUTHOR)) =>
            {
                VIA_REPORT
            }
            _ => return Ok(Some(("", Vec::new()))),
        };
        let prefix = format!("{}|{}|", EdgeKind::DerivedFrom.as_str(), n.id);
        // One more than the bound is asked for, so a node past it is told apart from one at it.
        let read =
            (self.store.inner()).first_with_prefix(kinds::EDGE, &prefix, SOURCES_READ + 1)?;
        if read.len() > SOURCES_READ {
            return Ok(None);
        }
        let mut out = Vec::new();
        for r in read {
            let e: Edge = r.decode()?;
            if e.via == route && !out.contains(&e.to) {
                out.push(e.to);
            }
        }
        Ok(Some((route, out)))
    }

    /// The node `id` as it reads now; none when it was erased (its newest record a tombstone) or cannot be
    /// found: either way its words are not shown, as recall does not show a source it cannot read.
    fn readable(&self, id: &str) -> Result<Option<Node>> {
        Ok(match self.store.get_node(id)? {
            Some((_, n)) if !matches!(n.body, Body::Erased { .. }) => Some(n),
            _ => None,
        })
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

    /// The line for `id`, a node a stored rendering quoted (`quotes::render`), when it is labelled; none when it is
    /// not.
    pub fn quoted(&self, id: &str) -> Result<Option<String>> {
        match self.wrong.contains(id) {
            true => self.line(id, false).map(Some),
            false => Ok(None),
        }
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

/// Whether `n` showed words of a node erased since, or cannot be found (a look back's result by its copy
/// edges, a report by its report edge, either kept by reference by a quote that does not read as it was
/// written), or names more than are read: a replay of a request that laid it would send them again
/// (`own_bench::rebuild`).
pub fn shows_erased(store: &Store, n: &Node) -> Result<bool> {
    if let Body::ToolResult { parts, .. } | Body::UserMessage { parts, .. } = &n.body {
        let gone = |q| !matches!(crate::quotes::read(store, q), crate::quotes::Read::Field(_));
        if (parts.iter()).any(|p| matches!(p, crate::quotes::Part::Quote(q) if gone(q))) {
            return Ok(true);
        }
    }
    let none = BTreeSet::new();
    let s = Since::new(store, &none);
    let Some((_, sources)) = s.sources(n)? else {
        return Ok(true);
    };
    for source in sources {
        if s.readable(&source)?.is_none() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `source` is the one node of its session that the look back's result `result` links by its copy
/// edges (`copies::shows` asks it of an imported message's line, which names only its session). No when
/// `result` links more than are read, when it does not link `source`, or when a node it links cannot be read
/// as it is now (erased or not found): failing toward seen.
pub fn alone(store: &Store, result: &Node, source: &Node) -> Result<bool> {
    let none = BTreeSet::new();
    let Some((_, sources)) = Since::new(store, &none).sources(result)? else {
        return Ok(false);
    };
    if !sources.contains(&source.id) {
        return Ok(false);
    }
    let mut same = 0;
    for id in &sources {
        match store.get_node(id)? {
            Some((_, n)) if !matches!(n.body, Body::Erased { .. }) => {
                same += usize::from(n.session_id == source.session_id);
            }
            _ => return Ok(false),
        }
    }
    Ok(same == 1)
}

/// Whether `n` is a look back's result or a task's report kept by reference (`quotes`).
fn by_reference(n: &Node) -> bool {
    matches!(&n.body, Body::ToolResult { parts, .. } | Body::UserMessage { parts, .. } if !parts.is_empty())
}

/// `n` as laid with `said`: its words, the only part changed. A copy, in memory.
fn laid(n: &Node, said: Said) -> Node {
    let mut node = n.clone();
    match (&mut node.body, said) {
        (Body::ToolResult { content, .. }, Said::Erased) => *content = ERASED_RESULT.into(),
        (Body::UserMessage { text, .. }, Said::Erased) => *text = ERASED_REPORT.into(),
        (Body::ToolResult { content, .. }, Said::Unchecked) => *content = UNCHECKED_RESULT.into(),
        (Body::UserMessage { text, .. }, Said::Unchecked) => *text = UNCHECKED_REPORT.into(),
        (
            Body::ToolResult { content: text, .. } | Body::UserMessage { text, .. },
            Said::Lines(l),
        ) => {
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
#[cfg(test)]
mod tests_parts;
