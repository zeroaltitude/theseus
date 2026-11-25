//! A head of a written answer (issue 49): its parts show the head of its text, a quote the edge falls in cut to
//! the bytes whose form shows the head of its words there, and never past a known value's edge in them. All
//! values invented.

use super::*;
use crate::quotes::{Field, Form, Read};
use crate::view::text::kept::sample::{self, Draw};

/// What `w`'s parts show, each quote's field being `field`.
fn shows(w: &Written, field: &str) -> String {
    let (_, parts) = w.clone().into_parts();
    crate::quotes::render_with(&parts, |_| Read::Field(field.to_string()))
}

/// `field`'s bytes from `from` to `to` (each to a character's edge), quoted in `form`, between two code parts.
fn written(field: &str, (from, to): (usize, usize), form: Form) -> Written {
    let edge = |mut i: usize| {
        while !field.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (from, to) = (edge(from), edge(to).max(edge(from)));
    let mut w = Written::default();
    w.code("before: ");
    w.quote(
        Quote {
            node: "msg_1".into(),
            position: 7,
            field: Field::Text,
            from: from as u32,
            to: to as u32,
            form,
        },
        field,
    );
    w.code(" :after");
    w
}

/// Over random words in every form, cut at every edge of the text: the parts show the text's head there, exactly,
/// unless the words hold a known value or an open's `\r\n` (whose margin form is no head of its bytes'); then a
/// head of it, and a quote the head cuts ends inside no value.
#[test]
fn a_head_of_written_parts_shows_the_head_of_their_text() {
    let mut draw = Draw::new(0x2545_f491_4f6c_dd1d);
    for case in 0..400 {
        let (field, known) = draw.case(40, 60);
        let cuts = sample::cuts(&known);
        let form = [
            Form::Verbatim,
            Form::Flat1,
            Form::Clip1,
            Form::Margin1,
            Form::Line1,
        ][draw.below(5)];
        let (a, b) = (draw.below(field.len() + 1), draw.below(field.len() + 1));
        let w = written(&field, (a.min(b), a.max(b)), form);
        let text = w.text().to_string();
        // Where the whole quote ends: a head that keeps less ends it again.
        let whole = (w.clone().into_parts().1.iter())
            .find_map(|p| match p {
                Part::Quote(q) => Some(q.to),
                Part::Code(_) => None,
            })
            .unwrap_or(0);
        assert_eq!(shows(&w, &field), text, "case {case}");
        let exact = known.is_empty() && !(form == Form::Margin1 && field.contains('\r'));
        for k in (0..=text.len()).filter(|k| text.is_char_boundary(*k)) {
            let h = w.clone().head(k, &cuts);
            let shown = shows(&h, &field);
            assert_eq!(shown, h.text(), "case {case} at {k}");
            match exact {
                true => assert_eq!(
                    shown,
                    text[..k],
                    "case {case} at {k}: {field:?} in {form:?}"
                ),
                false => assert!(text[..k].starts_with(&shown), "case {case} at {k}"),
            }
            let (_, parts) = h.into_parts();
            for p in &parts {
                if let Part::Quote(q) = p {
                    if q.to == whole {
                        continue;
                    }
                    for (s, e) in sample::verbatim(&field, &known) {
                        let to = q.to as usize;
                        assert!(
                            !(s < to && to < e),
                            "case {case} at {k}: a quote ends inside a known value: {q:?} of {field:?}"
                        );
                    }
                }
            }
        }
    }
}

/// A known value that the one-line form hides from a cut on the text (a run of blanks inside it made one space)
/// is not cut through: the quote stops before it.
#[test]
fn a_head_stops_before_a_value_its_form_hides() {
    let v = "kelp-0417  ferry-vault";
    let cuts = sample::cuts(&[v]);
    let field = format!("the tide {v} at dawn");
    let w = written(&field, (0, field.len()), Form::Flat1);
    // The text shows the value as `kelp-0417 ferry-vault`, which the cuts do not know: cut inside it.
    let k = "before: the tide kelp-0417 fer".len();
    let h = w.head(k, &cuts);
    assert_eq!(h.text(), "before: the tide ");
    assert!(!shows(&h, &field).contains("kelp"));
}

/// Words a text does not begin with are never written as the code's: the fixed line of a quote that cannot be
/// read stands in their place.
#[test]
fn said_words_the_text_does_not_begin_with_are_not_written() {
    let field = "the tide at dawn";
    let q = quoted(
        ("msg_1", 7),
        (Field::Text, field),
        0..field.len(),
        Form::Flat1,
    );
    let mut w = Written::default();
    w.said("other words entirely", q.as_ref());
    let (text, parts) = w.into_parts();
    assert_eq!(text, UNREADABLE_QUOTE);
    assert_eq!(parts, vec![Part::Code(UNREADABLE_QUOTE.into())]);
}
