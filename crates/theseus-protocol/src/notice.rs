//! Health's waiting split (theseus-g00vv).

use serde::{Deserialize, Serialize};

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
