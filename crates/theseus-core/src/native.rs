//! The native tool a `proc.run` of the daemon's own CLI stands in for
//! (theseus-8c8s): a job that runs `theseus memory index search`, `theseus memory
//! search`, `theseus history` or `theseus sessions` asks for what
//! `memory.lookup` answers in-process, and its result ends with a line that
//! says so, with the call that does it.
//!
//! - **Why a line, not a refusal.** The CLI is the right thing to run when
//!   the work is the CLI itself (a check of its output, a bench, a bug in
//!   it), and the gate never guesses what a command is for (NOTIFY OVER
//!   BLOCK). The line is result metadata: the model reads it where it looks
//!   next, and the call it names is ready to copy.
//! - **What it reads.** The call's argv by words, as `external::Listed`
//!   does: `argv[0]` by file name, or a shell's or a launcher's command
//!   split on its quotes and operators, in every step of a batch. The first
//!   `theseus` whose subcommand a native tool answers names the line; a
//!   query held in a shell variable gives the call's shape with no value.
//! - **What it maps.** `memory index search` and `memory search` to `words` (to
//!   `when` when the query is a time, as "March 2026" is); `history` and
//!   `rpc session.history` to `session`; `sessions` and `sessions list` and
//!   `rpc import.sessions` to a span's listing. Nothing else has a native
//!   twin today, so nothing else gets a line.

use serde_json::Value;

/// The tool the line names, as the model calls it.
const WIRE: &str = "memory_lookup";

/// The line a result of `input`, a `proc.run` call's, ends with: the native
/// call that does what it asked of the daemon's CLI. None when it asks
/// nothing a native tool answers.
pub fn hint(input: &Value, now_ms: u64) -> Option<String> {
    if let Some(steps) = input.get("steps").and_then(Value::as_array) {
        return steps.iter().find_map(|s| hint(s, now_ms));
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
        .find_map(|(i, _)| native_call(&words[i + 1..], now_ms))?;
    Some(format!(
        "[native: {WIRE} {call} does this in-process, without a shell: it reads the same index \
         and sessions, runs beside the response's other calls, keeps the place rule and the \
         veils, and pages with a cursor. Prefer it to the theseus CLI unless the CLI itself is \
         the work]"
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

/// The native call `rest`, the words after a `theseus`, asks for, as JSON
/// with its fields in a fixed order.
fn native_call(rest: &[String], now_ms: u64) -> Option<String> {
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
    const SHAPE: &str = "…";
    let (sub, args) = positional.split_first()?;
    let fields: Vec<(&str, String)> = match (*sub, args) {
        ("memory", [verb, query @ ..]) if *verb == "search" => {
            words_or_when(&query.join(" "), now_ms)
        }
        ("memory", [index, verb, query @ ..]) if *index == "index" && *verb == "search" => {
            words_or_when(&query.join(" "), now_ms)
        }
        ("history", args) => vec![("session", args.first().map_or(SHAPE, |s| s).to_string())],
        ("sessions", []) => vec![("when", "last week".into())],
        ("sessions", [verb, ..]) if *verb == "list" => vec![("when", "last week".into())],
        ("rpc", [method, ..]) if *method == "session.history" => {
            vec![("session", SHAPE.into())]
        }
        ("rpc", [method, ..]) if *method == "import.sessions" => vec![("when", SHAPE.into())],
        _ => return None,
    };
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("\"{k}\":{}", Value::String(v.clone())))
        .collect();
    Some(format!("{{{}}}", body.join(",")))
}

/// A query as the native call asks it: a time it names goes to `when`, the
/// rest to `words`; a query in a shell variable (`$q`), or none, gives the
/// call's shape.
fn words_or_when(query: &str, now_ms: u64) -> Vec<(&'static str, String)> {
    let query = query.trim();
    if query.is_empty() || query.contains('$') {
        return vec![("words", "…".into()), ("when", "…".into())];
    }
    let Some(w) = crate::recall::when::read_local(query, now_ms) else {
        return vec![("words", query.into())];
    };
    let rest = query.replace(&w.said, " ");
    let rest = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut fields = Vec::new();
    if !rest.is_empty() {
        fields.push(("words", rest));
    }
    fields.push(("when", w.said));
    fields
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

    fn now() -> u64 {
        crate::wake::parse_at("2026-10-10T18:00:00Z").unwrap()
    }

    fn of(argv: &[&str]) -> Option<String> {
        hint(&json!({ "argv": argv }), now())
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
            h.contains(r#"memory_lookup {"words":"tide parser"}"#),
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

    #[test]
    fn a_query_that_is_a_time_goes_to_when() {
        let h = of(&["theseus", "memory", "index", "search", "March 2026"]).unwrap();
        assert!(h.contains(r#"{"when":"March 2026"}"#), "{h}");
        let h = of(&[
            "theseus", "memory", "index", "search", "kelp", "notes", "in", "March", "2026",
        ])
        .unwrap();
        assert!(h.contains(r#"{"words":"kelp notes"#), "{h}");
        assert!(
            h.contains(r#""when":""#) && h.contains("March 2026\"}"),
            "{h}"
        );
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
        assert!(h.contains(r#"{"words":"…","when":"…"}"#), "{h}");
        // A loop over queries in a variable: the shape, no value.
        let h = of(&[
            "bash",
            "-c",
            "for q in a b; do theseus memory index search -k 25 --json \"$q\"; done",
        ])
        .unwrap();
        assert!(h.contains(r#"{"words":"…","when":"…"}"#), "{h}");
        let h = hint(
            &json!({"steps": [{"argv": ["ls"]}, {"argv": ["theseus", "history", "ses_abc"]}]}),
            now(),
        )
        .unwrap();
        assert!(h.contains(r#"{"session":"ses_abc"}"#), "{h}");
    }

    #[test]
    fn sessions_and_their_rpcs_map_to_their_calls() {
        for argv in [
            &["theseus", "sessions"][..],
            &["theseus", "--json", "sessions", "list"],
        ] {
            let h = of(argv).unwrap();
            assert!(h.contains(r#"{"when":"last week"}"#), "{argv:?}: {h}");
        }
        let h = of(&[
            "theseus", "--socket", "/tmp/s", "history", "-n", "5", "ses_1",
        ])
        .unwrap();
        assert!(h.contains(r#"{"session":"ses_1"}"#), "{h}");
        let h = of(&["theseus", "rpc", "session.history", "{}"]).unwrap();
        assert!(h.contains(r#"{"session":"…"}"#), "{h}");
    }

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
        ] {
            assert_eq!(of(argv), None, "{argv:?}");
        }
        assert_eq!(hint(&json!({}), now()), None);
    }
}
