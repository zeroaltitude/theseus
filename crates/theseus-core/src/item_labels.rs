//! Jev's labels on the item (issue 44): what the content pack said about ONE item, kept beside its node
//! (`labels.<node>`), so "what does Jev say about this node?" has an answer wherever the node is read from.
//!
//! - **Beside, never instead.** The session's `ContentFlags` and everything that acts on them (the injection
//!   label's freeze, the cards, the footer) are untouched. The item's record is written in the same frame as
//!   the session's, from the same answer, through [`land`]: one function decides both, so the two cannot
//!   disagree (`tests::the_items_and_the_session_always_agree`).
//! - **Failed is not clean.** An ask that failed, was refused or could not be built is kept as that, and an
//!   answer already kept is never replaced by a later failure ([`ItemLabels::asked`]).
//! - **No record is "not judged then".** An item from before this record has none, and nothing backfills it.
//! - **A description, not a permission.** Nothing withholds on it: an assembler may read it as data
//!   ([`of_node`]); none does today.
//!
//! - **A touch beside it** (issue 44): each raised label also writes the touch `label:<flag>` at its item's
//!   exchange's start ([`touch_root`]), in the same frame, a reply's too, so "everything labelled personal" is a
//!   timeline the history tool reads, as a topic's is. Nothing that acts reads it.
//!
//! A new META family (`labels.`): no stored record gains a field, so no store-format step. The touch is the
//! touch index's own `Touch`, under a root of a family it already holds.

use serde::{Deserialize, Serialize};
use theseus_judge::band::Band;
use theseus_protocol::{ItemFlag, ItemLabelsView, ItemQuestion};
use theseus_store::{kinds, NewRecord};

use crate::content::{ContentFlags, Flag, ItemKind, Label, Removed};
use crate::store::Store;

/// An item's record: `labels.<node id>`.
pub const PREFIX: &str = "labels.";

/// The pack a label no model raised is said to come from.
pub const DETERMINISTIC: &str = "deterministic";

/// One question's answer, as the item keeps it. A question the pack did not ask has no answer here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub question: Flag,
    /// `yes`, `no`, or another of the pack's choices.
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
}

/// The content pack's ask about the item: what it said, or why it said nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub pack: String,
    pub at_ms: u64,
    /// `answered`, `shadow` (answered, in the pack's shadow mode: nothing raised), or why there is no answer:
    /// `failed` (twice), `refused` (the day's budget, or a reply's share of it), `unbuilt` (no state or client),
    /// `truncated` (a reply the batch's piece bound left no piece for). An answer with `truncated` set below
    /// was read in part, and is never read as clean.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<Answer>,
    #[serde(default)]
    pub pieces: u32,
    #[serde(default)]
    pub truncated: bool,
}

impl Asked {
    /// Whether it holds the pack's answer (a shadow answer is one).
    pub fn answered(&self) -> bool {
        matches!(self.outcome.as_str(), "answered" | "shadow")
    }
}

/// What Jev said about one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemLabels {
    pub node: String,
    pub kind: ItemKind,
    pub source: String,
    /// When the item entered its session: a removal of a flag on the session at or after it covers it.
    pub entered_ms: u64,
    /// The pack's ask, when it was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<Asked>,
    /// Each flag raised on the item, once: the pack's at its bar, or a source that needs no model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raised: Vec<Label>,
    /// The owner's removals of a session's label that named this item, each with the session it was
    /// removed from: a removal speaks for that session alone (a fork or a child carries the same node's
    /// label, and its removal is its own).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ItemRemoved>,
}

/// One removal kept on an item: the session's removal, and which session it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemRemoved {
    pub session: String,
    pub flag: Flag,
    pub by: String,
    pub via: String,
    pub at_ms: u64,
}

/// Whom an item is read for: a session, and the labels it carries now. A removal applies only when read for
/// the session it was made in.
#[derive(Clone, Copy)]
pub struct Reading<'a> {
    pub session: &'a str,
    pub flags: &'a ContentFlags,
}

impl ItemLabels {
    pub fn new(node: &str, kind: ItemKind, source: &str, entered_ms: u64) -> Self {
        Self {
            node: node.into(),
            kind,
            source: source.into(),
            entered_ms,
            asked: None,
            raised: Vec::new(),
            removed: Vec::new(),
        }
    }

    /// Keep `a` as the pack's ask, unless it is a failure and an answer is already kept: a later failure
    /// never makes an answered item read as unanswered, and nothing makes a failed one read as clean.
    pub fn asked(&mut self, a: Asked) {
        if !a.answered() && self.asked.as_ref().is_some_and(Asked::answered) {
            return;
        }
        self.asked = Some(a);
    }

    /// Whether `f` stands on the item, read for `r`: raised, and not removed there since the item entered.
    pub fn stands(&self, f: Flag, r: Reading<'_>) -> bool {
        self.raised.iter().any(|l| l.flag == f) && self.removal(f, r).is_none()
    }

    /// The owner's removal that covers `f` on this item read for `r`, as `(by, via)`: one kept on the item
    /// for that session, or, for an input, that session's own at or after the item entered (the rule
    /// `ContentFlags::raise_after` keeps). A removal made in another session never covers it here. A reply's
    /// label was never a session's, so no session's removal covers it.
    pub fn removal<'a>(&'a self, f: Flag, r: Reading<'a>) -> Option<(&'a str, &'a str)> {
        let own = (self.removed.iter().rev()).find(|x| x.flag == f && x.session == r.session);
        if let Some(x) = own {
            return Some((x.by.as_str(), x.via.as_str()));
        }
        if !self.kind.on_session() {
            return None;
        }
        (r.flags.removed.iter().rev())
            .find(|x| x.flag == f && x.at_ms >= self.entered_ms)
            .map(|x| (x.by.as_str(), x.via.as_str()))
    }

    /// Its record.
    pub fn record(&self) -> anyhow::Result<NewRecord> {
        NewRecord::json(kinds::META, Some(&key(&self.node)), self)
    }
}

/// The key of `node`'s record.
pub fn key(node: &str) -> String {
    format!("{PREFIX}{node}")
}

/// `node`'s record, as last written.
pub fn read(store: &Store, node: &str) -> anyhow::Result<Option<ItemLabels>> {
    store.get_meta(&key(node))
}

// ------------------------------------------------------------ the label's touch (issue 44)

/// The touch index's root a raised label is kept under, beside its exchange: `label:<flag>`, so "everything
/// labelled personal" is a timeline the history tool reads (`history_tool::page`), as a topic's is. A
/// description beside the label: nothing that acts reads it.
pub fn touch_root(flag: Flag) -> String {
    format!("label:{}", flag.as_str())
}

/// Where `node`'s labels touch: its exchange's turn, session and start; with no exchange on record, the node's
/// own turn, session and time; none for a node that names no turn, or that cannot be read.
pub fn landing(store: &Store, node: &str) -> Option<(String, String, u64)> {
    let (_, n) = store.get_node(node).ok().flatten()?;
    let turn = n.turn_id?;
    match crate::exchange::exchange(store, &turn) {
        Ok(Some(ex)) => Some((ex.turn_id, ex.session_id, ex.start_ms)),
        Ok(None) => Some((turn, n.session_id, n.created_at_ms)),
        Err(e) => {
            tracing::warn!(node, error = %format!("{e:#}"), "content: an item's exchange was not read, so its labels touch nothing");
            None
        }
    }
}

/// The touches of `flags`, raised on an item, at its `landing`: one a flag, the same key however often it is
/// written.
pub fn touches(
    (turn, session, at): (&str, &str, u64),
    flags: impl IntoIterator<Item = Flag>,
) -> Vec<NewRecord> {
    (flags.into_iter())
        .filter_map(|f| {
            let root = touch_root(f);
            crate::exchange::touch(crate::exchange::Root::Thing(&root), turn, session, at)
        })
        .collect()
}

/// The touches of the labels raised on each node of `raised`, each at its node's [`landing`]. Read before a
/// frame takes its session's lock, and written in that frame.
pub fn touches_of<'a>(
    store: &Store,
    raised: impl IntoIterator<Item = (&'a str, Vec<Flag>)>,
) -> Vec<NewRecord> {
    let mut out = Vec::new();
    for (node, flags) in raised.into_iter().filter(|(_, f)| !f.is_empty()) {
        if let Some((turn, session, at)) = landing(store, node) {
            out.extend(touches((&turn, &session, at), flags));
        }
    }
    out
}

/// **The one rule both records are written by**: `label`, raised by an item that entered at `entered_ms`, goes
/// on the session as `ContentFlags::raise_after` decides, unless the INCOMING label's own kind is one whose
/// labels stay on the item (a reply: `ItemKind::on_session`). That decision reads the label alone, never the
/// stored record. Beside it the label is kept on the item's record, when there is one to write (`item`: none
/// when the stored record could not be read, [`Pending::of`]). True when the session took it.
pub fn land(
    flags: &mut ContentFlags,
    item: Option<&mut ItemLabels>,
    label: Label,
    entered_ms: u64,
) -> bool {
    if let Some(item) = item {
        if !item.raised.iter().any(|l| l.flag == label.flag) {
            item.raised.push(label.clone());
            item.raised.sort_by_key(|l| l.flag);
        }
    }
    label.kind.on_session() && flags.raise_after(label, entered_ms)
}

/// The owner removed `removed.flag` from `session`, whose label named this item: kept on the item, as that
/// session's.
pub fn removed(item: &mut ItemLabels, session: &str, removed: &Removed) {
    item.removed.push(ItemRemoved {
        session: session.into(),
        flag: removed.flag,
        by: removed.by.clone(),
        via: removed.via.clone(),
        at_ms: removed.at_ms,
    });
}

/// The records of a frame, by node: read once from the store and changed in memory, so two landings of one
/// node in one frame write one record that holds both.
#[derive(Default)]
pub struct Pending {
    items: Vec<ItemLabels>,
    /// Nodes whose stored record could not be read: nothing is written for them in this frame, so a kept
    /// answer is never overwritten by a fresh record after a transient error.
    unread: Vec<String>,
}

impl Pending {
    /// `node`'s record in this frame: the one already changed, else the stored one, else a new one. None
    /// when the stored record could not be read (logged): the landing writes nothing for that item, and its
    /// session's labels land as ever.
    pub fn of(
        &mut self,
        store: &Store,
        (node, kind, source, entered_ms): (&str, ItemKind, &str, u64),
    ) -> Option<&mut ItemLabels> {
        if self.unread.iter().any(|n| n == node) {
            return None;
        }
        let at = match self.items.iter().position(|i| i.node == node) {
            Some(at) => at,
            None => {
                let stored = match read(store, node) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(node, error = %format!("{e:#}"), "content: an item's labels were not read, so this landing writes none for it");
                        self.unread.push(node.into());
                        return None;
                    }
                };
                let fresh = || ItemLabels::new(node, kind, source, entered_ms);
                self.items.push(stored.unwrap_or_else(fresh));
                self.items.len() - 1
            }
        };
        Some(&mut self.items[at])
    }

    /// The frame's records.
    pub fn records(&self) -> anyhow::Result<Vec<NewRecord>> {
        self.items.iter().map(ItemLabels::record).collect()
    }
}

// ------------------------------------------------------------ the reader

/// What an item's record says, against its session's labels: `labelled`, `clean`, `failed`, `not_judged`.
pub fn state(item: Option<&ItemLabels>, r: Reading<'_>) -> &'static str {
    let Some(item) = item else {
        return "not_judged";
    };
    if Flag::ALL.into_iter().any(|f| item.stands(f, r)) {
        return "labelled";
    }
    match &item.asked {
        // Answered, but not all of it was read (`truncated`): never clean (issue 44).
        Some(a) if a.answered() && !a.truncated => "clean",
        Some(_) => "failed",
        // Only a source that needs no model wrote it: what it raised was removed.
        None if !item.raised.is_empty() => "clean",
        None => "not_judged",
    }
}

/// The view of `item` (the record of `node`, if any), read for `r` and with its exchange's `topics`.
pub fn view(
    node: &str,
    item: Option<&ItemLabels>,
    r: Reading<'_>,
    topics: Vec<String>,
) -> ItemLabelsView {
    let band = |b: Band| match serde_json::to_value(b) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    };
    let mut v = ItemLabelsView {
        node: node.into(),
        state: state(item, r).into(),
        topics,
        ..Default::default()
    };
    let Some(item) = item else {
        return v;
    };
    v.kind = Some(item.kind.as_str().into());
    v.source = Some(item.source.clone());
    if let Some(a) = &item.asked {
        v.pack = Some(a.pack.clone());
        v.at_ms = Some(a.at_ms);
        v.why = match (a.answered(), a.truncated) {
            (false, _) => Some(a.outcome.clone()),
            (true, true) => Some("truncated".into()),
            (true, false) => None,
        };
        v.questions = Flag::ALL
            .into_iter()
            .map(|f| match a.answers.iter().find(|x| x.question == f) {
                Some(x) => ItemQuestion {
                    question: f.as_str().into(),
                    said: x.choice.clone(),
                    band: Some(band(x.band)),
                    confidence: Some(x.confidence),
                },
                None => ItemQuestion {
                    question: f.as_str().into(),
                    said: "not_asked".into(),
                    ..Default::default()
                },
            })
            .collect();
    }
    v.flags = item
        .raised
        .iter()
        .map(|l| ItemFlag {
            flag: l.flag.as_str().into(),
            band: band(l.band),
            confidence: l.confidence,
            source: l.source.clone(),
            pack: match l.judgment.is_empty() {
                true => DETERMINISTIC.into(),
                false => (item.asked.as_ref()).map_or_else(String::new, |a| a.pack.clone()),
            },
            at_ms: l.at_ms,
            removed: (item.removal(l.flag, r)).map(|(by, via)| format!("{by} {via}")),
        })
        .collect();
    v
}

/// The categories `onto.v1` filed the exchange of `turn` under, read from the touches it wrote
/// (`names::topics`): for each category the ontology holds, one read of the exact key the touch has, at the
/// exchange's start (or, where the sink wrote before the exchange was stored, its message's time). Nothing is
/// written; a turn with no exchange has none.
pub fn topics(
    store: &Store,
    ontology: &theseus_ontology::Ontology,
    turn: &str,
) -> anyhow::Result<Vec<String>> {
    let Some(ex) = crate::exchange::exchange(store, turn)? else {
        return Ok(Vec::new());
    };
    let mut times = vec![ex.start_ms];
    if let Some((_, n)) = (ex.ask.as_deref()).and_then(|a| store.get_node(a).ok().flatten()) {
        times.push(n.created_at_ms);
    }
    let sep = crate::exchange::SEP;
    let mut out = Vec::new();
    for c in (ontology.categories()).filter(|c| !c.is_gone() && c.kind() != "project") {
        let id = c.id.as_str();
        for at in &times {
            let key = format!(
                "{}thing:{id}{sep}{:020}{sep}{turn}",
                crate::exchange::TOUCH_PREFIX,
                u64::MAX - at
            );
            if store.get_meta::<crate::exchange::Touch>(&key)?.is_some() {
                out.push(id.to_string());
                break;
            }
        }
    }
    out.sort();
    Ok(out)
}

/// What Jev says of `node`, read for its own session: its record against that session's labels, with its
/// exchange's topics when an `ontology` is given. The one reader (`node.reach`, `view.show`, and any
/// assembler later).
pub fn of_node(
    store: &Store,
    ontology: Option<&theseus_ontology::Ontology>,
    node: &str,
) -> anyhow::Result<ItemLabelsView> {
    let own = store.get_node(node)?.map(|(_, n)| n.session_id);
    of_node_in(store, ontology, node, own.as_deref().unwrap_or_default())
}

/// What Jev says of `node`, read for `session` (a fork or a child holds the same node's label, and a removal
/// made there is its own): its record against that session's labels and removals.
pub fn of_node_in(
    store: &Store,
    ontology: Option<&theseus_ontology::Ontology>,
    node: &str,
    session: &str,
) -> anyhow::Result<ItemLabelsView> {
    let item = read(store, node)?;
    let found = store.get_node(node)?.map(|(_, n)| n);
    let flags = match session.is_empty() {
        true => ContentFlags::default(),
        false => store
            .get_session::<crate::session::SessionRecord>(session)?
            .and_then(|r| r.data)
            .map(|b| *b)
            .unwrap_or_default(),
    };
    let topics = match (ontology, found.as_ref().and_then(|n| n.turn_id.as_deref())) {
        (Some(o), Some(turn)) => topics(store, o, turn)?,
        _ => Vec::new(),
    };
    let r = Reading {
        session,
        flags: &flags,
    };
    Ok(view(node, item.as_ref(), r, topics))
}

/// The items a piece of a view is read from, by node, as `view show` reads its text (`view::record::text`):
/// the ask, the person's other messages and the reply of its first and last exchange, or the node a recall
/// found. None for a piece made from items (a fold, a card, a fact): its labels are its sources'.
pub fn items_of_piece(store: &Store, kind: &str, first: &str, last: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match kind {
        "exchanges" => {
            for turn in [first, last] {
                let Ok(Some(ex)) = crate::exchange::exchange(store, turn) else {
                    continue;
                };
                let ids = (ex.ask.iter()).chain(&ex.messages).chain(&ex.reply);
                for id in ids {
                    if !out.contains(id) {
                        out.push(id.clone());
                    }
                }
            }
        }
        "recalled" => out.push(first.to_string()),
        _ => {}
    }
    out
}

/// Each item of a piece, one line each (`<node>: <what>`), for a person reading the view.
pub fn piece_lines(
    store: &Store,
    ontology: Option<&theseus_ontology::Ontology>,
    (kind, first, last): (&str, &str, &str),
) -> Vec<String> {
    items_of_piece(store, kind, first, last)
        .into_iter()
        .map(|n| match of_node(store, ontology, &n) {
            Ok(v) => format!("{n}: {}", v.line()),
            Err(e) => format!("{n}: not read ({e})"),
        })
        .collect()
}

#[cfg(test)]
mod tests;
