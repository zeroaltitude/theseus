//! A subagent's answer as a message of its own (issue 28): the asked model's words as the body
//! and a footer in a reply's shape, apart from `render.rs` for the shape budget's file ceiling.
//!
//! The footer's words are `SubagentAnswer::footer_bits`, the one place they are chosen, so the
//! terminal, the terminal UI and the cockpit say what this says: the model part names the profile
//! and the model that answered, never the turn's that asked.

use theseus_protocol::SubagentAnswer;

use super::{fence_tables, split_text, DISCORD_LIMIT, PART_LIMIT};

/// The message's words as it opens: the icon before the first word, and the words with their
/// tables drawn as blocks (Discord draws none). The icon stands on a line of its own when a block
/// comes first, so the fence opens its line.
fn opening(a: &SubagentAnswer) -> String {
    let words = fence_tables(a.text.trim());
    let icon = theseus_protocol::subagent::SUBAGENT_ICON;
    match words.starts_with("```") {
        true => format!("{icon}\n{words}"),
        false => format!("{icon} {words}"),
    }
}

/// The message's parts, in order, under the call's own keys: its text, then the footer riding on
/// the last part when it fits, else standing alone.
pub fn subagent_parts(a: &SubagentAnswer) -> Vec<(String, String)> {
    let key = format!("subagent:{}", a.call);
    let mut out: Vec<(String, String)> = split_text(&opening(a), PART_LIMIT)
        .into_iter()
        .enumerate()
        .map(|(i, part)| (format!("{key}:p{i}"), part))
        .collect();
    let footer = format!("-# {}", a.footer());
    match out.last_mut() {
        Some((_, c)) if c.len() + footer.len() < DISCORD_LIMIT => {
            c.push('\n');
            c.push_str(&footer);
        }
        _ => out.push((format!("{key}:footer"), footer)),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(text: &str) -> SubagentAnswer {
        SubagentAnswer {
            profile: "haiku".into(),
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
            asked_by: "sonnet".into(),
            text: text.into(),
            cost_usd: Some(0.0031),
            elapsed_ms: 1_300,
            output_tokens: 12,
            cut: false,
            call: "act_1".into(),
            outcome: String::new(),
        }
    }

    /// The words, then a footer of a reply's shape that names the model that answered and says
    /// it is a subagent's.
    #[test]
    fn the_footer_rides_the_last_part_and_names_the_model_that_answered() {
        let parts = subagent_parts(&answer("Hello."));
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "subagent:act_1:p0");
        assert_eq!(
            parts[0].1,
            "🛰️ Hello.\n-# haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s"
        );
    }

    /// Words that fill a message leave the footer a message of its own, and every part is under
    /// Discord's limit.
    #[test]
    fn a_long_answer_is_split_and_its_footer_stands_alone_when_it_does_not_fit() {
        let long = "word ".repeat(1_000);
        let parts = subagent_parts(&answer(&long));
        assert!(parts.len() >= 3, "{}", parts.len());
        assert!(parts.iter().all(|(_, c)| c.len() < DISCORD_LIMIT));
        let all: String = parts.iter().map(|(_, c)| c.as_str()).collect();
        assert!(all.contains("-# haiku · claude-haiku-5-5 · subagent answer"));
        assert_eq!(
            parts.iter().filter(|(_, c)| c.contains("-# haiku")).count(),
            1
        );
    }

    fn rows(n: usize) -> String {
        let mut t = String::from("| # | File | Bytes |\n|--:|------|------:|\n");
        for i in 0..n {
            t.push_str(&format!("| {i} | src/module_{i}.rs | {} |\n", i * 1024));
        }
        t
    }

    /// A table in a subagent's words is a fenced block under its icon; the icon stands on its own
    /// line when the block comes first, and goes before the first word when words come first. The
    /// footer is still the last line.
    #[test]
    fn a_table_in_the_words_is_a_block_under_the_icon() {
        let only = subagent_parts(&answer("| a | b |\n|---|---|\n| 1 | 2 |"));
        assert_eq!(only.len(), 1);
        assert!(
            only[0]
                .1
                .starts_with("🛰️\n```\na  b\n-  -\n1  2\n```\n-# haiku"),
            "{}",
            only[0].1
        );
        let after = subagent_parts(&answer("Sizes:\n\n| a | b |\n|---|---|\n| 1 | 2 |"));
        assert!(
            after[0].1.starts_with("🛰️ Sizes:\n\n```\na  b"),
            "{}",
            after[0].1
        );
        assert!(!after[0].1.contains("|---|"), "{}", after[0].1);
    }

    /// A wide table keeps its columns aligned: every row starts each column where the header does.
    #[test]
    fn a_wide_table_keeps_its_columns_aligned() {
        let wide = "| name | description of the thing | a very long third column indeed | n |\n|---|---|---|--:|\n| a | short | x | 1 |\n| bbbbbbbbbbbbbbbb | a rather longer description than the header | yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy | 22222 |";
        let parts = subagent_parts(&answer(wide));
        let block: Vec<&str> = parts[0].1.lines().skip(2).take(4).collect();
        // The gaps between columns are where the rule's are, in every line.
        let gaps: Vec<usize> = block[1].match_indices("  ").map(|(i, _)| i).collect();
        assert_eq!(gaps.len(), 3, "{block:#?}");
        for line in &block {
            let chars: Vec<char> = line.chars().collect();
            for g in &gaps {
                assert!(chars.get(*g).is_none_or(|c| *c == ' '), "{block:#?}");
            }
        }
        assert!(block[3].contains("a rather longer description than the header"));
        assert!(block[3].ends_with("22222"), "{block:#?}");
    }

    /// A table longer than a part is cut at a row boundary, the fence closed at the end of the part
    /// and reopened at the next: no row is split, no fence is left open, no part is over the limit,
    /// and every row is there once.
    #[test]
    fn a_table_across_a_part_boundary_is_cut_at_a_row_and_its_fence_repaired() {
        let parts = subagent_parts(&answer(&rows(150)));
        assert!(parts.len() >= 3, "{}", parts.len());
        let all: Vec<&str> = parts.iter().flat_map(|(_, c)| c.lines()).collect();
        for (_, c) in &parts {
            assert!(c.len() < DISCORD_LIMIT, "{}", c.len());
            assert_eq!(c.matches("```").count() % 2, 0, "an open fence: {c}");
        }
        for i in 0..150 {
            let name = format!("src/module_{i}.rs");
            let hits: Vec<&&str> = all
                .iter()
                .filter(|l| l.contains(&format!("{name} ")) || l.ends_with(&name))
                .collect();
            assert_eq!(hits.len(), 1, "{name}: {hits:?}");
            let line = hits[0];
            assert!(
                line.trim_end().ends_with(&(i * 1024).to_string()),
                "a row cut short: {line}"
            );
        }
        assert!(all.last().unwrap().starts_with("-# haiku"));
    }

    /// Pipes that are not a table are the words as they were.
    #[test]
    fn pipes_that_are_not_a_table_are_not_touched() {
        let parts = subagent_parts(&answer("Use `a | b` here, and x | y there.\n\nrun ls | wc"));
        assert!(
            parts[0]
                .1
                .starts_with("🛰️ Use `a | b` here, and x | y there.\n\nrun ls | wc\n-# "),
            "{}",
            parts[0].1
        );
        assert!(!parts[0].1.contains("```"));
    }
}
