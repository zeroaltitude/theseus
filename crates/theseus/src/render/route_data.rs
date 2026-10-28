//! The status line's data words (theseus-a1wvq.11): ` · data gate: <why>`
//! when the rule kept the turn off a restricted-origin model, and a plain
//! turn's line as it always was.

use theseus_protocol::route::{TurnData, TurnRoute};
use theseus_protocol::TurnSubmitResult;

use super::status_line;

fn result(data: Option<TurnData>, mode: Option<&str>) -> TurnSubmitResult {
    TurnSubmitResult {
        route: Some(TurnRoute {
            mode: mode.map(Into::into),
            reason: "verdict".into(),
            from: "sonnet".into(),
            data,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn data(cn: &str, gate: Option<&str>) -> Option<TurnData> {
    Some(TurnData {
        cn: cn.into(),
        gate: gate.map(Into::into),
        origin: "us".into(),
    })
}

#[test]
fn the_status_line_ends_its_route_words_with_the_data_rules() {
    let plain = status_line(&result(None, Some("trivial")));
    assert!(
        plain.contains(" \u{b7} trivial (verdict) \u{b7} 0 loop(s)"),
        "{plain}"
    );
    // A `data` that words nothing leaves the line byte for byte as it was.
    assert_eq!(
        status_line(&result(data("allowed", None), Some("trivial"))),
        plain
    );
    let held = status_line(&result(
        data("confidential", Some("confidential")),
        Some("trivial"),
    ));
    assert!(
        held.contains(" \u{b7} trivial (verdict) \u{b7} data gate: confidential \u{b7} 0 loop(s)"),
        "{held}"
    );
}
