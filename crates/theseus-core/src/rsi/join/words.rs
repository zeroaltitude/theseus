//! What the loop says of its last join (issue 20): joined at a commit, and whether that commit is
//! what runs. `theseus self pipeline`, `theseus self log` and `health` read it, so none of them
//! can call a build done that is not what the service runs.

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
            ],
        )
        .ok()??;
        last_join(&rows)
    }

    /// The commit of a join that is not what the service runs, for `health`: the last join's,
    /// when the loop could not install it or it came back on another build.
    pub fn self_joined_not_running(&self) -> Option<String> {
        self.self_last_join()
            .filter(|j| matches!(j.install, Install::NotInstalled | Install::Mismatch))
            .map(|j| j.merge)
    }
}
