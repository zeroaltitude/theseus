//! The native tool a `proc.run` of the daemon's own CLI stands in for
//! (theseus-8c8s; issue 46): a job that runs `theseus memory search` or
//! `theseus memory index search` asks for what the history tool,
//! `context.history`, answers in-process, and its result ends with a line
//! that says so, with the call that does it.
//!
//! - **Why a line, not a refusal.** The CLI is the right thing to run when
//!   the work is the CLI itself (a check of its output, a bench, a bug in
//!   it), and the gate never guesses what a command is for (NOTIFY OVER
//!   BLOCK). The line is result metadata: the model reads it where it looks
//!   next, and the call it names is ready to copy.
//! - **What it reads.** The call's argv by words, as `external::Listed`
//!   does: `argv[0]` by file name, or a shell's or a launcher's command
//!   split on its quotes and operators, in every step of a batch. The first
//!   `theseus` whose subcommand the history tool answers names the line; a
//!   query held in a shell variable gives the call's shape with no value.
//! - **What it maps.** `memory search` and `memory index search` to
//!   `words`, the history tool's search over every place's exchanges.
//!   Nothing else gets a line: the history tool opens no session by its id
//!   (`theseus history`, `rpc session.history`) and lists no sessions by a
//!   span (`theseus sessions`, `rpc import.sessions`), so a line for those
//!   would steer the model to a call that does not do the thing.

use serde_json::Value;

/// The tool the line names, as the model calls it.
const WIRE: &str = "context_history";

/// The line a result of `input`, a `proc.run` call's, ends with: the native
/// call that does what it asked of the daemon's CLI. None when it asks
/// nothing the history tool answers.
pub fn hint(input: &Value) -> Option<String> {
    if let Some(steps) = input.get("steps").and_then(Value::as_array) {
        return steps.iter().find_map(hint);
    }
    let argv: Vec<&str> = input
        .get("argv")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let first = file_name(argv.first()?);
    let words: Vec<String> = match first {
        "theseus" => argv.iter().map(|a| (*a).to_string()).collect(),
        // A shell's or a launcher's command, by its words; any other
        // program's arguments are its own (`git log --grep 'theseus …'`).
        _ if crate::broker::launcher(first).is_some() => {
            argv[1..].iter().flat_map(|a| split(a)).collect()
        }
        _ => return None,
    };
    let call = words
        .iter()
        .enumerate()
        .filter(|(_, w)| file_name(w) == "theseus")
        .find_map(|(i, _)| native_call(&words[i + 1..]))?;
    Some(format!(
        "[native: {WIRE} {call} does this in-process, without a shell: it searches every place's \
         history for the words, runs beside the response's other calls, opens what it finds, and \
         pages with a cursor. Prefer it to the theseus CLI unless the CLI itself is the work]"
    ))
}

/// The global flags that take a value, and a subcommand's.
const VALUED: [&str; 12] = [
    "--socket",
    "-k",
    "--k",
    "--as-of",
    "--sources",
    "--session",
    "--arm",
    "-n",
    "--n",
    "--after",
    "--before",
    "--tag",
];

/// The native call `rest`, the words after a `theseus`, asks for, as JSON.
fn native_call(rest: &[String]) -> Option<String> {
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while let Some(w) = rest.get(i) {
        if is_operator(w) {
            break;
        }
        if VALUED.contains(&w.as_str()) {
            i += 2;
            continue;
        }
        if !w.starts_with('-') {
            positional.push(w);
        }
        i += 1;
    }
    let (sub, args) = positional.split_first()?;
    let query = match (*sub, args) {
        ("memory", [verb, query @ ..]) if *verb == "search" => query.join(" "),
        ("memory", [index, verb, query @ ..]) if *index == "index" && *verb == "search" => {
            query.join(" ")
        }
        _ => return None,
    };
    // A query in a shell variable (`$q`), or none, gives the call's shape.
    let query = query.trim();
    let words = match query.is_empty() || query.contains('$') {
        true => "…",
        false => query,
    };
    Some(format!("{{\"words\":{}}}", Value::String(words.into())))
}

/// A shell's operators and redirections end a command's words.
fn is_operator(w: &str) -> bool {
    matches!(
        w,
        "|" | "||" | "&" | "&&" | ";" | "(" | ")" | "do" | "done" | "then"
    ) || w
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .starts_with(['>', '<'])
}

/// A path's file name: `theseus` of `/usr/local/bin/theseus`.
fn file_name(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// A shell command's words: split on blanks outside quotes, each quote
/// taken off, and each operator a word of its own. It reads words, not what
/// runs, which is enough to name the command.
fn split(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let flush = |cur: &mut String, started: &mut bool, out: &mut Vec<String>| {
        if *started {
            out.push(std::mem::take(cur));
            *started = false;
        }
    };
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => flush(&mut cur, &mut started, &mut out),
            None if "|&;()".contains(c) => {
                flush(&mut cur, &mut started, &mut out);
                out.push(c.to_string());
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    flush(&mut cur, &mut started, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn of(argv: &[&str]) -> Option<String> {
        hint(&json!({ "argv": argv }))
    }

    #[test]
    fn a_search_of_the_index_names_the_native_call() {
        let h = of(&[
            "theseus",
            "memory",
            "index",
            "search",
            "-k",
            "20",
            "--json",
            "tide parser",
        ])
        .unwrap();
        assert!(
            h.contains(r#"context_history {"words":"tide parser"}"#),
            "{h}"
        );
        assert!(h.starts_with("[native: ") && h.ends_with(']'), "{h}");
        assert!(!h.contains('\n'), "one line: {h}");
        // By its path, and through a shell with a pipe after it.
        let h = of(&["/opt/tools/bin/theseus", "memory", "search", "kelp"]).unwrap();
        assert!(h.contains(r#"{"words":"kelp"}"#), "{h}");
        let h = of(&[
            "bash",
            "-c",
            "theseus memory index search -k 20 --json 'tide parser' 2>/dev/null | python3 -c 'x'",
        ])
        .unwrap();
        assert!(h.contains(r#"{"words":"tide parser"}"#), "{h}");
    }

    /// A time in the query is words to the history tool, as it is to the CLI's search.
    #[test]
    fn a_query_that_is_a_time_is_searched_as_words() {
        let h = of(&["theseus", "memory", "index", "search", "March 2026"]).unwrap();
        assert!(h.contains(r#"{"words":"March 2026"}"#), "{h}");
    }

    #[test]
    fn help_a_loop_and_a_batch_get_the_line_too() {
        // The DM's first call read the CLI's help: the line names the call.
        let h = of(&[
            "bash",
            "-c",
            "theseus memory index search --help 2>&1 | head -30",
        ])
        .unwrap();
        assert!(h.contains(r#"{"words":"…"}"#), "{h}");
        // A loop over queries in a variable: the shape, no value.
        let h = of(&[
            "bash",
            "-c",
            "for q in a b; do theseus memory index search -k 25 --json \"$q\"; done",
        ])
        .unwrap();
        assert!(h.contains(r#"{"words":"…"}"#), "{h}");
        let h = hint(&json!({"steps": [{"argv": ["ls"]}, {"argv": ["theseus", "memory", "search", "kelp"]}]}))
            .unwrap();
        assert!(h.contains(r#"{"words":"kelp"}"#), "{h}");
    }

    /// What the history tool does not do gets no line: it opens no session by its id and lists no
    /// sessions by a span (issue 46), so none of these is steered to it.
    #[test]
    fn what_no_native_tool_answers_gets_no_line() {
        for argv in [
            &["theseus", "ledger", "-n", "5"][..],
            &["theseus", "memory", "index", "status"],
            &["theseus", "sessions", "retire", "ses_1"],
            &["theseus", "ontology", "proposals"],
            &["cargo", "test", "-p", "theseus"],
            &["bash", "-c", "echo theseus; ls"],
            &["git", "log", "--grep", "theseus memory index search"],
            &["theseus", "sessions"],
            &["theseus", "--json", "sessions", "list"],
            &[
                "theseus", "--socket", "/tmp/s", "history", "-n", "5", "ses_1",
            ],
            &["theseus", "rpc", "session.history", "{}"],
            &["theseus", "rpc", "import.sessions", "{}"],
        ] {
            assert_eq!(of(argv), None, "{argv:?}");
        }
        assert_eq!(hint(&json!({})), None);
    }
}
