//! The text rules the records share: names, lines, prose, and digests.

use crate::refusal::Refusal;

/// The most characters in a category's name, as in a Discord channel's or
/// server's.
pub const NAME_MAX: usize = 100;
/// The most characters in a description.
pub const DESCRIPTION_MAX: usize = 2000;
/// The most characters in an `added_by`, or in a session's id.
pub const ADDED_BY_MAX: usize = 100;
/// The most bytes of one category's guidance under `chain` (a context file
/// may be 64 KiB; longer reference belongs in one).
pub const GUIDANCE_MAX: usize = 16 * 1024;
/// The most characters of one category's line under `intent_line`.
pub const INTENT_LINE_MAX: usize = 200;

/// A kind's name: a lowercase letter, then up to 31 lowercase letters,
/// digits, `_`, or `-`.
pub(crate) fn kind_name(s: &str) -> Result<(), Refusal> {
    let b = s.as_bytes();
    let ok = (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-');
    if ok {
        Ok(())
    } else {
        Err(Refusal::invalid(
            format!("the kind name {s:?}"),
            "a kind's name is a lowercase letter, then up to 31 lowercase letters, digits, `_`, or `-`",
        ))
    }
}

/// One line: not empty, no space at either end, no control character, and
/// at most `max` characters.
pub(crate) fn line(what: &str, s: &str, max: usize) -> Result<(), Refusal> {
    if s.trim().is_empty() {
        return Err(Refusal::invalid(what, "it is empty"));
    }
    if s.trim() != s {
        return Err(Refusal::invalid(what, "it begins or ends with a space"));
    }
    if s.chars().any(char::is_control) {
        return Err(Refusal::invalid(
            what,
            "it holds a line break, a tab, or another control character: it must be one line",
        ));
    }
    let n = s.chars().count();
    if n > max {
        return Err(Refusal::invalid(
            what,
            format!("it is {n} characters long, and the most is {max}"),
        ));
    }
    Ok(())
}

/// Prose: line breaks and tabs, but no other control character, and at most
/// `max` characters.
pub(crate) fn prose(what: &str, s: &str, max: usize) -> Result<(), Refusal> {
    if let Some(c) = s
        .chars()
        .find(|c| c.is_control() && *c != '\n' && *c != '\t')
    {
        return Err(Refusal::invalid(
            what,
            format!("it holds the control character {c:?}"),
        ));
    }
    let n = s.chars().count();
    if n > max {
        return Err(Refusal::invalid(
            what,
            format!("it is {n} characters long, and the most is {max}"),
        ));
    }
    Ok(())
}

/// Text as guidance stores it: `\r\n` made `\n`, and the ends trimmed.
pub(crate) fn normalized(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

/// The first 16 hex digits of the SHA-256 of `s`, as a context file's
/// digest in the manifest is.
pub(crate) fn sha16(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))[..16].to_string()
}
