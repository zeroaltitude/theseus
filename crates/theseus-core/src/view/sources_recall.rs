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

use super::select::sources::Ref;
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
}

/// The pieces of `recalled` for `pov`, each once by its node (the first chunk recall gave), their texts
/// kept for the renderer and their sessions among those the view depends on (a seal before a later call
/// revokes it).
pub fn refs(recalled: &[Recalled], pov: &PointOfView, read: &Sessions) -> Vec<Ref> {
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
        let here = matches!(&pov.anchor, Anchor::Place(p) if r.marks.place.as_deref() == Some(p));
        let mut marks = r.marks.clone();
        // The owner's word recalled from this very place would read as the conversation here: it is an
        // earlier note, so it is laid as words of unknown author, never as his present message.
        if here && matches!(marks.author, Author::His { .. }) {
            marks.author = Author::Unknown;
        }
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
    use std::collections::BTreeSet;

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
        let pieces: Vec<Admitted<Piece>> = (refs(&items, &pov, &read).into_iter())
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
        let got = refs(&[recalled(1, 3), recalled(1, 5)], &pov, &read);
        assert_eq!(got.len(), 1);
        assert_eq!(read.texts.borrow().len(), 1);
        assert!(read.depends.borrow().contains("ses_old"));
    }
}
