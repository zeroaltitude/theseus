//! A regular expression match with a bound on what it may cost, for a reader
//! outside this crate that takes a pattern from a person or a model (a
//! watch's `matches`, theseus-f2r2b): the pattern is compiled with a size
//! limit, and the engine is linear in the text.

use regex::RegexBuilder;

/// The compiled program's size limit in bytes.
const SIZE_LIMIT: usize = 1 << 20;

/// Whether `pattern` matches somewhere in `text`; an error names why the
/// pattern is not usable.
pub fn is_match(pattern: &str, text: &str) -> Result<bool, String> {
    RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT)
        .build()
        .map(|re| re.is_match(text))
        .map_err(|e| format!("`{pattern}` is not a usable pattern: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_or_says_why_not() {
        assert_eq!(is_match("^MERG(ED|E)$", "MERGED"), Ok(true));
        assert_eq!(is_match("^OPEN$", "MERGED"), Ok(false));
        assert!(is_match("(", "x")
            .unwrap_err()
            .contains("not a usable pattern"));
        assert!(
            is_match("a{1000}{1000}", "x").is_err(),
            "a program past the limit is refused"
        );
    }
}
