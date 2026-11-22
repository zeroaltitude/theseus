//! The status line's route words: `pinned` from the route's recorded reason
//! (theseus-h66v4.3). The data rule's words (theseus-a1wvq.11) went with the
//! restricted-origin rule (2026-11-21).

use theseus_protocol::route::TurnRoute;
use theseus_protocol::TurnSubmitResult;

use super::status_line;

fn result(mode: Option<&str>) -> TurnSubmitResult {
    TurnSubmitResult {
        route: Some(TurnRoute {
            mode: mode.map(Into::into),
            reason: "verdict".into(),
            from: "sonnet".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The status line already says `pinned` from the route's recorded reason
/// (theseus-h66v4.3 left it alone): a pin on the config's own profile and model
/// reads pinned, and the same turn chosen by Jev, or run on the default when
/// Jev said nothing, does not.
#[test]
fn the_status_line_says_pinned_by_the_reason_and_not_by_the_model() {
    let ran = |reason: &str, mode: Option<&str>| {
        let mut r = result(mode);
        r.profile = "sonnet".into();
        r.provider = "anthropic".into();
        r.model = "claude-sonnet-5-5".into();
        r.route.as_mut().unwrap().reason = reason.into();
        status_line(&r)
    };
    assert!(
        ran("pinned", None).contains(" \u{b7} route pinned \u{b7} "),
        "{}",
        ran("pinned", None)
    );
    assert!(ran("pinned", Some("quick")).contains("quick (pinned)"));
    for other in ["verdict", "no_verdict", "shadow"] {
        assert!(!ran(other, None).contains("pinned"), "{other}");
    }
}
