//! A reply split at sentence ends (design §2.8), so its first audio starts
//! after the first sentence is synthesized, not after the whole reply.

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
    use super::sentences;

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
}
