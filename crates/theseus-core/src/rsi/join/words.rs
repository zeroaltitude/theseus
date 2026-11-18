//! What the loop says of its last join (issue 20): joined at a commit, and whether that commit is
//! what runs. `theseus self pipeline`, `theseus self log` and `health` read it, so none of them
//! can call a build done that is not what the service runs.

use super::root_install::hand_words;
use super::rows_of;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// Where the install of a joined build stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// The service runs it (`self.installed`).
    Running,
    /// The loop could not install it, and told the owner the step.
    NotInstalled,
    /// It was installed and the service came back on another build.
    Mismatch,
    /// Theseus's own install tried and did not take, or was refused, or was not asked for: the install's last row says
    /// why (`self.root_install`).
    DidNotTake,
    /// No row says: an install in flight, or installs are off.
    Unconfirmed,
    /// The join was undone on main (`self.reverted`, or the owner's `self.vetoed`): it is never said to be running.
    Reverted,
}

/// Whether the build that runs now contains a joined commit: the answer the daemon can give from its own clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contained {
    /// The running build's commit is the join or has it as an ancestor.
    Yes(String),
    /// Both commits are in the clone and the running build's does not have the join in its history.
    No(String),
    /// The clone cannot say, and why.
    CannotTell(String),
}

/// The last join, and what the loop says of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastJoin {
    pub merge: String,
    pub install: Install,
    /// The sentence: `joined at 1a2b3c4d5e6f; installed and running`.
    pub line: String,
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}

/// The steps of an install of its own (`self.root_install`) that end it without the build running.
const ENDED_WITHOUT: &[&str] = &[
    "failed",
    "rolled_back",
    "unhealthy",
    "refused",
    "no_helper",
    "declined",
];

/// The words for an install whose last row ends it without the build running: that it is not what runs, since when, why
/// (the row's own), and the one command that installs it by hand. None for a row of an install still under way.
fn did_not_take(row: &LedgerRow) -> Option<(Install, String)> {
    let step = row.data["step"].as_str()?;
    if !ENDED_WITHOUT.contains(&step) {
        return None;
    }
    let text = |k: &str| row.data[k].as_str().filter(|s| !s.is_empty());
    let by_hand = hand_words(row.data["numbers"]["step"].as_str());
    let when = utc(row.at_unix_ms);
    // An install that was tried and did not take says why (the row's own); one that was never made (no card, no
    // helper, a start command that is not Theseus's to change, a no from the owner) says what the row says.
    let said = if ["failed", "rolled_back", "unhealthy"].contains(&step) {
        let why = text("why").unwrap_or("no reason was recorded");
        format!(
            "the service is not running it: the install of {when} did not take ({why}). {by_hand}"
        )
    } else {
        let what = text("what")
            .or_else(|| text("why"))
            .unwrap_or("no reason was recorded");
        format!("the service is not running it: it was not installed ({when}): {what}. {by_hand}")
    };
    Some((Install::DidNotTake, said))
}

/// A moment in UTC, `2026-11-18 07:36 UTC`: for a reader with no time zone of its own.
fn utc(ms: u64) -> String {
    format!(
        "{} {} UTC",
        crate::recall::render::utc_day(ms),
        theseus_protocol::utc_hm(ms)
    )
}

/// The words for a join that was undone on main. A revert is a later commit that has the join as an ancestor, so a build
/// that contains the join may well contain its revert: the join is never said to run, and where the running build
/// contains the revert, the revert is what runs. A revert made by hand on main, with no row, is not seen here.
fn reverted(
    merge: &str,
    undo: &LedgerRow,
    contained: &dyn Fn(&str) -> Option<Contained>,
) -> LastJoin {
    let by = if undo.kind == "self.vetoed" {
        "the owner's revert"
    } else {
        "the loop's own revert after a red signal"
    };
    let said = match undo.data["revert"].as_str() {
        None => format!("reverted: {by} undid it, so it is not what runs"),
        Some(revert) => {
            let r = short(revert);
            match contained(revert) {
                Some(Contained::Yes(running)) => format!(
                    "reverted: {by} ({r}) undid it, and the running build ({}) contains that revert, so it is not what runs",
                    short(&running)
                ),
                Some(Contained::No(running)) => format!(
                    "reverted on main by {by} ({r}); the running build ({}) does not contain that revert yet, so it may still run the change",
                    short(&running)
                ),
                Some(Contained::CannotTell(why)) => format!(
                    "reverted on main by {by} ({r}); cannot tell whether the running build contains that revert ({why})"
                ),
                None => format!("reverted on main by {by} ({r})"),
            }
        }
    };
    LastJoin {
        line: format!("joined at {}; {said}", short(merge)),
        merge: merge.to_string(),
        install: Install::Reverted,
    }
}

/// The newest `self.joined` of `rows` (oldest first), with the install rows of its merge.
#[cfg(test)]
pub(crate) fn last_join(rows: &[LedgerRow]) -> Option<LastJoin> {
    last_join_in(rows, &|_| None)
}

/// [`last_join`], with `contained` asked (once, for the join) when no row says the join's own install is what runs: a
/// build that came later, by hand or by another install, may contain the join, and then it runs.
pub(crate) fn last_join_in(
    rows: &[LedgerRow],
    contained: &dyn Fn(&str) -> Option<Contained>,
) -> Option<LastJoin> {
    let merge = rows.iter().rev().find(|r| r.kind == "self.joined")?.data["merge"]
        .as_str()?
        .to_string();
    let of = |kind: &str| {
        rows.iter()
            .rev()
            .find(|r| r.kind == kind && r.data["merge"] == merge.as_str())
    };
    let at = short(&merge);
    if let Some(undo) = ["self.reverted", "self.vetoed"].iter().find_map(|k| of(k)) {
        return Some(reverted(&merge, undo, contained));
    }
    let (install, said) = if of("self.installed").is_some() {
        (Install::Running, "installed and running".to_string())
    } else if let Some(m) = of("self.install.mismatch") {
        let now = m.data["running"]
            .as_str()
            .map_or("a build that names no commit", short);
        (
            Install::Mismatch,
            format!("installed, but the service came back running {now}, so it is not what runs"),
        )
    } else if let Some(n) = of("self.install.not_installed") {
        let why = n.data["why"]
            .as_str()
            .unwrap_or("this machine's service runs a build the loop cannot replace");
        let step = n.data["step"].as_str().unwrap_or("");
        (
            Install::NotInstalled,
            format!("joined but not installed: {why}. To install it: {step}"),
        )
    } else if let Some((install, said)) = of("self.root_install").and_then(did_not_take) {
        (install, said)
    } else {
        (Install::Unconfirmed, "not confirmed running".to_string())
    };
    // A row that says the join's own install runs is the answer: nothing is asked of the clone.
    let asked = (install != Install::Running)
        .then(|| contained(&merge))
        .flatten();
    let (install, said) = match (install, asked) {
        (Install::Running, _) | (_, None) => (install, said),
        (_, Some(Contained::Yes(running))) => (
            Install::Running,
            if running == merge {
                "installed and running".to_string()
            } else {
                format!(
                    "installed and running (inside the running build, {}, which contains it)",
                    short(&running)
                )
            },
        ),
        (Install::Unconfirmed, Some(Contained::No(running))) => (
            install,
            format!(
                "not running yet: the running build ({}) does not contain it, and no install of it has finished. {}",
                short(&running),
                hand_words(None)
            ),
        ),
        (Install::Unconfirmed, Some(Contained::CannotTell(why))) => (
            install,
            format!("not confirmed running: cannot tell whether the running build contains it ({why})"),
        ),
        // An install row already says why it is not what runs, and the one command.
        (_, Some(_)) => (install, said),
    };
    Some(LastJoin {
        line: format!("joined at {at}; {said}"),
        merge,
        install,
    })
}

impl Core {
    /// The last join and its install, from the ledger; none before any join, with the mode off,
    /// or while the index is built.
    pub fn self_last_join(&self) -> Option<LastJoin> {
        self.self_last_join_as(crate::build().commit.as_deref())
    }

    /// [`Self::self_last_join`] for a daemon whose build is `running`.
    pub(crate) fn self_last_join_as(&self, running: Option<&str>) -> Option<LastJoin> {
        if self.cfg.self_improve.mode == theseus_protocol::rsi::SelfMode::Off {
            return None;
        }
        let rows = rows_of(
            &self.store,
            &[
                "self.joined",
                "self.installed",
                "self.install.not_installed",
                "self.install.mismatch",
                "self.root_install",
                "self.reverted",
                "self.vetoed",
            ],
        )
        .ok()??;
        last_join_in(&rows, &|merge| Some(self.running_contains(merge, running)))
    }

    /// The commit of a join that is not what the service runs, for `health`: the last join's,
    /// when the loop could not install it or it came back on another build.
    pub fn self_joined_not_running(&self) -> Option<String> {
        self.self_last_join()
            .filter(|j| {
                matches!(
                    j.install,
                    Install::NotInstalled | Install::Mismatch | Install::DidNotTake
                )
            })
            .map(|j| j.merge)
    }
}
