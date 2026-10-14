//! The self backlog's facts (theseus-pw1q.7): `self.backlog.filed`,
//! `.ranked`, `.started` and `.dropped`, each written in the frame that
//! moves the item (`crate::rsi::backlog`). Every `self.*` row carries `what`,
//! `why`, `numbers` and `undo` beside its own fields
//! (`theseus_protocol::rsi::SELF_ROWS`), so `self.log` shows it as written.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// An item filed (or, for a cause seen again, given more evidence).
pub struct BacklogFiled {
    pub item: String,
    pub title: String,
    pub cause: String,
    /// The rows and lines that filed it, or that this night added.
    pub evidence: Vec<String>,
    /// Where the evidence came from: a ledger kind, `health`, `learning`.
    pub source: String,
    /// The cause was already open: this night only added evidence.
    pub again: bool,
    pub count: u64,
    pub keel: Option<String>,
}

impl Fact for BacklogFiled {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBacklogFiled);

    fn row(&self) -> Value {
        let what = match self.again {
            true => format!("backlog item {} seen again: {}", self.item, self.title),
            false => format!("filed on the self backlog: {}", self.title),
        };
        json!({"item": self.item, "title": self.title, "evidence": &self.evidence,
               "source": self.source, "cause": self.cause, "keel": self.keel,
               "what": what, "why": format!("{} seen {} times", self.cause, self.count),
               "numbers": {"count": self.count, "evidence": self.evidence.len()},
               "undo": format!("theseus self drop {}", self.item)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let held = self
            .keel
            .as_ref()
            .map(|k| format!(" It touches the keel ({k}), so it waits for the owner."))
            .unwrap_or_default();
        say.line(
            Approval,
            format!(
                "Self backlog: {} {} ({} seen {} times).{held}",
                if self.again {
                    "more evidence for"
                } else {
                    "filed"
                },
                self.title,
                self.cause,
                self.count
            ),
        );
    }
}

/// An item's place moved.
pub struct BacklogRanked {
    pub item: String,
    pub title: String,
    pub rank: u32,
    pub score: u64,
    pub was: Option<u32>,
    /// `night` (the score moved it) or `owner` (the owner's word).
    pub by: String,
}

impl Fact for BacklogRanked {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBacklogRanked);

    fn row(&self) -> Value {
        json!({"item": self.item, "rank": self.rank, "score": self.score, "was": self.was,
               "by": self.by,
               "what": format!("backlog item {} is now rank {}: {}", self.item, self.rank, self.title),
               "why": match self.by.as_str() { "owner" => "moved by the owner's word", _ => "its score moved" },
               "numbers": {"rank": self.rank, "score": self.score, "was": self.was},
               "undo": self.was.map(|w| format!("theseus self move {} {w}", self.item))})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self backlog: {} is now rank {} (was {}).",
                self.title,
                self.rank,
                self.was.map_or("new".to_string(), |w| w.to_string())
            ),
        );
    }
}

/// The top item started, in a session of its own.
pub struct BacklogStarted {
    pub item: String,
    pub title: String,
    pub session: String,
    pub rank: u32,
}

impl Fact for BacklogStarted {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBacklogStarted);

    fn row(&self) -> Value {
        // The branch and its reservation are self-pr's and self-budget's:
        // until they join the row names the session the item runs in.
        json!({"item": self.item, "branch": null, "reserved_usd": null, "session": self.session,
               "what": format!("started backlog item {}: {}", self.item, self.title),
               "why": format!("it was rank {} and carries no keel mark", self.rank),
               "numbers": {"rank": self.rank}, "undo": "theseus self halt [why]"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self backlog: started {} in session {}.",
                self.title, self.session
            ),
        );
    }
}

/// An item left the backlog.
pub struct BacklogDropped {
    pub item: String,
    pub title: String,
    pub cause: String,
    /// Who: the owner's label, or `night` for the size cap.
    pub by: String,
    pub reason: String,
    /// The cause is never filed again.
    pub remembered: bool,
}

impl Fact for BacklogDropped {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfBacklogDropped);

    fn row(&self) -> Value {
        json!({"item": self.item, "cause": self.cause, "by": self.by, "reason": self.reason,
               "remembered": self.remembered,
               "what": format!("backlog item {} dropped: {}", self.item, self.title),
               "why": self.reason, "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self backlog: dropped {} ({}){}.",
                self.title,
                self.reason,
                if self.remembered {
                    "; the cause is never filed again"
                } else {
                    ""
                }
            ),
        );
    }
}

/// Any of the four, so a night collects the rows of its frame and speaks
/// them once the frame is written.
pub enum Spoken {
    Filed(BacklogFiled),
    Ranked(BacklogRanked),
    Started(BacklogStarted),
    Dropped(BacklogDropped),
}

impl Spoken {
    /// The item it is about.
    pub fn item(&self) -> &str {
        match self {
            Spoken::Filed(f) => &f.item,
            Spoken::Ranked(f) => &f.item,
            Spoken::Started(f) => &f.item,
            Spoken::Dropped(f) => &f.item,
        }
    }

    /// The fact's ledger row.
    pub fn row(&self) -> anyhow::Result<theseus_store::NewRecord> {
        match self {
            Spoken::Filed(f) => super::row(f, None, None),
            Spoken::Ranked(f) => super::row(f, None, None),
            Spoken::Started(f) => super::row(f, None, None),
            Spoken::Dropped(f) => super::row(f, None, None),
        }
    }

    /// Its sentences, once its frame is written.
    pub fn announce(&self, rec: &super::Rec<'_>) {
        match self {
            Spoken::Filed(f) => rec.announce(f),
            Spoken::Ranked(f) => rec.announce(f),
            Spoken::Started(f) => rec.announce(f),
            Spoken::Dropped(f) => rec.announce(f),
        }
    }
}
