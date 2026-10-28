//! `proc.run`'s longest time limit (`[tools] proc_timeout_max_secs`), told to the model twice: in the tool's
//! description, which names the cap the operator set, and in the result of a call whose ask it cut, which says what
//! was asked, what the call was held to, and that the whole process tree stops at the limit (theseus-spntl).

use serde_json::{json, Value};
use theseus_kernel::patience::CEILING_FACTOR;
use theseus_tools::wire_name;

/// The key of the clamp's note in a result's meta.
pub(super) const KEY: &str = "timeout_clamped";

/// Say the longest limit in `proc_run`'s description: the tool's own text is static, and the cap is the
/// operator's. A call that names no limit gets `default_secs`.
pub(super) fn say_proc_cap(defs: &mut [Value], default_secs: u64, max_secs: u64) {
    let wire = wire_name("proc.run");
    for d in defs.iter_mut().filter(|d| d["name"] == wire.as_str()) {
        if let Some(text) = d["description"].as_str() {
            d["description"] = json!(format!(
                "{text} A run lasts at most {max_secs} s: a longer `timeout_secs` is cut to that, and at the \
                 limit the whole process tree is stopped. With no `timeout_secs` the limit is {default_secs} s."
            ));
        }
    }
}

/// The longest limit a call asked for beyond `max_secs`: its own `timeout_secs`, or any step's.
pub(super) fn asked_beyond(input: &Value, max_secs: u64) -> Option<u64> {
    let own = input.get("timeout_secs");
    let steps = input.get("steps").and_then(Value::as_array);
    own.into_iter()
        .chain(
            steps
                .into_iter()
                .flatten()
                .filter_map(|s| s.get("timeout_secs")),
        )
        .filter_map(Value::as_u64)
        .filter(|&t| t > max_secs)
        .max()
}

/// What the result says of a cut ask: the meta's note and the line the model reads.
///
/// theseus-x1zz8 made the limit a bound on SILENCE, so "the longest a run lasts" became false: a
/// run still printing is let run on, and the real backstop is the ceiling over it. This note is a
/// per-call line rather than part of the tool catalogue the locked DM golden's cost line tracks,
/// so unlike `proc.run`'s own description — which still describes the old clock and waits on the
/// owner, because correcting it moves that golden — it could simply be corrected here.
pub(super) fn note(asked: u64, max_secs: u64) -> (Value, String) {
    let ceiling = max_secs.saturating_mul(u64::from(CEILING_FACTOR));
    let line = format!(
        "[time limit: {asked} s was asked, held to {max_secs} s of silence; a run still printing is let run \
         on, and is stopped at {ceiling} s. At the stop the whole process tree is stopped]\n"
    );
    (
        json!({
            "asked_secs": asked,
            "held_to_secs": max_secs,
            "limit_is_on_silence": true,
            "ceiling_secs": ceiling,
            "tree_stopped_at_limit": true,
        }),
        line,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_ask_past_the_cap_is_named() {
        assert_eq!(asked_beyond(&json!({"argv": ["ls"]}), 3600), None);
        assert_eq!(asked_beyond(&json!({"timeout_secs": 60}), 3600), None);
        assert_eq!(asked_beyond(&json!({"timeout_secs": 3600}), 3600), None);
        assert_eq!(
            asked_beyond(&json!({"timeout_secs": 7200}), 3600),
            Some(7200)
        );
        let steps = json!({"timeout_secs": 10, "steps": [{"timeout_secs": 5000}, {"timeout_secs": 9000}, {}]});
        assert_eq!(asked_beyond(&steps, 3600), Some(9000));
    }

    #[test]
    fn the_note_names_durations_and_the_tree_and_nothing_else() {
        let (meta, line) = note(7200, 3600);
        assert_eq!(meta["asked_secs"], 7200);
        assert_eq!(meta["held_to_secs"], 3600);
        assert!(
            line.contains("7200 s was asked, held to 3600 s of silence"),
            "{line}"
        );
        assert!(line.contains("whole process tree is stopped"), "{line}");
        // theseus-x1zz8: the note must not promise a stop the code no longer makes.
        assert!(!line.contains("the longest a run lasts"), "{line}");
        assert!(line.contains("still printing is let run on"), "{line}");
        assert_eq!(meta["limit_is_on_silence"], true);
        assert_eq!(meta["ceiling_secs"], 3600 * u64::from(CEILING_FACTOR));
    }
}
