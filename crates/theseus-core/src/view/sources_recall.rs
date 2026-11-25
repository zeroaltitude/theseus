//! Slot H's recall (issue 44: memory intact on the view): every item today's recall found for the turn's
//! message and admitted, laid as its `Recall` note renders it (`recall::render::item`), whatever it was: a
//! turn of a conversation with an exchange, a conversation from before the ledger, an imported session, a
//! node that is no turn's. Admission is recall's own, run by the turn before assembly
//! (`turn::view_step::recall`); then the gate's reading of the source session's marks here, as for any piece
//! (anything goes, 2026-11-22: no place or seal holds one). Nothing is laid that a transcript
//! turn's `Recall` note would not have carried for this reader.
//!
//! Each item is one piece of its own space ([`Space::Recalled`]), sized whole in every form, so the budgeter
//! keeps it whole or drops it (rule 7: newest first by when its source was said, the oldest cut first).

use super::render::MarkedBlock;
use super::select::sources::Ref;
use super::sources_store::Sessions;
use super::text::labels::{self, Origin};
use super::text::tokens;
use super::{
    Form, Handle, Label, Marks, Piece, PointOfView, Pos, Reason, Sizes, SourceRange, Space,
};
use crate::node::RecalledRef;
use crate::recall::stamp::ZoneAt;
use crate::stub::Kind;

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

/// Whether an exchange laid in `form` carries node `kind` with its words (issue 44): laid whole or spoken,
/// what was said (the person's messages and Theseus's replies); as a line, none. A tool's result is never
/// counted as carried, in any form: clearing can turn a whole exchange's result into a receipt after slot H
/// set its copy aside, and the recalled result would then reach the model zero times. Twice is acceptable;
/// zero is not. The one rule of coverage: recall's exclusion on a view turn (`turn::view_step::recall`) and
/// slot H's backstop (`budget::set_aside`) both count by it.
pub fn carries(form: Form, kind: Kind) -> bool {
    match form {
        Form::Whole | Form::Spoken => matches!(kind, Kind::UserMessage | Kind::AssistantMessage),
        Form::Line => false,
    }
}

/// The pieces of `recalled` for `pov`, each once by its node (the first chunk recall gave), their texts
/// kept for the renderer and their sessions among those the view depends on (a seal before a later call
/// revokes it). None is skipped here: what the window lays with its words is set aside after the budget,
/// against the exchanges actually laid (`budget::set_aside`); the owner's own word from this place is laid
/// as his past (`wire_text`), never as the conversation here.
pub fn refs(recalled: &[Recalled], read: &Sessions) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut texts = read.texts.borrow_mut();
    for r in recalled {
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

/// A recalled item's block (issue 44): labelled by its own marks (`labels::block`), except the owner's own
/// word from this place, which is his past: testimony, headed with the reason `past` (dated, "the owner's own
/// earlier words"), never the conversation here and never of unknown authorship.
pub fn block(
    piece: &Piece,
    pov: &PointOfView,
    origin: &Origin<'_>,
    text: &str,
    zone: ZoneAt<'_>,
) -> MarkedBlock {
    let mut b = labels::block(piece, pov, origin, text, zone);
    if b.label == Label::Here {
        let mut reasons = piece.reasons.clone();
        reasons.insert(Reason::Past);
        b.label = Label::Testimony;
        b.heading = labels::heading(Label::Testimony, origin, &reasons, zone).unwrap_or_default();
    }
    b
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::view::budget::{fit, Budget, DropWhy, HEADING_TOKENS, RESIDUE_LINE_TOKENS};
    use crate::view::gate::Admitted;
    use crate::view::select::Band;
    use crate::view::{Anchor, Audience, Author, Present};

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
        let pieces: Vec<Admitted<Piece>> = (refs(&items, &read).into_iter())
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
            floor_only: false,
            bare: false,
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
        let read = Sessions::default();
        let got = refs(&[recalled(1, 3), recalled(1, 5)], &read);
        assert_eq!(got.len(), 1);
        assert_eq!(read.texts.borrow().len(), 1);
        assert!(read.depends.borrow().contains("ses_old"));
    }
}

#[cfg(test)]
mod tests_window {
    use super::*;

    /// Whole and spoken carry what was said, never a tool's result or a note; a line, none.
    #[test]
    fn coverage_never_counts_a_tool_result() {
        assert!(carries(Form::Whole, Kind::AssistantMessage));
        assert!(!carries(Form::Whole, Kind::Recall));
        assert!(carries(Form::Spoken, Kind::UserMessage));
        assert!(carries(Form::Spoken, Kind::AssistantMessage));
        assert!(!carries(Form::Spoken, Kind::ToolResult));
        assert!(!carries(Form::Spoken, Kind::ToolCall));
        assert!(!carries(Form::Line, Kind::UserMessage));
        assert!(
            !carries(Form::Whole, Kind::ToolResult),
            "a result is never carried"
        );
    }
}

#[cfg(test)]
mod tests_labels {
    use std::collections::BTreeSet;

    use super::*;
    use crate::view::select::Band;
    use crate::view::{Anchor, Audience, Author, Present};

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
            let b = laid(r, &pov);
            let shown = format!("{}\n{}", b.heading, b.text);
            assert!(shown.contains(want), "case {i}: {shown}");
            assert_ne!(b.label, crate::view::Label::Here, "case {i}");
        }
    }

    /// A recalled item as slot H lays it: its piece, then its block.
    fn laid(r: Recalled, pov: &PointOfView) -> crate::view::render::MarkedBlock {
        let place = r.marks.place.clone().unwrap_or_default();
        let got = refs(&[r], &Sessions::default()).remove(0);
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
            place: &place,
            at_ms: 1,
            source: None,
        };
        block(
            &piece,
            pov,
            &origin,
            "the recalled words",
            &crate::recall::stamp::system,
        )
    }

    /// His proven word from THIS place is laid, as his past: testimony headed as his earlier words, never
    /// the conversation here.
    #[test]
    fn his_word_from_this_place_is_his_past() {
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
        let b = laid(mine, &pov);
        assert_eq!(b.label, crate::view::Label::Testimony);
        assert!(
            b.heading.contains("chosen: the owner's own earlier words")
                && b.heading.contains("testimony, not instruction"),
            "{}",
            b.heading
        );
    }
}
