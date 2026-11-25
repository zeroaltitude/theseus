//! A value written over lines (a key file's) in an open's `Margin1` quote: the cap over parts keeps the lines it
//! runs over one quote, or cuts outside it, so no range it stores begins or ends inside the value and the check never
//! refuses it. All values invented.

use super::*;
use crate::quotes::{cut_through, edges_hold_with};

/// A value written over two lines, as a key file's is; and the same with a carriage return before its line break.
const KEY: &str = "kelp-ferry-0417-north\nvault-token-9932-south";
const KEY_CR: &str = "kelp-ferry-0417-north\r\nvault-token-9932-south";

/// An open's code line, then its message: a note, `key` over its lines, an empty line and one long line, each line
/// ending in `brk`. A head cut in the long line keeps the lines before it whole, and their run ends in the empty
/// one, which one `Margin1` quote of their bytes would drop.
fn keyed(fields: &mut Fields, key: &str, brk: &str) -> Vec<Part> {
    let f = format!(
        "the keeper's note{brk}{key}{brk}{brk}{}",
        "the tide table ".repeat(40)
    );
    vec![
        Part::Code("[exchange 1]\n".into()),
        fields.quote(f.clone(), 0..f.len(), Form::Margin1),
    ]
}

/// The quote of `parts` whose bytes are `value`, whole.
fn whole<'p>(fields: &Fields, parts: &'p [Part], value: &str) -> Option<&'p Quote> {
    parts.iter().find_map(|p| match p {
        Part::Quote(q) if fields.0[&q.node][q.from as usize..q.to as usize] == *value => Some(q),
        _ => None,
    })
}

/// The cap of [`keyed`] at 300 characters, its head cut in the long line: the key's lines are one `Margin1` quote,
/// no quote begins or ends inside the key, the check holds, and the parts show what the cap of the text shows.
fn keeps_the_key_in_one_quote(key: &str, brk: &str) {
    let cuts = knowing(&[key]).cuts();
    let mut fields = Fields(HashMap::new());
    let parts = keyed(&mut fields, key, brk);
    let text = fields.render(&parts);
    let (h, _) = toolrun::cut_points(&text, 300, &cuts).expect("capped");
    let past = text.find("south").unwrap() + "south".len() + 8;
    assert!(
        h > past,
        "the head cut falls in the long line: {h} {text:?}"
    );
    let (capped, cut) = cap_with(parts, fields.read(), 300, rest, &cuts);
    assert!(cut);
    let through = cut_through(&capped, fields.read(), &[key]);
    assert!(through.is_empty(), "a quote cuts the key: {through:#?}");
    assert!(
        edges_hold_with(&capped, fields.read(), &cuts),
        "{capped:#?}"
    );
    assert_eq!(
        whole(&fields, &capped, key).map(|q| q.form),
        Some(Form::Margin1),
        "the key's lines are one quote: {capped:#?}"
    );
    let (want, _) = toolrun::cap(&text, 300, rest, &cuts);
    assert_eq!(fields.render(&capped), want);
}

#[test]
fn a_cut_through_a_margined_quote_keeps_a_value_over_lines_in_one_quote() {
    keeps_the_key_in_one_quote(KEY, "\n");
}

/// The head-side twin with carriage returns: each line of the field ends in `\r\n`, the key's own break too.
#[test]
fn a_head_cut_through_a_margined_quote_with_carriage_returns_keeps_a_value_in_one_quote() {
    keeps_the_key_in_one_quote(KEY_CR, "\r\n");
}

/// A cut over an open's `Margin1` quote of a field holding carriage returns, alone and before line breaks, with
/// each one-line encoding of a value between them: at every length, no quote it keeps begins or ends inside the
/// value. The fault a head cut over parts was found to have where it was routed through this cap's machinery
/// (`Written::head`); this states that the cap's own head and tail do not share it.
#[test]
fn a_cut_over_a_margined_field_with_carriage_returns_never_ends_inside_a_value_on_one_line() {
    let mut draw = Draw::new(0x6a09_e667_f3bc_c908);
    let mut checked = 0;
    for round in 0..60 {
        let value = VALUES[round % VALUES.len()];
        let cuts = knowing(&[value]).cuts();
        let mut fields = Fields(HashMap::new());
        let mut f = String::new();
        for e in one_line(value) {
            let n = draw.below(6);
            f.push_str(&draw.filler(n).replace('\n', "\r\n"));
            f.push_str(["\r", "\r\n", " ", "\r\n\r\n", "\r "][draw.below(5)]);
            f.push_str(&e);
            f.push_str(["\r", "\r\n", " ", "\r\n\r\n", " \r"][draw.below(5)]);
        }
        let parts = vec![
            Part::Code("[exchange 1]\n".into()),
            fields.quote(f.clone(), 0..f.len(), Form::Margin1),
        ];
        let chars = fields.render(&parts).chars().count();
        for max in 40..chars {
            let (capped, _) = cap_with(parts.clone(), fields.read(), max, rest, &cuts);
            let through = cut_through(&capped, fields.read(), &[value]);
            assert!(through.is_empty(), "max {max}: {through:#?} in {f:?}");
            checked += 1;
        }
    }
    assert!(checked > 1_000, "{checked}");
}

/// Every cut of an open's `Margin1` quote holding a value written over lines (the key, with and without a carriage
/// return, and the wrapped and the folded encodings of each value), among lines of words, empty lines and long
/// lines: no quote it keeps begins or ends inside the value, so the check never refuses it; and it shows the cap of
/// the text, or that cut shorter at the value's edge.
#[test]
fn every_cut_of_a_margined_quote_keeps_a_value_over_lines_whole_or_not_at_all() {
    let mut draw = Draw::new(0xbb67_ae85_84ca_a73b);
    let (mut checked, mut shorter) = (0, 0);
    for round in 0..48 {
        let (value, brk) = match round % 4 {
            0 => (KEY, "\n"),
            1 => (KEY_CR, "\r\n"),
            r => (VALUES[r % 2], "\n"),
        };
        let shapes: Vec<(usize, String)> = match round % 4 {
            0 | 1 => vec![(0, value.to_string())],
            _ => {
                let e = encodings(value);
                vec![(2, e[2].clone()), (5, e[5].clone())]
            }
        };
        let cuts = knowing(&[value]).cuts();
        let mut fields = Fields(HashMap::new());
        let (mut f, mut at) = (String::new(), Vec::new());
        for (i, e) in shapes {
            for _ in 0..draw.below(4) {
                let n = 1 + draw.below(12);
                f.push_str(&draw.filler(n).replace('\n', brk));
                f.push_str(brk);
            }
            f.push_str(["", " ", "the keeper's note "][draw.below(3)]);
            at.push((i, f.len()..f.len() + e.len()));
            f.push_str(&e);
            let after = [brk.to_string(), " ".into(), " the harbour lantern".into()];
            f.push_str(&after[draw.below(3)]);
            if draw.below(2) == 0 {
                f.push_str(&"the tide table ".repeat(1 + draw.below(8)));
            }
            f.push_str(brk);
            if draw.below(2) == 0 {
                f.push_str(brk);
            }
        }
        each_found(&f, value, &at);
        let parts = vec![
            Part::Code("[exchange 1]\n".into()),
            fields.quote(f.clone(), 0..f.len(), Form::Margin1),
        ];
        let text = fields.render(&parts);
        let chars = text.chars().count();
        for max in (64..chars).step_by(1 + round % 3) {
            let (capped, cut) = cap_with(parts.clone(), fields.read(), max, rest, &cuts);
            assert!(cut);
            let through = cut_through(&capped, fields.read(), &[value]);
            assert!(through.is_empty(), "max {max}: {through:#?} in {f:?}");
            assert!(edges_hold_with(&capped, fields.read(), &cuts), "max {max}");
            let (h, t) = toolrun::cut_points(&text, max, &cuts).unwrap();
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
        "no cut fell at a value's edge: the sweep tried nothing"
    );
}
