//! Addressing for the tools: a 1-based line and a symbol's text on it, made
//! an LSP position.
//!
//! Models count columns badly, so the tools never take one. They take the
//! line a symbol is on (1-based, as an editor and `fs.read` number it), the
//! symbol's text, and, when that text is on the line more than once, which
//! occurrence (1-based). [`locate`] turns those into a [`Position`] whose
//! column is in UTF-16 code units, as LSP requires by default: `é` is one
//! unit and two bytes, `𝔁` two units and four bytes.
//!
//! **Which matches count.** When the symbol starts and ends with an
//! identifier character (a letter, a digit, `_`, or `$`), only whole-word
//! matches count: `total` on `subtotal = total` is the second word, once.
//! When the line has no whole-word match, or the symbol is not word-shaped
//! (`->`, `::new`), every match counts. A symbol on the line twice, given no
//! occurrence, is refused as ambiguous, with the count: guessing would send
//! a rename to the wrong place.
//!
//! The reverse, [`line_text`] and [`byte_column`], render a server's answer
//! back as a line number and the text of that line.

use crate::types::Position;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocateError {
    #[error("line {line} is past the end: the file has {lines} lines")]
    NoSuchLine { line: u32, lines: u32 },
    #[error("lines are numbered from 1")]
    LineZero,
    #[error("an empty symbol")]
    EmptySymbol,
    #[error("`{symbol}` is not on line {line}: {text:?}")]
    NotOnLine {
        symbol: String,
        line: u32,
        text: String,
    },
    #[error("`{symbol}` is on line {line} {count} times: give an occurrence from 1 to {count}")]
    Ambiguous {
        symbol: String,
        line: u32,
        count: usize,
    },
    #[error("occurrence {occurrence} of `{symbol}` on line {line}: it is there {count} times")]
    NoSuchOccurrence {
        symbol: String,
        line: u32,
        occurrence: u32,
        count: usize,
    },
}

/// Where `symbol` starts on 1-based `line` of `text`, as an LSP position.
/// `occurrence` (1-based) picks among several matches, and is required when
/// there are several.
pub fn locate(
    text: &str,
    line: u32,
    symbol: &str,
    occurrence: Option<u32>,
) -> Result<Position, LocateError> {
    if line == 0 {
        return Err(LocateError::LineZero);
    }
    if symbol.is_empty() {
        return Err(LocateError::EmptySymbol);
    }
    let line0 = line - 1;
    let Some(row) = line_text(text, line0) else {
        return Err(LocateError::NoSuchLine {
            line,
            lines: line_count(text),
        });
    };
    let starts = matches(row, symbol);
    let start = match (starts.len(), occurrence) {
        (0, _) => {
            return Err(LocateError::NotOnLine {
                symbol: symbol.into(),
                line,
                text: crate::jsonrpc::clip(row.trim(), 200),
            })
        }
        (1, None) => starts[0],
        (count, None) => {
            return Err(LocateError::Ambiguous {
                symbol: symbol.into(),
                line,
                count,
            })
        }
        (count, Some(n)) => *n
            .checked_sub(1)
            .and_then(|i| starts.get(i as usize))
            .ok_or(LocateError::NoSuchOccurrence {
                symbol: symbol.into(),
                line,
                occurrence: n,
                count,
            })?,
    };
    Ok(Position::new(line0, utf16_column(row, start)))
}

/// The byte offsets where `symbol` starts on `row`: whole words when the
/// symbol is word-shaped and the row has any, else every match.
fn matches(row: &str, symbol: &str) -> Vec<usize> {
    let all: Vec<usize> = row.match_indices(symbol).map(|(i, _)| i).collect();
    let wordish = |c: Option<char>| c.is_some_and(is_ident);
    if !(wordish(symbol.chars().next()) && wordish(symbol.chars().last())) {
        return all;
    }
    let whole: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| {
            !wordish(row[..i].chars().next_back())
                && !wordish(row[i + symbol.len()..].chars().next())
        })
        .collect();
    if whole.is_empty() {
        all
    } else {
        whole
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The 0-based line `line0` of `text`, without its line ending. LSP ends a
/// line at `\n`, `\r\n`, or a lone `\r`; this reads all three.
pub fn line_text(text: &str, line0: u32) -> Option<&str> {
    lines(text).nth(line0 as usize)
}

/// How many lines `text` has, as an editor counts them (an empty file has
/// one, and a final newline does not start another).
pub fn line_count(text: &str) -> u32 {
    let n = lines(text).count().max(1);
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let s = rest?;
        match s.find(['\n', '\r']) {
            Some(i) => {
                let skip = if s[i..].starts_with("\r\n") { 2 } else { 1 };
                let tail = &s[i + skip..];
                rest = (!tail.is_empty()).then_some(tail);
                Some(&s[..i])
            }
            None => {
                rest = None;
                (!s.is_empty()).then_some(s)
            }
        }
    })
}

/// The UTF-16 column of byte offset `byte` on `row`.
pub fn utf16_column(row: &str, byte: usize) -> u32 {
    let units: usize = row[..byte.min(row.len())]
        .chars()
        .map(char::len_utf16)
        .sum();
    u32::try_from(units).unwrap_or(u32::MAX)
}

/// The byte offset on `row` of UTF-16 column `column`. A column inside a
/// surrogate pair, or past the end, is clamped (the spec says a column past
/// the end means the end).
pub fn byte_column(row: &str, column: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in row.char_indices() {
        if units >= column {
            return i;
        }
        units += u32::try_from(c.len_utf16()).unwrap_or(2);
        if units > column {
            return i;
        }
    }
    row.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str =
        "def total(items):\n    subtotal = total(items) + total([])\r\n    s = \"é𝔁\"; total = 1\n";

    #[test]
    fn a_word_on_its_line() {
        assert_eq!(locate(TEXT, 1, "total", None), Ok(Position::new(0, 4)));
        assert_eq!(locate(TEXT, 1, "items", None), Ok(Position::new(0, 10)));
    }

    #[test]
    fn whole_words_count_and_an_occurrence_picks() {
        // `subtotal` holds `total` but is not it.
        assert_eq!(
            locate(TEXT, 2, "total", None),
            Err(LocateError::Ambiguous {
                symbol: "total".into(),
                line: 2,
                count: 2
            })
        );
        assert_eq!(locate(TEXT, 2, "total", Some(1)), Ok(Position::new(1, 15)));
        assert_eq!(locate(TEXT, 2, "total", Some(2)), Ok(Position::new(1, 30)));
        assert!(matches!(
            locate(TEXT, 2, "total", Some(3)),
            Err(LocateError::NoSuchOccurrence { count: 2, .. })
        ));
        // Only a part of a word on the line: the part counts.
        assert_eq!(locate(TEXT, 2, "subt", None), Ok(Position::new(1, 4)));
        // Not word-shaped: every match.
        assert_eq!(locate(TEXT, 2, "(", Some(2)), Ok(Position::new(1, 35)));
    }

    #[test]
    fn columns_are_utf16_units_on_multibyte_text() {
        // `é` is 2 bytes and 1 unit, `𝔁` 4 bytes and 2 units.
        let p = locate(TEXT, 3, "total", None).unwrap();
        let row = line_text(TEXT, 2).unwrap();
        let byte = row.find("total").unwrap();
        assert_eq!(byte, 18);
        assert_eq!(p, Position::new(2, 15));
        assert_eq!(byte_column(row, p.character), byte);
        assert_eq!(utf16_column(row, row.find('𝔁').unwrap()), 10);
        // Inside the surrogate pair: clamped to the character's start.
        assert_eq!(byte_column(row, 11), row.find('𝔁').unwrap());
        assert_eq!(byte_column(row, 999), row.len());
        let p = locate("let 𝔁𝔁 = 𝔁𝔁 + 1;", 1, "𝔁𝔁", Some(2)).unwrap();
        assert_eq!(p.character, 11);
    }

    #[test]
    fn lines_end_three_ways_and_errors_say_why() {
        assert_eq!(line_text("a\r\nb\rc\n", 1), Some("b"));
        assert_eq!(line_text("a\r\nb\rc\n", 2), Some("c"));
        assert_eq!(line_text("a\r\nb\rc\n", 3), None);
        assert_eq!(line_count("a\nb\n"), 2);
        assert_eq!(line_count(""), 1);
        assert_eq!(
            locate(TEXT, 9, "x", None),
            Err(LocateError::NoSuchLine { line: 9, lines: 3 })
        );
        assert_eq!(locate(TEXT, 0, "x", None), Err(LocateError::LineZero));
        assert_eq!(locate(TEXT, 1, "", None), Err(LocateError::EmptySymbol));
        let e = locate(TEXT, 1, "missing", None).unwrap_err();
        assert!(e.to_string().contains("def total(items):"), "{e}");
    }
}
