//! A file over the context-file limit, cut at its headings and never
//! summarized: every part is a slice of the original, and the parts joined
//! are the original, byte for byte.

/// The cut points of `text`, so that each part is at most `limit` bytes where
/// that can be done: at the start of a heading line (`#` to `######`) first,
/// then at a blank line, then at any line, and only when a single line is
/// longer than the limit, at a character boundary; never inside a value the
/// vault knows. Returns the parts in order; a text within the limit is one
/// part. None when a part would have to cut through a known value (one
/// longer than the limit starts it).
pub fn parts(text: &str, limit: usize) -> Option<Vec<&str>> {
    assert!(limit >= 4, "a part must hold at least one character");
    let cuts = crate::scrub::Cuts::live();
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > limit {
        let cut = cut_of(rest, limit, &cuts)?;
        let (head, tail) = rest.split_at(cut);
        out.push(head);
        rest = tail;
    }
    if !rest.is_empty() || out.is_empty() {
        out.push(rest);
    }
    Some(out)
}

/// Where to cut `rest` (longer than `limit`): the latest line start within
/// `limit` that is a heading's, else a paragraph's, else any line's, else a
/// character boundary; each one only where it is outside every known value.
/// None when the only place left is the start.
fn cut_of(rest: &str, limit: usize, cuts: &crate::scrub::Cuts) -> Option<usize> {
    let (mut heading, mut blank, mut line) = (Vec::new(), Vec::new(), Vec::new());
    let mut at = 0;
    let mut prev_blank = false;
    for l in rest.split_inclusive('\n') {
        if at > 0 && at <= limit {
            line.push(at);
            if is_heading(l) {
                heading.push(at);
            }
            if prev_blank {
                blank.push(at);
            }
        }
        prev_blank = l.trim().is_empty();
        at += l.len();
        if at > limit {
            break;
        }
    }
    // A line start inside a value that holds a line break is no place to cut.
    let outside = |c: &&usize| cuts.before(rest, **c) == **c;
    let found = [heading, blank, line]
        .iter()
        .find_map(|starts| starts.iter().rev().find(outside).copied());
    if found.is_some() {
        return found;
    }
    let c = crate::scrub::safe::head_bytes_with(cuts, rest, limit)
        .0
        .len();
    (c > 0).then_some(c)
}

fn is_heading(line: &str) -> bool {
    let t = line.trim_start_matches('#');
    let n = line.len() - t.len();
    (1..=6).contains(&n) && t.starts_with(' ')
}

/// `SOUL.md` as `SOUL.part2.md`.
pub fn part_name(rel: &str, n: usize) -> String {
    match rel.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.part{n}.{ext}"),
        None => format!("{rel}.part{n}"),
    }
}
