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
//! A new META family (`labels.`): no stored record gains a field, so no store-format step.

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
    /// `failed` (twice), `refused` (the day's budget), `unbuilt` (no state or client).
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
    /// The owner's removals: of this item's own label, or of the session's when its label was this item's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Removed>,
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

    /// Whether `f` stands on the item: raised, and not removed by the owner since the item entered.
    pub fn stands(&self, f: Flag, session: &ContentFlags) -> bool {
        self.raised.iter().any(|l| l.flag == f) && self.removal(f, session).is_none()
    }

    /// The owner's removal that covers `f` on this item: its own, or, for an input, the session's at or
    /// after the item entered (the rule `ContentFlags::raise_after` keeps). A reply's label was never the
    /// session's, so no removal of the session's covers it.
    pub fn removal<'a>(&'a self, f: Flag, session: &'a ContentFlags) -> Option<&'a Removed> {
        let own = self.removed.iter().rev().find(|r| r.flag == f);
        own.or_else(|| {
            if !self.kind.on_session() {
                return None;
            }
            (session.removed.iter().rev()).find(|r| r.flag == f && r.at_ms >= self.entered_ms)
        })
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

/// **The one rule both records are written by**: `label`, raised by an item that entered at `entered_ms`, is
/// kept on the item whatever the session holds (it is what Jev said of it), and goes on the session as
/// `ContentFlags::raise_after` decides, unless the item is one whose labels stay on it alone (a reply:
/// `ItemKind::on_session`). True when the session took it. A removal on the session covers the item as it
/// covers the session ([`ItemLabels::removal`]), so the item reads what the session reads.
pub fn land(
    flags: &mut ContentFlags,
    item: &mut ItemLabels,
    label: Label,
    entered_ms: u64,
) -> bool {
    if !item.raised.iter().any(|l| l.flag == label.flag) {
        item.raised.push(label.clone());
        item.raised.sort_by_key(|l| l.flag);
    }
    item.kind.on_session() && flags.raise_after(label, entered_ms)
}

/// The owner removed `removed.flag` from a session whose label named this item: kept on the item too.
pub fn removed(item: &mut ItemLabels, removed: &Removed) {
    item.removed.push(removed.clone());
}

/// The records of a frame, by node: read once from the store and changed in memory, so two landings of one
/// node in one frame write one record that holds both.
#[derive(Default)]
pub struct Pending {
    items: Vec<ItemLabels>,
}

impl Pending {
    /// `node`'s record in this frame: the one already changed, else the stored one, else a new one.
    pub fn of(
        &mut self,
        store: &Store,
        (node, kind, source, entered_ms): (&str, ItemKind, &str, u64),
    ) -> &mut ItemLabels {
        let at = match self.items.iter().position(|i| i.node == node) {
            Some(at) => at,
            None => {
                let stored = read(store, node).unwrap_or_else(|e| {
                    tracing::warn!(node, error = %format!("{e:#}"), "content: an item's labels were not read");
                    None
                });
                let fresh = || ItemLabels::new(node, kind, source, entered_ms);
                self.items.push(stored.unwrap_or_else(fresh));
                self.items.len() - 1
            }
        };
        &mut self.items[at]
    }

    /// The frame's records.
    pub fn records(&self) -> anyhow::Result<Vec<NewRecord>> {
        self.items.iter().map(ItemLabels::record).collect()
    }
}

// ------------------------------------------------------------ the reader

/// What an item's record says, against its session's labels: `labelled`, `clean`, `failed`, `not_judged`.
pub fn state(item: Option<&ItemLabels>, session: &ContentFlags) -> &'static str {
    let Some(item) = item else {
        return "not_judged";
    };
    if Flag::ALL.into_iter().any(|f| item.stands(f, session)) {
        return "labelled";
    }
    match &item.asked {
        Some(a) if a.answered() => "clean",
        Some(_) => "failed",
        // Only a source that needs no model wrote it: what it raised was removed.
        None if !item.raised.is_empty() => "clean",
        None => "not_judged",
    }
}

/// The view of `item` (the record of `node`, if any), against its session's labels and with its exchange's
/// `topics`.
pub fn view(
    node: &str,
    item: Option<&ItemLabels>,
    session: &ContentFlags,
    topics: Vec<String>,
) -> ItemLabelsView {
    let band = |b: Band| match serde_json::to_value(b) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    };
    let mut v = ItemLabelsView {
        node: node.into(),
        state: state(item, session).into(),
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
        v.why = (!a.answered()).then(|| a.outcome.clone());
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
            removed: item
                .removal(l.flag, session)
                .map(|r| format!("{} {}", r.by, r.via)),
        })
        .collect();
    v
}

/// The categories `onto.v1` filed the exchange of `turn` under, read from the touches it wrote
/// (`names::topics`): for each category the ontology holds, the few touches at the exchange's time. Nothing
/// is written; a turn with no exchange has none.
pub fn topics(
    store: &Store,
    ontology: &theseus_ontology::Ontology,
    turn: &str,
) -> anyhow::Result<Vec<String>> {
    use theseus_store::Store as _;
    let Some(ex) = crate::exchange::exchange(store, turn)? else {
        return Ok(Vec::new());
    };
    // A touch is keyed newest first: from the exchange's end (or a minute past its start), down to its
    // start, where `names::topics` keys it.
    let latest = ex.end_ms.unwrap_or(ex.start_ms + 60_000).max(ex.start_ms);
    let sep = crate::exchange::SEP;
    let mut out = Vec::new();
    for c in (ontology.categories()).filter(|c| !c.is_gone() && c.kind() != "project") {
        let id = c.id.as_str();
        let prefix = format!("{}thing:{id}{sep}", crate::exchange::TOUCH_PREFIX);
        let from = format!("{prefix}{:020}", u64::MAX - latest);
        let found = store
            .inner()
            .first_with_prefix_from(kinds::META, &prefix, &from, 4)?;
        let hit = found.iter().any(|r| {
            r.decode::<crate::exchange::Touch>()
                .is_ok_and(|t| t.exchange == turn && t.at_ms >= ex.start_ms)
        });
        if hit {
            out.push(id.to_string());
        }
    }
    out.sort();
    Ok(out)
}

/// What Jev says of `node`: its record, read against its own session's labels, with its exchange's topics
/// when an `ontology` is given. The one reader (`node.reach`, `view.show`, and any assembler later).
pub fn of_node(
    store: &Store,
    ontology: Option<&theseus_ontology::Ontology>,
    node: &str,
) -> anyhow::Result<ItemLabelsView> {
    let item = read(store, node)?;
    let found = store.get_node(node)?.map(|(_, n)| n);
    let session = match &found {
        Some(n) => store
            .get_session::<crate::session::SessionRecord>(&n.session_id)?
            .and_then(|r| r.data)
            .map(|b| *b)
            .unwrap_or_default(),
        None => ContentFlags::default(),
    };
    let topics = match (ontology, found.as_ref().and_then(|n| n.turn_id.as_deref())) {
        (Some(o), Some(turn)) => topics(store, o, turn)?,
        _ => Vec::new(),
    };
    Ok(view(node, item.as_ref(), &session, topics))
}

/// [`of_node`], read for `session` (round 2 makes the removals session-scoped; until then this is
/// [`of_node`]).
pub fn of_node_in(
    store: &Store,
    ontology: Option<&theseus_ontology::Ontology>,
    node: &str,
    _session: &str,
) -> anyhow::Result<ItemLabelsView> {
    of_node(store, ontology, node)
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
