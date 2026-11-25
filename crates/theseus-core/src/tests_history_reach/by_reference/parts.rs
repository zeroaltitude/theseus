//! A look back's result kept by reference says exactly what it showed: a retraction tells its reader by what its
//! parts quote, whatever the runtime cut. Written by hand, as no writer stores parts yet.

use super::*;
use crate::quotes::{Field, Form, Part, Quote};

/// A new conversation holding an open of `turn` the runtime cut, kept by reference: the opening line and the
/// seam in the code's words, and a quote of the start of `of`'s words (stored at `at`), with its copy edge, as
/// a completion frame writes them. Its id and the result's.
fn cut_open_by_hand(core: &Core, turn: &str, (of, at): (&str, u64)) -> (String, String) {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let parts = vec![
        Part::Code(format!(
            "[Exchange \"{turn}\", 2026-03-03: what was said]\n\u{2502} "
        )),
        Part::Quote(Quote {
            node: of.into(),
            position: at,
            field: Field::Text,
            from: 0,
            to: 10,
            form: Form::Margin1,
        }),
        Part::Code(
            "\n\u{2026}[3 lines (90 characters) not shown: a narrower call returns them]\u{2026}\n"
                .into(),
        ),
        Part::Code("That is the exchange.".into()),
    ];
    let mut n = crate::compiler::tests_view::result("turn_read", "h9", "");
    n.session_id = rec.session_id.clone();
    if let Body::ToolResult {
        tool,
        truncated,
        parts: p,
        ..
    } = &mut n.body
    {
        *tool = crate::view::history::HISTORY_TOOL.into();
        *truncated = true;
        *p = parts;
    }
    let e = Edge::new(EdgeKind::DerivedFrom, &n.id, of, VIA_HISTORY);
    core.store
        .append(&[n.record().unwrap(), e.record().unwrap()])
        .unwrap();
    (rec.session_id, n.id)
}

/// A conversation that read the line through an open the runtime cut, whose kept part quotes the message, is
/// told: its exposure is a session's, its next turn holds one correction note, and a second retraction writes
/// none. (A cut open stored with its words shows nothing and its reader is seen, `cut.rs`.)
#[tokio::test]
async fn a_reader_of_a_cut_open_whose_kept_part_shows_it_is_told_once() {
    let r = rig();
    let said = say(&r.core).await;
    let (at, _) = r.core.store.get_node(&said.node).unwrap().unwrap();
    let (reader, copy) = cut_open_by_hand(&r.core, &said.turn, (&said.node, at));
    let out = retract(&r.core, &said.node);
    let e = of_session(&out, &reader);
    assert_eq!(
        (e.kind.as_str(), e.node_id.as_str()),
        (kind::SESSION, copy.as_str()),
        "{e:?}"
    );
    turn(&r.core, &reader, "and the weir?").await;
    assert_eq!(notes_in(&r.core, &reader), 1);
    assert!(retract(&r.core, &said.node).already);
    turn(&r.core, &reader, "and at dawn?").await;
    assert_eq!(notes_in(&r.core, &reader), 1, "told once");
}
