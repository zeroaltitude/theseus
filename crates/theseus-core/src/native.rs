//! The native tool a `proc.run` of the daemon's own CLI points to
//! (theseus-8c8s; issue 46): a job that runs `theseus memory search` or
//! `theseus memory index search` with words alone ends with a line naming
//! the history tool's own search, `context.history` by words: a search of
//! THIS place's history, which a `place` turns to another place's. It is not
//! the CLI's search (that reads the memory index, every place's), and the
//! line never says it is.
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
//! - **What it maps.** `memory search` and `memory index search` with words
//!   alone to `words`. A search the line's arguments cannot express gets no
//!   line: one that names a session, a source filter, a date or a tag
//!   (`--session`, `--sources`, `--as-of`, `--after`, `--before`, `--tag`),
//!   or whose words name a time. Nothing else gets a line either: the
//!   history tool opens no session by its id (`theseus history`, `rpc
//!   session.history`) and lists no sessions by a span (`theseus sessions`,
//!   `rpc import.sessions`), so a line for those would steer the model to a
//!   call that does not do the thing.

use serde_json::Value;

/// The tool the line names, as the model calls it.
const WIRE: &str = "context_history";

/// The line a result of `input`, a `proc.run` call's, ends with: the
/// history tool's search by the same words, as of `now_ms`. None when the
/// call is not a search by words alone.
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
        "[native: {WIRE} {call} searches every place's history by those words, in-process and \
         without a shell; give it a place to keep to one. It is not the CLI's search, \
         which reads the memory index: run the CLI when its results are what you need]"
    ))
}

/// The flags that narrow a search in a way the line's arguments cannot say: a session, a source filter, a
/// date, a tag. A call that names one gets no line.
const NARROWING: [&str; 6] = [
    "--session",
    "--sources",
    "--as-of",
    "--after",
    "--before",
    "--tag",
];

fn narrows(w: &str) -> bool {
    NARROWING
        .iter()
        .any(|f| w == *f || w.strip_prefix(f).is_some_and(|r| r.starts_with('=')))
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

/// The native call `rest`, the words after a `theseus`, asks for, as JSON: none when it narrows the search
/// in a way the call cannot say.
fn native_call(rest: &[String], now_ms: u64) -> Option<String> {
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while let Some(w) = rest.get(i) {
        if is_operator(w) {
            break;
        }
        if narrows(w) {
            return None;
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
    // A query in a shell variable (`$q`), or none, gives the call's shape. Words that name a time are a date
    // the history tool's words cannot say: read as free text (`when::read`), not by the explicit argument's
    // reader, which refuses a day beside a clock time ("yesterday at 14:00") that free text reads as the day.
    let query = query.trim();
    let shape = query.is_empty() || query.contains('$');
    let offset = |ms| crate::wake::local(ms).offset_secs;
    if !shape && crate::recall::when::read(query, now_ms, &offset).is_some() {
        return None;
    }
    let words = match shape {
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

    /// The line says what the call is, and only that (review §4): a search of every place's history by the
    /// words (words with no place read every place, issue 44), which a place keeps to one; never the same search
    /// as the CLI's, never this place's alone.
    #[test]
    fn the_line_says_what_the_call_is_and_no_more() {
        let h = of(&["theseus", "memory", "search", "kelp"]).unwrap();
        assert!(
            h.contains("searches every place's history by those words"),
            "{h}"
        );
        assert!(h.contains("give it a place to keep to one"), "{h}");
        assert!(h.contains("It is not the CLI's search"), "{h}");
        for claim in ["does this", "this place's history", "same search"] {
            assert!(!h.contains(claim), "{claim:?}: {h}");
        }
    }

    /// Free words that name a day and a clock time name a date the call's words cannot say: no line. They are
    /// read as free text (`recall::when::read`), as before the explicit argument's reader took hours.
    #[test]
    fn free_words_with_a_day_and_a_clock_time_get_no_line() {
        for q in ["yesterday at 14:00", "kelp notes March 2026 at 9:30"] {
            assert_eq!(of(&["theseus", "memory", "search", q]), None, "{q}");
        }
    }

    /// A search the line's arguments cannot express gets no line (review §4): a session, a source filter, a
    /// date or a tag named, as a flag or as `--flag=value`, or words that name a time.
    #[test]
    fn a_search_the_call_cannot_express_gets_no_line() {
        for argv in [
            &["theseus", "memory", "search", "--session", "ses_1", "kelp"][..],
            &["theseus", "memory", "search", "--session=ses_1", "kelp"],
            &[
                "theseus",
                "memory",
                "index",
                "search",
                "--sources",
                "bm25",
                "kelp",
            ],
            &[
                "theseus", "memory", "index", "search", "--as-of", "41", "kelp",
            ],
            &[
                "theseus",
                "memory",
                "search",
                "--after",
                "2026-03-01",
                "kelp",
            ],
            &["theseus", "memory", "search", "--before=2026-03-31", "kelp"],
            &[
                "theseus",
                "memory",
                "search",
                "--tag",
                "openclaw-2026-10",
                "kelp",
            ],
            &["theseus", "memory", "index", "search", "March 2026"],
            &[
                "theseus", "memory", "index", "search", "kelp", "notes", "in", "March", "2026",
            ],
            &[
                "bash",
                "-c",
                "theseus memory search --session ses_1 'kelp' | head",
            ],
        ] {
            assert_eq!(of(argv), None, "{argv:?}");
        }
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
        let h = hint(
            &json!({"steps": [{"argv": ["ls"]}, {"argv": ["theseus", "memory", "search", "kelp"]}]}),
            now(),
        )
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
        assert_eq!(hint(&json!({}), now()), None);
    }
}
