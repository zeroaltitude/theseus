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
    let why = text("why")
        .or_else(|| text("what"))
        .unwrap_or("no reason was recorded");
    let by_hand = hand_words(row.data["numbers"]["step"].as_str());
    Some((
        Install::DidNotTake,
        format!(
            "the service is not running it: the install of {when} did not take ({why}). {by_hand}",
            when = utc(row.at_unix_ms)
        ),
    ))
}

/// A moment in UTC, `2026-11-18 07:36 UTC`: for a reader with no time zone of its own.
fn utc(ms: u64) -> String {
    format!(
        "{} {} UTC",
        crate::recall::render::utc_day(ms),
        theseus_protocol::utc_hm(ms)
    )
}

/// The newest `self.joined` of `rows` (oldest first), with the install rows of its merge.
pub(crate) fn last_join(rows: &[LedgerRow]) -> Option<LastJoin> {
    let merge = rows.iter().rev().find(|r| r.kind == "self.joined")?.data["merge"]
        .as_str()?
        .to_string();
    let of = |kind: &str| {
        rows.iter()
            .rev()
            .find(|r| r.kind == kind && r.data["merge"] == merge.as_str())
    };
    let at = short(&merge);
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
            ],
        )
        .ok()??;
        last_join(&rows)
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
