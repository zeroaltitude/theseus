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
        f.contains("claude-sonnet-5-5 · pinned · routed: Birch · Jev: quick · 1 loop"),
        "{f}"
    );
    let parts = reply_parts("t1", &[(0, "Done.".to_string())], Some(&r));
    let last = &parts.last().unwrap().1;
    assert!(
        last.starts_with("Done.\n-# sonnet · claude-sonnet-5-5 · pinned"),
        "{last}"
    );
}

/// On a pinned turn the model was not routed: `quick` is only Jev's shadow
/// read, so the footer words it as that and not as `routed: quick`, which sat
/// beside `pinned` and contradicted it (theseus-h66v4.3, F2). Every other
/// reason keeps `routed:` byte for byte.
#[test]
fn a_pinned_turns_mode_is_jevs_read_and_an_unpinned_turns_stays_routed() {
    let with_mode = |reason: &str| {
        let mut r = ran(reason);
        r.route.as_mut().unwrap().mode = Some("quick".into());
        r
    };
    let pinned = footer(&with_mode("pinned"));
    assert!(
        pinned.starts_with("-# sonnet · claude-sonnet-5-5 · pinned · Jev: quick · 1 loop"),
        "{pinned}"
    );
    assert!(!pinned.contains("routed:"), "{pinned}");
    for not_a_pin in [
        "verdict",
        "detour",
        "no_verdict",
        "late",
        "unsure",
        "fallback",
        "shadow",
    ] {
        let f = footer(&with_mode(not_a_pin));
        assert!(
            f.starts_with("-# sonnet · claude-sonnet-5-5 · routed: quick · 1 loop"),
            "{not_a_pin}: {f}"
        );
        assert!(!f.contains("Jev:"), "{not_a_pin}: {f}");
    }
    // Nothing to reword when Jev read nothing: no mode, no words, as before.
    assert!(!footer(&ran("pinned")).contains("Jev:"));
}

/// A task brief with no Jev mode names where the turn came from and nothing
/// else. On a pinned turn that is `pinned · task brief`, with no `routed:`
/// beside the pin (it read `pinned · routed: task brief` before the N1 pass of
/// theseus-h66v4.3); an unpinned brief still reads `routed: task brief`.
#[test]
fn a_task_brief_with_no_mode_is_not_routed_when_it_was_pinned() {
    let brief = |reason: &str| {
        let mut r = ran(reason);
        r.route.as_mut().unwrap().source = Some("task brief".into());
        r
    };
    let pinned = footer(&brief("pinned"));
    assert!(
        pinned.starts_with("-# sonnet · claude-sonnet-5-5 · pinned · task brief · 1 loop"),
        "{pinned}"
    );
    assert!(!pinned.contains("routed:"), "{pinned}");
    let plain = footer(&brief("verdict"));
    assert!(
        plain.starts_with("-# sonnet · claude-sonnet-5-5 · routed: task brief · 1 loop"),
        "{plain}"
    );
}
