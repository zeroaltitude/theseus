//! A runtime tightening (theseus-sgh): what `policy.tighten` and `policy.untighten` take and answer, and the record a
//! tightening leaves.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// A runtime tightening (theseus-sgh, spec §3.9): one press on a notice made
/// a tool ask first from then on. The gate applies it after the config's
/// posture, and the stricter of the two wins, so it never loosens anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Tightening {
    /// The tool's canonical name (`proc.run`).
    pub tool: String,
    /// What the tool asks at least: `approve`.
    pub posture: String,
    /// Who pressed, as a label (`discord:zeroaltitude`, `sock#3`, `web#1`).
    pub by: String,
    /// Who pressed, as the approval rule knows them: a Discord user by id,
    /// anyone else by label.
    #[serde(default)]
    pub who: String,
    /// The channel it came through, as `[approval].channels` names it
    /// (`cli`, `web`, `discord:dm`, `discord:<channel id>`).
    #[serde(default)]
    pub via: String,
    pub at_ms: u64,
    /// The call whose notice was pressed. With `digest` and `tool`, it is a
    /// labeled example for later judgment work (Jev, M5). A press from the
    /// CLI without `--call` names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// The call's proposal digest (its action's `args_digest`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
}

/// `policy.tighten`: make a tool ask first from now on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyTightenParams {
    pub tool: String,
    /// The call whose notice was pressed, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    /// Who pressed, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user the press came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `policy.untighten`: the tool goes back to what the config says.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyUntightenParams {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What a tighten or an undo did, and the tool's posture now. Also the
/// params of `policy.tightened` and `policy.untightened`, where a field a
/// notification lacks reads as its default (theseus-0g4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TightenResult {
    pub tool: String,
    /// Who made this change: the one who pressed, or the one who undid it.
    pub by: String,
    /// The tightening recorded (tighten) or removed (untighten).
    pub tightening: Tightening,
    /// The posture the gate applies to the tool now, and the setting that
    /// chose it.
    pub posture: String,
    pub setting: String,
    /// What the config alone says.
    pub config_posture: String,
    pub config_setting: String,
    /// The tool's posture changed. False for a press under a config that
    /// already asks, and for an undo the config still asks under.
    pub changed: bool,
    /// The tool was tightened already, so nothing was recorded.
    #[serde(default)]
    pub already: bool,
}
