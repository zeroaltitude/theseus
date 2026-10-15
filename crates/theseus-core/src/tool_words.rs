//! What a question and a finished edit show of a call (theseus-z6au): the
//! preview a `confirm.requested` carries, computed when the call is planned,
//! and the diff a `tool.ended` carries, scrubbed as the model's copy is. The
//! diffs themselves are theseus-tools' `change`, so a preview of an edit is the
//! diff its run makes when the file did not change between.

use serde_json::{json, Value};
use theseus_protocol::diff::{cap, stat};
use theseus_protocol::{ConfirmPreview, ToolDiff};
use theseus_tools::{Plan, ToolCtx};

use crate::narrative;
use crate::scrub::Scrubber;
use crate::toolrun::ToolRuntime;

/// A word as a shell would take it: bare when it is safe, else in single
/// quotes.
fn quoted(w: &str) -> String {
    let safe = !w.is_empty()
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
    if safe {
        w.to_string()
    } else {
        format!("'{}'", w.replace('\'', r"'\''"))
    }
}

/// A command as a shell line, with its directory under it.
fn command(input: &Value, ctx: &ToolCtx) -> Option<ConfirmPreview> {
    let argv = input.get("argv").and_then(Value::as_array)?;
    let line = argv
        .iter()
        .filter_map(Value::as_str)
        .map(quoted)
        .collect::<Vec<_>>()
        .join(" ");
    let dir = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(|d| ctx.resolve(d))
        .unwrap_or_else(|| ctx.cwd.clone());
    Some(ConfirmPreview {
        kind: ConfirmPreview::COMMAND.into(),
        body: format!("{line}\nin {}", dir.display()),
    })
}

/// A plan's steps as its question shows them.
fn plan_text(input: &Value) -> Option<ConfirmPreview> {
    let steps: Vec<String> = input
        .get("steps")?
        .as_array()?
        .iter()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    Some(ConfirmPreview {
        kind: ConfirmPreview::TEXT.into(),
        body: theseus_protocol::modes::plan_text(&steps),
    })
}

impl ToolRuntime {
    /// A question's preview of the call it asks about, from the files as they
    /// are now, scrubbed as the model's copy is.
    pub(crate) fn preview(
        &self,
        dir: Option<&str>,
        tool: &str,
        input: &Value,
    ) -> Option<ConfirmPreview> {
        let mut ctx = self.ctx.clone();
        ctx.cwd = self.cwd_for(dir);
        let ctx = &ctx;
        let mut p = match tool {
            "proc.run" => command(input, ctx),
            // The plan is the question (theseus-lyxy): the steps, numbered.
            crate::policy::modes::PLAN_PRESENT => plan_text(input),
            _ => theseus_tools::change::preview(tool, input, ctx),
        }?;
        p.body = self.scrubber.scrub(&p.body).0;
        Some(p)
    }

    /// A call's main resource for the narrative, scrubbed of any secret value.
    pub(crate) fn subject(&self, tool: &str, plan: &Plan) -> String {
        let s = match &plan.url {
            // A network call's subject is what it asks for (DD5).
            Some(url) => format!("{tool} {url}"),
            None => narrative::subject(
                tool,
                plan.argv.as_deref(),
                plan.resources.first().map(|r| r.path.as_path()),
                &self.ctx.cwd,
            ),
        };
        self.scrubber.scrub(&s).0
    }
}

/// A result's meta with its diff scrubbed (the model's copy of the diff is
/// scrubbed with the rest of the text; this is the event's) and counted again.
pub(crate) fn scrub_diff(scrubber: &Scrubber, mut meta: Value) -> Value {
    let Some(d) = meta.get("diff").filter(|d| d.is_object()) else {
        return meta;
    };
    let Ok(mut diff) = serde_json::from_value::<ToolDiff>(d.clone()) else {
        meta["diff"] = Value::Null;
        return meta;
    };
    let (unified, changed) = scrubber.scrub(&diff.unified);
    diff.path = scrubber.scrub(&diff.path).0;
    if changed > 0 {
        diff.unified = unified;
        // Counts stay as the tool made them when the diff was cut; a
        // redaction does not change a line's count.
        if !diff.truncated {
            (diff.added, diff.removed) = stat(&diff.unified);
        }
        let (unified, cut) = cap(&diff.unified);
        diff.truncated |= cut;
        diff.unified = unified;
    }
    meta["diff"] = json!(diff);
    meta
}
