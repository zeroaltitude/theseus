//! What the owner is told when free space under the state dir crosses a line
//! or comes back (theseus-f337): the words of a `disk` post. Apart from
//! `render.rs`, which is at its ceiling.

use serde_json::Value;

/// `Free space under the state dir fell below the warning: 4,000 MB free of
/// 100,000 MB (health warns below 5,120 MB). ...`
pub(crate) fn disk_note(body: &Value) -> String {
    use theseus_core::owner_note::{note, Kind};
    let n = |k: &str| theseus_core::narrative::thousands(body[k].as_u64().unwrap_or(0));
    let facts = format!(
        "free space under the state dir: {} MB free of {} MB",
        n("free_mb"),
        n("total_mb")
    );
    let warns = format!("{facts} (health warns below {} MB)", n("warn_mb"));
    match body["state"].as_str() {
        Some("below_floor") => note(
            Kind::Alert,
            "The disk is below the floor: new jobs are refused",
            &format!("{facts} (floor {} MB)", n("floor_mb")),
            "Running jobs are stopped.",
        ),
        Some("low") => match body["left"].as_str() {
            Some("below_floor") => note(
                Kind::Fyi,
                "The disk is back over the floor, but still low",
                &warns,
                "Jobs run again.",
            ),
            _ => note(Kind::Alert, "The disk is low", &warns, ""),
        },
        _ => note(Kind::Fyi, "The disk is back to normal", &facts, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_state_has_its_words() {
        let body = |state: &str, left: Value| {
            json!({"kind": "disk", "state": state, "left": left, "free_mb": 4000,
                "total_mb": 100000, "warn_mb": 5120, "floor_mb": 1024})
        };
        assert_eq!(
            disk_note(&body("low", json!("ok"))),
            "⚠️ Alert · The disk is low\nfree space under the state dir: 4,000 MB free of \
             100,000 MB (health warns below 5,120 MB)"
        );
        assert_eq!(
            disk_note(&body("below_floor", json!("low"))),
            "⚠️ Alert · The disk is below the floor: new jobs are refused\nfree space under the \
             state dir: 4,000 MB free of 100,000 MB (floor 1,024 MB)\nRunning jobs are stopped."
        );
        assert!(disk_note(&body("low", json!("below_floor")))
            .contains("back over the floor, but still low"));
        assert_eq!(
            disk_note(&body("ok", json!("low"))),
            "ℹ️ FYI · The disk is back to normal\nfree space under the state dir: 4,000 MB free \
             of 100,000 MB"
        );
    }
}
