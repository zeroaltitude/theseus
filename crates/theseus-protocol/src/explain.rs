//! `policy.explain` (step 42a, theseus-ext.7; M7 §2.6): why a call waits, on
//! one screen. For each tool, every layer of the gate in its order, what it
//! says, the setting that says it, and the posture after it; and the layers
//! that depend on the call, as conditions with their entries. For a session,
//! or for every bound place and the CLI. `theseus policy explain` and the
//! cockpit's Policy tab (42b) read it.

use serde::{Deserialize, Serialize};

use crate::{ExternalText, PlaceCeiling, PlaceClass};

/// `policy.explain`'s params: one session's place, or every place; one
/// tool, or every tool the daemon has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyExplainParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// A canonical tool name (`proc.run`, `mcp:<server>/<tool>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tool: Option<String>,
}

/// `policy.explain`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PolicyExplainResult {
    /// The session's place, or the CLI then every bound place.
    pub places: Vec<PlaceExplain>,
    /// The workspace roots: each tool's result is for a call inside them.
    pub roots: Vec<String>,
}

/// One place: what it is, and each tool there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PlaceExplain {
    /// `cli`, or the place's target (`discord:channel:<id>`).
    pub place: String,
    /// `CLI`, `#pier`, `DM @owner`.
    pub name: String,
    pub class: PlaceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ceiling: Option<PlaceCeiling>,
    /// The session explained, when one was asked for (or the place's own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// T1's hold: what the session read, so its calls that act wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub hold: Option<ExternalText>,
    pub tools: Vec<ToolExplain>,
}

/// One tool in one place.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolExplain {
    pub tool: String,
    /// `read`, `write`, or `run`.
    pub class: String,
    /// Offered to the model here. One not offered is refused if called,
    /// and `refused` says why.
    pub offered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub refused: Option<String>,
    /// Every layer in the gate's order, for a call inside the roots that no
    /// condition below matches.
    pub layers: Vec<ExplainLayer>,
    /// The layers that depend on the call, with their entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<ExplainCondition>,
    /// `open`, `notify`, `approve`, or `refused`.
    pub result: String,
    /// The gate's reason for that call, as its record would keep it.
    pub reason: String,
}

/// One layer of the gate.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExplainLayer {
    /// The layer's name: `place`, `ceiling`, `class`, `posture`,
    /// `tightening`, `grant`, `lsp`, `floor`, `hold`, `mcp_client`.
    pub layer: String,
    /// What it says, in words.
    pub says: String,
    /// The setting that says it (`[policy.tools] "proc.run" = approve`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub setting: Option<String>,
    /// The posture after this layer.
    pub result: String,
    /// This layer raised the posture.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raised: bool,
}

/// A layer that depends on the call: when the call matches, `then`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ExplainCondition {
    /// `floor`, `approve_paths`, `outside_roots`, `private_address`,
    /// `approve_argv`, `destructive`, `allow_argv`, `aws`, `l1`, `grant`,
    /// `lsp_start`, `public_paths`.
    pub layer: String,
    /// When it applies, in words.
    pub when: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<String>,
    /// What then: a posture, `at least <posture>`, or `refused`.
    pub then: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape on the wire: a place, a tool's layers and a condition,
    /// optional fields absent when unset, and the bytes read back.
    #[test]
    fn an_explanation_keeps_its_shape_on_the_wire() {
        let r = PolicyExplainResult {
            places: vec![PlaceExplain {
                place: "cli".into(),
                name: "CLI".into(),
                class: PlaceClass::Private,
                tools: vec![ToolExplain {
                    tool: "proc.run".into(),
                    class: "run".into(),
                    offered: true,
                    layers: vec![ExplainLayer {
                        layer: "posture".into(),
                        says: "the config's posture".into(),
                        setting: Some("enforcement = approve".into()),
                        result: "approve".into(),
                        raised: true,
                    }],
                    conditions: vec![ExplainCondition {
                        layer: "allow_argv".into(),
                        when: "its argv starts with an entry".into(),
                        entries: vec!["git status".into()],
                        then: "open".into(),
                    }],
                    result: "approve".into(),
                    reason: "a call of proc.run: proc.run — approve (enforcement = approve)".into(),
                    ..Default::default()
                }],
                ceiling: None,
                session_id: None,
                hold: None,
            }],
            roots: vec!["/w".into()],
        };
        let text = serde_json::to_string(&r).unwrap();
        assert_eq!(
            text,
            r#"{"places":[{"place":"cli","name":"CLI","class":"private","tools":[{"tool":"proc.run","class":"run","offered":true,"layers":[{"layer":"posture","says":"the config's posture","setting":"enforcement = approve","result":"approve","raised":true}],"conditions":[{"layer":"allow_argv","when":"its argv starts with an entry","entries":["git status"],"then":"open"}],"result":"approve","reason":"a call of proc.run: proc.run — approve (enforcement = approve)"}]}],"roots":["/w"]}"#
        );
        let back: PolicyExplainResult = serde_json::from_str(&text).unwrap();
        assert_eq!(back, r);
        let p: PolicyExplainParams = serde_json::from_str("{}").unwrap();
        assert_eq!(p, PolicyExplainParams::default());
    }
}
