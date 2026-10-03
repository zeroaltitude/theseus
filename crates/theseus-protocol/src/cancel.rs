//! A cancel's verdict on the wire (M4 18a; design §2.3, §2.11): how each
//! call a cancel or a stop ended is known to have stopped, as
//! `execution.cancel` and `execution.stop` answer it, and what health counts
//! of each backend's cancels since the daemon started.

use serde::{Deserialize, Serialize};

/// How one call a cancel or a stop reached ended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CancelVerdict {
    pub correlation_id: String,
    pub tool: String,
    /// The cancel's last state: `termination_verified`, `outcome_uncertain`,
    /// or `unsupported`.
    pub state: String,
    /// How it is known: `pidns` (an L1 job's pid namespace), `cgroup`, `tree`
    /// (an L0 job's process tree, its wrapper's descendants), `group` (a
    /// wrapper from before 18a: its process group), `task` (an async tool's),
    /// or `none` (a call that cannot be stopped). For one not verified, what
    /// was tried.
    pub verified_by: String,
    /// The processes the stop ended (a job's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub killed: Option<u32>,
    /// The processes still alive after it: 0 when verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub survivors: Option<u32>,
    /// What the stop could see: `descendants` at L0, `namespace`, `cgroup`,
    /// `group`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub scope: Option<String>,
    /// From the stop's signal to its verdict.
    #[serde(default)]
    pub ms: u64,
    /// Why it is not verified, when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
}

impl CancelVerdict {
    /// What every surface shows: "verified: pid namespace, 4 processes", or
    /// "not verified: its wrapper did not answer within 4500 ms".
    pub fn words(&self) -> String {
        words(
            self.state == "termination_verified",
            &self.verified_by,
            self.killed,
            self.survivors,
            self.why.as_deref(),
        )
    }
}

/// A verdict in words, the one wording every surface shares.
pub fn words(
    verified: bool,
    verified_by: &str,
    killed: Option<u32>,
    survivors: Option<u32>,
    why: Option<&str>,
) -> String {
    if !verified {
        return match (why, survivors) {
            (Some(why), _) => format!("not verified: {why}"),
            (None, Some(n)) if n > 0 => format!("not verified: {n} left"),
            _ => "not verified".into(),
        };
    }
    let by = match verified_by {
        "pidns" => "pid namespace",
        "tree" => "process tree",
        "group" => "process group",
        other => other,
    };
    match killed {
        Some(n) => format!(
            "verified: {by}, {n} {}",
            if n == 1 { "process" } else { "processes" }
        ),
        None => format!("verified: {by}"),
    }
}

/// The short form a tool line adds to W1's `⏹️ stopped by …`: " (verified)",
/// or " (not verified: …)", from `tool.ended`'s `verified`; nothing without
/// one.
pub fn note(verified: Option<&str>) -> String {
    match verified {
        Some(w) if w.starts_with("verified") => " (verified)".into(),
        Some(w) => format!(" ({w})"),
        None => String::new(),
    }
}

/// Health's count of one backend's cancels in one state, since the daemon
/// started (`theseus.cancel{backend,state}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CancelCount {
    /// `l0`, `l1`, `async`, or `inproc`; `job` for a job whose class its
    /// verdict cannot tell.
    pub backend: String,
    /// `verified`, `uncertain`, or `unsupported`.
    pub state: String,
    pub n: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_reads_the_same_on_every_surface() {
        let mut v = CancelVerdict {
            state: "termination_verified".into(),
            verified_by: "pidns".into(),
            killed: Some(4),
            survivors: Some(0),
            ..Default::default()
        };
        assert_eq!(v.words(), "verified: pid namespace, 4 processes");
        v.verified_by = "task".into();
        v.killed = None;
        assert_eq!(v.words(), "verified: task");
        v.verified_by = "tree".into();
        v.killed = Some(1);
        assert_eq!(v.words(), "verified: process tree, 1 process");
        v.state = "outcome_uncertain".into();
        v.why = Some("1 process outlived the kill: pid 42 (state D)".into());
        assert_eq!(
            v.words(),
            "not verified: 1 process outlived the kill: pid 42 (state D)"
        );
        // A tool line's short form.
        assert_eq!(note(Some("verified: task")), " (verified)");
        assert_eq!(
            note(Some(&v.words())),
            " (not verified: 1 process outlived the kill: pid 42 (state D))"
        );
        assert_eq!(note(None), "");
    }
}
