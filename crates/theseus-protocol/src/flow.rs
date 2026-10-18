//! Flows on the wire (theseus-pw1q.21.4): what `Core::flow_list` tells of a
//! loaded flow, and the structured state a flow hands back when a step needs
//! the model's judgment. The spec itself is core's (`theseus_core::flow::spec`).

use serde::{Deserialize, Serialize};

/// One loaded flow, as the board holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlowInfo {
    pub name: String,
    /// The frozen spec's SHA-256, in hex.
    pub digest: String,
    /// The frozen copy: `<state>/self/mint/<name>/<digest>/spec.json`.
    pub frozen: String,
    pub steps: u64,
    /// The repositories it is offered in, by id (`r:<hex>`).
    pub repos: Vec<String>,
    /// The kinds of place it is offered in; empty: every kind.
    pub places: Vec<String>,
    pub loaded_by: String,
    pub loaded_at_ms: u64,
}

/// What a flow that stopped for the model's judgment hands back: the step it
/// stopped at, why, and the state its exit names. The model reads it as JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FlowHandback {
    /// The exit's name (`conflicts-left`) or the engine's (`step-failed`,
    /// `needs-approval`, `refused`, `timeout`, `loop-bound`).
    pub handback: String,
    /// The step the flow stopped at.
    pub step: String,
    /// One line: why it stopped.
    pub why: String,
    /// The state the exit names: `conflict_files`, `rerere_resolved`,
    /// `output_tail`, `step`.
    pub state: serde_json::Map<String, serde_json::Value>,
}
