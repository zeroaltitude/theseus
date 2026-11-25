//! The writers that cut a message's words cut them where a cut may fall (`Cuts`), so the ranges they store begin
//! and end outside every known value, verbatim or in any encoding the snapshot reads (issue 49): the page's line and
//! a session open's long message. Each is checked with a made-up value across each edge it stores: the range holds
//! the value whole or not at all (the scrubber's own span finder, `quotes::cut_through`), and what is rendered from
//! it holds no part of it. A writer that does not cut (the opens' trimmed words, an import hit's excerpt) is held to
//! the same rule where the runtime stores its answer (`quotes::edges_hold`). All values and words invented.

use super::parts::put_at;
use super::*;
use crate::history_tool::page::{REPLY_CHARS, WORDS_CHARS};
use crate::history_tool::session;
use crate::quotes::{self, cut_through, no_part_of, Part, Quote, Read};
use crate::scrub::{oracle::encodings, Cuts, Scrubber};

const SID: &str = "ses_edges";
const VALUE: &str = "kelp-ferry-0417-vault-token";
/// A value that its rewrite to one line changes (a space, a backslash, the page's quote marks).
const ODD: &str = "tern\\gull 0417/\u{ab}x\u{bb}";

fn knowing(values: &[&str]) -> Cuts {
    Scrubber::with_values(
        (values.iter().enumerate())
            .map(|(i, v)| ((*v).to_string(), format!("v{i}")))
            .collect(),
    )
    .cuts()
}

/// Each quote read as it was written.
fn as_written(s: &Store) -> impl FnMut(&Quote) -> Read + '_ {
    |q| quotes::read_as_written(s, q)
}

/// That `parts` render as `text` and store no range through `values`.
fn holds_whole(s: &Store, text: &str, parts: &[Part], values: &[&str]) {
    assert_eq!(quotes::render_with(parts, as_written(s)), text);
    let through = cut_through(parts, as_written(s), values);
    assert!(
        through.is_empty(),
        "a stored range cuts a value: {through:#?}\n{text}"
    );
}

/// The history page, through its writer: a message's words and a reply's beginning, each with a value (in each
/// encoding) across the place its line is cut (300 characters, 200), at several offsets.
#[test]
fn a_pages_quotes_never_begin_or_end_inside_a_known_value() {
    let mut checked = 0;
    for value in [VALUE, ODD] {
        for (i, e) in encodings(value).into_iter().enumerate() {
            let n = e.chars().count();
            for shift in [1, n / 3, n / 2, n - 1] {
                let (_d, s) = store();
                let words = format!("{} {e} weir mill", "x".repeat(WORDS_CHARS - shift - 1));
                let reply = format!("{} {e} weir mill", "y".repeat(REPLY_CHARS - shift - 1));
                let x = ex(0, HERE, SID);
                put_at(&s, &Ex { words, ..x }, &reply);
                let l = Lens {
                    cuts: knowing(&[value]),
                    ..lens(&s, HERE)
                };
                let p = walk(&l, &HistoryArgs::default(), None, 0);
                let w = super::super::page::written_page(&p, (HERE, None), 0, None, &l.cuts);
                let (text, parts) = w.into_parts();
                assert_eq!(p.lines.len(), 1, "{text}");
                holds_whole(&s, &text, &parts, &[value]);
                if i == 0 {
                    assert!(no_part_of(&text, value), "{text}");
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 48);
}

/// An open of a session whose newest message is past its allowance, cut in words: the message is a value over and
/// over, so the cut falls across one unless it stops at a value's edge.
#[test]
fn a_session_opens_cut_never_ends_inside_a_known_value() {
    let (_d, s) = store();
    let big = format!("BIG {}", format!("{VALUE} ").repeat(6_000));
    let x = ex(1, HERE, SID);
    put_at(
        &s,
        &Ex {
            words: "a short ask".into(),
            ..x
        },
        &big,
    );
    let l = Lens {
        cuts: knowing(&[VALUE]),
        ..lens(&s, HERE)
    };
    let o = session::open(&l, SID, None).unwrap();
    assert!(
        o.text.contains("[cut here: this message is about"),
        "{}",
        o.text
    );
    holds_whole(&s, &o.text, &o.parts, &[VALUE]);
    assert!(no_part_of(&o.text, VALUE));
}
