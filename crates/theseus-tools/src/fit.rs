//! A call that fits the tools (theseus-9dt2): the names and shapes a model
//! reaches for, read as the call it meant, and an error that names the field
//! or the tool it should have used. Pure: nothing here reads a clock, a file,
//! or a policy. The runtime applies `Registry::fit` where it takes a call
//! from the model, so the gate judges the call it will run.

use serde_json::{Map, Value};

use crate::proc::shell_argv;
use crate::Registry;

/// The tool a shell call is made to, by its wire name.
const SHELL: &str = "proc_run";

/// A call read as the one it meant.
#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    pub wire: String,
    pub input: Value,
    /// The result's first line, when the call was not made as it ran.
    pub note: Option<String>,
}

impl Registry {
    /// `bash` or `shell` (no such tool) with a `command` runs as `proc_run`;
    /// a string where `argv`'s array goes is a shell line; and `proc_run`'s
    /// `command` becomes the `["bash", "-c", line]` the gate judges as it
    /// judges that call. None when the call is as it should be.
    pub fn fit(&self, wire: &str, input: &Value) -> Option<Fitted> {
        let routed = matches!(wire, "bash" | "shell")
            && self.by_wire(wire).is_none()
            && input.get("command").is_some_and(Value::is_string);
        if (!routed && wire != SHELL) || self.by_wire(SHELL).is_none() {
            return None;
        }
        let mut obj: Map<String, Value> = input.as_object()?.clone();
        let mut note =
            routed.then(|| format!("[ran as {SHELL} {{command}}: there is no {wire} tool]"));
        let alone = |o: &Map<String, Value>, k: &str| {
            ["argv", "steps", "command"]
                .iter()
                .all(|other| *other == k || !o.contains_key(*other))
        };
        // `{"argv": "cp /a /b"}`: a line, said where a program goes.
        if matches!(obj.get("argv"), Some(Value::String(_))) && alone(&obj, "argv") {
            let line = obj.remove("argv")?;
            obj.insert("command".into(), line);
            note.get_or_insert_with(|| {
                format!(
                    "[ran as {SHELL} {{command}}: argv takes an array, so this string ran as a \
                     shell line]"
                )
            });
        }
        // `command` is `["bash", "-c", command]` to everything after it.
        if let Some(Value::String(line)) = obj.get("command") {
            if alone(&obj, "command") {
                let argv = shell_argv(line);
                obj.remove("command");
                obj.insert("argv".into(), argv.into());
            }
        }
        let fitted = Value::Object(obj);
        (routed || &fitted != input).then(|| Fitted {
            wire: SHELL.into(),
            input: fitted,
            note,
        })
    }

    /// An error for `wire`'s input that names the field: one that a sibling
    /// tool takes says whose it is, and the fields this tool takes, read from
    /// the schemas. None when the error is not an unknown field of the top
    /// level (the serde message already says the rest).
    pub fn field_hint(&self, wire: &str, error: &str) -> Option<String> {
        let field = error.split_once("unknown field `")?.1.split_once('`')?.0;
        let tool = self.by_wire(wire)?;
        let own = fields_of(&tool.input_schema());
        if own.iter().any(|f| f == field) {
            return None;
        }
        let owners: Vec<String> = self
            .all()
            .filter(|t| t.wire_name() != wire)
            .filter(|t| fields_of(&t.input_schema()).iter().any(|f| f == field))
            .map(|t| t.wire_name())
            .collect();
        let takes = format!("{wire} takes {}", own.join(", "));
        Some(match owners.as_slice() {
            [] => format!("no field `{field}`; {takes}"),
            [one] => format!("`{field}` is {one}'s; {takes}"),
            many => format!("`{field}` is {}'s; {takes}", many.join("'s and ")),
        })
    }
}

/// A schema's property names, in the order the schema lists them.
fn fields_of(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// A streamed input that did not parse, as `{"argv": cp /a /b}`: the line
/// it holds, as the `{command}` call it meant, only when nothing in it is
/// another field, so there is one way to read it.
pub fn recover_line(raw: &str) -> Option<Value> {
    let body = raw.trim().strip_prefix('{')?.strip_suffix('}')?;
    let rest = body.trim_start().strip_prefix("\"argv\"")?;
    let line = rest.trim_start().strip_prefix(':')?.trim();
    // A bare line: not an array, a string, or an object, which would have parsed.
    if line.is_empty() || line.starts_with(['[', '"', '{']) || names_a_field(line) {
        return None;
    }
    Some(serde_json::json!({ "command": line }))
}

/// A quote, then a colon: another key of the object.
fn names_a_field(s: &str) -> bool {
    s.match_indices('"')
        .any(|(i, _)| s[i + 1..].trim_start().starts_with(':'))
}

/// What a call to `proc_run` whose JSON did not parse is told: the field and
/// the two shapes, in one line (theseus-9dt2).
pub const SHELL_SHAPES: &str = "the input was not valid JSON. proc_run takes \
    {\"argv\": [\"bash\", \"-c\", \"…\"]} (a program, with a shell as its first element) \
    or {\"command\": \"…\"} (a line run in bash)";

/// Wire names a model reaches for, by what it means (`fs_read` for `cat`).
const SLIPS: [(&str, &str); 24] = [
    ("bash", "proc_run"),
    ("shell", "proc_run"),
    ("sh", "proc_run"),
    ("run", "proc_run"),
    ("exec", "proc_run"),
    ("execute", "proc_run"),
    ("terminal", "proc_run"),
    ("read", "fs_read"),
    ("read_file", "fs_read"),
    ("cat", "fs_read"),
    ("view", "fs_read"),
    ("write", "fs_write"),
    ("write_file", "fs_write"),
    ("create_file", "fs_write"),
    ("edit", "fs_edit"),
    ("edit_file", "fs_edit"),
    ("str_replace", "fs_edit"),
    ("grep", "fs_grep"),
    ("search", "fs_grep"),
    ("glob", "fs_glob"),
    ("find", "fs_glob"),
    ("ls", "fs_list"),
    ("list", "fs_list"),
    ("list_dir", "fs_list"),
];

/// The one or two of `names` nearest to `name`: the tool a slip means, then
/// those that hold it or that it holds, then by edit distance.
pub fn nearest<'a>(name: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let lower = name.to_ascii_lowercase();
    let meant = SLIPS.iter().find(|(s, _)| *s == lower).map(|(_, t)| *t);
    let mut scored: Vec<(usize, &str)> = names
        .into_iter()
        .map(|n| {
            let d = if Some(n) == meant {
                0
            } else if n.contains(&lower) || lower.contains(n) {
                1
            } else {
                2 + distance(&lower, n)
            };
            (d, n)
        })
        .collect();
    scored.sort();
    scored.into_iter().take(2).map(|(_, n)| n).collect()
}

/// Levenshtein's distance, over chars.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::default_registry;

    #[test]
    fn a_bash_call_with_a_command_runs_as_the_shell_tool_and_says_so() {
        let r = default_registry();
        let f = r.fit("bash", &json!({"command": "pwd && ls"})).unwrap();
        assert_eq!(f.wire, "proc_run");
        assert_eq!(f.input, json!({"argv": ["bash", "-c", "pwd && ls"]}));
        assert_eq!(
            f.note.as_deref(),
            Some("[ran as proc_run {command}: there is no bash tool]")
        );
        let s = r
            .fit("shell", &json!({"command": "ls", "cwd": "sub"}))
            .unwrap();
        assert_eq!(s.input, json!({"argv": ["bash", "-c", "ls"], "cwd": "sub"}));
        assert!(s.note.unwrap().contains("there is no shell tool"));
        // No command: nothing to run as, so it stays an unknown tool.
        assert!(r.fit("bash", &json!({"cmd": "ls"})).is_none());
        assert!(r.fit("fs_read", &json!({"path": "a"})).is_none());
    }

    #[test]
    fn proc_runs_command_is_the_argv_of_a_bash_c_call() {
        let r = default_registry();
        let f = r
            .fit(
                "proc_run",
                &json!({"command": "echo $((1+1))", "timeout_secs": 5}),
            )
            .unwrap();
        assert_eq!(
            f.input,
            json!({"argv": ["bash", "-c", "echo $((1+1))"], "timeout_secs": 5})
        );
        assert_eq!(f.note, None, "the call is as it should be");
        // Two of the three are left for the tool to refuse by name.
        assert!(r
            .fit("proc_run", &json!({"command": "a", "argv": ["b"]}))
            .is_none());
        assert!(r.fit("proc_run", &json!({"argv": ["ls"]})).is_none());
    }

    #[test]
    fn a_string_where_argv_goes_is_a_shell_line() {
        let r = default_registry();
        let f = r.fit("proc_run", &json!({"argv": "cp /a /b"})).unwrap();
        assert_eq!(f.input, json!({"argv": ["bash", "-c", "cp /a /b"]}));
        assert!(f.note.unwrap().contains("shell line"));
    }

    #[test]
    fn a_bare_line_after_argv_is_recovered_only_when_it_is_the_only_reading() {
        assert_eq!(
            recover_line(r#"{"argv": cp /a /b}"#),
            Some(json!({"command": "cp /a /b"}))
        );
        assert_eq!(
            recover_line("{ \"argv\" :  echo \"hi there\" }"),
            Some(json!({"command": "echo \"hi there\""}))
        );
        // Another key, an array that broke, a string that broke: no one reading.
        assert_eq!(recover_line(r#"{"argv": cp /a /b, "cwd": "/x"}"#), None);
        assert_eq!(
            recover_line(r#"{"argv": ["bash", "-c", "echo "hi""]}"#),
            None
        );
        assert_eq!(recover_line(r#"{"argv": "echo "hi""}"#), None);
        assert_eq!(recover_line(r#"{"cwd": "/x"}"#), None);
        assert_eq!(recover_line("not json"), None);
    }

    #[test]
    fn a_field_a_sibling_takes_names_the_sibling_and_this_tools_fields() {
        let r = default_registry();
        let e = r
            .field_hint(
                "fs_read",
                "invalid input: unknown field `max_results`, expected one of `path`",
            )
            .unwrap();
        assert_eq!(
            e,
            "`max_results` is fs_grep's; fs_read takes limit, offset, pages, path"
        );
        let none = r
            .field_hint(
                "fs_read",
                "invalid input: unknown field `zzz`, expected `path`",
            )
            .unwrap();
        assert!(none.starts_with("no field `zzz`; fs_read takes limit"));
        // Its own field, or another kind of error: the message stands.
        assert!(r
            .field_hint("fs_read", "invalid input: unknown field `path`")
            .is_none());
        assert!(r
            .field_hint("fs_read", "invalid input: missing field `path`")
            .is_none());
    }

    /// The schemas the model sees do not change: aliases are serde's alone.
    /// The names a model plausibly sends for a path and for what a write or
    /// an edit holds are read on every fs tool that takes them.
    #[test]
    fn the_fs_tools_read_the_names_a_model_reaches_for() {
        use crate::{Tool, ToolCtx};
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
        let c = ToolCtx::for_tests(d.path());
        let path = |p: &crate::Plan| p.resources[0].path.clone();
        let a = d.path().join("a.txt");
        for key in ["path", "file", "filename", "file_path"] {
            let at = json!({ key: "a.txt" });
            assert_eq!(path(&crate::fs::Read.plan(&at, &c).unwrap()), a, "{key}");
            let w = json!({ key: "a.txt", "content": "x" });
            assert_eq!(
                path(&crate::fs::WriteFile.plan(&w, &c).unwrap()),
                a,
                "{key}"
            );
            let e = json!({ key: "a.txt", "old_string": "o", "new_string": "n" });
            assert_eq!(path(&crate::fs::Edit.plan(&e, &c).unwrap()), a, "{key}");
            let g = json!({ "pattern": "o", key: "a.txt" });
            assert!(crate::fs::Grep.plan(&g, &c).is_ok(), "{key}");
            let g = json!({ "pattern": "*.txt", key: "." });
            assert!(crate::fs::Glob.plan(&g, &c).is_ok(), "{key}");
            assert!(
                crate::fs::List.plan(&json!({ key: "." }), &c).is_ok(),
                "{key}"
            );
        }
        for content in ["content", "file_text", "contents"] {
            let w = json!({ "path": "a.txt", content: "x" });
            assert!(crate::fs::WriteFile.plan(&w, &c).is_ok(), "{content}");
        }
        for (old, new) in [
            ("old_string", "new_string"),
            ("old_str", "new_str"),
            ("old_text", "new_text"),
        ] {
            let e = json!({ "path": "a.txt", old: "o", new: "n" });
            assert!(crate::fs::Edit.plan(&e, &c).is_ok(), "{old}");
        }
        let diff = "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-one\n+two\n";
        for key in ["patch", "diff"] {
            assert!(
                crate::fs::Patch.plan(&json!({ key: diff }), &c).is_ok(),
                "{key}"
            );
        }
        // Two names for one field is a call with a field twice: refused.
        let twice = json!({"path": "a.txt", "file": "a.txt", "content": "x"});
        assert!(crate::fs::WriteFile.plan(&twice, &c).is_err());
        // The schema is what it was: the model is never shown an alias.
        let schema = crate::fs::WriteFile.input_schema().to_string();
        assert!(
            !schema.contains("file_text") && !schema.contains("filename"),
            "{schema}"
        );
    }

    /// Only the tools whose input is the content a model writes stream it
    /// eagerly, so `proc_run` and the rest never break their JSON.
    #[test]
    fn only_the_write_tools_ask_for_eager_input_streaming() {
        let defs = default_registry().definitions(true);
        let eager: Vec<&str> = defs
            .iter()
            .filter(|d| d["eager_input_streaming"] == true)
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert_eq!(eager, ["fs_edit", "fs_patch", "fs_write"]);
        assert_eq!(defs.len(), 11);
        assert!(default_registry()
            .definitions(false)
            .iter()
            .all(|d| d.get("eager_input_streaming").is_none()));
    }

    #[test]
    fn the_nearest_tools_are_one_or_two_and_a_slip_finds_its_tool() {
        let names = [
            "fs_edit", "fs_glob", "fs_grep", "fs_list", "fs_read", "fs_write", "proc_run",
        ];
        assert_eq!(nearest("bash", names)[0], "proc_run");
        assert_eq!(nearest("cat", names)[0], "fs_read");
        assert_eq!(nearest("fs_reed", names)[0], "fs_read");
        assert_eq!(nearest("grep", names)[0], "fs_grep");
        assert_eq!(nearest("anything", names).len(), 2);
    }
}
