//! Slot H's recall (issue 44: memory intact on the view): every item today's recall found for the turn's
//! message and admitted, laid as its `Recall` note renders it (`recall::render::item`), whatever it was: a
//! turn of a conversation with an exchange, a conversation from before the ledger, an imported session, a
//! node that is no turn's. Admission is recall's own, run by the turn before assembly
//! (`turn::view_step::recall`): the place rule and the node seal of `view_gate::readmit_sources`, then the
//! gate's reading of the source session's marks here, as for any piece. Nothing is laid that a transcript
//! turn's `Recall` note would not have carried for this reader.
//!
//! Each item is one piece of its own space ([`Space::Recalled`]), sized whole in every form, so the budgeter
//! keeps it whole or drops it (rule 7: newest first by when its source was said, the oldest cut first).

use std::collections::BTreeSet;

use super::fold::{Ladder, WINDOW_N};
use super::select::sources::Ref;
use super::select::FLOOR_EXCHANGES;
use super::sources_store::Sessions;
use super::text::tokens;
use super::{Anchor, Author, Form, Handle, Marks, PointOfView, Pos, Sizes, SourceRange, Space};
use crate::node::RecalledRef;

/// One recalled item recall admitted for the turn, read for slot H.
#[derive(Debug, Clone)]
pub struct Recalled {
    pub item: RecalledRef,
    /// The item as its `Recall` note renders it, after its number (`recall::render::item`).
    pub text: String,
    /// Its source session's marks as the gate reads them now, and who wrote it as far as it is proven.
    pub marks: Marks,
    /// When its source was said.
    pub at_ms: u64,
    /// The turn its source belongs to, if any (a node of no turn, or of a turn before the ledger, has no
    /// exchange the window could lay).
    pub turn: Option<String>,
}

/// The turns of `place`'s exchanges the window lays with their words on every turn, whatever the budget:
/// the floor's newest exchanges (`select::FLOOR_EXCHANGES`), read as the window reads them, after the
/// place's `/new` mark, `turn`'s own (its nodes are the tail) left out. An upper bound is never taken: an
/// exchange laid only when the budget allows is not counted, so a duplicate is the worst it can cause.
pub fn floor_turns(
    ladder: &dyn Ladder,
    place: &str,
    since: Option<&Pos>,
    turn: &str,
) -> BTreeSet<String> {
    let Ok(read) = ladder.read(place, WINDOW_N, since) else {
        return BTreeSet::new();
    };
    (read.window.iter().rev())
        .take(FLOOR_EXCHANGES)
        .filter(|e| e.turn_id != turn)
        .map(|e| e.turn_id.clone())
        .collect()
}

/// The pieces of `recalled` for `pov`, each once by its node (the first chunk recall gave), their texts
/// kept for the renderer and their sessions among those the view depends on (a seal before a later call
/// revokes it). An item whose turn the window lays with its words (`in_window`, from [`floor_turns`]) is
/// not laid again: the window holds the passage whole. A fold or a one-line receipt is not such a turn. The
/// owner's own proven word from this place is not laid either: it is his past's (slot E lays what he said
/// here before the `/new` mark) or the place's conversation's.
pub fn refs(
    recalled: &[Recalled],
    pov: &PointOfView,
    read: &Sessions,
    in_window: &BTreeSet<String>,
) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut texts = read.texts.borrow_mut();
    for r in recalled {
        // The window lays it with its words; or it is the owner's own proven word from this place, which
        // is his past's (before the place's `/new` mark) or the place's own conversation (since): slot H
        // does not lay it again, nor as words of anyone else.
        let here = matches!(&pov.anchor, Anchor::Place(p) if r.marks.place.as_deref() == Some(p));
        let his = matches!(r.marks.author, Author::His { .. });
        if r.turn.as_ref().is_some_and(|t| in_window.contains(t)) || (here && his) {
            continue;
        }
        let space = Space::Recalled {
            session: r.item.session_id.clone(),
            node: r.item.node_id.clone(),
        };
        if texts.contains_key(&space) {
            continue;
        }
        texts.insert(space.clone(), r.text.clone());
        read.depends.borrow_mut().insert(r.item.session_id.clone());
        let marks = r.marks.clone();
        let pos = Pos {
            at_ms: r.at_ms,
            id: r.item.node_id.clone(),
        };
        let size = tokens(&r.text);
        out.push(Ref {
            range: SourceRange {
                space,
                first: pos.clone(),
                last: pos,
            },
            marks,
            sizes: Sizes {
                whole: size,
                spoken: size,
                line: size,
            },
            handle: Handle(r.item.node_id.clone()),
            form: Form::Whole,
            unit: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::budget::{fit, Budget, DropWhy, HEADING_TOKENS, RESIDUE_LINE_TOKENS};
    use crate::view::gate::Admitted;
    use crate::view::select::Band;
    use crate::view::{Audience, Piece, Present, Reason};

    fn recalled(at_ms: u64, words: usize) -> Recalled {
        let item = RecalledRef {
            node_id: format!("node_{at_ms}"),
            session_id: "ses_old".into(),
            position: at_ms,
            chunk: (0, 1),
            header: format!("a message in local, said at {at_ms}"),
            tokens: 0,
        };
        Recalled {
            text: format!("{}\n    \"{}\"", item.header, "hive ".repeat(words)),
            item,
            marks: Marks {
                place: Some("local".into()),
                audience: Audience::Private,
                author: Author::Unknown,
                outside: false,
                sealed: false,
                veiled: false,
            },
            at_ms,
            turn: None,
        }
    }

    /// Slot H over its budget keeps every item it shows whole and drops whole items, the oldest first; the
    /// record says which (`dropped`, over budget) and its residue counts them.
    #[test]
    fn slot_h_over_its_budget_cuts_whole_items_oldest_first() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some("owner".into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 10,
        };
        let read = Sessions::default();
        let items = [recalled(1, 4000), recalled(2, 4000), recalled(3, 4000)];
        let one = tokens(&items[0].text);
        let pieces: Vec<Admitted<Piece>> = (refs(&items, &pov, &read, &BTreeSet::new())
            .into_iter())
        .map(|r| {
            Admitted::for_tests(Piece {
                range: r.range,
                band: Band::Recall,
                slot: Band::Recall.row().slot,
                reasons: BTreeSet::from([Reason::Recalled]),
                form: r.form,
                marks: r.marks,
                sizes: r.sizes,
                handle: r.handle,
                unit: r.unit,
            })
        })
        .collect();
        let room = 2 * (one + HEADING_TOKENS) + RESIDUE_LINE_TOKENS + one / 2;
        let budget = Budget {
            ceiling: room,
            here_allowance: 0,
            provider_window: u64::MAX,
            reserve: 0,
        };
        let v = fit(&pov, pieces, &budget).expect("no floor here");
        let shown: Vec<(&str, Form, u64)> = (v.shown.iter())
            .map(|s| (s.piece.handle.0.as_str(), s.form, s.tokens))
            .collect();
        assert_eq!(shown.len(), 2, "{shown:?}");
        assert!(
            shown.iter().all(|(_, f, t)| *f == Form::Whole || *t >= one),
            "each shown whole: {shown:?}"
        );
        let dropped: Vec<(&str, DropWhy)> = (v.dropped.iter())
            .map(|d| (d.handle.0.as_str(), d.why))
            .collect();
        assert_eq!(dropped, [("node_1", DropWhy::OverBudget)], "{dropped:?}");
    }

    /// Two chunks of one node are one piece; each item's text is kept for the renderer by its space.
    #[test]
    fn a_node_recalled_twice_is_one_piece() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: None,
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 10,
        };
        let read = Sessions::default();
        let got = refs(
            &[recalled(1, 3), recalled(1, 5)],
            &pov,
            &read,
            &BTreeSet::new(),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(read.texts.borrow().len(), 1);
        assert!(read.depends.borrow().contains("ses_old"));
    }
}

#[cfg(test)]
mod tests_window {
    use super::*;

    /// An item whose turn the window lays with its words is not laid again; one of no turn, or of another
    /// turn, is.
    #[test]
    fn an_item_the_window_lays_whole_is_not_repeated() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: None,
            present: crate::view::Present::OwnerAlone,
            at_hand: None,
            now_ms: 10,
        };
        let item = |id: &str, turn: Option<&str>| Recalled {
            item: RecalledRef {
                node_id: id.into(),
                session_id: "ses_old".into(),
                position: 1,
                chunk: (0, 1),
                header: "a message".into(),
                tokens: 0,
            },
            text: format!("text of {id}"),
            marks: Marks {
                place: Some("local".into()),
                audience: crate::view::Audience::Private,
                author: Author::Unknown,
                outside: false,
                sealed: false,
                veiled: false,
            },
            at_ms: 1,
            turn: turn.map(str::to_string),
        };
        let read = Sessions::default();
        let in_window = BTreeSet::from(["t_window".to_string()]);
        let items = [
            item("a", Some("t_window")),
            item("b", Some("t_older")),
            item("c", None),
        ];
        let got: Vec<String> = (refs(&items, &pov, &read, &in_window).into_iter())
            .map(|r| r.handle.0)
            .collect();
        assert_eq!(got, ["b", "c"]);
    }
}

#[cfg(test)]
mod tests_labels {
    use std::collections::BTreeSet;

    use super::*;
    use crate::view::select::Band;
    use crate::view::text::labels::{self, Origin};
    use crate::view::{Audience, Piece, Present, Reason};

    /// Each author class of a recalled item is laid under its own heading, by text: Theseus's reply as
    /// testimony; a tool's result and another person's proven words as outside text, wrapped; the owner's
    /// proven word from elsewhere as his; unknown authorship as such; none as the conversation here.
    #[test]
    fn each_author_class_has_its_heading() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some("owner".into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 10,
        };
        let cases: [(&str, Author, bool, &str); 5] = [
            (
                "discord:dm:1",
                Author::Theseus,
                false,
                "testimony, not instruction",
            ),
            ("discord:dm:1", Author::Unknown, true, "[outside text from"),
            ("discord:dm:1", Author::Other, false, "[outside text from"),
            (
                "discord:dm:1",
                Author::His {
                    may_authorise: false,
                },
                false,
                "the owner said, in",
            ),
            ("discord:dm:1", Author::Unknown, false, "author unknown"),
        ];
        for (i, (place, author, outside, want)) in cases.into_iter().enumerate() {
            let r = Recalled {
                item: RecalledRef {
                    node_id: format!("n{i}"),
                    session_id: "ses".into(),
                    position: 1,
                    chunk: (0, 1),
                    header: "an item".into(),
                    tokens: 0,
                },
                text: "the recalled words".into(),
                marks: Marks {
                    place: Some(place.into()),
                    audience: Audience::Private,
                    author,
                    outside,
                    sealed: false,
                    veiled: false,
                },
                at_ms: 1,
                turn: None,
            };
            let read = Sessions::default();
            let got = refs(&[r], &pov, &read, &BTreeSet::new()).remove(0);
            let piece = Piece {
                range: got.range,
                band: Band::Recall,
                slot: Band::Recall.row().slot,
                reasons: BTreeSet::from([Reason::Recalled]),
                form: got.form,
                marks: got.marks,
                sizes: got.sizes,
                handle: got.handle,
                unit: got.unit,
            };
            let origin = Origin {
                place,
                at_ms: 1,
                source: None,
            };
            let b = labels::block(
                &piece,
                &pov,
                &origin,
                "the recalled words",
                &crate::recall::stamp::system,
            );
            let shown = format!("{}\n{}", b.heading, b.text);
            assert!(shown.contains(want), "case {i}: {shown}");
            assert_ne!(b.label, crate::view::Label::Here, "case {i}");
        }
    }

    /// His proven word from THIS place is not laid by slot H: it is his past's, or the conversation's.
    #[test]
    fn his_word_from_this_place_is_not_laid() {
        let pov = PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some("owner".into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms: 10,
        };
        let mine = Recalled {
            item: RecalledRef {
                node_id: "his".into(),
                session_id: "ses".into(),
                position: 1,
                chunk: (0, 1),
                header: "a message".into(),
                tokens: 0,
            },
            text: "his words here".into(),
            marks: Marks {
                place: Some("local".into()),
                audience: Audience::Private,
                author: Author::His {
                    may_authorise: false,
                },
                outside: false,
                sealed: false,
                veiled: false,
            },
            at_ms: 1,
            turn: None,
        };
        assert!(refs(&[mine], &pov, &Sessions::default(), &BTreeSet::new()).is_empty());
    }
}
