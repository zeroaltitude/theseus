//! The one seam through which the chat renders what a model wrote and what a
//! tool did (theseus-cyha). Nothing else in the chat lays out reply text or
//! words a tool call.
//!
//! [`reply_lines`] is term-render's renderer (theseus-rn7s, pointed here at its
//! join). [`tool_line`] is a plain stand-in, **replaced at tool-words' join**
//! (theseus-z6au words each tool call): that join points it here, about ten
//! lines, and nothing else in the chat changes. Until then:
//!
//! - [`tool_line`] words a call from its input's `path`, `command`, `argv`,
//!   `url` or `query`: the verb for the tool, then what it touched.

use serde_json::Value;
use theseus_client::render::md::Markdown;
use theseus_client::render::style::Mode;

/// The reply as lines no wider than `width` display columns, drawn by the
/// CLI's markdown renderer (term-render's, theseus-rn7s: headings, emphasis,
/// lists, tables, highlighted code), which keeps prose one column short of the
/// edge. `ansi`: the caller's terminal takes colour; else no escape at all.
pub fn reply_lines(text: &str, width: usize, ansi: bool) -> Vec<String> {
    let mode = if ansi { Mode::Ansi } else { Mode::Plain };
    Markdown::render(text.trim_end(), width.max(8), mode)
        .iter()
        .map(|l| if ansi { l.ansi() } else { l.plain() })
        .collect()
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
    use unicode_width::UnicodeWidthStr;

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
