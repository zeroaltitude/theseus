//! Clearance for a request's data (theseus-a1wvq.3): a request to a model of
//! restricted origin (`cn` or `unknown`) goes out only with a [`DataPass`]
//! minted for that one compiled request, covering its digest. The names here
//! are shared with the rows that build open requests and judge them; keep them.
//!
//! Nothing mints a pass yet: every turn's clearance is
//! `Clearance::closed(DataWhy::Unchecked)`, so the floor (`data_gate.rs`,
//! `turn/data_step.rs`) keeps every request off a restricted model.

use serde::{Deserialize, Serialize};

/// Leave to send one compiled request to a model of restricted origin. It
/// holds that request's digest and covers nothing else: minted, never copied
/// into another request (no `Clone`, no `Default`).
#[derive(Debug)]
pub struct DataPass {
    digest: String,
}

impl DataPass {
    /// Whether this pass was minted for the request whose digest is `digest`.
    pub fn covers(&self, digest: &str) -> bool {
        self.digest == digest
    }

    /// A pass for a test's request, by its digest.
    #[cfg(test)]
    pub(crate) fn for_test(digest: &str) -> Self {
        Self {
            digest: digest.into(),
        }
    }
}

/// Why a request is not cleared for a restricted model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataWhy {
    /// Nothing judged the request (every turn, until the open-request row).
    Unchecked,
    /// The request carries the owner's private view.
    PrivateView,
    /// A node of the request is outside the open run.
    NotInRun,
    /// A node no blessed writer vetted.
    Unvetted,
    /// A sealed thread's material.
    Sealed,
    /// An attachment.
    Attachment,
    /// Jev judged the message private.
    VerdictPrivate,
    /// Jev judged the message unclear.
    VerdictUnclear,
    /// Jev's answer is under the act band.
    BelowAct,
    /// Jev gave no verdict.
    NoVerdict,
    /// Jev's verdict came after the wait.
    Late,
    /// The pack that judges data does not act live.
    PackNotLive,
    /// A tool result that did not pass the screen.
    ToolResult,
}

impl DataWhy {
    pub fn as_str(self) -> &'static str {
        match self {
            DataWhy::Unchecked => "unchecked",
            DataWhy::PrivateView => "private_view",
            DataWhy::NotInRun => "not_in_run",
            DataWhy::Unvetted => "unvetted",
            DataWhy::Sealed => "sealed",
            DataWhy::Attachment => "attachment",
            DataWhy::VerdictPrivate => "verdict_private",
            DataWhy::VerdictUnclear => "verdict_unclear",
            DataWhy::BelowAct => "below_act",
            DataWhy::NoVerdict => "no_verdict",
            DataWhy::Late => "late",
            DataWhy::PackNotLive => "pack_not_live",
            DataWhy::ToolResult => "tool_result",
        }
    }
}

/// Whether a turn's requests may go to a restricted model. Opaque: only this
/// module builds a cleared one (the open-request row adds the checked
/// constructor), so anything else holds a closed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearance {
    closed: Option<DataWhy>,
}

impl Clearance {
    /// Not cleared, and why.
    pub fn closed(why: DataWhy) -> Self {
        Self { closed: Some(why) }
    }

    pub fn is_cleared(&self) -> bool {
        self.closed.is_none()
    }

    pub fn why(&self) -> Option<DataWhy> {
        self.closed
    }

    /// A pass for the request whose digest is `digest`, when cleared.
    pub fn pass(&self, digest: &str) -> Option<DataPass> {
        self.is_cleared().then(|| DataPass {
            digest: digest.into(),
        })
    }
}

/// A turn starts closed: unchecked, so fail closed.
impl Default for Clearance {
    fn default() -> Self {
        Self::closed(DataWhy::Unchecked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_clearance_mints_no_pass() {
        let c = Clearance::default();
        assert!(!c.is_cleared());
        assert_eq!(c.why(), Some(DataWhy::Unchecked));
        assert!(c.pass("abc").is_none());
        assert_eq!(
            serde_json::to_string(&DataWhy::VerdictUnclear).unwrap(),
            "\"verdict_unclear\""
        );
        assert_eq!(DataWhy::PackNotLive.as_str(), "pack_not_live");
    }

    #[test]
    fn a_pass_covers_its_own_digest_alone() {
        let p = DataPass::for_test("abc");
        assert!(p.covers("abc"));
        assert!(!p.covers("abd"));
    }
}
