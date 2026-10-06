//! A reply split at sentence ends (design §2.8), so its first audio starts
//! after the first sentence is synthesized, not after the whole reply; and
//! made speakable first (theseus-rkvl): what was written for a screen, its
//! markdown, tables, code and addresses, is not read aloud.

/// Said for a run of table rows: the table stays in the text channel.
pub const TABLE: &str = "There's a table in the text channel.";
/// Said for a fenced code block.
pub const CODE: &str = "There's code in the text channel.";

/// `text` as it is spoken, a sentence at a time: emphasis markers (`**`,
/// `__`, and `*` or `_` at a word's edge, so snake_case stays), backticks,
/// heading marks, quote marks, and bullets and list numbers at a line's
/// start stripped; a link read as its label, a bare web address dropped; a
/// run of table rows one sentence, [`TABLE`], and a fenced code block one,
/// [`CODE`]; then split as [`sentences`] splits. The text channel keeps the
/// reply as it was written.
pub fn speakable(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut spoken: Vec<String> = Vec::new();
    let mut fenced = false;
    let mut table = false;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !fenced {
                spoken.push(CODE.to_string());
            }
            fenced = !fenced;
            table = false;
            continue;
        }
        if fenced {
            continue;
        }
        let row = is_separator(trimmed)
            || (trimmed.starts_with('|') && trimmed.len() > 1)
            || (trimmed.contains('|')
                && (table || lines.get(i + 1).is_some_and(|l| is_separator(l.trim()))));
        if row {
            if !table {
                spoken.push(TABLE.to_string());
            }
            table = true;
            continue;
        }
        table = false;
        spoken.push(plain(trimmed));
    }
    sentences(&spoken.join("\n"))
}

/// A table's separator row: `|---|:--:|`, its pipes optional but one.
fn is_separator(line: &str) -> bool {
    line.contains('-')
        && line.contains('|')
        && line
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// One line, its marks stripped.
fn plain(line: &str) -> String {
    let mut rest = line;
    // Quote marks, then a heading's, then a bullet or a list number.
    loop {
        let r = rest.trim_start();
        match r.strip_prefix('>') {
            Some(after) => rest = after,
            None => {
                rest = r;
                break;
            }
        }
    }
    let hashes = rest.chars().take_while(|&c| c == '#').count();
    if hashes > 0 && rest[hashes..].starts_with([' ', '\t']) {
        rest = rest[hashes..].trim_start();
    }
    for bullet in ["- ", "* ", "+ ", "• "] {
        if let Some(after) = rest.strip_prefix(bullet) {
            rest = after.trim_start();
            break;
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with(['.', ')']) {
        let after = &rest[digits + 1..];
        if after.starts_with([' ', '\t']) {
            rest = after.trim_start();
        }
    }
    let unlinked = links(rest);
    emphasis(&unlinked)
}

/// Links as their labels, bare web addresses dropped.
fn links(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // `[label](address)`: the label.
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']').map(|p| p + i) {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close..].iter().position(|&c| c == ')') {
                        out.extend(&chars[i + 1..close]);
                        i = close + end + 1;
                        continue;
                    }
                }
            }
        }
        // `<https://…>` or a bare `https://…`: dropped.
        let at: String = chars[i..chars.len().min(i + 9)].iter().collect();
        let opens =
            chars[i] == '<' && (at[1..].starts_with("http://") || at[1..].starts_with("https://"));
        if opens
            || at.starts_with("http://")
            || at.starts_with("https://")
            || at.starts_with("www.")
        {
            let mut end = chars[i..]
                .iter()
                .position(|c| c.is_whitespace() || (!opens && *c == ')') || (opens && *c == '>'))
                .map_or(chars.len(), |p| p + i);
            // Punctuation after a bare address ends the sentence, not it.
            while !opens && end > i && matches!(chars[end - 1], '.' | ',' | '!' | '?' | ';' | ':') {
                end -= 1;
            }
            i = if opens {
                (end + 1).min(chars.len())
            } else {
                end
            };
            // A sentence's end that followed the address stays.
            while out.ends_with(' ')
                && chars
                    .get(i)
                    .is_some_and(|c| matches!(c, '.' | ',' | '!' | '?' | ';' | ':'))
            {
                out.pop();
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Emphasis markers and backticks stripped: `**`, `__`, and a `*` or `_`
/// with a word on one side only, so snake_case and `2 * 3` stay.
fn emphasis(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            i += 1;
            continue;
        }
        if matches!(c, '*' | '_') {
            let run = chars[i..].iter().take_while(|&&d| d == c).count();
            let before = word(i.checked_sub(1).and_then(|b| chars.get(b)));
            let after = word(chars.get(i + run));
            if run >= 2 || before != after {
                i += run;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Split `text` into sentences: after `.`, `!`, `?`, or `…` (and any closing
/// quotes or brackets) when whitespace follows and the next word doesn't
/// start in lower case ("e.g. this" stays whole), and at every line break.
/// Empty pieces are dropped, and each piece is trimmed.
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut start = 0;
        let mut i = 0;
        while i < chars.len() {
            if !matches!(chars[i], '.' | '!' | '?' | '…') {
                i += 1;
                continue;
            }
            // The whole run of ends and closers: "?!", "...", ".)", "!\"".
            let mut end = i + 1;
            while end < chars.len()
                && matches!(
                    chars[end],
                    '.' | '!' | '?' | '…' | '"' | '\'' | '”' | '’' | ')' | ']'
                )
            {
                end += 1;
            }
            let next_word = chars[end..].iter().position(|c| !c.is_whitespace());
            let breaks = match next_word {
                // The line's end.
                None => true,
                // No whitespace after it: "3.14", "v1.2".
                Some(0) => false,
                Some(n) => !chars[end + n].is_lowercase(),
            };
            if breaks {
                push(&mut out, &chars[start..end]);
                start = end;
            }
            i = end;
        }
        push(&mut out, &chars[start..]);
    }
    out
}

fn push(out: &mut Vec<String>, piece: &[char]) {
    let piece: String = piece.iter().collect();
    let piece = piece.trim();
    if !piece.is_empty() {
        out.push(piece.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{sentences, speakable, CODE, TABLE};

    #[test]
    fn a_reply_splits_at_its_sentence_ends() {
        assert_eq!(
            sentences("Done. The build passed! Want the log? Here it is…"),
            ["Done.", "The build passed!", "Want the log?", "Here it is…"]
        );
    }

    #[test]
    fn numbers_abbreviations_and_closers_stay_whole() {
        assert_eq!(
            sentences("Pi is 3.14, e.g. close. He said \"stop.\" Then (quietly.) Left"),
            [
                "Pi is 3.14, e.g. close.",
                "He said \"stop.\"",
                "Then (quietly.)",
                "Left"
            ]
        );
        assert_eq!(sentences("Use v1.2 now. Ok"), ["Use v1.2 now.", "Ok"]);
        assert_eq!(sentences("Really?! Yes..."), ["Really?!", "Yes..."]);
    }

    #[test]
    fn line_breaks_split_and_blank_pieces_drop() {
        assert_eq!(
            sentences("First line\n\n- a list item\n  . \nlast"),
            ["First line", "- a list item", ".", "last"]
        );
        assert!(sentences("  \n ").is_empty());
    }

    #[test]
    fn emphasis_markers_and_backticks_are_not_said_and_snake_case_stays() {
        assert_eq!(
            speakable(
                "**Spend** rose, __twice__ as *fast* as _planned_. Run `git_log` on my_file."
            ),
            [
                "Spend rose, twice as fast as planned.",
                "Run git_log on my_file."
            ]
        );
        assert_eq!(speakable("Two * three is six."), ["Two * three is six."]);
    }

    #[test]
    fn heading_and_quote_marks_bullets_and_list_numbers_go() {
        assert_eq!(
            speakable("## Summary\n> Quoted here.\n- first item\n* second item\n1. One step\n12) Two steps"),
            ["Summary", "Quoted here.", "first item", "second item", "One step", "Two steps"]
        );
        // A number that opens a sentence stays.
        assert_eq!(
            speakable("3.5 hours, then #2 is next."),
            ["3.5 hours, then #2 is next."]
        );
    }

    #[test]
    fn a_link_is_its_label_and_a_bare_address_goes() {
        assert_eq!(
            speakable("See [the build log](https://ci.example.test/run/7) for it. Or https://ci.example.test/run/7."),
            ["See the build log for it.", "Or."]
        );
        assert_eq!(
            speakable("Docs at <https://docs.example.test/a> and www.example.test today."),
            ["Docs at and today."]
        );
    }

    #[test]
    fn a_tables_rows_are_one_sentence() {
        let text = "Here it is.\n| Day | Spend |\n|---|---:|\n| Mon | $4.10 |\n| Tue | $3.90 |\nMonday was the most.";
        assert_eq!(
            speakable(text),
            ["Here it is.", TABLE, "Monday was the most."]
        );
        // Without its outer pipes.
        let bare = "Day | Spend\n--- | ---\nMon | 4\nDone.";
        assert_eq!(speakable(bare), [TABLE, "Done."]);
    }

    #[test]
    fn a_fenced_code_block_is_one_sentence() {
        let text = "Run this:\n```sh\ncargo build --release\n# not a heading\n```\nThen wait.";
        assert_eq!(speakable(text), ["Run this:", CODE, "Then wait."]);
    }
}
