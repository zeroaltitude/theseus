//! A reply's footer says when its model was pinned and not chosen
//! (theseus-h66v4.3): `-# sonnet · claude-sonnet-5-5 · pinned · 1 loop`. The
//! word is the route's recorded `reason = pinned` and nothing inferred from the
//! model: every turn here ran `sonnet` on `claude-sonnet-5-5`, the config's own
//! default, so a footer that only told a pin from a different model would pass
//! the one case and fail the other.

use serde_json::json;
use theseus_protocol::route::{RoutePersona, TurnData, TurnRoute};
use theseus_protocol::TurnSubmitResult;

use super::footer;
use super::reply::reply_parts;

/// A turn on `sonnet`, with the route routing recorded for it: `from` is the
/// profile it ran on before routing, `sonnet` too, so profile and model are the
/// default's whatever the reason.
fn ran(reason: &str) -> TurnSubmitResult {
    let mut r: TurnSubmitResult = serde_json::from_value(json!({
        "session_id": "s", "turn_id": "t1", "loops": 1, "output": "", "stop_reason": "no_tool_calls",
        "provider_stop_reason": null, "model": "claude-sonnet-5-5", "provider": "anthropic",
        "profile": "sonnet",
        "usage": {"input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
        "elapsed_ms": 4200, "tool_calls": 0, "continuation": false}))
    .unwrap();
    r.route = Some(TurnRoute {
        reason: reason.into(),
        from: "sonnet".into(),
        ..Default::default()
    });
    r
}

#[test]
fn a_pinned_turns_footer_says_pinned_after_its_model() {
    let f = footer(&ran("pinned"));
    assert!(
        f.starts_with("-# sonnet · claude-sonnet-5-5 · pinned · 1 loop"),
        "{f}"
    );
    assert_eq!(f.lines().count(), 1, "one line: {f}");
}

/// Same profile, same model, same `from`: only the recorded reason differs, and
/// only a pin reads pinned. The config's default (what a turn runs when Jev
/// says nothing, or says Sonnet) is no pin.
#[test]
fn a_pin_on_the_default_model_reads_pinned_and_the_default_does_not() {
    let plain = footer(&ran("verdict"));
    assert!(!plain.contains("pinned"), "{plain}");
    for not_a_pin in [
        "no_verdict",
        "late",
        "unsure",
        "fallback",
        "shadow",
        "correction",
    ] {
        assert_eq!(footer(&ran(not_a_pin)), plain, "{not_a_pin}");
    }
    assert_eq!(
        footer(&ran("pinned")),
        plain.replacen(" · 1 loop", " · pinned · 1 loop", 1)
    );
}

/// A turn the data rule moved off a pinned restricted model ran the default
/// and says `data gate`, not `pinned`; a result from before routing carries no
/// route and words as it always did.
#[test]
fn a_gated_turn_and_a_turn_with_no_route_are_not_pinned() {
    let mut gated = ran("data_gate");
    gated.route.as_mut().unwrap().data = Some(TurnData {
        cn: "confidential".into(),
        gate: Some("confidential".into()),
        origin: "us".into(),
    });
    let f = footer(&gated);
    assert!(f.contains("data gate: confidential"), "{f}");
    assert!(!f.contains("pinned"), "{f}");
    let mut none = ran("pinned");
    none.route = None;
    assert!(!footer(&none).contains("pinned"));
}

/// The clause sits beside the persona's and the mode's words, not in place of
/// them, and rides the reply's last part as the footer always does.
#[test]
fn the_pinned_word_joins_the_route_words_and_rides_the_reply() {
    let mut r = ran("pinned");
    let t = r.route.as_mut().unwrap();
    t.mode = Some("quick".into());
    t.persona = Some(RoutePersona {
        key: Some("birch".into()),
        name: "Birch".into(),
        how: "explicit".into(),
        why: String::new(),
    });
    let f = footer(&r);
    assert!(
        f.contains("claude-sonnet-5-5 · pinned · routed: Birch · quick · 1 loop"),
        "{f}"
    );
    let parts = reply_parts("t1", &[(0, "Done.".to_string())], Some(&r));
    let last = &parts.last().unwrap().1;
    assert!(
        last.starts_with("Done.\n-# sonnet · claude-sonnet-5-5 · pinned"),
        "{last}"
    );
}
