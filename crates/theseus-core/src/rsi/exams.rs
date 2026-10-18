//! Self-exams (theseus-pw1q.11): the owner's corrections, a judge's wrong
//! labels, failed tasks and over-budget turns become exam items that guard
//! the next change. Off with `[self] mode = "off"`; on, one pass a night
//! (`pass.rs`), after `rsi::gate`.
//!
//! - **Candidates** (`harvest.rs`): a `route.corrected` row or a correction
//!   note on a remembered fact (`fact.corrected`); an operator's `judge.label`
//!   that contradicts the judge's answer, on the **train split** only (a
//!   judgment older than the learning loop's holdout window); a `task.ended`
//!   row that ended `failed` or `budget_exhausted`; a `budget.reached` row
//!   (a turn that went past its session's limit). Each becomes an [`Item`]
//!   in this module's own versioned format ([`VERSION`]; theseus-exam's
//!   check language for its `check`, but its own families: the exam crate
//!   depends on the core, never the other way), with its source rows as
//!   provenance, scrubbed ([`Item::scrubbed`]) with the daemon's scrubber for
//!   secrets and by shape and by name for people. An item never quotes a secret.
//! - **The split** ([`half`]): a keyed hash of the item (the key is a META
//!   record made once, never chosen by the writer) puts it in the open half
//!   or the sealed half, so the split is stable and the writer cannot steer an
//!   item. The open half is written to `<state>/self/exams/open/<family>/`
//!   (`shelf.rs`). The sealed half goes through a [`SealedSink`], the
//!   sealed store when `[self.sealed] dir` is set; with none, a sealed item
//!   is held in memory for the pass and dropped with a count, never written
//!   where Theseus can read.
//! - **The review** (`review.rs`): an item counts only after a reviewer on
//!   another model says it is fair ([`Verdict`]): it tests the behaviour and
//!   not the wording, its expected answer is right, and it is not trivially
//!   gamed. A fair one is written and `self.exam.counted` lands; an unfair one
//!   is dropped with its reason (`self.exam.rejected`). A reviewer that cannot
//!   answer leaves the item unwritten and uncounted.
//! - **The proposer rule** ([`Writer`], [`PROTECTED`]): every item records the
//!   `"self"` session whose mistake made it, and a self branch may not add,
//!   change or delete an item: [`Shelf`]'s writes refuse a [`Writer::SelfBranch`],
//!   and [`touches_exams`] names the paths the branch builder must refuse.
//! - **The cap**: `[self] exams_day_max` candidates a local day.
//!
//! The rows: `self.exam.counted`, `self.exam.rejected` (an item's), and one
//! `self.exam.pass` a night. A sealed item's row carries its id only.

pub mod harvest;
mod pass;
mod review;
mod shelf;
mod show;
#[cfg(test)]
mod tests;

use std::fmt;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use pass::{run, Deps, PassReport};
pub use review::{CoreReviewer, REVIEWER_PROFILE};
pub use shelf::Shelf;

/// The item format's version.
pub const VERSION: &str = "self-exam-v1";

/// What the proposer rule protects: under the state dir, the open half and
/// everything beside it; in the tree, the exam crate's data. A self branch's
/// change to a path that [`touches_exams`] is refused.
pub const PROTECTED: [&str; 2] = ["self/exams/", "crates/theseus-exam/exam/"];

/// The families of self-exam items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// The owner's correction: a routing correction, or a correction note on
    /// a remembered fact.
    Correction,
    /// An operator's label that says the judge was wrong (train split).
    WrongLabel,
    /// A task that ended failed or out of budget.
    FailedTask,
    /// A turn that went past its session's spend limit.
    OverBudget,
}

impl Family {
    pub const ALL: [Family; 4] = [
        Family::Correction,
        Family::WrongLabel,
        Family::FailedTask,
        Family::OverBudget,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Family::Correction => "self_correction",
            Family::WrongLabel => "self_wrong_label",
            Family::FailedTask => "self_failed_task",
            Family::OverBudget => "self_over_budget",
        }
    }

    pub fn parse(s: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|f| f.as_str() == s)
    }
}

/// A candidate, as a source made it, before it is an item.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    pub family: Family,
    /// The ledger rows it came from, `ledger:<position>`.
    pub source_rows: Vec<String>,
    /// The `"self"` session or branch whose mistake made it, when one did.
    pub proposer: Option<String>,
    /// The present: what the exam asks.
    pub task: String,
    /// The check, in theseus-exam's check language (one line a clause).
    pub check: String,
    /// Why the item is built as it is.
    pub note: String,
    pub at_ms: u64,
}

/// An item's id: 16 hex characters of a digest of its family, task and check.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(pub String);

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One exam item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub version: String,
    pub id: ItemId,
    pub family: Family,
    pub task: String,
    pub check: String,
    pub note: String,
    /// Where it came from: ledger rows, by position.
    pub provenance: Vec<String>,
    /// The `"self"` session or branch whose mistake made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposer: Option<String>,
    pub at_ms: u64,
}

/// What a scrub does to a text: secrets by the daemon's scrubber, then
/// shapes of people and the names it is given.
pub struct Scrub<'a> {
    pub secrets: &'a (dyn Fn(&str) -> String + Sync),
    pub names: &'a [String],
}

impl Scrub<'_> {
    pub fn text(&self, s: &str) -> String {
        scrub_names((self.secrets)(s).as_str(), self.names)
    }
}

/// People out of a text: an email address, an `@handle`, a `kind:id` handle
/// (`discord:…`, `slack:…`, `person:…`, `name:…`), and each of `names` as a
/// whole word, any case.
pub fn scrub_names(text: &str, names: &[String]) -> String {
    let mut out = Vec::new();
    for tok in text.split_inclusive(char::is_whitespace) {
        let word = tok.trim_end();
        let tail = &tok[word.len()..];
        let core = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != ':');
        let hit = core.contains('@')
            || ["discord:", "slack:", "person:", "name:", "email:"]
                .iter()
                .any(|p| core.starts_with(p));
        if hit && !core.is_empty() {
            out.push(word.replacen(core, "[person]", 1) + tail);
        } else {
            out.push(tok.to_string());
        }
    }
    let mut s = out.concat();
    for n in names
        .iter()
        .map(|n| n.trim())
        .filter(|n| n.chars().count() >= 3)
    {
        s = replace_word(&s, n, "[person]");
    }
    s
}

/// `from` as a whole word, any case, replaced.
fn replace_word(text: &str, from: &str, to: &str) -> String {
    let (lower, f) = (text.to_lowercase(), from.to_lowercase());
    // A lowercase copy of different byte length cannot be mapped back.
    if lower.len() != text.len() {
        return text.to_string();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let (mut out, mut at) = (String::new(), 0);
    while let Some(i) = lower[at..].find(&f) {
        let (s, e) = (at + i, at + i + f.len());
        let (before, after) = (text[..s].chars().next_back(), text[e..].chars().next());
        out.push_str(&text[at..s]);
        out.push_str(if word(before) || word(after) {
            &text[s..e]
        } else {
            to
        });
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

impl Item {
    /// The item a source made, with every text scrubbed and the id taken
    /// from what the item says, so the same mistake is the same item.
    pub fn scrubbed(raw: &Raw, scrub: &Scrub<'_>) -> Item {
        let (task, check) = (scrub.text(&raw.task), scrub.text(&raw.check));
        Item {
            version: VERSION.into(),
            id: id_of(raw.family, &task, &check),
            family: raw.family,
            task,
            check,
            note: scrub.text(&raw.note),
            provenance: raw.source_rows.clone(),
            proposer: raw.proposer.clone(),
            at_ms: raw.at_ms,
        }
    }

    /// What a sealed store is handed: the item whole, as JSON.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
    }
}

/// An item's id.
pub fn id_of(family: Family, task: &str, check: &str) -> ItemId {
    let d = Sha256::digest(format!("{VERSION}\0{}\0{task}\0{check}", family.as_str()));
    ItemId(hex::encode(&d[..8]))
}

/// The two halves of a family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Half {
    Open,
    Sealed,
}

/// The split key: 32 bytes, made once and kept in the store's META record
/// `self.exams.key`. The writer of an item never sees it chosen.
#[derive(Clone, PartialEq, Eq)]
pub struct SplitKey(pub [u8; 32]);

impl fmt::Debug for SplitKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SplitKey(..)")
    }
}

impl SplitKey {
    /// A fresh key from the daemon's id generator and the clock.
    pub fn fresh() -> SplitKey {
        let d = Sha256::digest(format!(
            "{}\0{}",
            crate::new_id("exk"),
            theseus_protocol::now_unix_ms()
        ));
        SplitKey(d.into())
    }

    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<SplitKey> {
        hex::decode(s).ok()?.try_into().ok().map(SplitKey)
    }
}

/// Which half `item` belongs to: the parity of a keyed hash of its family
/// and id. Stable for a key; about half and half over a family.
pub fn half(key: &SplitKey, family: Family, id: &ItemId) -> Half {
    let mut h = Sha256::new();
    h.update(key.0);
    h.update(family.as_str().as_bytes());
    h.update([0]);
    h.update(id.0.as_bytes());
    if h.finalize()[0] & 1 == 0 {
        Half::Open
    } else {
        Half::Sealed
    }
}

/// Where sealed items go: `rsi::sealed::put`'s shape, which the sealed
/// store implements (`pass.rs`). With no sealed store a sealed item is
/// dropped with a count.
pub trait SealedSink: Send + Sync {
    /// Keep `bytes` in `set` under `family`; the id it was kept as.
    fn put(&self, set: &str, family: &str, bytes: &[u8]) -> Result<ItemId>;
}

/// The reviewer's three questions, and its words.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// It tests the behaviour, not the wording.
    pub tests_behaviour: bool,
    /// Its expected answer is right.
    pub answer_right: bool,
    /// It is not trivially gamed.
    pub not_gamed: bool,
    pub reason: String,
    /// The model that reviewed, as it answered.
    pub reviewer: String,
}

impl Verdict {
    /// Fair means all three.
    pub fn fair(&self) -> bool {
        self.tests_behaviour && self.answer_right && self.not_gamed
    }

    /// Why an unfair one is: the questions it failed.
    pub fn why(&self) -> String {
        let mut no = Vec::new();
        for (ok, w) in [
            (
                self.tests_behaviour,
                "it tests the wording, not the behaviour",
            ),
            (self.answer_right, "its expected answer is not right"),
            (self.not_gamed, "it is trivially gamed"),
        ] {
            if !ok {
                no.push(w);
            }
        }
        match (no.is_empty(), self.reason.trim().is_empty()) {
            (true, _) => self.reason.clone(),
            (false, true) => no.join("; "),
            (false, false) => format!("{}: {}", no.join("; "), self.reason.trim()),
        }
    }
}

/// An independent review of one item.
pub trait Reviewer: Send + Sync {
    fn review<'a>(&'a self, item: &'a Item)
        -> futures_util::future::BoxFuture<'a, Result<Verdict>>;
}

/// Who writes to the exam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Writer {
    /// The nightly pass, in the daemon.
    Pass,
    /// A self branch (seam S4): the branch's name.
    SelfBranch(String),
}

impl Writer {
    /// The writer a principal is: `"self"` (seam S1) is a self branch.
    pub fn of_principal(principal: &str, branch: &str) -> Writer {
        if principal == "self" {
            Writer::SelfBranch(branch.to_string())
        } else {
            Writer::Pass
        }
    }

    /// Refuse a self branch's write to the exam.
    pub fn check(&self, what: &str) -> Result<()> {
        match self {
            Writer::Pass => Ok(()),
            Writer::SelfBranch(b) => bail!(
                "the self branch {b} may not {what}: the proposer of a fix never edits the \
                 exam that judges it ({})",
                PROTECTED.join(", ")
            ),
        }
    }
}

/// Whether `path` (relative to the state dir or to the tree) is the exam's.
pub fn touches_exams(path: &str) -> bool {
    let p = path.trim_start_matches("./").replace('\\', "/");
    PROTECTED
        .iter()
        .any(|d| p.starts_with(d) || p.contains(&format!("/{d}")))
}

/// The META key of the split key.
pub const KEY_META: &str = "self.exams.key";
/// The META key of the last pass.
pub const LAST_META: &str = "self.exams.last";
