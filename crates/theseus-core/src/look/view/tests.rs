//! The model's own look at its view: its bound. Presence and today's sources, through a whole core, are
//! `tests_view_record.rs`.

use theseus_protocol::view::{ViewPiece, ViewShown, ViewSlot};

use super::*;

fn pieces(n: usize) -> Vec<ViewPiece> {
    (0..n)
        .map(|i| ViewPiece {
            handle: format!("x:t{i}"),
            ..ViewPiece::default()
        })
        .collect()
}

#[test]
fn the_look_lists_at_most_its_bound_and_says_how_many_more_it_may_read() {
    let v = ViewShown {
        slots: vec![
            ViewSlot {
                slot: "here".into(),
                pieces: pieces(100),
                tokens: 0,
            },
            ViewSlot {
                slot: "now".into(),
                pieces: pieces(3),
                tokens: 0,
            },
        ],
        ..ViewShown::default()
    };
    let said = bounded(v, 0);
    assert_eq!(said.matches("x:t").count(), PIECES, "{said}");
    assert!(
        !said.contains("  now "),
        "a slot with nothing listed is not named"
    );
    assert!(said.contains(&format!(
        "(43 more pieces you may read are not listed: the look lists {PIECES})"
    )));
    let small = ViewShown {
        slots: vec![ViewSlot {
            slot: "here".into(),
            pieces: pieces(2),
            tokens: 0,
        }],
        ..ViewShown::default()
    };
    assert!(!bounded(small.clone(), 0).contains("more pieces"));
    let said = bounded(small, 3);
    assert!(
        said.contains(&format!(
            "  (3 folds or task cards were not checked and are not listed: a look proves at most {PROOF} of \
             their sources)\n"
        )),
        "{said}"
    );
    assert!(!said.contains("more pieces"), "{said}");
}

#[test]
fn a_missing_view_does_not_suggest_a_retired_setting() {
    assert!(NONE.contains("no view record"));
    assert!(!NONE.contains("assembly"));
    assert!(!NONE.contains("transcript"));
}
