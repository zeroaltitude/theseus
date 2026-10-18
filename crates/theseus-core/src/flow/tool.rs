//! A loaded flow as the model sees it: one tool, `flow:<name>` (wire
//! `flow__<name>`), in its own deferred family. It is `flow.run` for that
//! spec: the call's input is the flow's inputs, and `ToolRuntime::run_flow`
//! runs the steps. The call itself only plans; each step is gated as the
//! model's own call would be.

use std::sync::Arc;

use serde_json::Value;
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::spec::{repo_id, repo_root, Spec};
use crate::tools_defer::PlaceKind;

/// The canonical prefix of a flow's tool, and its wire's.
pub const PREFIX: &str = "flow:";
pub const WIRE_PREFIX: &str = "flow__";

/// Whether a canonical tool name is a flow's.
pub fn is_flow(name: &str) -> bool {
    name.starts_with(PREFIX)
}

/// Whether `spec` is offered for a session working in `dir`, in a place of
/// `kind`: its repositories hold the directory, and its kinds include this
/// one (a headless daemon's place is the CLI's, to a flow that names `headless`).
pub fn in_scope(spec: &Spec, dir: &std::path::Path, kind: PlaceKind) -> bool {
    let kinds = &spec.scope.places;
    let here = kinds.is_empty()
        || kinds
            .iter()
            .any(|p| p == kind.as_str() || (kind == PlaceKind::Cli && p == "headless"));
    here && repo_root(dir)
        .map(|r| repo_id(&r))
        .is_some_and(|id| spec.scope.repos.contains(&id))
}

pub struct FlowTool {
    canonical: String,
    wire: String,
    description: String,
    schema: Value,
    pub(super) spec: Arc<Spec>,
}

impl FlowTool {
    pub fn new(spec: Arc<Spec>) -> Self {
        Self {
            canonical: format!("{PREFIX}{}", spec.name),
            wire: format!("{WIRE_PREFIX}{}", spec.name),
            description: spec.tool_description(),
            schema: spec.input_schema(),
            spec,
        }
    }
}

impl Tool for FlowTool {
    fn name(&self) -> &str {
        &self.canonical
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    /// A run: its steps are programs, and two flows never run side by side.
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // A flow that was running when the daemon died may have merged or
        // pushed: it is never run again, and its steps' own rows say how far it got.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let inputs = self.spec.resolve_inputs(input)?;
        let given: Vec<String> = inputs
            .iter()
            .map(|(k, v)| {
                let t = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                format!("{k}={}", t.chars().take(60).collect::<String>())
            })
            .collect();
        Ok(Plan {
            summary: format!("run flow {} ({})", self.spec.name, given.join(", ")),
            ..Default::default()
        })
    }

    fn wire_name(&self) -> String {
        self.wire.clone()
    }

    fn family(&self) -> &str {
        "flow"
    }
}
