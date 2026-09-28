//! `text.diff` (spec §3.24): a unified diff of two texts or two files.

use std::fs;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    parse, Access, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

pub struct Diff;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffArgs {
    #[serde(default)]
    a: Option<String>,
    #[serde(default)]
    b: Option<String>,
    #[serde(default)]
    a_path: Option<String>,
    #[serde(default)]
    b_path: Option<String>,
    #[serde(default)]
    context: Option<usize>,
}

impl DiffArgs {
    fn check(&self) -> Result<(), String> {
        if self.a.is_some() == self.a_path.is_some() {
            return Err("give exactly one of a (text) or a_path (file)".into());
        }
        if self.b.is_some() == self.b_path.is_some() {
            return Err("give exactly one of b (text) or b_path (file)".into());
        }
        Ok(())
    }
}

impl Tool for Diff {
    fn name(&self) -> &'static str {
        "text.diff"
    }
    fn description(&self) -> &'static str {
        "Unified diff between two texts or two files (each side: text or path). Use to compare versions, not to change files."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "a": {"type": "string", "description": "Left side as text."},
                "a_path": {"type": "string", "description": "Left side as a file."},
                "b": {"type": "string", "description": "Right side as text."},
                "b_path": {"type": "string", "description": "Right side as a file."},
                "context": {"type": "integer", "minimum": 0, "maximum": 20, "description": "Context lines. Default 3."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: DiffArgs = parse(input)?;
        a.check()?;
        let resources = [a.a_path.as_deref(), a.b_path.as_deref()]
            .into_iter()
            .flatten()
            .map(|p| Resource {
                path: ctx.resolve(p),
                access: Access::Read,
            })
            .collect();
        Ok(Plan {
            summary: "diff two texts".into(),
            resources,
            argv: None,
            consequences: vec![],
        })
    }
    fn run(&self, input: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolFailure> {
        let a: DiffArgs = parse(input).map_err(ToolFailure::new)?;
        a.check().map_err(ToolFailure::new)?;
        let side = |text: &Option<String>,
                    path: &Option<String>,
                    name: &str|
         -> Result<(String, String), ToolFailure> {
            match (text, path) {
                (Some(t), None) => Ok((t.clone(), name.to_string())),
                (None, Some(p)) => {
                    let r = ctx.resolve(p);
                    let t = fs::read_to_string(&r).map_err(|e| {
                        ToolFailure::new(format!("cannot read {}: {e}", r.display()))
                    })?;
                    Ok((t, r.display().to_string()))
                }
                _ => unreachable!(),
            }
        };
        let (at, an) = side(&a.a, &a.a_path, "a")?;
        let (bt, bn) = side(&a.b, &a.b_path, "b")?;
        let d = similar::TextDiff::from_lines(&at, &bt);
        let text = d
            .unified_diff()
            .context_radius(a.context.unwrap_or(3))
            .header(&an, &bn)
            .to_string();
        let ratio = d.ratio();
        Ok(ToolOutput {
            text: if text.is_empty() {
                "The two sides are identical.".into()
            } else {
                text
            },
            meta: json!({"similarity": ratio, "a": an, "b": bn}),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_texts_and_rejects_ambiguous_sides() {
        let d = tempfile::tempdir().unwrap();
        let c = ToolCtx::for_tests(d.path());
        let out = Diff
            .run(&json!({"a": "x\ny\n", "b": "x\nz\n"}), &c)
            .unwrap();
        assert!(
            out.text.contains("-y") && out.text.contains("+z"),
            "{}",
            out.text
        );
        assert!(Diff
            .plan(&json!({"a": "x", "a_path": "p", "b": "y"}), &c)
            .is_err());
        assert_eq!(
            Diff.run(&json!({"a": "same", "b": "same"}), &c)
                .unwrap()
                .text,
            "The two sides are identical."
        );
    }
}
