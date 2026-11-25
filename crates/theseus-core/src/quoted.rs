//! Laying a result kept by reference (issue 49): every reader that lays or shows a stored result reads a
//! result that holds parts through the one renderer (`quotes::render`), and a turn lays each rendering as it
//! first made it.
//!
//! - **Where.** A result that holds parts stores no words (`content` empty), so a reader of `content` alone
//!   would lay nothing. The window and the turn's own results ([`lay_window`], [`freeze`]), the sizes the view
//!   pays for an exchange, `turn.inspect`, `node.list`, a session's page and publish read the rendering first
//!   ([`laid`], [`shown`]). `tests::no_reader_reads_content_of_a_parts_result` lists every reader of a
//!   result's `content` and why it may read it.
//! - **What has been said since.** A rendering says it per quote (`quotes::render`): an erased quote's words
//!   blank alone, and a quoted source marked wrong has its line once. The window then lays every node through
//!   `Since::lay` with the same `Since`, which reads a result kept by reference by its parts, not its copy
//!   edges, so it adds only the result's own mark; the result keeps its parts, which is how it is known.
//! - **Frozen within a turn.** A provider binds a thinking block to every byte before it, so nothing is
//!   rendered again within a turn: the window's results are rendered when the view is frozen, and the turn's
//!   own once, at the first call that lays them, kept on the turn's view ([`freeze`]) and laid as they are by
//!   every later call (`compiler::view`).
//! - **An erase during a turn.** Each rendering keeps the nodes it read words of. One erased since makes the
//!   view gone ([`erased_since`]): the turn is laid again on a new chain, its answers so far without their
//!   thinking, and the erased words are not sent again.
//! - **Nothing is written.** All of it is in memory: the stored result is as it was.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;

use crate::compiler::view::Frozen;
use crate::copies::since::Since;
use crate::node::{Body, Node};
use crate::quotes::{self, Field, Part, UNREADABLE_QUOTE};
use crate::store::Store;
use crate::stub::{Kind, Stub};

/// A result's rendering as a turn laid it: its words, and the nodes it read words of, none of them erased
/// when it was made.
#[derive(Debug, Clone, Default)]
pub struct Rendering {
    pub text: String,
    pub read: Vec<String>,
}

/// `n`'s parts, when it is a result kept by reference.
fn parts_of(n: &Node) -> Option<&[Part]> {
    match &n.body {
        Body::ToolResult { parts, .. } if !parts.is_empty() => Some(parts),
        _ => None,
    }
}

/// Whether `n` is a result kept by reference.
pub fn holds_parts(n: &Node) -> bool {
    parts_of(n).is_some()
}

/// `n`'s rendering now, with what `since` says has been said since; none for a node that holds no parts.
pub fn rendering(store: &Store, n: &Node, since: &Since<'_>) -> Result<Option<Rendering>> {
    rendering_then(store, n, since, || {})
}

/// [`rendering`], `after` run once its words are read. The nodes it may read words of are taken before them, so
/// one erased while they are read is still among them, and a later check of them finds it.
fn rendering_then(
    store: &Store,
    n: &Node,
    since: &Since<'_>,
    after: impl FnOnce(),
) -> Result<Option<Rendering>> {
    let Some(parts) = parts_of(n) else {
        return Ok(None);
    };
    let mut read: Vec<String> = (parts.iter())
        .filter_map(|p| match p {
            Part::Quote(q) if q.field != Field::Did => Some(q.node.clone()),
            _ => None,
        })
        .filter(|id| !erased(store, id))
        .collect();
    read.sort();
    read.dedup();
    let text = quotes::render(store, parts, since)?;
    after();
    Ok(Some(Rendering { text, read }))
}

/// Whether `id`'s newest record is a tombstone (an exchange's `did` line is never erased).
fn erased(store: &Store, id: &str) -> bool {
    #[cfg(test)]
    ERASED_READS.with(|c| c.set(c.get() + 1));
    matches!(store.get_node(id), Ok(Some((_, n))) if matches!(n.body, Body::Erased { .. }))
}

#[cfg(test)]
thread_local! {
    /// How many newest records [`erased`] has read on this thread: a test's count of what laying reads.
    pub(crate) static ERASED_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether a node a rendering read words of is erased now.
pub fn erased_since<'a>(store: &Store, read: impl IntoIterator<Item = &'a String>) -> bool {
    read.into_iter().any(|id| erased(store, id))
}

/// `n` with `text` as its words; its parts stay, so it is still known as a result kept by reference.
pub fn with_text(n: &Node, text: &str) -> Node {
    let mut node = n.clone();
    if let Body::ToolResult { content, .. } = &mut node.body {
        *content = text.to_string();
    }
    node
}

/// `n` as a conversation lays it: a result kept by reference with its rendering as its words; any other node
/// as it is.
pub fn laid(store: &Store, n: &Node, since: &Since<'_>) -> Result<Node> {
    Ok(match rendering(store, n, since)? {
        Some(r) => with_text(n, &r.text),
        None => n.clone(),
    })
}

/// The words `parts` show to a reader outside a conversation (inspect, a node's page, publish, the sizes the
/// view pays for): the rendering with nothing marked wrong; one that cannot be made shows [`UNREADABLE_QUOTE`].
pub fn shown_text(store: &Store, parts: &[Part]) -> String {
    let none = BTreeSet::new();
    quotes::render(store, parts, &Since::new(store, &none))
        .unwrap_or_else(|_| UNREADABLE_QUOTE.to_string())
}

/// What a reader with no store at hand reads of a result: its words, or for one kept by reference the fixed
/// line of a quote that cannot be read now ([`UNREADABLE_QUOTE`]), never an empty text.
pub fn words_or_line<'a>(content: &'a str, parts: &[Part]) -> &'a str {
    match parts.is_empty() {
        true => content,
        false => UNREADABLE_QUOTE,
    }
}

/// `n`'s nodes as a reader outside a conversation shows them ([`shown`]).
pub fn shown_all(store: &Store, nodes: &[(u64, Stub)]) -> Vec<(u64, Stub)> {
    (nodes.iter())
        .map(|(p, n)| match holds_parts(n) {
            true => (*p, Stub::from(shown(store, n))),
            false => (*p, n.clone()),
        })
        .collect()
}

/// `n` as a reader outside a conversation shows it ([`shown_text`]); any other node as it is.
pub fn shown(store: &Store, n: &Node) -> Node {
    match parts_of(n) {
        Some(parts) => with_text(n, &shown_text(store, parts)),
        None => n.clone(),
    }
}

/// The window's nodes with each result kept by reference laid ([`laid`]), and the nodes those renderings read
/// words of added to `read`.
pub fn lay_window(
    store: &Store,
    nodes: Vec<(u64, Stub)>,
    since: &Since<'_>,
    read: &mut Vec<String>,
) -> Result<Vec<(u64, Stub)>> {
    let mut out = Vec::with_capacity(nodes.len());
    for (p, n) in nodes {
        match (n.kind == Kind::ToolResult)
            .then(|| rendering(store, &n, since))
            .transpose()?
            .flatten()
        {
            Some(r) => {
                read.extend(r.read.iter().cloned());
                out.push((p, Stub::from(with_text(&n, &r.text))));
            }
            None => out.push((p, n)),
        }
    }
    Ok(out)
}

/// Each of the turn's own results kept by reference that `f` has not rendered yet (`compiler::view::own`),
/// rendered now and kept on it: the bytes every later call of the turn lays. `wrong` (the label set) is read
/// only when there is one to render.
pub fn freeze(
    store: &Store,
    f: &mut Frozen,
    nodes: &[(u64, Stub)],
    wrong: impl FnOnce() -> Result<BTreeSet<String>>,
) -> Result<()> {
    let todo: Vec<&Stub> = crate::compiler::view::own(nodes, f)
        .into_iter()
        .filter(|n| n.kind == Kind::ToolResult && !f.quoted.contains_key(&n.id))
        .filter(|n| holds_parts(n))
        .collect();
    if todo.is_empty() {
        return Ok(());
    }
    let wrong = wrong()?;
    let since = Since::new(store, &wrong);
    let mut made = HashMap::new();
    for n in todo {
        if let Some(r) = rendering(store, n, &since)? {
            made.insert(n.id.clone(), r);
        }
    }
    f.quoted.extend(made);
    Ok(())
}

/// The renderings a view re-frozen within the same turn keeps: each but one that read a node erased since.
pub fn carried(store: &Store, quoted: HashMap<String, Rendering>) -> HashMap<String, Rendering> {
    (quoted.into_iter())
        .filter(|(_, r)| !erased_since(store, &r.read))
        .collect()
}

/// A node of the turn's own as the request lays it: a result kept by reference as the turn first rendered it,
/// or as [`UNREADABLE_QUOTE`] when it was not (never empty, never rendered again here).
pub fn own_laid(n: Stub, f: &Frozen) -> Stub {
    if n.kind != Kind::ToolResult || !holds_parts(&n) {
        return n;
    }
    let text = (f.quoted.get(&n.id)).map_or(UNREADABLE_QUOTE, |r| r.text.as_str());
    Stub::from(with_text(&n, text))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_cost;
