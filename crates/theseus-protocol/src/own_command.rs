//! Theseus's own command line, answered with the tool that does the same (issue 28).
//!
//! A turn that wants to look at Theseus has tools for it (`look.theseus`, `config.show`, …) and
//! nevertheless runs `theseus health` in a shell: what it learns comes back as terminal text, the
//! command is judged as a shell command and not as the look it is, and the model never learns the
//! tool exists. **The daemon answers it**, before it runs, with the tool's name, and no prompt
//! text carries the rule. This file is the rule; the gate (`toolrun/own_command.rs`) is where it
//! is applied and where its walls are.
//!
//! What is answered, and nothing else:
//! - a call whose **only program is `theseus`**, spelled exactly so (a path, `theseusd`, `op`, an
//!   environment prefix, `cd x &&` are none of it);
//! - whose verb (and sub-verb) is a **look that has a tool** in [`VERBS`], and which names no
//!   operand and no flag but `--json`, `--plain` and `--no-stream`, so that a command that asks for
//!   more than the tool gives (another session's history, a stream) runs as it does today;
//! - alone, or piped into **pure filters of its output** and nothing else (the narrowest line
//!   that still covers what a model writes: `| head -N`, `| tail -N`, `| wc -l`, `| sort`, `| uniq`,
//!   `| grep PATTERN`, each with no operand that could name a file). Anything else, a second
//!   command, a redirection (but `2>&1` and `2>/dev/null`), a substitution, a glob, runs as it does.
//!
//! An act, a look with no tool, and a verb the table does not know, run as they do today. The table is
//! checked against the command tree by a test in the CLI crate: a new verb fails it until it is
//! classified here, and a verb that has a look with a tool must name every one of its sub-verbs.

use serde_json::Value;

/// What a verb does, and whether a turn has a tool for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    /// A look, and this is the tool a turn has for it (its dotted name).
    Look(&'static str),
    /// A look no tool does yet: it runs.
    Unserved,
    /// An act (or an interactive command): it runs, as it always did.
    Act,
}

/// One row: a verb, the sub-verbs it covers (`""` is the verb alone; none listed is the rest of the
/// verb), and what they are.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub verb: &'static str,
    pub subs: &'static [&'static str],
    pub way: Way,
}

const fn e(verb: &'static str, subs: &'static [&'static str], way: Way) -> Entry {
    Entry { verb, subs, way }
}

use Way::{Act, Look, Unserved};

/// The command tree, classified. Every verb of `theseus help all` has a row.
pub const VERBS: &[Entry] = &[
    // The looks that have a tool.
    e("health", &[], Look("look.theseus")),
    e("profile", &["", "list"], Look("look.theseus")),
    e("profile", &["use", "release", "unpin"], Act),
    e("profile", &["pins"], Unserved),
    e("profile", &[], Unserved),
    // `config show` prints a waiting change whole; the tool `config.show` reads settings by name.
    // They are different looks, so the command runs as before.
    e("config", &["show"], Unserved),
    e("config", &["", "history"], Look("config.history")),
    e("config", &[], Unserved),
    // `look.work` is Theseus's work whole, as `tasks` and `status` are. `history` bare is the most
    // recently active session's whole transcript (its tool calls and results too), which the tool does
    // not give, and `history SESSION` is another's: it runs.
    e("tasks", &[""], Look("look.work")),
    e("status", &[], Look("look.work")),
    // The looks no tool does yet.
    e("history", &[], Unserved),
    e("watch", &[], Unserved),
    e("tasks", &["forecast"], Unserved),
    e("tasks", &[], Unserved),
    e("agenda", &[], Unserved),
    e("digest", &[], Unserved),
    e("places", &[], Unserved),
    e("census", &[], Unserved),
    e("attach", &[], Unserved),
    e("peek", &[], Unserved),
    e("budgets", &[], Unserved),
    e("ledger", &[], Unserved),
    e("memory", &[], Unserved),
    e("model", &[], Unserved),
    e(
        "ontology",
        &["", "kinds", "categories", "proposals"],
        Look("look.theseus"),
    ),
    e("ontology", &["member", "never"], Unserved),
    e(
        "ontology",
        &[
            "topic", "undo", "kind", "guide", "accept", "person", "reject",
        ],
        Act,
    ),
    e("ontology", &[], Unserved),
    e("packs", &[], Unserved),
    e("persona", &[], Unserved),
    e("postmortem", &[], Unserved),
    e("reach", &[], Unserved),
    e("why", &[], Unserved),
    e("inspect", &[], Unserved),
    // `view show TURN` names a turn; `look.theseus` (`view`) reads this turn's view alone.
    e("view", &[], Unserved),
    e("judge", &[], Unserved),
    e("self", &[], Unserved),
    e("sessions", &[], Unserved),
    e("standing", &[], Unserved),
    e("tools", &[], Unserved),
    e("undo", &[], Unserved),
    e("wait", &[], Unserved),
    e("watch-for", &[], Unserved),
    e("wake", &[], Unserved),
    e("routine", &[], Unserved),
    e("project", &[], Unserved),
    // The owner's own instruction file: the show runs, and the accept is the owner's act at a terminal.
    e(crate::own_file::COMMAND, &["accept"], Act),
    e(crate::own_file::COMMAND, &[], Unserved),
    e("thread", &[], Unserved),
    e("card", &[], Unserved),
    e("extend", &[], Unserved),
    e("google", &[], Unserved),
    e("import", &[], Unserved),
    e("mcp", &[], Unserved),
    e("policy", &[], Unserved),
    e("aws", &[], Unserved),
    e("help", &[], Unserved),
    // The acts, and the commands that are a screen.
    e("ask", &[], Act),
    e("prompt", &[], Act),
    e("answer", &[], Act),
    // A note's Undo, from a terminal (`told.undo`): the owner's act.
    e("told", &[], Act),
    e("stop", &[], Act),
    e("remind", &[], Act),
    e("cancel", &[], Act),
    e("dismiss", &[], Act),
    e("retry", &[], Act),
    e("fork", &[], Act),
    e("rewind", &[], Act),
    e("publish", &[], Act),
    e("retract", &[], Act),
    e("content", &[], Act),
    e("mode", &[], Act),
    e("owner", &[], Act),
    e("vault", &[], Act),
    e("task", &[], Act),
    e("rpc", &[], Act),
    e("shutdown", &[], Act),
    e("herdr", &[], Act),
    e("tui", &[], Act),
    e("chat", &[], Act),
    e("open", &[], Act),
];

/// The tool a look has, and the words that named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// The tool's dotted name.
    pub tool: &'static str,
    /// The command as it reads without its flags: `health`, `config show`.
    pub verb: String,
}

/// The global flags a command may carry and still be answered: they change how the text is drawn,
/// and nothing about what is read.
const FLAGS: [&str; 3] = ["--json", "--plain", "--no-stream"];

/// The way of `verb sub` in the table: the row whose sub-verbs name `sub` (`""` for none), else the
/// verb's row for the rest.
pub fn way_of(verb: &str, sub: &str) -> Option<Way> {
    let rows = || VERBS.iter().filter(|r| r.verb == verb);
    rows()
        .find(|r| r.subs.contains(&sub))
        .or_else(|| rows().find(|r| r.subs.is_empty()))
        .map(|r| r.way)
}

/// What `words` (a program and its arguments) is answered with, when it is a look that has a tool.
pub fn answer_words(words: &[String]) -> Option<Answered> {
    let (program, args) = words.split_first()?;
    if program != "theseus" {
        return None;
    }
    let mut positional = Vec::new();
    for a in args {
        if a.starts_with('-') {
            if !FLAGS.contains(&a.as_str()) {
                return None;
            }
        } else {
            positional.push(a.as_str());
        }
    }
    let verb = *positional.first()?;
    let sub = positional.get(1).copied().unwrap_or("");
    let Some(Way::Look(tool)) = way_of(verb, sub) else {
        return None;
    };
    // A look with a tool takes no operand: `profile list` is the list; `profile list x` is not.
    let named = usize::from(
        !sub.is_empty()
            && VERBS
                .iter()
                .any(|r| r.verb == verb && r.subs.contains(&sub)),
    );
    if positional.len() > 1 + named {
        return None;
    }
    let verb = positional[..1 + named].join(" ");
    Some(Answered { tool, verb })
}

/// What a `proc.run` input is answered with, when it is.
pub fn answer_input(input: &Value) -> Option<Answered> {
    // Anything beyond the one program (steps, an environment) runs as it does.
    if input.get("steps").is_some_and(|s| !s.is_null())
        || input
            .get("env")
            .and_then(Value::as_object)
            .is_some_and(|m| !m.is_empty())
    {
        return None;
    }
    match (input.get("argv"), input.get("command")) {
        (Some(Value::Array(a)), None | Some(Value::Null)) => {
            let words: Vec<String> = a
                .iter()
                .map(|w| w.as_str().map(str::to_string))
                .collect::<Option<_>>()?;
            // A line run in bash reaches the gate as `bash -c LINE` (`proc.run`'s own `command`,
            // as the fitting of a call makes it): the line is read, and nothing else about bash.
            match words.as_slice() {
                [sh, c, line] if sh == "bash" && c == "-c" => answer_line(line),
                _ => answer_words(&words),
            }
        }
        (None | Some(Value::Null), Some(Value::String(line))) => answer_line(line),
        _ => None,
    }
}

/// What a line run in bash is answered with: `theseus …`, and pure filters of its output.
pub fn answer_line(line: &str) -> Option<Answered> {
    let stages = stages(line)?;
    let (first, filters) = stages.split_first()?;
    let answered = answer_words(first)?;
    filters.iter().all(|f| pure_filter(f)).then_some(answered)
}

/// The words of each stage of a pipeline, or none when the line holds anything but plain words
/// (quoted or not), `|` between stages, and the two redirections that only choose where a
/// diagnostic goes (`2>&1`, `2>/dev/null`). A substitution, a glob, a second command, a
/// background, an assignment's `$`, a comment, a backslash: the line is not read.
fn stages(line: &str) -> Option<Vec<Vec<String>>> {
    let (mut stages, mut words) = (Vec::new(), Vec::new());
    let (mut word, mut bad, mut open) = (String::new(), false, false);
    let mut chars = line.trim().chars().peekable();
    let end = |word: &mut String, bad: &mut bool, open: &mut bool, words: &mut Vec<String>| {
        if *open {
            let w = std::mem::take(word);
            match (*bad, w.as_str()) {
                (true, "2>&1" | "2>/dev/null") => {}
                (true, _) => return false,
                (false, _) => words.push(w),
            }
            *bad = false;
            *open = false;
        }
        true
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                open = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                open = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '$' | '`' | '\\' | '!' => return None,
                        c => word.push(c),
                    }
                }
            }
            c if c.is_whitespace() => {
                if c == '\n' || c == '\r' {
                    return None;
                }
                if !end(&mut word, &mut bad, &mut open, &mut words) {
                    return None;
                }
            }
            '|' => {
                if chars.peek() == Some(&'|') || !end(&mut word, &mut bad, &mut open, &mut words) {
                    return None;
                }
                if words.is_empty() {
                    return None;
                }
                stages.push(std::mem::take(&mut words));
            }
            ';' | '&' | '<' | '>' | '(' | ')' | '`' | '$' | '\\' | '*' | '?' | '[' | ']' | '{'
            | '}' | '~' | '#' | '!' => {
                bad = true;
                open = true;
                word.push(c);
            }
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    if !end(&mut word, &mut bad, &mut open, &mut words) || words.is_empty() {
        return None;
    }
    stages.push(words);
    Some(stages)
}

/// Whether a stage is a pure filter of the text the command before it wrote: a program that reads
/// its input and names no file to read. The list is the narrowest that covers what a model writes
/// after a look: `head`, `tail`, `wc`, `sort`, `uniq`, and `grep` with one pattern.
fn pure_filter(words: &[String]) -> bool {
    let Some((program, args)) = words.split_first() else {
        return false;
    };
    let number = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
    match program.as_str() {
        // `-n N`, `-N`, `-c N`: no file operand.
        "head" | "tail" => {
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-n" | "-c" => {
                        if !it.next().is_some_and(|n| number(n)) {
                            return false;
                        }
                    }
                    f if f.strip_prefix('-').is_some_and(number) => {}
                    _ => return false,
                }
            }
            true
        }
        "wc" | "sort" | "uniq" => args.iter().all(|a| {
            let flags = a.strip_prefix('-').unwrap_or("");
            !a.starts_with("--")
                && !flags.is_empty()
                && flags.chars().all(|c| "lwcmnrufi".contains(c))
        }),
        // One pattern and a few flags that read nothing (no `-r`, `-f`, `-e`, no second operand).
        "grep" => {
            let (mut flags, mut operands) = (true, 0);
            for a in args {
                match a.strip_prefix('-') {
                    Some(f) if flags && !f.is_empty() && !a.starts_with("--") => {
                        if !f.chars().all(|c| "ivcEFnwoqs".contains(c)) {
                            return false;
                        }
                    }
                    _ => {
                        flags = false;
                        operands += 1;
                    }
                }
            }
            operands == 1
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn said(line: &str) -> Option<(&'static str, String)> {
        answer_input(&json!({"command": line})).map(|a| (a.tool, a.verb))
    }

    /// A look that has a tool is answered, with its flags and with what a model pipes it into.
    #[test]
    fn a_look_that_has_a_tool_is_answered_alone_and_through_pure_filters() {
        let health = Some(("look.theseus", "health".to_string()));
        for line in [
            "theseus health",
            "theseus --json health",
            "theseus health --json | head -20",
            "theseus health 2>&1",
            "theseus health 2>/dev/null | grep provider | wc -l",
            "theseus health | tail -n 5",
            "theseus health | sort | uniq",
            "theseus  health   |  head -3",
            "theseus 'health'",
        ] {
            assert_eq!(said(line), health, "{line}");
        }
        for (line, verb) in [
            ("theseus ontology", "ontology"),
            ("theseus ontology kinds", "ontology kinds"),
            ("theseus ontology categories", "ontology categories"),
            ("theseus ontology proposals", "ontology proposals"),
        ] {
            assert_eq!(
                said(line),
                Some(("look.theseus", verb.to_string())),
                "{line}"
            );
        }
        for line in ["theseus tasks", "theseus status"] {
            assert_eq!(said(line).map(|a| a.0), Some("look.work"), "{line}");
        }
        assert_eq!(
            said("theseus profile"),
            Some(("look.theseus", "profile".into()))
        );
        assert_eq!(
            said("theseus profile list"),
            Some(("look.theseus", "profile list".into()))
        );
        // The command prints a waiting change whole; the tool `config.show` reads settings by name.
        // It is not that look, so it is not answered with that tool and runs as it did.
        assert_eq!(said("theseus config show"), None);
        assert_eq!(
            said("theseus config history"),
            Some(("config.history", "config history".into()))
        );
        assert_eq!(
            said("theseus config"),
            Some(("config.history", "config".into()))
        );
        assert_eq!(
            answer_input(&json!({"argv": ["theseus", "--json", "health"]})).map(|a| a.tool),
            Some("look.theseus")
        );
        // As the gate sees a `command`: bash and the line.
        assert_eq!(
            answer_input(&json!({"argv": ["bash", "-c", "theseus health | head -3"]}))
                .map(|a| a.tool),
            Some("look.theseus")
        );
    }

    /// An act, a look with no tool, a verb the table does not know, and an operand or a flag the
    /// tool does not give: run as they do.
    #[test]
    fn what_has_no_tool_or_asks_for_more_than_the_tool_gives_runs() {
        for line in [
            "theseus profile use glm",
            "theseus profile use",
            "theseus profile unpin",
            "theseus profile unpin glm",
            "theseus profile release",
            "theseus profile pins",
            "theseus ask hello",
            "theseus ask -P mythos hello",
            "theseus health --watch",
            "theseus tasks forecast",
            "theseus history",
            "theseus history ses_other",
            "theseus ontology proposals --each",
            "theseus ontology person add Ada",
            "theseus ontology member ses_x +topic:y",
            "theseus ontology accept jdg_1",
            "theseus ontology topic",
            "theseus ontology undo",
            "theseus ontology kind",
            "theseus ontology guide",
            "theseus ontology accept",
            "theseus ontology person",
            "theseus ontology reject",
            "theseus ontology member",
            "theseus ontology never",
            "theseus health extra",
            "theseus profile list x",
            "theseus config show x",
            "theseus judge log",
            "theseus nonsense",
            "theseus",
            "theseus --json",
            "theseus --socket /x health",
            "theseus -P x health",
        ] {
            assert_eq!(said(line), None, "{line}");
        }
    }

    /// It never answers a command that is not Theseus's own, and never one with anything else in it.
    #[test]
    fn a_command_that_is_not_theseuss_own_or_holds_anything_else_runs() {
        for line in [
            "theseusd health",
            "/usr/local/bin/theseus health",
            "./theseus health",
            "op whoami",
            "echo theseus health",
            "FOO=1 theseus health",
            "cd /tmp && theseus health",
            "theseus health; rm -rf x",
            "theseus health && echo done",
            "theseus health || true",
            "theseus health &",
            "theseus health > out.txt",
            "theseus health 2> err.txt",
            "theseus health | tee out.txt",
            "theseus health | xargs rm",
            "theseus health | sh",
            "theseus health | bash -c x",
            "theseus health | head file.txt",
            "theseus health | grep -r provider",
            "theseus health | grep -f pats provider",
            "theseus health | grep a b",
            "theseus health | cat",
            "theseus health | awk '{print}'",
            "theseus health | sort other.txt",
            "theseus $(echo health)",
            "theseus `echo health`",
            "theseus \"$X\"",
            "theseus *",
            "theseus hea*",
            "theseus health | | head",
            "theseus health |",
            "| theseus health",
            "theseus health\ntheseus ask x",
            "theseus 'health",
            "theseus health # a note",
            "bash -c 'theseus health'",
            "",
        ] {
            assert_eq!(said(line), None, "{line:?}");
        }
        for input in [
            json!({"argv": ["theseusd", "health"]}),
            json!({"argv": ["theseus", "health"], "env": {"THESEUS_SOCKET": "/x"}}),
            json!({"argv": ["bash", "-c", "theseus health; echo x"]}),
            json!({"argv": ["bash", "-c", "theseus health", "extra"]}),
            json!({"argv": ["bash", "-lc", "theseus health"]}),
            json!({"argv": ["sh", "-c", "theseus health"]}),
            json!({"steps": [{"argv": ["theseus", "health"]}]}),
            json!({"argv": ["theseus", "health"], "command": "theseus health"}),
            json!({"argv": ["theseus", 1]}),
            json!({}),
        ] {
            assert_eq!(answer_input(&input), None, "{input}");
        }
    }

    /// The table: a verb is classified once for each sub-verb it names, and the rest of it has a row.
    #[test]
    fn the_table_names_each_verb_once_and_each_look_with_a_tool_names_its_subs() {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for r in VERBS {
            for s in r.subs {
                assert!(!seen.contains(&(r.verb, s)), "{} {s:?} twice", r.verb);
                seen.push((r.verb, s));
            }
        }
        for v in VERBS.iter().map(|r| r.verb) {
            assert!(
                VERBS.iter().any(|r| r.verb == v && r.subs.is_empty()),
                "{v} has no row for the rest of it"
            );
        }
        for r in VERBS.iter().filter(|r| matches!(r.way, Look(_))) {
            let tool = match r.way {
                Look(t) => t,
                _ => unreachable!(),
            };
            assert!(tool.contains('.'), "{tool} is a dotted tool name");
        }
    }
}
