//! Initiative (theseus-slvl.9): how readily a persona speaks first, and the
//! owner's verdict on what it said unprompted. `Initiative` is the persona's
//! `initiative` field; `Rules::initiative` carries it into `notices::policy`,
//! the one ping rule, which asks `Initiative::lets` what a kind of
//! unprompted message may do at that level. The wire types are
//! `initiative.rate` (a 👍 or 👎 on a proactive message) and
//! `initiative.week` (promises kept and the verdicts, for `theseus persona`).

use serde::{Deserialize, Serialize};

/// How readily a persona speaks first: `quiet` sends only what needs the
/// owner and what the owner asked for, `normal` adds promised follow-ups,
/// finished work and one morning note on days something happened, `chatty`
/// adds observations it is confident are useful.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Initiative {
    Quiet,
    #[default]
    Normal,
    Chatty,
}

impl Initiative {
    pub fn as_str(self) -> &'static str {
        match self {
            Initiative::Quiet => "quiet",
            Initiative::Normal => "normal",
            Initiative::Chatty => "chatty",
        }
    }

    /// Whether `p` may reach the owner unprompted at this level: the one
    /// table the ping rule and the daemon's own proactive posts read.
    pub fn lets(self, p: Proactive) -> bool {
        match p {
            Proactive::NeedsYou | Proactive::Asked => true,
            Proactive::Followup | Proactive::Finished | Proactive::Morning => {
                self != Initiative::Quiet
            }
            Proactive::Observation => self == Initiative::Chatty,
        }
    }
}

/// The kinds of message Theseus may send first, from most to least owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proactive {
    /// A question, a failure, a block: what needs the owner.
    NeedsYou,
    /// What the owner asked for: a reminder, a watch that was met.
    Asked,
    /// A follow-up the model promised and scheduled.
    Followup,
    /// Work that finished, a task's report.
    Finished,
    /// The one note of a day on which something happened.
    Morning,
    /// Something noticed, which the model is confident is useful.
    Observation,
}

/// `initiative.rate`: the owner's 👍 or 👎 on a message Theseus sent first
/// (a wake's or a task's reply, a voiced note). It names the turn that said
/// it, else the session's latest proactive message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InitiativeRateParams {
    /// The session it was said in; absent: any (a note the daemon wrote
    /// itself belongs to none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// The message rated: a turn's id, or a note's own; absent: the latest
    /// proactive one (the session's when it is named).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    /// `up` or `down`.
    pub rating: String,
    /// How it came: `reaction`, `cli`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub via: Option<String>,
    /// Where it came from, which only the binding names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What `initiative.rate` recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InitiativeRateResult {
    /// The proactive message's id (its `initiative.spoke` row's key).
    pub id: String,
    pub turn_id: String,
    pub rating: String,
}

/// `initiative.week`: the last seven days of what Theseus said first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InitiativeWeekParams {
    /// The window's end; absent: now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
}

/// The week's measure: the maintainer's, over a week, is promises kept at 95%
/// or more and the 👍 share.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InitiativeWeekResult {
    pub since_ms: u64,
    pub until_ms: u64,
    /// Replies that promised a follow-up and scheduled one in the same turn,
    /// and those that did not (`promise.kept`, `promise.unkept`).
    pub kept: u64,
    pub unkept: u64,
    /// Kept over both; absent while none was promised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kept_share: Option<f64>,
    /// Messages sent first, and the owner's verdicts on them.
    pub spoke: u64,
    pub up: u64,
    pub down: u64,
    /// 👍 over both verdicts; absent while none was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub up_share: Option<f64>,
    /// What the voiced messages cost, against `[initiative] day_usd`.
    pub voiced: u64,
    pub voiced_usd: f64,
    /// The initiative level of the persona in play.
    pub level: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_lets_what_it_adds_and_nothing_before_it() {
        use Proactive::*;
        let all = [NeedsYou, Asked, Followup, Finished, Morning, Observation];
        let lets = |l: Initiative| all.iter().filter(|p| l.lets(**p)).count();
        assert_eq!(lets(Initiative::Quiet), 2);
        assert_eq!(lets(Initiative::Normal), 5);
        assert_eq!(lets(Initiative::Chatty), 6);
        for p in [NeedsYou, Asked] {
            assert!(Initiative::Quiet.lets(p), "quiet still sends {p:?}");
        }
        assert!(!Initiative::Normal.lets(Observation));
        assert_eq!(Initiative::default(), Initiative::Normal);
    }
}
