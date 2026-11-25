//! The cap over parts shows what the cap of their rendering shows, its seam code; a quote it cuts is cut into
//! quotes of its bytes, never copied; and a range it stores never ends inside a known value, even one the rendering
//! hides from the cut on the text. All values invented.

use std::collections::HashMap;

use super::*;
use crate::history_tool::page::cut_kept;
use crate::quotes::render_with;
use crate::quotes::tests_oracle::{each_found, inside, knowing, Draw, VALUES};
use crate::scrub::oracle::{encodings, spans_of};

mod lines;

/// Fields by node id, read whole, as a store reads a quote's field at its position.
struct Fields(HashMap<String, String>);

impl Fields {
    fn read(&self) -> impl FnMut(&Quote) -> Read + '_ {
        |q| match self.0.get(&q.node) {
            Some(f) => Read::Field(f.clone()),
            None => Read::Unreadable,
        }
    }

    /// A quote of `field` over `range`, in `form`, the field kept under a new id.
    fn quote(&mut self, field: String, range: Range<usize>, form: Form) -> Part {
        let node = format!("msg_{}", self.0.len());
        self.0.insert(node.clone(), field);
        Part::Quote(Quote {
            node,
            position: 1,
            field: crate::quotes::Field::Text,
            from: range.start as u32,
            to: range.end as u32,
            form,
        })
    }

    fn render(&self, parts: &[Part]) -> String {
        render_with(parts, self.read())
    }
}

/// What a cap leaves out, said from the text it left out, so the seam is checked too.
fn rest(left: &str) -> String {
    format!("{} bytes are kept", left.len())
}

/// `value`'s encodings that stay on one line, which no form's rewrite hides from a cut on the text.
fn one_line(value: &str) -> Vec<String> {
    let all = encodings(value);
    vec![
        all[0].clone(),
        all[1].clone(),
        all[3].clone(),
        all[4].clone(),
    ]
}

/// A rendering kept by reference, of every form, whose quotes hold known values only where the rendering shows
/// them as the words do: whole fields verbatim with every encoding; a page's cut (`cut_kept`, which keeps a value
/// its rewrite would change out of the bytes it keeps); and one-line encodings in an open's margined lines, in a
/// look's line and in `Line1`.
fn shown_parts(draw: &mut Draw, fields: &mut Fields, value: &str, cuts: &Cuts) -> Vec<Part> {
    let mut parts = Vec::new();
    for _ in 0..3 + draw.below(6) {
        let n = draw.below(30);
        let code = draw.filler(n);
        parts.push(Part::Code(format!(" {code} ")));
        let part = match draw.below(5) {
            0 => {
                let f = draw.text_with(value);
                fields.quote(f.clone(), 0..f.len(), Form::Verbatim)
            }
            1 => {
                let f = draw.text_with(value);
                let max = 1 + draw.below(f.chars().count() + 4);
                let (_, kept) = cut_kept(&f, max, cuts);
                fields.quote(f, kept, Form::Flat1)
            }
            2 => {
                let n = 1 + draw.below(40);
                let f = draw.filler(n);
                fields.quote(f.clone(), 0..f.len(), Form::Clip1)
            }
            form => {
                let mut f = draw.filler(2);
                for e in one_line(value) {
                    f.push(' ');
                    f.push_str(&e);
                    f.push(' ');
                    let n = draw.below(8);
                    f.push_str(&draw.filler(n));
                }
                let form = if form == 3 {
                    Form::Margin1
                } else {
                    Form::Line1
                };
                fields.quote(f.clone(), 0..f.len(), form)
            }
        };
        parts.push(part);
    }
    parts
}

/// The cap of the parts renders as the cap of their rendering, byte for byte, seam and all, at every length, with
/// known values in the words.
#[test]
fn render_of_cap_parts_is_cap_of_render() {
    let mut draw = Draw::new(0x51_7cc1_b727_220a);
    let mut cut = 0;
    for round in 0..120 {
        let value = VALUES[round % VALUES.len()];
        let cuts = knowing(&[value]).cuts();
        let mut fields = Fields(HashMap::new());
        let parts = shown_parts(&mut draw, &mut fields, value, &cuts);
        let text = fields.render(&parts);
        let chars = text.chars().count();
        for max in (60..chars + 8).step_by(7 + round % 11) {
            let (capped, was_cut) = cap_with(parts.clone(), fields.read(), max, rest, &cuts);
            let (want, want_cut) = toolrun::cap(&text, max, rest, &cuts);
            assert_eq!(was_cut, want_cut, "max {max}");
            assert_eq!(fields.render(&capped), want, "max {max}: {parts:?}");
            cut += usize::from(was_cut);
        }
    }
    assert!(cut > 1_000, "{cut}");
}

/// The seam is the cap's own line, one code part, and no quote holds it; a head cut at a line's edge keeps the
/// line's words and leaves its line break to the seam.
#[test]
fn a_seam_is_code_and_never_part_of_a_quote() {
    let rows: String = (1..=40)
        .map(|i| format!("row {i:02} of the tide table\n"))
        .collect();
    let mut fields = Fields(HashMap::new());
    let parts = vec![
        Part::Code("[exit code 0]\n".into()),
        fields.quote(rows.clone(), 0..rows.len(), Form::Verbatim),
    ];
    let text = fields.render(&parts);
    let (capped, cut) = cap_with(parts, fields.read(), 300, rest, &Cuts::none());
    assert!(cut);
    let (h, t) = toolrun::cut_points(&text, 300, &Cuts::none()).unwrap();
    assert!(text[..h].ends_with('\n'), "the head ends at a line's edge");
    let seam = toolrun::seam(&text, h, t, rest);
    let start = "[exit code 0]\n".len();
    let quote = |from: usize, to: usize| {
        Part::Quote(Quote {
            node: "msg_0".into(),
            position: 1,
            field: crate::quotes::Field::Text,
            from: from as u32,
            to: to as u32,
            form: Form::Verbatim,
        })
    };
    assert_eq!(
        capped,
        vec![
            Part::Code("[exit code 0]\n".into()),
            quote(0, h - 1 - start),
            Part::Code(seam.clone()),
            quote(t - start, rows.len()),
        ]
    );
    assert!(seam.contains("not shown: "), "{seam}");
    for p in &capped {
        if let Part::Quote(q) = p {
            let shown = &rows[q.from as usize..q.to as usize];
            assert!(!shown.contains("not shown"), "{shown}");
        }
    }
}

/// An open's margined quote cut inside a line: the whole lines it keeps stay one `Margin1` quote, the cut line's
/// margin and break are code, and its kept words a `Line1` quote of exactly those bytes; a run of whole lines that
/// ends in an empty line (which `Margin1` of its bytes would drop) is kept line by line.
#[test]
fn a_cut_inside_an_opens_line_keeps_its_margin_as_code_and_its_words_as_line1() {
    let q = |from: u32, to: u32, form: Form| {
        Part::Quote(Quote {
            node: "msg_0".into(),
            position: 1,
            field: crate::quotes::Field::Text,
            from,
            to,
            form,
        })
    };
    let bytes = "first line\n\tsecond\r\nthird";
    let whole = Quote {
        node: "msg_0".into(),
        position: 1,
        field: crate::quotes::Field::Text,
        from: 0,
        to: bytes.len() as u32,
        form: Form::Margin1,
    };
    let shown = Form::Margin1.apply(bytes);
    assert_eq!(
        shown,
        "\u{2502} first line\n\u{2502}  second\n\u{2502} third"
    );
    let head = tidy(within(&whole, bytes, 0..23, &Cuts::none()));
    assert_eq!(
        head,
        vec![
            q(0, 10, Form::Margin1),
            Part::Code("\n\u{2502} ".into()),
            q(11, 15, Form::Line1),
        ]
    );
    let tail = tidy(within(&whole, bytes, 23..shown.len(), &Cuts::none()));
    assert_eq!(
        tail,
        vec![
            q(15, 18, Form::Line1),
            Part::Code("\n".into()),
            q(20, 25, Form::Margin1),
        ]
    );
    let fields = Fields(HashMap::from([("msg_0".to_string(), bytes.to_string())]));
    assert_eq!(fields.render(&head), shown[..23]);
    assert_eq!(fields.render(&tail), shown[23..]);
    // A run of whole lines ending in an empty one, then a cut line.
    let bytes = "one\n\ntwo";
    let whole = Quote {
        to: bytes.len() as u32,
        ..whole
    };
    let shown = Form::Margin1.apply(bytes);
    let head = tidy(within(&whole, bytes, 0..18, &Cuts::none()));
    assert_eq!(
        head,
        vec![
            Part::Code("\u{2502} ".into()),
            q(0, 3, Form::Line1),
            Part::Code("\n\u{2502} \n\u{2502} ".into()),
            q(5, 6, Form::Line1),
        ]
    );
    let fields = Fields(HashMap::from([("msg_0".to_string(), bytes.to_string())]));
    assert_eq!(fields.render(&head), shown[..18]);
}

/// The fit note before a result and the hint and group line after it are the runtime's words, code, exactly as the
/// text path writes them.
#[test]
fn the_runtime_words_around_parts_are_code() {
    let mut fields = Fields(HashMap::new());
    let parts = vec![fields.quote("the tide table".into(), 0..14, Form::Verbatim)];
    let noted = noted(Some("read with offset 0"), parts.clone());
    assert_eq!(noted[0], Part::Code(toolrun::noted("read with offset 0")));
    assert_eq!(fields.render(&noted), "read with offset 0\nthe tide table");
    let ended = ended(
        noted,
        Some("call the history tool"),
        Some("ran at once with 1 other call"),
    );
    assert_eq!(
        fields.render(&ended),
        "read with offset 0\nthe tide table\ncall the history tool\nran at once with 1 other call"
    );
    assert!(matches!(ended.last(), Some(Part::Code(_))));
    assert_eq!(super::ended(parts.clone(), None, None), parts);
    assert_eq!(super::noted(None, parts.clone()), parts);
}

/// What the cut keeps of each quote, on each side of the seam: its bytes, the ranges of one quote that touch (or
/// that only a line break parts, which an open's lines keep) made one.
fn kept_ranges(fields: &Fields, parts: &[Part]) -> Vec<(String, Range<usize>)> {
    let mut out: Vec<(String, Range<usize>)> = Vec::new();
    for p in parts {
        let Part::Quote(q) = p else {
            continue;
        };
        let f = &fields.0[&q.node];
        let r = q.from as usize..q.to as usize;
        match out.last_mut() {
            Some((node, last))
                if *node == q.node
                    && last.end <= r.start
                    && f[last.end..r.start].chars().all(|c| c == '\n' || c == '\r') =>
            {
                last.end = r.end;
            }
            _ => out.push((q.node.clone(), r)),
        }
    }
    out
}

/// The span oracle for the cap over parts: over every form, with every encoding, wrapped and folded ones too (which
/// an open's margin or a page's one line hides from the cut on the text), each found where it was put. What it
/// checks is each kept run of a field: the quotes of one node that the cap kept side by side, with nothing but line
/// breaks between them, are joined first (`kept_ranges`), and the run begins and ends outside every span the
/// scrubber's finder sees in the field. So it states the edges of the runs a cut keeps, not of each stored quote's
/// own `from..to` (a quote may end at a line break inside a run). And what it shows is the cap of the text, or that
/// cut shorter at a value's edge.
#[test]
fn the_kept_runs_of_a_cut_through_parts_never_begin_or_end_inside_an_encoded_known_value() {
    let mut draw = Draw::new(0x2545_f491_4f6c_dd1d);
    let (mut checked, mut shorter) = (0, 0);
    for round in 0..150 {
        let value = VALUES[round % VALUES.len()];
        let cuts = knowing(&[value]).cuts();
        let mut fields = Fields(HashMap::new());
        let mut parts = Vec::new();
        for _ in 0..2 + draw.below(4) {
            let n = draw.below(12);
            parts.push(Part::Code(format!(" {} ", draw.filler(n))));
            let (f, at) = draw.text_at(value);
            each_found(&f, value, &at);
            let form = [
                Form::Verbatim,
                Form::Flat1,
                Form::Clip1,
                Form::Margin1,
                Form::Line1,
            ][draw.below(5)];
            parts.push(fields.quote(f.clone(), 0..f.len(), form));
        }
        let text = fields.render(&parts);
        let chars = text.chars().count();
        for max in (64..chars).step_by(3 + round % 5) {
            let (capped, cut) = cap_with(parts.clone(), fields.read(), max, rest, &cuts);
            assert!(cut);
            let seam = (capped.iter())
                .position(|p| matches!(p, Part::Code(c) if c.contains(" not shown: ")))
                .expect("one seam");
            for side in [&capped[..seam], &capped[seam + 1..]] {
                for (node, r) in kept_ranges(&fields, side) {
                    let spans = spans_of(&fields.0[&node], &[value]);
                    for edge in [r.start, r.end] {
                        assert_eq!(
                            inside(&spans, edge),
                            None,
                            "max {max}: {node} kept {r:?}, which ends inside a value: {:?}",
                            fields.0[&node]
                        );
                    }
                }
            }
            let (h, t) = toolrun::cut_points(&text, max, &cuts).unwrap();
            // The seam's code may hold code beside it: the head and the tail are what lies outside its marks.
            let shown = fields.render(&capped);
            let head = &shown[..shown.find("\n\u{2026}[").unwrap()];
            let tail = &shown[shown.find("]\u{2026}\n").unwrap() + "]\u{2026}\n".len()..];
            assert!(
                text.starts_with(head) && head.len() <= head_end(&text, h),
                "max {max}"
            );
            assert!(
                text.ends_with(tail) && tail.len() <= text.len() - t,
                "max {max}"
            );
            shorter += usize::from(head.len() < head_end(&text, h) || tail.len() < text.len() - t);
            checked += 1;
        }
    }
    assert!(checked > 2_000, "{checked}");
    assert!(
        shorter > 0,
        "no cut met a value the rendering hides: the oracle tried nothing"
    );
}

/// Each range the cap over parts would store, over the same cases as the runs' test, one quote at a time: where a
/// quote of it begins or ends inside a value (the scrubber's own finder, `quotes::cut_through`), the runtime's check
/// says no (`quotes::edges_hold`), and the result keeps its words; where none does, it says yes.
#[test]
fn a_capped_quote_that_ends_inside_a_value_is_never_stored_by_reference() {
    let mut draw = Draw::new(0x2545_f491_4f6c_dd1d);
    let (mut checked, mut refused) = (0, 0);
    for round in 0..150 {
        let value = VALUES[round % VALUES.len()];
        let cuts = knowing(&[value]).cuts();
        let mut fields = Fields(HashMap::new());
        let mut parts = Vec::new();
        for _ in 0..2 + draw.below(4) {
            let n = draw.below(12);
            parts.push(Part::Code(format!(" {} ", draw.filler(n))));
            let (f, _) = draw.text_at(value);
            let form = [
                Form::Verbatim,
                Form::Flat1,
                Form::Clip1,
                Form::Margin1,
                Form::Line1,
            ][draw.below(5)];
            parts.push(fields.quote(f.clone(), 0..f.len(), form));
        }
        let chars = fields.render(&parts).chars().count();
        for max in (64..chars).step_by(3 + round % 5) {
            let (capped, _) = cap_with(parts.clone(), fields.read(), max, rest, &cuts);
            let through = crate::quotes::cut_through(&capped, fields.read(), &[value]);
            let held = crate::quotes::edges_hold_with(&capped, fields.read(), &cuts);
            assert_eq!(held, through.is_empty(), "max {max}: {through:#?}");
            refused += usize::from(!held);
            checked += 1;
        }
    }
    assert!(checked > 2_000, "{checked}");
    // The cap alone does not keep this rule: a quote it keeps may end at a line break inside a value its run holds
    // whole. The check refuses those, and the runtime stores their words.
    assert!(
        refused > 0,
        "no capped quote ended inside a value: the check refused nothing"
    );
    eprintln!("capped {checked}, refused {refused}");
}
