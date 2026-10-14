//! The one seam through which the chat renders what a model wrote and what a
//! tool did (theseus-cyha). Nothing else in the chat lays out reply text or
//! words a tool call.
//!
//! [`reply_lines`] is term-render's renderer (theseus-rn7s) and [`tool_line`]
//! tool-words' summarizer (theseus-z6au), each pointed here at its join.

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

/// A tool call in words (tool-words' summarizer, theseus-z6au, pointed here at
/// its join): `Read README.md`, `Run `./survey.sh``, `Edit tides.py`, the one
/// wording every surface uses. A model's `fs_read` is the daemon's `fs.read`.
pub fn tool_line(tool: &str, input: &Value) -> String {
    theseus_protocol::term::words::summarize(&tool.replace('_', "."), input)
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
