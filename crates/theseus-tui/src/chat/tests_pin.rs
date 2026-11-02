//! A turn's line says when its model was pinned and not chosen
//! (theseus-h66v4.3): `900 ms · Sonnet 5.5 · pinned`. Every turn here runs
//! Sonnet 5.5, the config's own default, so the word can only come from the
//! route's recorded `reason = pinned`, never from the model being the default's.

use std::sync::Arc;

use crossterm::event::KeyCode;
use theseus_protocol::route::TurnRoute;
use theseus_protocol::TurnSubmitResult;

use super::tests::{Rig, SID};
use crate::tests::Answer;

/// The turns end on Sonnet 5.5, each with the route reason of its place in
/// `reasons` (the last repeats).
fn script_of(reasons: &'static [&'static str]) -> crate::tests::Script {
    let base = super::tests::script();
    let sent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Arc::new(move |m, p| -> Answer {
        if m != "turn.submit" {
            return base(m, p);
        }
        let n = sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let reason = reasons[n.min(reasons.len() - 1)];
        let r = TurnSubmitResult {
            session_id: SID.into(),
            turn_id: format!("turn_p{n}"),
            output: format!("Answer {n}."),
            model: "claude-sonnet-5-5".into(),
            profile: "sonnet".into(),
            elapsed_ms: 900,
            stop_reason: "end_turn".into(),
            route: Some(TurnRoute {
                reason: reason.into(),
                from: "sonnet".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        Ok(serde_json::to_value(r).unwrap())
    })
}

/// A pinned turn's line carries `pinned`; the next turn, on the same profile
/// and model with Jev's verdict, and one on the default after Jev said
/// nothing, do not.
#[tokio::test(start_paused = true)]
async fn a_pinned_turn_says_pinned_and_the_same_model_unpinned_does_not() {
    let mut r = Rig::with(100, 40, script_of(&["pinned", "verdict", "no_verdict"])).await;
    r.ready().await;
    r.type_text("one").await;
    r.press(KeyCode::Enter).await;
    r.shows("900 ms · Sonnet 5.5 · pinned").await;
    r.type_text("two").await;
    r.press(KeyCode::Enter).await;
    r.shows("Answer 1.").await;
    r.type_text("three").await;
    r.press(KeyCode::Enter).await;
    r.shows("Answer 2.").await;
    let text = r.text();
    assert_eq!(text.matches("pinned").count(), 1, "{text}");
    assert_eq!(text.matches("900 ms · Sonnet 5.5").count(), 3, "{text}");
}
