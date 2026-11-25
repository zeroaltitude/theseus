//! What the owner's listing prints of a record made only of the kinds it listed before folds and task cards
//! were proven: the same bytes as before.

use std::collections::BTreeSet;

use theseus_protocol::view::lines;

use super::show::{Reader, Source};
use super::ViewRecord;
use crate::view::budget::{DropWhy, Dropped, Residue, Shown, View};
use crate::view::select::Band;
use crate::view::{
    Anchor, AtHand, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Present,
    Provenance, Reason, Sizes, SourceRange, Space,
};

/// Every source may be shown.
struct All;

impl Source for All {
    fn may_show(&self, _: &SourceRange, _: Option<&Marks>) -> bool {
        true
    }
    fn at_hand(&self, _: &AtHand) -> bool {
        true
    }
}

fn piece(space: Space, id: &str, band: Band, reason: Reason, whole: u64) -> Piece {
    let pos = Pos {
        at_ms: 1,
        id: id.into(),
    };
    Piece {
        range: SourceRange {
            space,
            first: pos.clone(),
            last: pos,
        },
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([reason]),
        form: Form::Whole,
        marks: Marks {
            place: Some("local".into()),
            audience: Audience::Private,
            author: Author::His {
                may_authorise: true,
            },
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole,
            spoken: whole / 2,
            line: whole / 4,
        },
        handle: Handle(format!("x:{id}")),
        unit: None,
    }
}

fn exchanges(place: &str) -> Space {
    Space::Exchanges {
        place: place.into(),
    }
}

/// One piece of each kind listed before: code, exchanges here and elsewhere, a person's facts, a named
/// thing's card, a waiting line and a recalled item.
fn todays_pieces() -> Vec<Piece> {
    let from = || Some(Provenance::default());
    vec![
        piece(
            Space::Code {
                part: "system".into(),
            },
            "system",
            Band::Frame,
            Reason::Fixed,
            900,
        ),
        piece(exchanges("local"), "t2", Band::Floor, Reason::Floor, 120),
        piece(
            exchanges("local"),
            "t1",
            Band::WindowWords,
            Reason::Window,
            80,
        ),
        piece(
            exchanges("discord:dm:9"),
            "far",
            Band::Everywhere,
            Reason::Elsewhere,
            60,
        ),
        piece(
            Space::Facts {
                thing: "person:owner".into(),
            },
            "fact_1",
            Band::WhoAndWhere,
            Reason::Fixed,
            30,
        ),
        piece(
            Space::Card {
                thing: "thread:kelp".into(),
                from: from(),
            },
            "thread:kelp",
            Band::Named,
            Reason::Named,
            50,
        ),
        piece(
            Space::Waiting {
                id: "q_1".into(),
                from: from(),
            },
            "q_1",
            Band::Waiting,
            Reason::Waiting,
            20,
        ),
        piece(
            Space::Recalled {
                session: "s1".into(),
                node: "n_9".into(),
            },
            "n_9",
            Band::Recall,
            Reason::Recalled,
            40,
        ),
    ]
}

/// [`todays_pieces`], with a drop and its band's residue.
fn todays_kinds() -> ViewRecord {
    let pieces = todays_pieces();
    let shown: Vec<Shown> = (pieces.into_iter())
        .map(|p| Shown {
            form: p.form,
            tokens: p.tokens(),
            piece: p,
        })
        .collect();
    let dropped = piece(
        exchanges("local"),
        "t0",
        Band::WindowWords,
        Reason::Window,
        70,
    );
    ViewRecord::new(
        "cmp_golden",
        View {
            pov: PointOfView {
                anchor: Anchor::Place("local".into()),
                person: Some("owner".into()),
                present: Present::OwnerAlone,
                at_hand: None,
                now_ms: 7,
            },
            tokens: shown.iter().map(|s| s.tokens).sum(),
            shown,
            dropped: vec![Dropped {
                range: dropped.range,
                slot: dropped.slot,
                band: dropped.band,
                reasons: dropped.reasons,
                handle: dropped.handle,
                tokens: 70,
                why: DropWhy::OverBudget,
            }],
            residue: vec![Residue {
                band: Band::WindowWords,
                pieces: 1,
                tokens: 70,
            }],
        },
    )
}

/// What the owner was shown of [`todays_kinds`] before folds and task cards were proven.
const GOLDEN: &str = "\
view cmp_golden (view): place local, present owner_alone, at 7
  frame (900 tokens)
    x:system code system [frame] whole 900 tokens, here: fixed
  who_and_where (30 tokens)
    x:fact_1 facts fact_1 [who_and_where] whole 30 tokens, here: fixed
  waiting (20 tokens)
    x:q_1 waiting q_1 [waiting] whole 20 tokens, here: waiting
  elsewhere (60 tokens)
    x:far exchanges far [everywhere] whole 60 tokens, here: elsewhere
  here (200 tokens)
    x:t2 exchanges t2 [floor] whole 120 tokens, here: floor
    x:t1 exchanges t1 [window_words] whole 80 tokens, here: window
  called (90 tokens)
    x:thread:kelp card thread:kelp [named] whole 50 tokens, here: named
    x:n_9 recalled n_9 [recall] whole 40 tokens, here: recalled
  dropped
    x:t0 exchanges here [window_words] 70 tokens, over_budget: window
  residue window_words: 1 pieces, 70 tokens
  total: 8 shown, 1300 tokens; 1 dropped, 70 tokens
";

#[test]
fn a_record_of_todays_kinds_prints_as_before() {
    let reader = Reader {
        source: &All,
        text: None,
        restricted: false,
    };
    let said = lines(&reader.show(&todays_kinds()));
    assert_eq!(said, GOLDEN, "\n{said}");
}
