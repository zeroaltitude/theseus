//! Fold nodes (`fold.<place>␁<level>␁<first ms>`) and their rungs (`rung.<place>␁<level>␁…`). A
//! first-level node stands for [`FOLD_STEP`](super::FOLD_STEP) consecutive exchanges of a
//! place: the person's words in them by node id, never copied; a title and
//! an account written once by a small model from their asks, their `did`
//! lines and their final replies, each cut by code; the union of their
//! marks; and its id, the handle that opens them. A node of a higher level is
//! built by code from its children's typed fields: their titles and handles,
//! and of the person's lines only those that are corrections or made a
//! standing record, the rest counted. A node never changes once written.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord, Store as _};

use super::did::{self, Read};
use super::writer::{Ask, Work, Writer, Written};
use super::{cut, union, Marks, SEP};
use crate::store::Store;

/// A node: `fold.<place>␁<level>␁<first exchange's ms, 20 digits>`, the view
/// contract's key (`crate::view::fold::FOLD_PREFIX`).
pub use crate::view::fold::FOLD_PREFIX;
/// A node's key by its handle: `foldh.<id>`.
pub const HANDLE_PREFIX: &str = "foldh.";
/// A node's place in its level: `rung.<place>␁<level, 2 digits>␁<u64::MAX - last_ms, 20 digits>␁<id>`.
pub const RUNG_PREFIX: &str = "rung.";
/// A title's characters at most, and an account's.
pub const TITLE_CHARS: usize = 120;
pub const ACCOUNT_CHARS: usize = 2_400;
/// The most of each exchange's message, and of its reply, a fold's writer reads.
pub const ASK_CHARS: usize = 1_500;
pub const REPLY_CHARS: usize = 600;

/// One of the person's messages a node covers, by node id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HisLine {
    pub node: String,
    /// The judge's answer called it a correction.
    pub correction: bool,
    /// A standing record was made from it.
    pub standing: bool,
}

/// A node one level down that a higher node stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Child {
    pub id: String,
    pub title: String,
}

/// One fold node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FoldNode {
    /// Its handle.
    pub id: String,
    pub place: String,
    /// 1 for a node over exchanges; 2 and up for a node over nodes.
    pub level: u8,
    /// Every exchange it stands for, by turn, oldest first.
    pub exchanges: Vec<String>,
    /// The first and the last of them's start.
    pub first_ms: u64,
    pub last_ms: u64,
    /// The person's lines it keeps, by node id: every one at the first
    /// level; above it, the corrections and those that made a standing record.
    pub his: Vec<HisLine>,
    /// The person's lines it stands for and does not keep.
    pub counted: u32,
    /// Above the first level: the nodes it stands for, oldest first.
    pub children: Vec<Child>,
    pub title: String,
    pub account: String,
    pub marks: Marks,
    /// The model that wrote the first level's words, and the version of its
    /// instructions; none above it, which code builds.
    pub model: Option<String>,
    pub writer: u32,
    pub at_ms: u64,
}

/// Whether a line of the person's is a correction or made a standing record.
pub trait Flags: Sync {
    /// Whether the message `node`, which opened exchange `turn`, is a correction.
    fn correction(&self, turn: &str, node: &str) -> bool;
    fn standing(&self, node: &str) -> bool;
}

/// No line flagged.
pub struct NoFlags;

impl Flags for NoFlags {
    fn correction(&self, _: &str, _: &str) -> bool {
        false
    }
    fn standing(&self, _: &str) -> bool {
        false
    }
}

fn rung_level(place: &str, level: u8) -> String {
    format!("{RUNG_PREFIX}{place}{SEP}{level:02}{SEP}")
}

/// A range's identity: its place, its level and its exact exchanges.
pub fn range_key(place: &str, level: u8, first_ms: u64, exchanges: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for t in exchanges {
        h.update(t.as_bytes());
        h.update([0u8]);
    }
    let range = hex::encode(&h.finalize()[..8]);
    format!("{FOLD_PREFIX}{place}{SEP}{level}{SEP}{first_ms:020}{SEP}{range}")
}

/// A node's key: the view contract's (`fold.<place>␁<level>␁<first ms>`),
/// with its range's digest after it, so two ranges that begin in one
/// millisecond are two keys and one range is always one.
pub fn key(n: &FoldNode) -> String {
    range_key(&n.place, n.level, n.first_ms, &n.exchanges)
}

/// The node already written for `n`'s range, else `n` written with its
/// indexes, once. Called under [`super::WRITING`].
pub fn write_once(store: &Store, n: FoldNode) -> anyhow::Result<FoldNode> {
    if let Some(had) = theseus_store::blocking(|| store.get_meta::<FoldNode>(&key(&n)))? {
        return Ok(had);
    }
    let recs = records(&n)?;
    theseus_store::blocking(|| store.append(&recs))?;
    Ok(n)
}

/// A node's three records: itself, its rung and its handle, the last two
/// naming its key.
pub fn records(n: &FoldNode) -> anyhow::Result<Vec<NewRecord>> {
    let rung = format!(
        "{}{:020}{SEP}{}",
        rung_level(&n.place, n.level),
        u64::MAX - n.last_ms,
        n.id
    );
    let key = key(n);
    Ok(vec![
        NewRecord::json(kinds::META, Some(&key), n)?,
        NewRecord::json(kinds::META, Some(&rung), &key)?,
        NewRecord::json(kinds::META, Some(&format!("{HANDLE_PREFIX}{}", n.id)), &key)?,
    ])
}

/// The node whose handle is `id`.
pub fn node(store: &Store, id: &str) -> anyhow::Result<Option<FoldNode>> {
    match store.get_meta::<String>(&format!("{HANDLE_PREFIX}{id}"))? {
        Some(key) => store.get_meta(&key),
        None => Ok(None),
    }
}

/// The newest `limit` nodes of `place` at `level`, newest first: one read of
/// the rungs' first keys.
pub fn newest(
    store: &Store,
    place: &str,
    level: u8,
    limit: usize,
) -> anyhow::Result<Vec<FoldNode>> {
    let keys = store
        .inner()
        .first_with_prefix(kinds::META, &rung_level(place, level), limit)?;
    let mut out = Vec::new();
    for r in keys {
        let key: String = r.decode()?;
        // The prefix is not the boundary: a node is this ladder's only when
        // it says so itself.
        out.extend(
            store
                .get_meta::<FoldNode>(&key)?
                .filter(|n| n.place == place && n.level == level),
        );
    }
    Ok(out)
}

/// The exchanges a node opens to, oldest first: those still in the store.
pub fn open(store: &Store, n: &FoldNode) -> anyhow::Result<Vec<crate::exchange::Exchange>> {
    let mut out = Vec::new();
    for t in &n.exchanges {
        out.extend(crate::exchange::exchange(store, t)?);
    }
    Ok(out)
}

/// Every exchange `n` stands for, oldest first; none when any of them cannot be resolved today.
pub fn whole(store: &Store, n: &FoldNode) -> Option<Vec<crate::exchange::Exchange>> {
    let read = open(store, n).ok()?;
    (!n.exchanges.is_empty() && read.len() == n.exchanges.len()).then_some(read)
}

/// Every exchange fold node `id` stands for ([`whole`]); none when the node does not read: the rule the
/// model's assembly and every reader of a view record hold a fold to.
pub fn constituents(store: &Store, id: &str) -> Option<Vec<crate::exchange::Exchange>> {
    whole(store, &node(store, id).ok()??)
}

/// What a first-level node's writer reads: each exchange's message, its
/// `did` line (or that it has none) and the start of its reply, cut by code.
pub fn material(reads: &[(Read, did::Said)]) -> String {
    let mut out = String::new();
    for (i, (r, said)) in reads.iter().enumerate() {
        let day = crate::judge::spend::local_day(r.ex.start_ms);
        let ask = r
            .ask
            .as_deref()
            .map_or("(no message)".into(), |a| cut(a, ASK_CHARS));
        let reply = cut(r.reply.as_deref().unwrap_or(""), REPLY_CHARS);
        let body = format!(
            "{}: {ask}\nWhat Theseus did: {}\nIts reply began: {reply}",
            super::did::said_by(r.author),
            match said {
                // A line written before its writer was told the author is not fed to a new summary.
                did::Said::Line(d) if d.writer < super::writer::AUTHORSHIP_SINCE =>
                    did::NOT_SUMMARISED,
                _ => said.text(),
            }
        );
        let body = match r.marks.outside {
            true => super::writer::outside(Some("a tool's result this exchange read"), &body),
            false => body,
        };
        out.push_str(&format!("Exchange {} ({day}):\n{body}\n\n", i + 1));
    }
    out
}

/// A writer's answer as a title and an account: the first line, and the
/// rest, each cut by code; none when either is empty.
pub fn parse(answer: &str) -> Option<(String, String)> {
    let answer = answer.trim();
    let (title, account) = answer.split_once('\n')?;
    let (title, account) = (cut(title, TITLE_CHARS), cut(account, ACCOUNT_CHARS));
    (!title.is_empty() && !account.is_empty()).then_some((title, account))
}

/// The union of `marks`; a missing place when there are none.
fn node_marks<'a>(mut marks: impl Iterator<Item = &'a Marks>) -> Marks {
    let first = marks.next().cloned().unwrap_or(Marks {
        place: None,
        audience: crate::view::Audience::Unknown,
        author: crate::view::Author::Theseus,
        outside: false,
        sealed: false,
        veiled: false,
    });
    marks.fold(first, union)
}

/// The person's lines of `reads`, flagged.
fn his_lines(reads: &[(Read, did::Said)], flags: &dyn Flags) -> Vec<HisLine> {
    reads
        .iter()
        .flat_map(|(r, _)| r.ex.messages.iter().map(move |n| (&r.ex, n)))
        .map(|(ex, n)| HisLine {
            node: n.clone(),
            correction: flags.correction(&ex.turn_id, n),
            standing: flags.standing(n),
        })
        .collect()
}

/// A first-level node over `reads` (oldest first, all of one place), its
/// words written by `writer`, its call reserved on `purse`: the node, or why
/// there is none, with what the call cost either way.
pub async fn first_level(
    reads: &[(Read, did::Said)],
    writer: &dyn Writer,
    flags: &dyn Flags,
    purse: &dyn super::Purse,
) -> (Result<FoldNode, String>, Written) {
    let (Some((first, _)), Some((last, _))) = (reads.first(), reads.last()) else {
        return (Err("no exchanges".into()), Written::default());
    };
    let ask = Ask {
        work: Work::Fold,
        text: material(reads),
        sources: reads.len() as u32,
    };
    let w = writer.write(ask, purse).await;
    let Some((title, account)) = w.text.as_deref().and_then(parse) else {
        let why = w
            .failed
            .clone()
            .unwrap_or_else(|| "the answer held no title and account".into());
        return (Err(why), w);
    };
    let node = FoldNode {
        id: crate::new_id("fld"),
        place: first.ex.place.clone(),
        level: 1,
        exchanges: reads.iter().map(|(r, _)| r.ex.turn_id.clone()).collect(),
        first_ms: first.ex.start_ms,
        last_ms: last.ex.start_ms,
        his: his_lines(reads, flags),
        counted: 0,
        children: Vec::new(),
        title,
        account,
        marks: node_marks(reads.iter().map(|(r, _)| &r.marks)),
        model: Some(w.model.clone()),
        writer: super::writer::WRITER_VERSION,
        at_ms: theseus_protocol::now_unix_ms(),
    };
    (Ok(node), w)
}

/// A node of the level above `children` (oldest first, one level, one
/// place), built by code from their fields alone.
pub fn above(children: &[FoldNode]) -> Option<FoldNode> {
    let (first, last) = (children.first()?, children.last()?);
    if children
        .iter()
        .any(|c| c.place != first.place || c.level != first.level)
    {
        return None;
    }
    let kept: Vec<HisLine> = children
        .iter()
        .flat_map(|c| c.his.iter())
        .filter(|h| h.correction || h.standing)
        .cloned()
        .collect();
    let all: u32 = children
        .iter()
        .map(|c| c.his.len() as u32 + c.counted)
        .sum();
    let from = crate::judge::spend::local_day(first.first_ms);
    let to = crate::judge::spend::local_day(last.last_ms);
    let account = children
        .iter()
        .map(|c| format!("- {} [{}]", c.title, c.id))
        .collect::<Vec<_>>()
        .join("\n");
    Some(FoldNode {
        id: crate::new_id("fld"),
        place: first.place.clone(),
        level: first.level + 1,
        exchanges: children
            .iter()
            .flat_map(|c| c.exchanges.iter().cloned())
            .collect(),
        first_ms: first.first_ms,
        last_ms: last.last_ms,
        counted: all - kept.len() as u32,
        his: kept,
        children: children
            .iter()
            .map(|c| Child {
                id: c.id.clone(),
                title: c.title.clone(),
            })
            .collect(),
        title: cut(
            &format!("{} parts, {from} to {to}: {}", children.len(), first.title),
            TITLE_CHARS,
        ),
        account,
        marks: node_marks(children.iter().map(|c| &c.marks)),
        model: None,
        // Its title and account are its children's titles: as old as the oldest of them.
        writer: children
            .iter()
            .map(|c| c.writer)
            .min()
            .unwrap_or(super::writer::WRITER_VERSION),
        at_ms: theseus_protocol::now_unix_ms(),
    })
}
