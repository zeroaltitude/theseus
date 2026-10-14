//! An edit's diff on the wire and as rows (theseus-z6au): `tool.ended`'s `diff`
//! for `fs.edit`, `fs.write` and `fs.patch`, `confirm.requested`'s `preview`,
//! and the pure reading of a unified diff every surface draws from, with the
//! changed words of a paired removed/added line marked. No style here: a
//! surface colours the rows its own way.

use serde::{Deserialize, Serialize};

/// The most a diff on the wire holds (16 KB); a longer one is cut at a line
/// and says so (`truncated`).
pub const DIFF_CAP: usize = 16 * 1024;

/// What a `fs.edit`, `fs.write` or `fs.patch` changed, as `tool.ended` carries
/// it: the diff the tool made for the model, after the scrubber, capped.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolDiff {
    /// The file (a patch of several files: `3 files`).
    pub path: String,
    /// Lines added and removed, counted over the whole diff.
    pub added: u32,
    pub removed: u32,
    /// The unified diff, three lines of context.
    pub unified: String,
    /// `unified` was cut at `DIFF_CAP`.
    #[serde(default)]
    pub truncated: bool,
}

/// What a question shows of the call it asks about: an edit's diff against
/// the file as it was when the call was planned, a write's content or diff,
/// a command as a shell line with its directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmPreview {
    /// `diff`, `command` or `text`.
    pub kind: String,
    pub body: String,
}

impl ConfirmPreview {
    pub const DIFF: &'static str = "diff";
    pub const COMMAND: &'static str = "command";
    pub const TEXT: &'static str = "text";
}

/// `unified` cut at a line edge to `DIFF_CAP` bytes, and whether it was cut.
pub fn cap(unified: &str) -> (String, bool) {
    if unified.len() <= DIFF_CAP {
        return (unified.to_string(), false);
    }
    let mut end = DIFF_CAP;
    while !unified.is_char_boundary(end) {
        end -= 1;
    }
    let cut = unified[..end].rfind('\n').map_or(end, |i| i + 1);
    (unified[..cut].to_string(), true)
}

/// Lines added and removed in a unified diff, headers left out.
pub fn stat(unified: &str) -> (u32, u32) {
    let rows = rows(unified);
    let n = |k: Kind| rows.iter().filter(|r| r.kind == k).count() as u32;
    (n(Kind::Added), n(Kind::Removed))
}

/// What a row of a unified diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `--- a` / `+++ b`.
    File,
    /// `@@ -49,3 +49,3 @@`.
    Hunk,
    Context,
    Removed,
    Added,
    /// `\ No newline at end of file`.
    Note,
}

/// One row of a unified diff, numbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    /// The line's number in the old file (a context or removed line), the new
    /// file (a context or added line).
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// The line without its `+`, `-` or space (a header whole).
    pub text: String,
    /// Byte ranges of `text` whose words differ from the paired line.
    pub marks: Vec<(usize, usize)>,
}

fn row(kind: Kind, old: Option<u32>, new: Option<u32>, text: &str) -> Row {
    Row {
        kind,
        old,
        new,
        text: text.to_string(),
        marks: Vec::new(),
    }
}

/// `@@ -a,b +c,d @@` as (a, b, c, d); a missing length is 1.
fn hunk_header(l: &str) -> Option<(u32, u32, u32, u32)> {
    let body = l.strip_prefix("@@ -")?;
    let (old, rest) = body.split_once(" +")?;
    let new = rest.split(" @@").next()?;
    let pair = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = pair(old)?;
    let (c, d) = pair(new)?;
    Some((a, b, c, d))
}

/// A unified diff as numbered rows, with the changed words of each paired
/// removed/added line marked. A line that fits no shape is a context row.
pub fn rows(unified: &str) -> Vec<Row> {
    let lines: Vec<&str> = unified.lines().collect();
    let mut out: Vec<Row> = Vec::new();
    let (mut old_left, mut new_left) = (0u32, 0u32);
    let (mut o, mut n) = (0u32, 0u32);
    for (i, l) in lines.iter().enumerate() {
        let in_hunk = old_left > 0 || new_left > 0;
        if !in_hunk {
            if l.starts_with("--- ") && lines.get(i + 1).is_some_and(|x| x.starts_with("+++ ")) {
                out.push(row(Kind::File, None, None, l));
                continue;
            }
            if l.starts_with("+++ ") && matches!(out.last(), Some(r) if r.kind == Kind::File) {
                out.push(row(Kind::File, None, None, l));
                continue;
            }
            if let Some((a, b, c, d)) = hunk_header(l) {
                (o, n, old_left, new_left) = (a, c, b, d);
                out.push(row(Kind::Hunk, Some(a), Some(c), l));
                continue;
            }
        }
        if let Some(t) = l.strip_prefix('+') {
            out.push(row(Kind::Added, None, Some(n), t));
            n += 1;
            new_left = new_left.saturating_sub(1);
        } else if let Some(t) = l.strip_prefix('-') {
            out.push(row(Kind::Removed, Some(o), None, t));
            o += 1;
            old_left = old_left.saturating_sub(1);
        } else if l.starts_with('\\') {
            out.push(row(Kind::Note, None, None, l));
        } else {
            out.push(row(
                Kind::Context,
                Some(o),
                Some(n),
                l.strip_prefix(' ').unwrap_or(l),
            ));
            o += 1;
            n += 1;
            old_left = old_left.saturating_sub(1);
            new_left = new_left.saturating_sub(1);
        }
    }
    pair_marks(&mut out);
    out
}

/// A run of removed lines followed by a run of added ones: the i-th of each
/// is a pair, and their differing words are marked.
fn pair_marks(rows: &mut [Row]) {
    let mut i = 0;
    while i < rows.len() {
        if rows[i].kind != Kind::Removed {
            i += 1;
            continue;
        }
        let rem = rows[i..]
            .iter()
            .take_while(|r| r.kind == Kind::Removed)
            .count();
        let add = rows[i + rem..]
            .iter()
            .take_while(|r| r.kind == Kind::Added)
            .count();
        for k in 0..rem.min(add) {
            let (a, b) = words(&rows[i + k].text, &rows[i + rem + k].text);
            rows[i + k].marks = a;
            rows[i + rem + k].marks = b;
        }
        i += rem + add;
    }
}

/// Byte ranges of a line.
pub type Marks = Vec<(usize, usize)>;

/// Words, spaces and single other characters, with their byte offsets.
fn tokens(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let class = |c: char| (c.is_alphanumeric() || c == '_', c.is_whitespace());
        let (word, space) = class(c);
        let mut end = i + c.len_utf8();
        if word || space {
            while let Some(&(j, d)) = chars.peek() {
                if class(d) != (word, space) {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
        }
        out.push((i, &s[i..end]));
    }
    out
}

/// The byte ranges of `a` and of `b` that a word-level diff of the two
/// changes. Lines of more than 400 tokens mark nothing.
pub fn words(a: &str, b: &str) -> (Marks, Marks) {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.len() > 400 || tb.len() > 400 {
        return (Vec::new(), Vec::new());
    }
    // The longest common run of tokens, by the usual table.
    let (n, m) = (ta.len(), tb.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if ta[i].1 == tb[j].1 {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut ca, mut cb) = (vec![true; n], vec![true; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if ta[i].1 == tb[j].1 {
            (ca[i], cb[j]) = (false, false);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let ranges = |t: &[(usize, &str)], changed: &[bool]| {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (k, (at, s)) in t.iter().enumerate() {
            if !changed[k] || s.trim().is_empty() {
                continue;
            }
            match out.last_mut() {
                // Join a run, spaces between its words included.
                Some(last)
                    if t[..k]
                        .iter()
                        .rev()
                        .take_while(|x| x.0 >= last.1)
                        .all(|x| x.1.trim().is_empty()) =>
                {
                    last.1 = at + s.len();
                }
                _ => out.push((*at, at + s.len())),
            }
        }
        out
    };
    (ranges(&ta, &ca), ranges(&tb, &cb))
}

/// The line number of the first changed row (`tides.py:49`), if any.
pub fn first_changed(rows: &[Row]) -> Option<u32> {
    rows.iter()
        .find(|r| matches!(r.kind, Kind::Removed | Kind::Added))
        .and_then(|r| r.old.or(r.new))
}

/// A new file's diff: every line added, against nothing.
pub fn creation(name: &str, content: &str) -> String {
    let mut out = format!("--- /dev/null\n+++ {name}\n");
    let n = content.lines().count();
    if n == 0 {
        return out;
    }
    out.push_str(&format!("@@ -0,0 +1,{n} @@\n"));
    for l in content.lines() {
        out.push('+');
        out.push_str(l);
        out.push('\n');
    }
    if !content.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDIT: &str = "--- tides.py\n+++ tides.py\n@@ -47,5 +47,5 @@\n def tidal_range(heights):\n     lowest = min(heights)\n     highest = max(heights)\n-    return lowest - highest\n+    return highest - lowest\n \n";

    #[test]
    fn a_diff_is_numbered_and_its_paired_lines_mark_the_changed_words() {
        let r = rows(EDIT);
        assert_eq!(r[0].kind, Kind::File);
        assert_eq!(r[2].kind, Kind::Hunk);
        let rem = r.iter().find(|x| x.kind == Kind::Removed).unwrap();
        let add = r.iter().find(|x| x.kind == Kind::Added).unwrap();
        assert_eq!((rem.old, add.new), (Some(50), Some(50)));
        assert_eq!(first_changed(&r), Some(50));
        // `lowest` and `highest` swap places; ` - ` stays.
        let shown = |row: &Row| -> Vec<String> {
            row.marks
                .iter()
                .map(|&(a, b)| row.text[a..b].to_string())
                .collect()
        };
        assert_eq!(shown(rem), ["lowest", "highest"]);
        assert_eq!(shown(add), ["highest", "lowest"]);
        assert_eq!(stat(EDIT), (1, 1));
    }

    #[test]
    fn an_unpaired_line_marks_nothing_and_a_dashed_line_is_not_a_header() {
        let d = "--- a\n+++ a\n@@ -1,2 +1,1 @@\n--- not a header\n-b\n+c\n";
        let r = rows(d);
        // `--- not a header` is a removed line inside the hunk.
        assert_eq!(r.iter().filter(|x| x.kind == Kind::Removed).count(), 2);
        assert_eq!(stat(d), (1, 2));
        assert_eq!(words("same", "same"), (vec![], vec![]));
    }

    #[test]
    fn a_diff_over_the_cap_is_cut_at_a_line_and_says_so() {
        let long: String = (0..4000).map(|i| format!("+line {i}\n")).collect();
        let (cut, truncated) = cap(&long);
        assert!(truncated && cut.len() <= DIFF_CAP && cut.ends_with('\n'));
        assert_eq!(cap("+a\n"), ("+a\n".to_string(), false));
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let d = creation("a.txt", "x\ny");
        assert_eq!(stat(&d), (2, 0));
        assert!(d.starts_with("--- /dev/null\n+++ a.txt\n@@ -0,0 +1,2 @@\n+x\n+y\n"));
        let r = rows(&d);
        assert_eq!(r.last().map(|x| x.kind), Some(Kind::Note));
    }
}
