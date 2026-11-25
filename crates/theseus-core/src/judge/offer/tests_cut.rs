//! The offered brief, which a reply's chip or an `offer` post carries,
//! holds a known value whole or not at all. The value is invented.

use super::{Waiting, BRIEF_CHARS};
use crate::provider::Scripted;
use crate::tests_cut_sites::{knowing_core, whole_or_absent, V};

/// The value runs across the brief's limit: the brief stops before it.
#[tokio::test]
async fn an_offered_brief_stops_before_a_known_value() {
    let r = knowing_core(V, |_| {}, |_| Scripted::text("Noted."));
    let before = "x".repeat(BRIEF_CHARS - 10);
    let judge = &r.core.runner.judge;
    judge.note_offer("turn_1", &format!("{before}{V} and after"));
    let brief = judge
        .take_offer(Waiting {
            turn: "turn_1",
            session: "ses_a",
            execution: "exe_a",
            target: "discord:dm:42",
        })
        .expect("the brief was kept for the reply");
    assert!(brief.starts_with(&before), "{brief}");
    whole_or_absent("the offered brief", &brief, V);
}
