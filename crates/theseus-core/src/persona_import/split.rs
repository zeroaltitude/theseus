//! A file over the context-file limit, cut at its headings and never
//! summarized: every part is a slice of the original, and the parts joined
//! are the original, byte for byte.

/// The cut points of `text`, so that each part is at most `limit` bytes where
/// that can be done: at the start of a heading line (`#` to `######`) first,
/// then at a blank line, then at any line, and only when a single line is
/// longer than the limit, at a character boundary. Returns the parts in
/// order; a text within the limit is one part.
pub fn parts(text: &str, limit: usize) -> Vec<&str> {
    assert!(limit >= 4, "a part must hold at least one character");
    let mut out = Vec::new();
    let mut rest = text;
    while rest.len() > limit {
        let cut = cut_of(rest, limit);
        let (head, tail) = rest.split_at(cut);
        out.push(head);
        rest = tail;
    }
    if !rest.is_empty() || out.is_empty() {
        out.push(rest);
    }
    out
}

/// Where to cut `rest` (longer than `limit`): the latest line start within
/// `limit` that is a heading's, else a paragraph's, else any line's, else a
/// character boundary.
fn cut_of(rest: &str, limit: usize) -> usize {
    let mut heading = None;
    let mut blank = None;
    let mut line = None;
    let mut at = 0;
    let mut prev_blank = false;
    for l in rest.split_inclusive('\n') {
        if at > 0 && at <= limit {
            line = Some(at);
            if is_heading(l) {
                heading = Some(at);
            }
            if prev_blank {
                blank = Some(at);
            }
        }
        prev_blank = l.trim().is_empty();
        at += l.len();
        if at > limit {
            break;
        }
    }
    if let Some(c) = heading.or(blank).or(line) {
        return c;
    }
    let mut c = limit;
    while !rest.is_char_boundary(c) {
        c -= 1;
    }
    c
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
