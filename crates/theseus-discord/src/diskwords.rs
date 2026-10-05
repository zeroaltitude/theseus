//! What the owner is told when free space under the state dir crosses a line
//! or comes back (theseus-f337): the words of a `disk` post. Apart from
//! `render.rs`, which is at its ceiling.

use serde_json::Value;

/// `Free space under the state dir fell below the warning: 4,000 MB free of
/// 100,000 MB (health warns below 5,120 MB). ...`
pub(crate) fn disk_note(body: &Value) -> String {
    let n = |k: &str| theseus_core::narrative::thousands(body[k].as_u64().unwrap_or(0));
    let facts = format!("{} MB free of {} MB", n("free_mb"), n("total_mb"));
    match body["state"].as_str() {
        Some("below_floor") => format!(
            "Free space under the state dir is below the floor: {facts} (floor {} MB). New jobs \
             are refused, and running ones are stopped.",
            n("floor_mb")
        ),
        Some("low") => match body["left"].as_str() {
            Some("below_floor") => format!(
                "Free space under the state dir is back over the floor, but still low: {facts} \
                 (health warns below {} MB). Jobs run again.",
                n("warn_mb")
            ),
            _ => format!(
                "Free space under the state dir is low: {facts} (health warns below {} MB).",
                n("warn_mb")
            ),
        },
        _ => format!("Free space under the state dir is back to normal: {facts}."),
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
            "Free space under the state dir is low: 4,000 MB free of 100,000 MB (health warns \
             below 5,120 MB)."
        );
        assert_eq!(
            disk_note(&body("below_floor", json!("low"))),
            "Free space under the state dir is below the floor: 4,000 MB free of 100,000 MB \
             (floor 1,024 MB). New jobs are refused, and running ones are stopped."
        );
        assert!(disk_note(&body("low", json!("below_floor")))
            .contains("back over the floor, but still low"));
        assert_eq!(
            disk_note(&body("ok", json!("low"))),
            "Free space under the state dir is back to normal: 4,000 MB free of 100,000 MB."
        );
    }
}
