//! Content flags on the wire (theseus-9ymz0): the labels a session carries, as
//! `session.list` and `session.get` show them, and `content.unlabel`, the
//! operator's removal of one. The rule itself is the core's (`content.rs`);
//! these are the shapes a client reads.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// One label on a session, and what raised it. The words are the core's:
/// `flag` is `injection`, `secret`, `confidential` or `personal`; `kind` is the
/// item's (`operator_message`, `tool_result`, `fetched_page`, `search_result`,
/// `task_brief`, `task_report`); `band` is `act` or `confirm`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabelView {
    pub flag: String,
    /// The node whose content raised it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub node: Option<String>,
    pub kind: String,
    /// The tool and the resource or address it named, or the place a message
    /// arrived in.
    pub source: String,
    pub confidence: f64,
    pub band: String,
    /// When it was raised.
    pub at_ms: u64,
    /// The judgment that raised it, which a "wrong" press labels; none for a
    /// label raised before store format 53.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub judgment: Option<String>,
}

/// `content.unlabel`: remove one label from a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContentUnlabelParams {
    pub session_id: String,
    /// The flag's word: `injection`, `secret`, `confidential` or `personal`.
    pub flag: String,
    /// Who removed it, as a label. Default: the connection. It names and
    /// proves nothing; the connection's surface decides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What a removal took off a session, and who removed it: `content.unlabel`'s
/// result, and the params of `session.unlabelled`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UnlabelResult {
    pub session_id: String,
    pub flag: String,
    /// Who removed it, as a label.
    pub by: String,
    /// Who, as the approval rule knows them, and the channel it came through.
    #[serde(default)]
    pub who: String,
    #[serde(default)]
    pub via: String,
    /// The method that did it: `content.unlabel`, or `action.confirm` for a
    /// card's press.
    pub how: String,
    /// The approval that did it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    pub at_ms: u64,
    /// The label it removed.
    pub label: LabelView,
}

/// A label was raised on a session: the params of `session.labelled`, to the
/// session's watchers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabelRaised {
    pub session_id: String,
    pub label: LabelView,
}

/// One label a session carries, as a reply's footer shows it (theseus-emqbm):
/// the flag's word, and the Ship address that removes that flag from that
/// session. The footer lists what the session carries *now*, so a label can be
/// taken off at any time after the one message that raised it, which is
/// reachable only at the moment it was raised.
///
/// `remove_url` is absent when no web UI runs and no `[cockpit] public_url`
/// names one: the surface then says the label and offers no link.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabelLink {
    /// `injection`, `secret`, `confidential` or `personal`.
    pub flag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub remove_url: Option<String>,
}

/// Where the Ship removes one label from one session:
/// `<base>/session/<session>/unlabel/<flag>`. A base's trailing slash is
/// dropped, and the address carries no key or token: it is the cockpit's own
/// client-side route, behind the web UI's own door.
pub fn unlabel_url(base: &str, session_id: &str, flag: &str) -> String {
    format!(
        "{}/session/{session_id}/unlabel/{flag}",
        base.trim_end_matches('/')
    )
}
