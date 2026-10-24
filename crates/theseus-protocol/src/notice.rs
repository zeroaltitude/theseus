//! `policy.notice_seen` (theseus-g00vv): the owner has seen a hold's notice.
//! Under `[policy] external_text = "notify"` a hold tells once, at its first
//! call that acts; this marks it told without a call, so the cockpit's
//! "Dismiss" can clear the item. The hold, its ledger row and the session
//! all stay; a later hold has a new `since_ms` and tells again.

use serde::{Deserialize, Serialize};

use crate::{DiscordOrigin, ExternalText};

/// `policy.notice_seen`: the session whose hold's notice the owner has seen.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyNoticeSeenParams {
    pub session_id: String,
    /// Who saw it, as a label. Default: the connection. It names and proves
    /// nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `policy.notice_seen` marked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NoticeSeenResult {
    pub session_id: String,
    /// The hold now marked told (its `since_ms` is what the mark holds).
    pub held: ExternalText,
    /// False when the hold had told the owner already: nothing was written.
    pub wrote: bool,
}

/// Health's waiting executions, split by what each waits on
/// (theseus-g00vv). They add up to `executions_by_state["waiting"]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WaitingSplit {
    /// Live conversations at rest on the owner's input.
    pub at_rest: u64,
    /// Retired sessions' executions on input (kept, never a leak).
    pub retired: u64,
    /// On work: an action, a question, a budget reset or a child task.
    pub work: u64,
    /// On a due time.
    pub time: u64,
}

impl WaitingSplit {
    /// Nothing counted: a daemon before the split, or none waiting.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The line's tail: `180 at rest on input, 9 retired, 0 on work, 0 on a time`.
    pub fn words(&self) -> String {
        format!(
            "{} at rest on input, {} retired, {} on work, {} on a time",
            self.at_rest, self.retired, self.work, self.time
        )
    }
}
