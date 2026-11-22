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

/// What Jev says about one item (issue 44): the content answers kept on the item itself, beside its node,
/// as `node.reach` and `theseus view show` read them. A description, never a permission: nothing withholds
/// on it.
///
/// `state` is `labelled` (something was raised and stands), `clean` (asked, and nothing raised or all of it
/// removed by the owner), `failed` (the ask failed, was refused or could not be built: `why` says which;
/// never read as clean), or `not_judged` (no record: an item from before items kept their labels, or one
/// that was never asked).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ItemLabelsView {
    pub node: String,
    pub state: String,
    /// The item's kind (`operator_message`, `tool_result`, `reply`, …) and its source; absent when not judged.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub source: Option<String>,
    /// The pack asked (`content.v4`), when it was answered, and why it was not when it was not.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub why: Option<String>,
    /// Each of the four questions: `yes`, `no`, or `not_asked`.
    pub questions: Vec<ItemQuestion>,
    /// Each flag raised on the item, by the pack or by a source that needs no model.
    pub flags: Vec<ItemFlag>,
    /// The categories `onto.v1` filed the item's exchange under (`topic:rust-harness`).
    pub topics: Vec<String>,
}

impl ItemLabelsView {
    /// One line for a person: `personal (confirm 0.91, content.v4)`, `asked, clean`, `failed (failed)`,
    /// `not judged then`; a flag the owner removed reads `personal removed`, and the exchange's topics follow.
    pub fn line(&self) -> String {
        let flags: Vec<String> = (self.flags.iter())
            .map(|f| match &f.removed {
                Some(_) => format!("{} removed", f.flag),
                None => format!("{} ({} {:.2}, {})", f.flag, f.band, f.confidence, f.pack),
            })
            .collect();
        let what = match self.state.as_str() {
            "not_judged" => "not judged then".to_string(),
            "failed" => format!("failed ({})", self.why.as_deref().unwrap_or("unknown")),
            "clean" if flags.is_empty() => "asked, clean".to_string(),
            "clean" => format!("asked, clean; {}", flags.join(", ")),
            _ => flags.join(", "),
        };
        match self.topics.is_empty() {
            true => what,
            false => format!("{what}; topics {}", self.topics.join(", ")),
        }
    }
}

/// One content question's answer about an item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ItemQuestion {
    pub question: String,
    /// `yes`, `no` (or another of the pack's choices), or `not_asked`.
    pub said: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub band: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub confidence: Option<f64>,
}

/// One flag raised on an item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ItemFlag {
    pub flag: String,
    pub band: String,
    pub confidence: f64,
    pub source: String,
    /// `content.v4`, or `deterministic` for a label no model raised.
    pub pack: String,
    pub at_ms: u64,
    /// `by via`, when the owner removed it (from this item, or from its session after the item entered).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub removed: Option<String>,
}
