//! `project.attach`: the model ties this conversation to a project
//! (theseus-n5ui2). From the next turn on, every request ends with the
//! project's slice (`project::slice`): its goal, its open milestones with
//! their dates, and its next steps, which no compaction can take. The owner's
//! verb is `project.attach { session, project }`, the same record.
//!
//! Private places only, as the other tools that change what the model
//! committed to are (`places::public_tool` does not list it).

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::toolrun::TurnCtx;

pub const ATTACH: &str = "project.attach";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [ATTACH];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    /// A project's id (`tsk_…`, or the end of it), or `none` to detach.
    project: String,
}

/// The tool: the harness runs it.
pub struct ProjectAttach;

impl Tool for ProjectAttach {
    fn name(&self) -> &'static str {
        ATTACH
    }

    fn description(&self) -> &'static str {
        "Attach this conversation to a project (kind `project`): from the next turn every request \
         ends with its slice (goal, milestones, next steps). `none` detaches."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "project": {"type": "string", "description": "A project id, or none."}
            },
            "required": ["project"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: Input = parse(input)?;
        Ok(Plan {
            summary: match i.project.trim() {
                "none" => "detach this conversation from its project".into(),
                p => format!("attach this conversation to project {p}"),
            },
            ..Default::default()
        })
    }
}

/// Run `project.attach` (the harness's side).
pub fn attach(tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    if let Some(why) = crate::places::refusal(tc.class, ATTACH, &Plan::default(), &[]) {
        return Err(format!("Refused: {why}."));
    }
    let i: Input = parse(input)?;
    let name = i.project.trim();
    let target = if name == "none" {
        None
    } else {
        let all = crate::task_graph::all(tc.store).map_err(|e| format!("{e:#}"))?;
        let p = crate::task_graph::resolve(&all, name)?;
        if !crate::project::is_project(p) {
            return Err(format!(
                "task {} is a {}, not a project: make one with task_create `kind: project`",
                p.id,
                p.kind.as_str()
            ));
        }
        Some(p.clone())
    };
    let id = target.as_ref().map(|p| p.id.clone());
    tc.store
        .update_session(tc.session_id, |rec| {
            rec.project.clone_from(&id);
            Ok(vec![])
        })
        .map_err(|e| format!("{e:#}"))?
        .ok_or("this session has no record")?;
    let text = match &target {
        Some(p) => format!(
            "Attached to project {} (\"{}\"). From the next turn, every request ends with its \
             slice.",
            p.id, p.title
        ),
        None => "Detached: this conversation has no project.".into(),
    };
    Ok((text, json!({"project": id})))
}
