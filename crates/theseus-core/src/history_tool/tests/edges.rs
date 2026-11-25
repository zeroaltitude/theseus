//! A look back's writers never store a range that begins or ends inside a known value (issue 49): the one renderer
//! refuses no in-bounds range, so each writer puts its quotes' edges where a cut may fall (`Cuts`), verbatim or in
//! any encoding the snapshot reads. Each writer is checked with a made-up value across each edge it stores: the
//! range holds the value whole or not at all (the scrubber's own span finder, `quotes::cut_through`), and what is
//! rendered from it holds no part of it. All values and words invented.

use super::parts::put_at;
use super::*;
use crate::history_tool::page::{REPLY_CHARS, WORDS_CHARS};
use crate::history_tool::{imports, open::open_marked, session};
use crate::quotes::{self, cut_through, no_part_of, Part, Quote, Read};
use crate::scrub::{oracle::encodings, Cuts, Scrubber};

const SID: &str = "ses_edges";
const VALUE: &str = "kelp-ferry-0417-vault-token";
/// A value that its rewrite to one line changes (a space, a backslash, the page's quote marks).
const ODD: &str = "tern\\gull 0417/\u{ab}x\u{bb}";
/// Values that begin or end with a blank, which a writer's trim of a message's edges would cut through.
const LEAD: &str = "\tkelp-ferry-0417-vault-token";
const TRAIL: &str = "tern-gull-0419-vault-token\n";

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

/// An open of an exchange and an open of its session, through their writers: a message and a reply that begin and
/// end with a value whose own edge is a blank, which their trim cut through; each range holds both whole.
#[test]
fn an_opens_quotes_keep_a_value_at_a_trimmed_edge_whole() {
    let (_d, s) = store();
    let x = ex(1, HERE, SID);
    let turn = x.turn.clone();
    let words = format!("{LEAD} lies by the weir and {TRAIL}");
    put_at(&s, &Ex { words, ..x }, &format!("{LEAD} is noted, {TRAIL}"));
    let l = Lens {
        cuts: knowing(&[LEAD, TRAIL]),
        ..lens(&s, HERE)
    };
    let o = open_marked(&l, &Handle(turn)).unwrap();
    assert!(
        o.parts.iter().any(|p| matches!(p, Part::Quote(_))),
        "{}",
        o.text
    );
    holds_whole(&s, &o.text, &o.parts, &[LEAD, TRAIL]);
    let o = session::open(&l, SID, None).unwrap();
    assert!(o.parts.iter().any(|p| matches!(p, Part::Quote(_))));
    holds_whole(&s, &o.text, &o.parts, &[LEAD, TRAIL]);
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

/// The import search page, through its writer: a hit whose excerpt the search began inside one value and ended
/// inside another quotes only the words between them, and shows no part of either.
#[test]
fn an_import_hits_quote_never_begins_or_ends_inside_a_known_value() {
    use crate::memory_lookup::ImportHit;
    let (_d, s) = store();
    let sid = format!("{}{}", crate::import::SESSION_PREFIX, "e".repeat(64));
    let field = format!("the log says {VALUE} then the harbour lantern {VALUE} and more");
    let n = Node::imported(
        "imp_e0".into(),
        &sid,
        "openclaw",
        1_790_000_000_000,
        Body::Imported {
            text: field.clone(),
            integrity: crate::import::Integrity::Operator,
            source: "openclaw-store".into(),
            unit: "u0".into(),
            sha256: "0".repeat(64),
            idx: 0,
        },
    );
    s.append(&[n.record().unwrap()]).unwrap();
    let (a, b) = (
        field.find(VALUE).unwrap() + 9,
        field.rfind(VALUE).unwrap() + 9,
    );
    let hit = ImportHit {
        node_id: "imp_e0".into(),
        session_id: sid,
        at_ms: 1_790_000_000_000,
        book: None,
        text: format!("{}\u{2026}", &field[a..b]),
    };
    let cuts = knowing(&[VALUE]);
    let (lines, _, dropped) = imports::project(&s, vec![hit], &cuts);
    assert_eq!((lines.len(), dropped), (1, 0));
    let p = HistoryPage {
        lines,
        next: None,
        end: PageEnd::HistoryExhausted,
    };
    let a = args(json!({"place": "import", "words": "harbour"}));
    let (text, parts) = imports::written_page(&p, &a, None, None, &cuts).into_parts();
    holds_whole(&s, &text, &parts, &[VALUE]);
    assert!(no_part_of(&text, VALUE), "{text}");
    assert!(
        text.contains("\u{ab}then the harbour lantern\u{2026}\u{bb}"),
        "{text}"
    );
}
