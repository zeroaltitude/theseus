//! The one seam through which the chat renders what a model wrote and what a
//! tool did (theseus-cyha). Nothing else in the chat lays out reply text or
//! words a tool call.
//!
//! Both functions are plain stand-ins, **replaced at term-render's and
//! tool-words' joins** (theseus-rn7s renders the reply's markdown, theseus-z6au
//! words each tool call): each join points its function here, about ten lines,
//! and nothing else in the chat changes. Until then:
//!
//! - [`reply_lines`] wraps the reply at word boundaries by display width, keeps
//!   a line's indentation on its continuations, and leaves blank lines and
//!   fenced code as the model wrote them (a code line is cut at the edge, never
//!   re-flowed).
//! - [`tool_line`] words a call from its input's `path`, `command`, `argv`,
//!   `url` or `query`: the verb for the tool, then what it touched.

use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The reply as lines no wider than `width` display columns. `ansi`: the
/// caller's terminal takes colour (a renderer that styles its output asks).
pub fn reply_lines(text: &str, width: usize, _ansi: bool) -> Vec<String> {
    // One column short of the terminal's: a line that fills the last column
    // leaves the terminal in a half-wrapped state some of them draw badly.
    let width = width.saturating_sub(1).max(8);
    let mut out = Vec::new();
    let mut fenced = false;
    for raw in text.trim_end().lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.extend(cut(line, width));
        } else if fenced {
            out.extend(cut(line, width));
        } else if line.trim().is_empty() {
            out.push(String::new());
        } else {
            out.extend(flow(line, width));
        }
    }
    out
}

/// A code line, cut at the edge into rows of at most `width` columns.
fn cut(line: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut w = 0;
    for c in line.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > width {
            rows.push(String::new());
            w = 0;
        }
        rows.last_mut().expect("a row").push(c);
        w += cw;
    }
    rows
}

/// A prose line, wrapped at spaces; a continuation keeps the line's indent
/// (and a list marker's width).
fn flow(line: &str, width: usize) -> Vec<String> {
    let lead = line.len() - line.trim_start().len();
    let body = line.trim_start();
    let marker = list_marker(body);
    let indent = lead + marker;
    let mut rows: Vec<String> = Vec::new();
    let mut cur = " ".repeat(lead);
    let mut w = lead;
    let mut fresh = true;
    for (i, word) in body.split(' ').enumerate() {
        let ww = word.width();
        if word.is_empty() {
            continue;
        }
        if !fresh && w + 1 + ww > width {
            rows.push(std::mem::replace(&mut cur, " ".repeat(indent)));
            w = indent;
            fresh = true;
        }
        if fresh && w + ww > width {
            // A word wider than a row (a path, a URL): cut it.
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if w + cw > width {
                    rows.push(std::mem::replace(&mut cur, " ".repeat(indent)));
                    w = indent;
                }
                cur.push(c);
                w += cw;
            }
            fresh = false;
            continue;
        }
        if !fresh {
            cur.push(' ');
            w += 1;
        }
        cur.push_str(word);
        w += ww;
        fresh = false;
        let _ = i;
    }
    rows.push(cur);
    rows
}

/// The width of a list marker at the start of `body` (`- `, `* `, `3. `), so
/// a wrapped item hangs under its text; 0 for anything else.
fn list_marker(body: &str) -> usize {
    let b = body.as_bytes();
    if b.len() > 1 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return 2;
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0
        && digits < 4
        && b.len() > digits + 1
        && b[digits] == b'.'
        && b[digits + 1] == b' '
    {
        return digits + 2;
    }
    0
}

/// A tool call in plain words: `Read README.md`, `Run ./survey.sh`. The
/// tool's name is the fallback verb, so no call is shown by a bare name when
/// its input says what it touched.
pub fn tool_line(tool: &str, input: &Value) -> String {
    let name = tool.replace('_', ".");
    let verb = match name.as_str() {
        "fs.read" => "Read",
        "fs.write" => "Write",
        "fs.edit" => "Edit",
        "fs.patch" => "Patch",
        "fs.list" | "fs.ls" => "List",
        "fs.glob" => "Find",
        "fs.grep" | "fs.search" => "Search",
        "proc.run" => "Run",
        "git.diff" => "Diff",
        "git.log" => "Log",
        "text.diff" => "Compare",
        "web.fetch" => "Fetch",
        "web.search" => "Search the web",
        "term.read" => "Read terminal",
        "job.read" => "Read job",
        "job.wait" => "Wait for job",
        "job.stop" => "Stop job",
        _ => "",
    };
    let target = target(input);
    match (verb.is_empty(), target.is_empty()) {
        (true, true) => tool.to_string(),
        (true, false) => format!("{tool} {target}"),
        (false, true) => verb.to_string(),
        (false, false) => format!("{verb} {target}"),
    }
}

/// What a call touched: its path, its command, its argv joined, its URL or
/// query, on one line.
fn target(input: &Value) -> String {
    for key in ["path", "file", "command", "url", "query", "name"] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            return one_line(s);
        }
    }
    if let Some(argv) = input.get("argv").and_then(Value::as_array) {
        let words: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
        if !words.is_empty() {
            return one_line(&words.join(" "));
        }
    }
    String::new()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_reply_wraps_at_words_by_display_width() {
        let rows = reply_lines(
            "The tides module predicts high and low water for a station.",
            20,
            false,
        );
        assert_eq!(
            rows,
            vec![
                "The tides module",
                "predicts high and",
                "low water for a",
                "station."
            ]
        );
        assert!(rows.iter().all(|r| r.width() <= 20));
    }

    #[test]
    fn wide_characters_count_two_columns() {
        let rows = reply_lines("潮汐 潮汐 潮汐 潮汐", 10, false);
        assert!(rows.iter().all(|r| r.width() <= 9), "{rows:?}");
        assert_eq!(rows, vec!["潮汐 潮汐", "潮汐 潮汐"]);
    }

    #[test]
    fn a_list_item_hangs_under_its_text_and_blank_lines_stay() {
        let rows = reply_lines("Intro\n\n- one two three four five", 13, false);
        assert_eq!(
            rows,
            vec!["Intro", "", "- one two", "  three four", "  five"]
        );
    }

    #[test]
    fn code_is_cut_at_the_edge_not_reflowed() {
        let rows = reply_lines("```\nlet a_long_name = 1;\n```", 11, false);
        assert_eq!(rows, vec!["```", "let a_long", "_name = 1;", "```"]);
    }

    #[test]
    fn a_word_wider_than_the_row_is_cut() {
        let rows = reply_lines("see /very/long/path/to/a/file.rs now", 13, false);
        assert!(rows.iter().all(|r| r.width() <= 12), "{rows:?}");
    }

    #[test]
    fn a_call_is_worded_by_what_it_touched() {
        assert_eq!(
            tool_line("fs.read", &json!({"path": "README.md"})),
            "Read README.md"
        );
        assert_eq!(
            tool_line("proc_run", &json!({"argv": ["python3", "slow_survey.py"]})),
            "Run python3 slow_survey.py"
        );
        assert_eq!(
            tool_line("proc.run", &json!({"command": "ls  -l\n"})),
            "Run ls -l"
        );
        assert_eq!(
            tool_line("memory.lookup", &json!({"query": "tides"})),
            "memory.lookup tides"
        );
        assert_eq!(tool_line("mystery", &json!({})), "mystery");
    }
}
