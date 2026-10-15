//! What a file tool changed, as a diff (theseus-z6au): the one place the
//! `tool.ended` diff and a question's preview are made, so the preview of an
//! edit is the diff its execution makes when the file did not change between.
//! The text here is not yet scrubbed: the runtime scrubs it as it does the
//! model's copy, before anything leaves the process.

use std::path::Path;

use serde_json::Value;
use theseus_protocol::diff::{cap, creation, stat};
use theseus_protocol::{ConfirmPreview, ToolDiff};

use crate::ToolCtx;

/// The largest file whose old text a diff reads (2 MiB).
const OLD_MAX: u64 = 2 * 1024 * 1024;

/// A file's text as it is now, when it is a regular text file of a size a diff
/// reads.
pub fn old_text(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    (m.is_file() && m.len() <= OLD_MAX).then(|| std::fs::read_to_string(path).ok())?
}

/// A file's name as a diff shows it: relative to the working directory when it
/// is under it.
pub fn shown(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

/// The unified diff of one file's change; a new file's is all additions.
pub fn file_diff(name: &str, before: Option<&str>, after: &str) -> String {
    match before {
        None => creation(name, after),
        Some(b) => similar::TextDiff::from_lines(b, after)
            .unified_diff()
            .context_radius(3)
            .header(name, name)
            .to_string(),
    }
}

/// The text a recorded unified diff was made from, given the text it made
/// (`current`): the diff applied backwards. It fails when the context does
/// not match, so a file changed since is refused, never guessed.
pub fn unapply(unified: &str, current: &str) -> Result<String, String> {
    let patch = diffy::Patch::from_str(unified).map_err(|e| e.to_string())?;
    diffy::apply(current, &patch.reverse()).map_err(|e| e.to_string())
}

/// `unified` as the wire's `ToolDiff`: counted whole, then capped.
pub fn tool_diff(path: String, unified: &str) -> ToolDiff {
    let (added, removed) = stat(unified);
    let (unified, truncated) = cap(unified);
    ToolDiff {
        path,
        added,
        removed,
        unified,
        truncated,
    }
}

/// The diff as a tool's `meta["diff"]`.
pub fn meta(diff: &ToolDiff) -> Value {
    serde_json::to_value(diff).unwrap_or(Value::Null)
}

/// Several files' diffs as one (`fs.patch`): the path of the one file, else
/// `N files`; the diffs in order.
pub fn combine(files: &[(String, String)]) -> ToolDiff {
    let path = match files {
        [(p, _)] => p.clone(),
        many => format!("{} files", many.len()),
    };
    let unified: String = files.iter().map(|(_, d)| d.as_str()).collect();
    tool_diff(path, &unified)
}

/// The text an `fs.edit` leaves, or none when its `old_string` is absent or
/// ambiguous (the run refuses those and says why).
pub fn edit_after(before: &str, old: &str, new: &str, all: bool) -> Option<String> {
    let n = before.matches(old).count();
    if old.is_empty() || n == 0 || (n > 1 && !all) {
        return None;
    }
    Some(if all {
        before.replace(old, new)
    } else {
        before.replacen(old, new, 1)
    })
}

/// A question's preview of an `fs.edit`, `fs.write` or `fs.patch`, from the file
/// as it is now: the diff its run would make. None for any other call, or when
/// the file cannot be read as text.
pub fn preview(tool: &str, input: &Value, ctx: &ToolCtx) -> Option<ConfirmPreview> {
    let (cwd, resolve) = (&ctx.cwd, |p: &str| ctx.resolve(p));
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let read = |p: &Path| old_text(p);
    let diff = |unified: String| ConfirmPreview {
        kind: ConfirmPreview::DIFF.into(),
        body: cap(&unified).0,
    };
    match tool {
        "fs.edit" => {
            let path = resolve(s("path")?);
            let before = read(&path)?;
            let all = input
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let after = edit_after(&before, s("old_string")?, s("new_string")?, all)?;
            Some(diff(file_diff(&shown(&path, cwd), Some(&before), &after)))
        }
        "fs.write" => {
            let path = resolve(s("path")?);
            let content = s("content")?;
            let name = shown(&path, cwd);
            Some(diff(match read(&path) {
                Some(before) if path.is_file() => file_diff(&name, Some(&before), content),
                _ => creation(&name, content),
            }))
        }
        // A patch is already a diff: the question shows it as given.
        "fs.patch" => Some(diff(s("patch").or_else(|| s("diff"))?.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edits_diff_names_its_file_and_counts_its_lines() {
        let d = file_diff("a.py", Some("x = 1\ny = 2\n"), "x = 1\ny = 3\n");
        let t = tool_diff("a.py".into(), &d);
        assert_eq!((t.added, t.removed, t.truncated), (1, 1, false));
        assert!(t.unified.contains("-y = 2\n+y = 3\n"));
        assert_eq!(edit_after("a a", "a", "b", false), None);
        assert_eq!(edit_after("a a", "a", "b", true).as_deref(), Some("b b"));
    }

    fn ran(tool: &dyn crate::Tool, input: &Value, ctx: &ToolCtx) -> crate::ToolOutput {
        tool.run(input, ctx)
            .unwrap_or_else(|e| panic!("{}", e.message))
    }

    #[test]
    fn an_edits_preview_is_the_diff_its_run_makes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::for_tests(dir.path());
        std::fs::write(
            dir.path().join("t.py"),
            "a = 1\nb = lowest - highest\nc = 3\n",
        )
        .unwrap();
        let input = serde_json::json!({"path": "t.py", "old_string": "lowest - highest", "new_string": "highest - lowest"});
        let pre = preview("fs.edit", &input, &ctx).unwrap();
        assert_eq!(pre.kind, "diff");
        let out = ran(&crate::fs::Edit, &input, &ctx);
        assert_eq!(out.meta["diff"]["unified"], pre.body);
        assert_eq!(out.meta["diff"]["path"], "t.py");
        assert_eq!(
            (
                out.meta["diff"]["added"].as_u64(),
                out.meta["diff"]["removed"].as_u64()
            ),
            (Some(1), Some(1))
        );
        // An edit that cannot apply has no preview.
        assert!(preview(
            "fs.edit",
            &serde_json::json!({"path": "t.py", "old_string": "zzz", "new_string": "y"}),
            &ctx
        )
        .is_none());
    }

    #[test]
    fn a_patch_and_a_write_end_with_the_diffs_they_made() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::for_tests(dir.path());
        std::fs::write(dir.path().join("n.py"), "x = 1\ny = 2\n").unwrap();
        let patch = "--- a/n.py\n+++ b/n.py\n@@ -1,2 +1,2 @@\n x = 1\n-y = 2\n+y = 3\n";
        let out = ran(
            &crate::fs::Patch,
            &serde_json::json!({"patch": patch}),
            &ctx,
        );
        let d = &out.meta["diff"];
        assert_eq!(
            (
                d["path"].as_str(),
                d["added"].as_u64(),
                d["removed"].as_u64()
            ),
            (Some("n.py"), Some(1), Some(1))
        );
        assert!(d["unified"].as_str().unwrap().contains("-y = 2\n+y = 3\n"));
        let out = ran(
            &crate::fs::WriteFile,
            &serde_json::json!({"path": "n.py", "content": "x = 1\ny = 4\n"}),
            &ctx,
        );
        assert!(out.meta["diff"]["unified"]
            .as_str()
            .unwrap()
            .contains("-y = 3\n+y = 4\n"));
        let out = ran(
            &crate::fs::WriteFile,
            &serde_json::json!({"path": "new.py", "content": "z\n"}),
            &ctx,
        );
        assert!(out.meta["diff"]["unified"]
            .as_str()
            .unwrap()
            .starts_with("--- /dev/null\n+++ new.py\n"));
    }
}
