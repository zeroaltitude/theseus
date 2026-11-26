//! The repair in place (issue 36; the owner, 2026-11-20: make the reviewer a reviewer and fixer).
//! Design note: `docs/design/self-fix.md`.
//!
//! A small fault of a self branch is given ONCE at each of two points to a **fixer**, a turn on
//! the reviewer's profile (resolved by the review's own rule, [`super::reviewing::reviewer_for`],
//! and refused as the review is when it is the builder's model):
//!
//! - **at the gate** (`rsi::pr`): a gate red in the change's own work ([`own_work`]: formatting,
//!   lint, a build error, a failing test), never a keel stop, a red main, a machine fault, a check
//!   that kept aborting, or a gate that named no phase;
//! - **at review** ([`at_review`]): the reviewer answered `VERDICT: fix`.
//!
//! The fixer works in the item's own tree with exactly a builder's confinement (its session's
//! root is the item's, `own_tree`), and ends `FIX: done` or `FIX: none`. The CODE makes its one
//! commit ([`make`]), the bounds judge that commit ([`bounds`]), the gate runs again, and only
//! then is the commit read alone by the **builder's** profile, which must accept it ([`turns`]):
//! nobody approves their own work. At review, the gate and the proof step then run again and a
//! fresh independent review reads the whole. Anything else is today's path, with the reason
//! saying a fix was tried; a fix uses none of the item's tries. `[self_fix] mode = "off"` runs
//! none of this.
//!
//! Every step is one `self.fix.*` row ([`crate::fact::rsi_fix`]), which the pipeline's view reads
//! to name a branch being fixed, and both model calls are `"self"`-principal turns, so the turn's
//! own self-budget step reserves and settles each against the day's self spend.

pub(crate) mod at_review;
pub(crate) mod bounds;
pub(crate) mod make;
pub(crate) mod regions;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_outgoing;
#[cfg(test)]
mod tests_outgoing_leave;
#[cfg(test)]
mod tests_review;
pub(crate) mod turns;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::rpc::Core;
use crate::turn::Target;

/// Where a fix is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    /// After a red gate of the build.
    Gate,
    /// After a review that answered `fix`.
    Review,
}

impl Point {
    /// As the rows name it.
    pub fn name(self) -> &'static str {
        match self {
            Point::Gate => "gate",
            Point::Review => "review",
        }
    }

    /// As a person reads it.
    pub fn words(self) -> &'static str {
        match self {
            Point::Gate => "at the gate",
            Point::Review => "at review",
        }
    }
}

/// The gate's phases whose failure is the change's own work: `scripts/gate.sh`'s own names. A
/// phase not listed (the door and the keel, the scrub, the disk floor, the benches, the cockpit's
/// modules, the dependency check) is never given to a fixer. Nor is "protocol types": that phase
/// writes its own repair into the tree (the regenerated `cockpit/src/protocol.gen`) and fails
/// with "git add it", and the builder's next call commits it, as on main; a fix that did not
/// hold would put the tree back and throw the gate's repair away (review round 3, R3-1). No phase
/// listed writes into the tree.
pub const OWN_WORK: &[&str] = &[
    "fmt",
    "clippy",
    "clippy-probe",
    "shape",
    "test build",
    "suite",
    "reader rule",
];

/// What the fixer is given.
#[derive(Debug, Clone)]
pub struct FixAsk {
    pub item: String,
    pub branch: String,
    /// The head it starts from.
    pub head: String,
    pub point: Point,
    /// The tree it writes in: the item's own.
    pub tree: String,
    /// The tree's slug, which names the item's tree and build folder (`own_tree`).
    pub slug: String,
    /// The branch's diff against where it left main.
    pub diff: String,
    /// What failed: the gate's last lines, or the review's reasons for the fix.
    pub failure: String,
    pub evidence: String,
    /// The reviewer's target.
    pub target: Target,
    /// The bounds it is told.
    pub lines: u64,
    pub files: u64,
}

/// What the fixer answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixed {
    /// `FIX: done`.
    pub done: bool,
    /// Its one sentence of why.
    pub why: String,
    pub cost_usd: f64,
    pub session: Option<String>,
    /// The profile and the model that answered, from the turn's own result.
    pub profile: String,
    pub model: String,
}

/// What the builder's profile is given to approve a fix: the fix's commit alone.
#[derive(Debug, Clone)]
pub struct ApproveAsk {
    pub item: String,
    pub branch: String,
    /// The fix's commit.
    pub head: String,
    pub point: Point,
    /// The commit's own diff, and nothing else of the branch.
    pub diff: String,
    /// The fixer's reason.
    pub reason: String,
    /// The fault it answered.
    pub failure: String,
    /// Who made it: `fable (claude-fable-5-2)`.
    pub fixer: String,
    /// The builder's profile.
    pub target: Target,
}

pub type FixFut<'a> = Pin<Box<dyn Future<Output = Result<Fixed>> + Send + 'a>>;

/// Who fixes and who approves a fix: the core's own turns ([`turns::Turns`]), or a test's
/// stand-in.
pub trait Fixer: Send + Sync {
    /// Make the fix in `ask.tree`, and say whether it did.
    fn fix<'a>(&'a self, core: &'a Arc<Core>, ask: &'a FixAsk) -> FixFut<'a>;
    /// Read the fix's commit alone and accept or reject it.
    fn approve<'a>(&'a self, core: &'a Arc<Core>, ask: &'a ApproveAsk) -> super::review::Fut<'a>;
}

/// The gate's red as the fixer sees it: the phase it failed in when that is the change's own
/// work, or why it is not given to a fixer. `run` is a red gate's run.
pub fn own_work(run: &crate::rsi::tree::GateRun) -> Result<String, String> {
    use crate::rsi::infra::{of_attempt, Outcome};
    use crate::rsi::tree::Verdict;
    if run.verdict == Verdict::Keel {
        return Err("a keel stop is the owner's".into());
    }
    if run.aborted() {
        return Err("its check kept aborting, which is no verdict on the code".into());
    }
    if of_attempt(run.verdict.name(), Some(run)) == Some(Outcome::Infrastructure) {
        return Err("the gate did not judge the code: a fault of the machine".into());
    }
    let Some(phase) = crate::rsi::pr::failed_phase(&run.tail) else {
        return Err("the gate named no phase it failed in".into());
    };
    match OWN_WORK.contains(&phase.as_str()) {
        true => Ok(phase),
        false => Err(format!(
            "the gate failed in {phase}, which is not the change's own work"
        )),
    }
}

/// The fixer's answer: its last `FIX:` line, `done` or `none`, and its sentence of why (the rest
/// of that line, else the next line). No such line is a `none`.
pub fn fix_of(reply: &str) -> (bool, String) {
    let lines: Vec<&str> = reply.lines().collect();
    let Some(at) = lines.iter().rposition(|l| {
        l.trim()
            .trim_matches('`')
            .to_ascii_lowercase()
            .starts_with("fix:")
    }) else {
        return (false, "the fixer gave no FIX line".into());
    };
    let rest = lines[at]
        .trim()
        .trim_matches('`')
        .split_once(':')
        .map(|(_, w)| w.trim().to_string())
        .unwrap_or_default();
    let low = rest.to_ascii_lowercase();
    let done = low.starts_with("done");
    let word = if done { "done" } else { "none" };
    let mut why = rest
        .get(word.len().min(rest.len())..)
        .unwrap_or_default()
        .trim_start_matches([' ', '.', ',', ':', '-', '—'])
        .trim()
        .to_string();
    if why.is_empty() {
        why = lines[at + 1..]
            .iter()
            .map(|l| l.trim().trim_start_matches(['-', '*', ' ']))
            .find(|l| !l.is_empty())
            .unwrap_or("it gave no reason")
            .to_string();
    }
    if !done && !low.starts_with("none") {
        why = format!("its FIX line said neither done nor none: {rest}");
    }
    (done, crate::scrub::safe::head(&why, 500).0.to_string())
}

/// Whether a `self.fix.verdict` row is an accepted fix made at review: the branch's new head.
pub(crate) fn accepted_at_review(d: &Value) -> bool {
    d["point"] == "review" && d["verdict"] == "accept" && d["head"].is_string()
}

/// What a fresh review is told of an accepted fix, from its `self.fix.verdict` row.
pub(crate) fn told(d: &Value) -> String {
    let head = d["head"].as_str().unwrap_or_default();
    format!(
        "commit {} by {}, approved by {}, which repaired: {}",
        &head[..head.len().min(12)],
        d["fixer"].as_str().unwrap_or("another model"),
        d["approver"].as_str().unwrap_or("the builder's model"),
        d["failure"].as_str().unwrap_or("what the review asked")
    )
}

/// The `self.fix.*` rows of one branch at one point, oldest first; none while the index is built.
pub(crate) fn rows_of(
    store: &crate::store::Store,
    branch: &str,
    point: Point,
) -> Result<Option<Vec<crate::ledger::LedgerRow>>> {
    let Some(rows) = super::rows_of(store, &KINDS)? else {
        return Ok(None);
    };
    Ok(Some(
        rows.into_iter()
            .filter(|r| r.data["branch"] == branch && r.data["point"] == point.name())
            .collect(),
    ))
}

/// The `self.fix.*` rows of one item, every branch of it, at one point, oldest first; none while
/// the index is built.
pub(crate) fn item_rows(
    store: &crate::store::Store,
    item: &str,
    point: Point,
) -> Result<Option<Vec<crate::ledger::LedgerRow>>> {
    let Some(rows) = super::rows_of(store, &KINDS)? else {
        return Ok(None);
    };
    Ok(Some(
        rows.into_iter()
            .filter(|r| r.data["item"] == item && r.data["point"] == point.name())
            .collect(),
    ))
}

/// The kinds a fix writes.
pub const KINDS: [&str; 5] = [
    "self.fix.started",
    "self.fix.made",
    "self.fix.refused",
    "self.fix.none",
    "self.fix.verdict",
];

/// Whether a fix's row ends it: anything but its start and its commit.
pub(crate) fn ends(kind: &str) -> bool {
    matches!(
        kind,
        "self.fix.refused" | "self.fix.none" | "self.fix.verdict"
    )
}

/// `text` (a model's words, for a commit's body) with every line that reads as a fix commit's
/// [`TRAILER`], in any case, marked as quoted: only the code writes that trailer, so a builder's
/// `why` that carries it does not make its own commit read as a fixer's (review round 2, note 2).
pub fn neutralised(text: &str) -> String {
    let key = TRAILER.to_ascii_lowercase();
    text.lines()
        .map(|l| match l.trim().to_ascii_lowercase().starts_with(&key) {
            true => format!("(quoted) {}", l.trim()),
            false => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The trailer every fix commit carries, with its point: how [`unapproved`] knows one by itself.
pub const TRAILER: &str = "Self-Fix-Point:";

/// The one rule every green head is held to (issue 36, round 2), whatever path led to it: a
/// fixer's commit in `range` (read in `dir`) is there only if the builder's model accepted THAT
/// commit (`self.fix.verdict` accept with its head). A fixer's commit is one that carries
/// [`TRAILER`] or that a `self.fix.made` row names. Why not, or none when the range holds no
/// unapproved fix.
pub(crate) fn unapproved(
    store: &crate::store::Store,
    place: &crate::rsi::tree::Place,
    dir: &std::path::Path,
    range: &str,
) -> Result<Option<String>> {
    // While the ledger's index is still built, no fix can be shown approved: a commit that carries
    // the trailer (every fixer's commit does) is then refused, and a branch with none passes.
    let rows = super::rows_of(store, &["self.fix.made", "self.fix.verdict"])?.unwrap_or_default();
    let made: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == "self.fix.made")
        .filter_map(|r| r.data["head"].as_str())
        .collect();
    let approved: Vec<&str> = rows
        .iter()
        .filter(|r| r.kind == "self.fix.verdict" && r.data["verdict"] == "accept")
        .filter_map(|r| r.data["head"].as_str())
        .collect();
    let log = place.git_ok(dir, &["log", "--format=%H%x1f%B%x1e", range])?;
    for entry in log.split('\x1e') {
        let Some((hash, body)) = entry.trim().split_once('\x1f') else {
            continue;
        };
        let fix = made.contains(&hash) || body.lines().any(|l| l.trim().starts_with(TRAILER));
        if fix && !approved.contains(&hash) {
            return Ok(Some(format!(
                "the branch holds commit {}, a fixer's, which no other model approved: a fixer's \
                 commit goes green only once the model that built the branch accepts it",
                &hash[..hash.len().min(12)]
            )));
        }
    }
    Ok(None)
}
