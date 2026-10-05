//! `fs.patch`'s recount (theseus-inw): each hunk header's lengths rewritten
//! from its body, as `git apply --recount` and GNU patch do, so a hunk whose
//! `@@ -a,b +c,d @@` counts are off while its body is right still applies.
//! The start lines are kept, and the body is not touched beyond a blank line:
//! diffy then checks the context and the removed lines against the file as
//! it does for any hunk.
//!
//! A line with nothing on it is a blank context line whose leading space was
//! dropped, as models write one, when another body line follows it in the
//! hunk; the empty lines that end a hunk are its end, not its body, unless
//! its header counts them (then as many as it counts are context).

/// A section's text with every hunk header recounted, and how many headers
/// changed. A section whose headers all match their bodies comes back as it
/// was, and 0.
pub(crate) fn recount(section: &str) -> (String, usize) {
    let lines: Vec<&str> = section.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = 0;
    let mut i = 0;
    while i < lines.len() {
        let Some(h) = Header::parse(lines[i]) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };
        i += 1;
        let start = i;
        while i < lines.len() && is_body(lines[i]) {
            i += 1;
        }
        let body = &lines[start..i];
        let trailing = body.iter().rev().take_while(|l| l.is_empty()).count();
        let (old, new) = count(&body[..body.len() - trailing]);
        // The header may count some of the trailing empty lines as blank
        // context: then they are, and the header is right.
        let kept = (0..=trailing).find(|k| (old + k, new + k) == (h.old_len, h.new_len));
        let k = kept.unwrap_or(0);
        if kept.is_none() {
            changed += 1;
        }
        out.push(h.with(old + k, new + k));
        let used = body.len() - trailing + k;
        out.extend(body[..used].iter().map(|l| match l.is_empty() {
            true => " ".to_string(),
            false => l.to_string(),
        }));
    }
    if changed == 0 {
        return (section.to_string(), 0);
    }
    let mut text = out.join("\n");
    text.push('\n');
    (text, changed)
}

/// `recount`, with the line the result gives a section whose headers it
/// changed ("recounted 2 hunk headers in src/a.rs") pushed to `notes`.
pub(crate) fn noted(section: &str, target: &str, notes: &mut Vec<String>) -> String {
    let (text, n) = recount(section);
    if n > 0 {
        let s = if n == 1 { "" } else { "s" };
        notes.push(format!("recounted {n} hunk header{s} in {target}"));
    }
    text
}

/// A line of a hunk's body: context, removed, added, a `\ No newline`
/// marker, or a blank context line written empty.
fn is_body(l: &str) -> bool {
    l.is_empty() || matches!(l.as_bytes()[0], b' ' | b'-' | b'+' | b'\\')
}

/// The old and new lengths a body gives: context counts on both sides, a
/// removed line on the old, an added one on the new, a marker on neither.
fn count(body: &[&str]) -> (usize, usize) {
    body.iter()
        .fold((0, 0), |(o, n), l| match l.bytes().next() {
            None | Some(b' ') => (o + 1, n + 1),
            Some(b'-') => (o + 1, n),
            Some(b'+') => (o, n + 1),
            _ => (o, n),
        })
}

/// A hunk header, `@@ -a[,b] +c[,d] @@ [section heading]`: a missing length
/// is 1.
struct Header<'a> {
    old_start: &'a str,
    old_len: usize,
    new_start: &'a str,
    new_len: usize,
    rest: &'a str,
}

impl<'a> Header<'a> {
    fn parse(l: &'a str) -> Option<Self> {
        let inner = l.strip_prefix("@@ -")?;
        let (ranges, rest) = inner.split_once(" @@")?;
        let (old, new) = ranges.split_once(" +")?;
        let range = |r: &'a str| -> Option<(&'a str, usize)> {
            let (s, n) = match r.split_once(',') {
                Some((s, n)) => (s, n.parse().ok()?),
                None => (r, 1),
            };
            s.parse::<usize>().ok()?;
            Some((s, n))
        };
        let (old_start, old_len) = range(old)?;
        let (new_start, new_len) = range(new)?;
        Some(Self {
            old_start,
            old_len,
            new_start,
            new_len,
            rest,
        })
    }

    fn with(&self, old_len: usize, new_len: usize) -> String {
        format!(
            "@@ -{},{old_len} +{},{new_len} @@{}",
            self.old_start, self.new_start, self.rest
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "--- a/x.txt\n+++ b/x.txt\n";

    #[test]
    fn right_counts_come_back_as_they_were() {
        let s = format!("{HEAD}@@ -1,3 +1,3 @@ fn main\n one\n-two\n+TWO\n three\n");
        assert_eq!(recount(&s), (s.clone(), 0));
        // A missing length is 1.
        let s = format!("{HEAD}@@ -2 +2 @@\n-two\n+TWO\n");
        assert_eq!(recount(&s), (s.clone(), 0));
    }

    #[test]
    fn counts_off_either_way_are_rewritten_from_the_body() {
        let short = format!("{HEAD}@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n three\n");
        let long = format!("{HEAD}@@ -1,4 +1,5 @@\n one\n-two\n+TWO\n three\n");
        let right = format!("{HEAD}@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n");
        assert_eq!(recount(&short), (right.clone(), 1));
        assert_eq!(recount(&long), (right, 1));
    }

    #[test]
    fn a_marker_counts_for_nothing_and_each_hunk_is_its_own() {
        let s = format!(
            "{HEAD}@@ -1,9 +1,9 @@ heading kept\n one\n-two\n+TWO\n\\ No newline at end of file\n@@ -7,3 +7,3 @@\n seven\n-eight\n+EIGHT\n"
        );
        let want = format!(
            "{HEAD}@@ -1,2 +1,2 @@ heading kept\n one\n-two\n+TWO\n\\ No newline at end of file\n@@ -7,2 +7,2 @@\n seven\n-eight\n+EIGHT\n"
        );
        assert_eq!(recount(&s), (want, 2));
    }

    /// An empty line inside a hunk is a blank context line; the empty lines
    /// at its end are its end, unless its header counts them.
    #[test]
    fn an_empty_line_is_blank_context_inside_and_the_end_after() {
        let inside = format!("{HEAD}@@ -1,2 +1,2 @@\n one\n\n-three\n+THREE\n");
        let want = format!("{HEAD}@@ -1,3 +1,3 @@\n one\n \n-three\n+THREE\n");
        assert_eq!(recount(&inside), (want, 1));
        let after = format!("{HEAD}@@ -1,1 +1,1 @@\n one\n-two\n+TWO\n\n\n");
        let want = format!("{HEAD}@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n");
        assert_eq!(recount(&after), (want, 1));
        let counted = format!("{HEAD}@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n\n\n");
        assert_eq!(recount(&counted), (counted.clone(), 0));
    }
}
