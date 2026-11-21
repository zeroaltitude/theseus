//! The narrow test rule's reach into an ordinary source file (issue 36, round 2, the review's note
//! 1): a test can live inline, in a `#[cfg(test)]` module or a `#[test]` function of a file that is
//! not named as a test file. Main's keel guard and the line rule let three edits to one through: an
//! early `return;` in the test, `#[cfg(any())]` put above it, and a changed value the assertion
//! reads. A fix may not change any line of a test region of a file on the merge-base, nor put a
//! line right above one, so the first two are refused. The third is a line outside the test, and
//! no reading of lines can tell it from a repair: the approver is told to look for it.
//!
//! A region is read from the file as it stood before the fix: from the test's attribute lines to
//! its closing brace, braces counted outside strings and comments. It counts when a test of that
//! name was in the file on the merge-base: a test the branch wrote is the fixer's to repair. A
//! `#[cfg(test)]` module that was on the merge-base counts whole, the branch's own tests in it
//! included: the stricter reading.

/// A test region: its first and last line (1-based, inclusive), and the test's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub name: String,
}

/// Whether a line is an attribute that makes what follows a test.
fn test_attr(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("#[cfg(test)]")
        || l.starts_with("#[test]")
        || l.starts_with("#[tokio::test")
        || l.starts_with("#[test_case")
}

/// `{` minus `}` in `line`, outside strings, chars and a trailing comment.
fn braces(line: &str) -> i64 {
    let (mut n, mut in_str, mut esc) = (0i64, false, false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '\'' if chars.peek().is_some_and(|n| *n != '{' && *n != '}') => {}
            '\'' => {
                // A char literal of a brace: '{' or '}'.
                chars.next();
            }
            '/' if chars.peek() == Some(&'/') => break,
            '{' => n += 1,
            '}' => n -= 1,
            _ => {}
        }
    }
    n
}

/// The item's name: the word after `fn` or `mod`.
fn name_of(line: &str) -> Option<String> {
    let mut words = line.split(|c: char| !(c.is_alphanumeric() || c == '_'));
    while let Some(w) = words.next() {
        if w == "fn" || w == "mod" {
            return words.find(|w| !w.is_empty()).map(str::to_string);
        }
    }
    None
}

/// Every test region of `text`.
pub fn regions(text: &str) -> Vec<Region> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !test_attr(l) {
            continue;
        }
        // Its attribute block: the lines above it that are attributes too.
        let mut start = i;
        while start > 0 && lines[start - 1].trim_start().starts_with("#[") {
            start -= 1;
        }
        // The item: the first line after the attributes, then to its closing brace.
        let Some(item) = (i + 1..lines.len()).find(|&j| !lines[j].trim_start().starts_with("#["))
        else {
            continue;
        };
        let Some(name) = name_of(lines[item]) else {
            continue;
        };
        let (mut depth, mut opened, mut end) = (0i64, false, lines.len() - 1);
        for (j, line) in lines.iter().enumerate().skip(item) {
            if !opened && line.trim_end().ends_with(';') && !line.contains('{') {
                break;
            }
            depth += braces(line);
            opened |= line.contains('{');
            if opened && depth <= 0 {
                end = j;
                break;
            }
        }
        if !opened {
            continue;
        }
        out.push(Region {
            start: start + 1,
            end: end + 1,
            name,
        });
    }
    out
}

/// A hunk's old side, from its `@@ -a[,b] …` header: its first line and how many it covers.
fn hunk(header: &str) -> Option<(usize, usize)> {
    let old = header.strip_prefix("@@ -")?.split(' ').next()?;
    let (a, b) = old.split_once(',').unwrap_or((old, "1"));
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// Whether a hunk whose old side is `(a, b)` touches `r`: it changes a line of it, or puts a line
/// in it or right above it.
fn touches((a, b): (usize, usize), r: &Region) -> bool {
    match b {
        // An insertion after line `a`.
        0 => r.start.saturating_sub(1) <= a && a < r.end,
        _ => a <= r.end && a + b > r.start,
    }
}

/// The first test region of `path` that a `-U0` diff of it (`hunks`, its `@@` lines) touches,
/// counting only the tests that `base_text` holds by name. `before_text` is the file the diff was
/// made from.
pub fn touched(before_text: &str, base_text: &str, hunks: &[&str]) -> Option<Region> {
    let on_base: Vec<String> = regions(base_text).into_iter().map(|r| r.name).collect();
    let locked: Vec<Region> = regions(before_text)
        .into_iter()
        .filter(|r| on_base.contains(&r.name))
        .collect();
    hunks
        .iter()
        .filter_map(|h| hunk(h))
        .find_map(|h| locked.iter().find(|r| touches(h, r)).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "pub fn add(a: u32) -> u32 {\n    a + 1\n}\n\n#[cfg(test)]\nmod tests {\n    \
                        use super::*;\n\n    #[test]\n    fn adds() {\n        let s = \"}{\";\n        \
                        assert_eq!(add(1), 2);\n    }\n}\n";

    #[test]
    fn the_regions_of_an_inline_test_module_and_its_tests() {
        let r = regions(FILE);
        assert_eq!(
            r[0],
            Region {
                start: 5,
                end: 14,
                name: "tests".into()
            }
        );
        assert_eq!(
            r[1],
            Region {
                start: 9,
                end: 13,
                name: "adds".into()
            }
        );
        assert!(regions("#[cfg(test)]\nmod tests_x;\nfn f() {}\n").is_empty());
    }

    /// The review's three edits: an early return and a `#[cfg(any())]` above the test are refused;
    /// a changed value outside the test is not (the approver is told to look for it).
    #[test]
    fn a_line_in_or_right_above_an_old_test_is_refused_and_one_outside_is_not() {
        let early_return = ["@@ -10,0 +11 @@"];
        assert_eq!(touched(FILE, FILE, &early_return).unwrap().name, "tests");
        let above = ["@@ -8,0 +9 @@"];
        assert_eq!(touched(FILE, FILE, &above).unwrap().name, "tests");
        let outside = ["@@ -2 +2 @@"];
        assert_eq!(touched(FILE, FILE, &outside), None);
        // A test the branch wrote is the fixer's to repair.
        let base = "pub fn add(a: u32) -> u32 {\n    a + 1\n}\n";
        assert_eq!(touched(FILE, base, &early_return), None);
    }
}
