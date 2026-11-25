//! The cap over a rendering kept by reference (issue 44): [`cap_parts`] cuts parts where `toolrun::cap` cuts their
//! rendering (`toolrun::cut_points`), so they show what the cap of their text shows, its seam a [`Part::Code`].
//!
//! - **Where it cuts.** At the cap's points on the rendered text; then, where a point falls inside a quote, at a
//!   known value's edge in the words the quote reads, by the same snapshot (`Cuts`). A rendering can hide a value
//!   from the cut on the text (an open's margin inside a wrapped encoding, a run of blanks made one space), and a
//!   stored range must never end inside one. That only ever shows less, and only then differs from the cap of the
//!   text.
//! - **How a quote is cut.** Into quotes of the same form over the bytes kept, which show exactly the kept part of
//!   its rendering: every edge in a one-line form's rendering is an edge of its bytes. An open's `Margin1` quote
//!   keeps the whole lines it keeps as `Margin1`, and of a line it cuts, the margin as code and the words as
//!   `Line1`.
//! - **What does not read.** A quote whose record does not read as words when the cap runs is the fixed line it
//!   shows, as code: it names nothing that could be read again.
//! - **The runtime's words** around a result are code: the fit note before it ([`noted`]), which the cap may cut
//!   as it cuts the text, and after the cap the hint and the group line ([`ended`]).

use std::ops::Range;

use super::{read, shown, Form, Part, Quote, Read};
use crate::history_tool::open::MARGIN;
use crate::scrub::Cuts;
use crate::store::Store;
use crate::toolrun;

/// A part as the cap reads it: code, or a quote with its rendering and the whole text of its field.
enum Laid {
    Code(String),
    Quote {
        q: Quote,
        shown: String,
        field: String,
    },
}

impl Laid {
    fn text(&self) -> &str {
        match self {
            Laid::Code(c) => c,
            Laid::Quote { shown, .. } => shown,
        }
    }
}

/// Which end of the cut: the head kept before the seam, or the tail kept after it.
#[derive(Clone, Copy)]
enum Side {
    Head,
    Tail,
}

/// `parts` capped at `max` characters as `toolrun::cap` caps their rendering, `rest` saying how to get what is not
/// shown; and whether anything was cut.
pub fn cap_parts(
    store: &Store,
    parts: Vec<Part>,
    max: usize,
    rest: impl FnOnce(&str) -> String,
    cuts: &Cuts,
) -> (Vec<Part>, bool) {
    cap_with(parts, |q| read(store, q), max, rest, cuts)
}

/// [`cap_parts`], each quote's record read by `read`.
pub(crate) fn cap_with(
    parts: Vec<Part>,
    mut read: impl FnMut(&Quote) -> Read,
    max: usize,
    rest: impl FnOnce(&str) -> String,
    cuts: &Cuts,
) -> (Vec<Part>, bool) {
    let laid: Vec<Laid> = parts.iter().cloned().map(|p| lay(p, &mut read)).collect();
    let text: String = laid.iter().map(Laid::text).collect();
    let Some((h, t)) = toolrun::cut_points(&text, max, cuts) else {
        return (parts, false);
    };
    let h = guarded(&laid, head_end(&text, h), cuts, Side::Head).unwrap_or(h);
    let t = guarded(&laid, t, cuts, Side::Tail).unwrap_or(t);
    let mut out = kept(&laid, 0..head_end(&text, h));
    out.push(Part::Code(toolrun::seam(&text, h, t, rest)));
    out.extend(kept(&laid, t..text.len()));
    (tidy(out), true)
}

/// A result's parts after its call's fit note, as `toolrun` puts the note before its text: code.
pub fn noted(note: Option<&str>, parts: Vec<Part>) -> Vec<Part> {
    let note = note.map(|n| Part::Code(toolrun::noted(n)));
    tidy(note.into_iter().chain(parts).collect())
}

/// A result's capped parts with the runtime's words after them, as `toolrun` puts them after its capped text:
/// code.
pub fn ended(mut parts: Vec<Part>, hint: Option<&str>, group: Option<&str>) -> Vec<Part> {
    parts.push(Part::Code(toolrun::ended(hint, group)));
    tidy(parts)
}

/// Where the cap's head ends in `text` cut at `h`: without one line break it ends with.
fn head_end(text: &str, h: usize) -> usize {
    match text[..h].ends_with('\n') {
        true => h - 1,
        false => h,
    }
}

fn lay(p: Part, read: &mut impl FnMut(&Quote) -> Read) -> Laid {
    let q = match p {
        Part::Code(c) => return Laid::Code(c),
        Part::Quote(q) => q,
    };
    match read(&q) {
        Read::Field(field) if field.get(q.from as usize..q.to as usize).is_some() => Laid::Quote {
            shown: q.form.apply(&field[q.from as usize..q.to as usize]),
            q,
            field,
        },
        r => Laid::Code(shown(&q, r)),
    }
}

/// Where a cut at `at` in the rendering of `laid` falls when it is inside a quote and a known value in the quote's
/// field runs across the place it cuts: at the value's edge, by `cuts`, which shows less. None when it is not moved.
fn guarded(laid: &[Laid], at: usize, cuts: &Cuts, side: Side) -> Option<usize> {
    let mut s = 0;
    for l in laid {
        let e = s + l.text().len();
        if let Laid::Quote { q, field, .. } = l {
            if s < at && at < e {
                let p = source_at(q, field, at - s, side);
                let moved = match side {
                    Side::Head => cuts.before(field, p),
                    Side::Tail => cuts.after(field, p),
                };
                if moved == p {
                    return None;
                }
                let k = s + rendered_at(q, field, moved, side);
                return Some(match side {
                    Side::Head => k.min(at),
                    Side::Tail => k.max(at),
                });
            }
        }
        s = e;
    }
    None
}

/// The parts that show `range` of the rendering of `laid`.
fn kept(laid: &[Laid], range: Range<usize>) -> Vec<Part> {
    let mut out = Vec::new();
    let mut s = 0;
    for l in laid {
        let e = s + l.text().len();
        let (a, b) = (range.start.max(s), range.end.min(e));
        if a < b {
            match l {
                Laid::Code(c) => out.push(Part::Code(c[a - s..b - s].to_string())),
                Laid::Quote { q, .. } if (a, b) == (s, e) => out.push(Part::Quote(q.clone())),
                Laid::Quote { q, field, .. } => out.extend(within(q, field, a - s..b - s)),
            }
        }
        s = e;
    }
    out
}

/// The quotes that show `k`, a range of `q`'s rendering, its field `field`.
fn within(q: &Quote, field: &str, k: Range<usize>) -> Vec<Part> {
    let bytes = &field[q.from as usize..q.to as usize];
    if q.form == Form::Margin1 {
        return margined(q, bytes, k);
    }
    let points = points(q.form, bytes);
    let a = source(&points, k.start, Side::Tail);
    let b = source(&points, k.end, Side::Head);
    vec![Part::Quote(sub(q, a..b.max(a), q.form))]
}

/// `q` over `range` of its bytes, in `form`.
fn sub(q: &Quote, range: Range<usize>, form: Form) -> Quote {
    Quote {
        from: q.from + range.start as u32,
        to: q.from + range.end as u32,
        form,
        ..q.clone()
    }
}

/// The edges of `bytes` rendered in `form` (any but `Margin1`) that are edges of the bytes too, as (byte of `bytes`,
/// byte of the rendering), the rendering's strictly rising: each character of the rendering is one character of
/// the bytes, or in a one-line form one run of blanks, so every edge of the rendering is one.
fn points(form: Form, bytes: &str) -> Vec<(usize, usize)> {
    let one_line = matches!(form, Form::Flat1 | Form::Clip1);
    let mut out = Vec::with_capacity(bytes.len() + 1);
    let (mut o, mut run) = (0, false);
    for (i, c) in bytes.char_indices() {
        if one_line && crate::view::text::kept::blank(c) {
            if !run {
                out.push((i, o));
                o += 1;
                run = true;
            }
            continue;
        }
        out.push((i, o));
        run = false;
        o += form.apply(c.encode_utf8(&mut [0; 4])).len();
    }
    out.push((bytes.len(), o));
    out
}

/// The byte of the bytes at `k`, an edge of their rendering; one that is not an edge falls to the side that shows
/// less: the next edge for a tail's start, the one before for a head's end.
fn source(points: &[(usize, usize)], k: usize, side: Side) -> usize {
    match points.binary_search_by_key(&k, |p| p.1) {
        Ok(i) => points[i].0,
        Err(i) => match side {
            Side::Tail => points.get(i).map_or(points[points.len() - 1].0, |p| p.0),
            Side::Head => points[i.saturating_sub(1)].0,
        },
    }
}

/// The edge of the rendering at byte `p` of the bytes: the last at or before it for a head's end, the first at or
/// after it for a tail's start.
fn rendered(points: &[(usize, usize)], p: usize, side: Side) -> usize {
    match side {
        Side::Head => points.iter().rev().find(|x| x.0 <= p).map_or(0, |x| x.1),
        Side::Tail => points
            .iter()
            .find(|x| x.0 >= p)
            .map_or(points[points.len() - 1].1, |x| x.1),
    }
}

/// One line of a `Margin1` quote's bytes: its words in the bytes, and in the rendering where its margin begins and
/// where its words lie (its line break, if another line follows, right after them).
struct Line {
    src: Range<usize>,
    start: usize,
    words: Range<usize>,
}

fn lines(bytes: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut o = 0;
    for l in bytes.lines() {
        let at = l.as_ptr() as usize - bytes.as_ptr() as usize;
        let words = o + MARGIN.len()..o + MARGIN.len() + Form::Line1.apply(l).len();
        out.push(Line {
            src: at..at + l.len(),
            start: o,
            words: words.clone(),
        });
        o = words.end + 1;
    }
    out
}

/// The byte of `q`'s field where a cut at `k` of its rendering falls: in a line's words, the byte there; between
/// lines (in a line break or a margin), the end of the line before for a head, the start of the next for a tail.
fn source_at(q: &Quote, field: &str, k: usize, side: Side) -> usize {
    let bytes = &field[q.from as usize..q.to as usize];
    let at = match q.form {
        Form::Margin1 => {
            let lines = lines(bytes);
            match lines.iter().position(|l| k <= l.words.end) {
                Some(j) if k >= lines[j].words.start => {
                    let l = &lines[j];
                    l.src.start
                        + source(
                            &points(Form::Line1, &bytes[l.src.clone()]),
                            k - l.words.start,
                            side,
                        )
                }
                Some(j) => match side {
                    Side::Head => j.checked_sub(1).map_or(0, |i| lines[i].src.end),
                    Side::Tail => lines[j].src.start,
                },
                None => bytes.len(),
            }
        }
        form => source(&points(form, bytes), k, side),
    };
    q.from as usize + at
}

/// Where byte `p` of `q`'s field falls in its rendering, toward the side that shows less: for a head, the last edge
/// at or before it; for a tail, the first at or after it (between lines, a head ends after the line before and a
/// tail starts at the next line's margin).
fn rendered_at(q: &Quote, field: &str, p: usize, side: Side) -> usize {
    let bytes = &field[q.from as usize..q.to as usize];
    let p = p.clamp(q.from as usize, q.to as usize) - q.from as usize;
    if q.form != Form::Margin1 {
        return rendered(&points(q.form, bytes), p, side);
    }
    let lines = lines(bytes);
    let len = lines.last().map_or(0, |l| l.words.end);
    match lines.iter().position(|l| p <= l.src.end) {
        Some(j) if p >= lines[j].src.start => {
            let l = &lines[j];
            match (side, p == l.src.start, j) {
                (Side::Head, true, 0) => 0,
                (Side::Head, true, j) => lines[j - 1].words.end,
                (Side::Tail, true, _) => l.start,
                _ => {
                    let at = rendered(
                        &points(Form::Line1, &bytes[l.src.clone()]),
                        p - l.src.start,
                        side,
                    );
                    l.words.start + at
                }
            }
        }
        // In the line break before line `j`.
        Some(j) => match side {
            Side::Head => j.checked_sub(1).map_or(0, |i| lines[i].words.end),
            Side::Tail => lines[j].start,
        },
        None => len,
    }
}

/// The parts that show `k`, a range of the rendering of the `Margin1` quote `q` over `bytes`: each run of whole
/// lines as one `Margin1` quote when it shows them so, and of every other line, the margin and line break it keeps
/// as code and the words it keeps as `Line1`.
fn margined(q: &Quote, bytes: &str, k: Range<usize>) -> Vec<Part> {
    let shown = Form::Margin1.apply(bytes);
    let lines = lines(bytes);
    let whole = |j: usize| {
        lines
            .get(j)
            .is_some_and(|l| k.start <= l.start && l.words.end <= k.end)
    };
    let mut out = Vec::new();
    let mut run: Option<usize> = None;
    for (j, l) in lines.iter().enumerate() {
        if whole(j) {
            run.get_or_insert(j);
        } else {
            if let Some(first) = run.take() {
                out.extend(whole_lines(q, bytes, &shown, &lines[first..j]));
            }
            let margin = clip(l.start..l.words.start, &k);
            if !margin.is_empty() {
                out.push(Part::Code(
                    MARGIN[margin.start - l.start..margin.end - l.start].to_string(),
                ));
            }
            let words = clip(l.words.clone(), &k);
            if !words.is_empty() {
                let line = &bytes[l.src.clone()];
                let points = points(Form::Line1, line);
                let a = source(&points, words.start - l.words.start, Side::Tail);
                let b = source(&points, words.end - l.words.start, Side::Head);
                let range = l.src.start + a..l.src.start + b.max(a);
                out.push(Part::Quote(sub(q, range, Form::Line1)));
            }
        }
        let brk = clip(
            l.words.end..l.words.end + usize::from(j + 1 < lines.len()),
            &k,
        );
        // A break between two whole lines is the run's own; any other kept break is code.
        if !brk.is_empty() && !(whole(j) && whole(j + 1)) {
            if let Some(first) = run.take() {
                out.extend(whole_lines(q, bytes, &shown, &lines[first..=j]));
            }
            out.push(Part::Code("\n".into()));
        }
    }
    if let Some(first) = run.take() {
        out.extend(whole_lines(q, bytes, &shown, &lines[first..]));
    }
    out
}

/// `lines`, whole and in a row, as one `Margin1` quote when that shows exactly their rendering (a run that ends in an
/// empty line does not: `str::lines` drops it), else each as its margin, its words as `Line1`, and the breaks.
fn whole_lines(q: &Quote, bytes: &str, shown: &str, lines: &[Line]) -> Vec<Part> {
    let (first, last) = (&lines[0], &lines[lines.len() - 1]);
    let one = sub(q, first.src.start..last.src.end, Form::Margin1);
    let range = one.from as usize - q.from as usize..one.to as usize - q.from as usize;
    if Form::Margin1.apply(&bytes[range]) == shown[first.start..last.words.end] {
        return vec![Part::Quote(one)];
    }
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push(Part::Code("\n".into()));
        }
        out.push(Part::Code(MARGIN.into()));
        out.push(Part::Quote(sub(q, l.src.clone(), Form::Line1)));
    }
    out
}

/// `r` within `k`; empty when they do not meet.
fn clip(r: Range<usize>, k: &Range<usize>) -> Range<usize> {
    let (a, b) = (r.start.max(k.start), r.end.min(k.end));
    a..b.max(a)
}

/// `parts` without empty code or empty quotes, each run of code one part.
fn tidy(parts: Vec<Part>) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::with_capacity(parts.len());
    for p in parts {
        match p {
            Part::Code(c) if c.is_empty() => {}
            Part::Quote(q) if q.from == q.to => {}
            Part::Code(c) => match out.last_mut() {
                Some(Part::Code(last)) => last.push_str(&c),
                _ => out.push(Part::Code(c)),
            },
            q => out.push(q),
        }
    }
    out
}

#[cfg(test)]
mod tests;
