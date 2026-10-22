//! A tool call in words, for every surface (theseus-z6au): `summarize` is the
//! line a person reads (`Run `python3 -m unittest -v``, `Edit tides.py`),
//! built on `argument`, the one thing a call is about, which is what Discord's
//! tool lines have always shown (its bytes are unchanged).

use serde_json::Value;

/// `s` in at most `max` characters, the last one `…` when it was cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The first `max` characters of `s`, then `…` when there were more (`max + 1`
/// in all).
pub fn cut_marked(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

/// The first `max` characters of `s`, with no mark.
pub fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn text(input: &Value, k: &str) -> Option<String> {
    input.get(k).and_then(Value::as_str).map(str::to_string)
}

/// One short line for a tool call: the argument a person would look for. A
/// terminal call names its terminal and keys, a program its argv, a search its
/// pattern, a file tool its path, a fetch its URL.
pub fn argument(tool: &str, input: &Value) -> String {
    let s = |k: &str| text(input, k);
    let text = if let Some(line) = super::summary(tool, input) {
        line
    } else if let Some(argv) = input.get("argv").and_then(Value::as_array) {
        argv.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(p) = s("pattern") {
        match s("path") {
            Some(d) => format!("{p} in {d}"),
            None => p,
        }
    } else if let Some(p) = s("path").or_else(|| s("rev")).or_else(|| s("file")) {
        p
    } else if tool == "fs.patch" {
        "(patch)".into()
    } else if let Some(u) = s("url") {
        u
    } else if let Some(q) = s("query") {
        format!("\"{q}\"")
    } else if let Some(svc) = s("service") {
        // `aws.describe`'s service and operation.
        match s("operation") {
            Some(op) => format!("{svc}:{op}"),
            None => svc,
        }
    } else {
        serde_json::to_string(input).unwrap_or_default()
    };
    clip(&text.replace('`', "'").replace('\n', " "), 90)
}

/// The files a unified diff names (`+++ b/path`, or the old side of a delete).
fn patch_files(patch: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in patch.lines() {
        let Some(p) = l.strip_prefix("+++ ") else {
            continue;
        };
        let p = p.split('\t').next().unwrap_or(p).trim();
        let p = p.strip_prefix("b/").unwrap_or(p);
        if p != "/dev/null" && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// A tool call as a person would say it: `Run `cargo test -q``, `Edit
/// tides.py`, `Read README.md`, `Search for `retry` in src/`. A tool this does
/// not know is its name and its argument.
pub fn summarize(tool: &str, input: &Value) -> String {
    // A call whose input was never seen (a late result's end) has no argument to say.
    let arg = if input.is_null() {
        String::new()
    } else {
        argument(tool, input)
    };
    let path = || text(input, "path").unwrap_or_else(|| arg.clone());
    let quoted = |k: &str| text(input, k).map(|p| format!("`{}`", p.replace('`', "'")));
    let line = match tool {
        "proc.run" => match input.get("steps").and_then(Value::as_array) {
            Some(steps) if input.get("argv").is_none() => format!("Run {} commands", steps.len()),
            _ if arg.is_empty() => "Run a command".to_string(),
            _ => format!("Run `{arg}`"),
        },
        "fs.read" => format!("Read {}", path()),
        "fs.write" => format!("Write {}", path()),
        "fs.edit" => format!("Edit {}", path()),
        "fs.patch" => {
            let files = patch_files(
                &text(input, "patch")
                    .or_else(|| text(input, "diff"))
                    .unwrap_or_default(),
            );
            match files.as_slice() {
                [] => "Patch".to_string(),
                [one] => format!("Patch {one}"),
                many => format!("Patch {} files", many.len()),
            }
        }
        "fs.list" => format!("List {}", path()),
        "fs.glob" => match (quoted("pattern"), text(input, "path")) {
            (Some(p), Some(d)) => format!("Find files matching {p} in {d}"),
            (Some(p), None) => format!("Find files matching {p}"),
            _ => "Find files".into(),
        },
        "fs.grep" => match (quoted("pattern"), text(input, "path")) {
            (Some(p), Some(d)) => format!("Search for {p} in {d}"),
            (Some(p), None) => format!("Search for {p}"),
            _ => "Search".into(),
        },
        "git.diff" => "Diff the working tree".into(),
        "git.log" => "Read the git log".into(),
        "text.diff" => "Compare two texts".into(),
        "http.fetch" => format!("Fetch {arg}"),
        "web.search" => format!("Search the web for {arg}"),
        "term.open" => format!("Open a terminal: `{arg}`"),
        "term.send" => format!("Type into terminal {arg}"),
        "term.read" => format!("Read terminal {arg}"),
        "term.close" => format!("Close terminal {arg}"),
        "job.read" => format!("Read job {arg}"),
        "job.wait" if arg.is_empty() => "Wait for a job".to_string(),
        "job.wait" => format!("Wait for job {arg}"),
        "job.stop" => format!("Stop job {arg}"),
        "plan.present" => {
            let n = input
                .get("steps")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            format!(
                "Approve this plan ({n} step{})",
                if n == 1 { "" } else { "s" }
            )
        }
        "plan.step" => format!(
            "Plan step {}: {}",
            input.get("index").map_or("?".into(), |i| i.to_string()),
            text(input, "state").unwrap_or_default()
        ),
        t if t.starts_with("aws.") => format!("AWS {arg}"),
        _ => {
            return if arg.is_empty() || arg == "{}" {
                tool.to_string()
            } else {
                format!("{tool} {arg}")
            }
        }
    };
    clip(&line.replace('\n', " "), 120)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clip_at_most_n_counts_the_mark() {
        let long = "x".repeat(300);
        assert_eq!(clip(&long, 200).chars().count(), 200);
        assert_eq!(clip(&long, 200), format!("{}…", "x".repeat(199)));
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abcd", 3), "ab…");
        assert_eq!(clip("héllo wörld", 6), "héllo…");
    }

    #[test]
    fn cut_marked_keeps_n_characters_then_the_mark() {
        assert_eq!(cut_marked("ééé", 2), "éé…");
        assert_eq!(cut_marked("abcdef", 3), "abc…");
        assert_eq!(cut_marked("abc", 3), "abc");
        assert_eq!(cut_marked("", 0), "");
    }

    #[test]
    fn cut_keeps_n_characters_and_no_mark() {
        assert_eq!(cut("héllo", 2), "hé");
        assert_eq!(cut("hi", 9), "hi");
        assert_eq!(cut(&"x".repeat(300), 200).chars().count(), 200);
    }

    /// A call whose input was never seen says what it is, never `Run `null``.
    #[test]
    fn a_call_with_no_input_is_said_without_an_argument() {
        assert_eq!(summarize("proc.run", &Value::Null), "Run a command");
        assert_eq!(summarize("job.wait", &Value::Null), "Wait for a job");
    }

    #[test]
    fn a_call_is_said_in_words() {
        let s = summarize;
        assert_eq!(
            s(
                "proc.run",
                &json!({"argv": ["python3", "-m", "unittest", "-v"]})
            ),
            "Run `python3 -m unittest -v`"
        );
        assert_eq!(
            s(
                "fs.edit",
                &json!({"path": "tides.py", "old_string": "a", "new_string": "b"})
            ),
            "Edit tides.py"
        );
        assert_eq!(
            s("fs.read", &json!({"path": "README.md"})),
            "Read README.md"
        );
        assert_eq!(
            s("fs.grep", &json!({"pattern": "retry", "path": "src/"})),
            "Search for `retry` in src/"
        );
        assert_eq!(
            s(
                "fs.patch",
                &json!({"patch": "--- a/x.py\n+++ b/x.py\n@@ -1 +1 @@\n-a\n+b\n--- a/y.py\n+++ b/y.py\n"})
            ),
            "Patch 2 files"
        );
        assert_eq!(
            s("term.close", &json!({"terminal": "t3"})),
            "Close terminal t3"
        );
        assert_eq!(s("memory.lookup", &json!({})), "memory.lookup");
        // The unchanged part: the one argument, which Discord's lines show.
        assert_eq!(
            argument("fs.grep", &json!({"pattern": "fn main", "path": "src"})),
            "fn main in src"
        );
        assert_eq!(argument("fs.read", &json!({"path": "a`b"})), "a'b");
    }
}
