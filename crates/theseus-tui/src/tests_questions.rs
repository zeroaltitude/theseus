//! The sidebar's questions are the whole daemon's (theseus-7zs0f): `confirm.list` answers for every
//! session, and the board keeps a question whichever turn asked it. The chat's `ours_in` filter
//! belongs to one conversation's own transcript; applied here it would hide a question the owner
//! must answer.

use serde_json::json;
use theseus_protocol::utc_hm;

use crate::app::{App, Purpose};
use crate::board::Question;
use crate::tests::{harbour, question};

#[test]
fn a_question_of_another_turn_in_a_placed_session_still_shows() {
    theseus_client::render::time::pin_for_tests();
    let mut app = App::new(utc_hm);
    app.answered(Purpose::Snapshot, Ok(harbour().0));
    let mut q = question("ses_dm0001", "cor_other01", "run the gate", 500);
    q.turn_id = Some("turn_from_discord".into());
    app.answered(Purpose::Questions, Ok(json!({"confirms": [q]})));
    let shown = app.board.questions("ses_dm0001");
    assert!(
        shown
            .iter()
            .any(|s| matches!(s, Question::Whole(c) if c.correlation_id == "cor_other01")),
        "the sidebar lists every turn's question"
    );
}
