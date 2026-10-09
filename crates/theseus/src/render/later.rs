//! What a `theseus --spawn ask` run left behind as it ended (theseus-mqxk):
//! its daemon ends with the run, so a job still running, a result no turn
//! read, and a wake not yet due never come back. The status line names them.

use theseus_protocol::later::Later;
use theseus_protocol::TurnSubmitResult;

/// ` · 1 wake not fired: the run ended`, or nothing when the turn left
/// nothing (a socket daemon's results never say).
pub fn left(r: &TurnSubmitResult) -> String {
    match r.later.as_ref().map(words) {
        Some(w) if !w.is_empty() => format!(" · {w}: the run ended"),
        _ => String::new(),
    }
}

/// `1 job still running, 2 wakes not fired`: each thing left, counted.
pub fn words(l: &Later) -> String {
    let count = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let mut parts = Vec::new();
    if l.jobs > 0 {
        parts.push(count(
            l.jobs as usize,
            "job still running",
            "jobs still running",
        ));
    }
    if l.queued {
        parts.push("a result not read".to_string());
    }
    if !l.wakes.is_empty() {
        parts.push(count(l.wakes.len(), "wake not fired", "wakes not fired"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::later::LaterWake;

    #[test]
    fn the_status_line_names_what_the_run_left() {
        let mut r = TurnSubmitResult::default();
        assert_eq!(left(&r), "", "a socket daemon's result");
        r.later = Some(Later::default());
        assert_eq!(left(&r), "", "nothing left");
        let wake = |id: &str| LaterWake {
            wake_id: id.into(),
            due_at_ms: 1,
            note: "check the lighthouse".into(),
            fires: false,
        };
        r.later = Some(Later {
            jobs: 1,
            queued: false,
            wakes: vec![wake("wak_1")],
            ends_at_ms: 2,
        });
        assert_eq!(
            left(&r),
            " · 1 job still running, 1 wake not fired: the run ended"
        );
        r.later = Some(Later {
            jobs: 0,
            queued: true,
            wakes: vec![wake("wak_1"), wake("wak_2")],
            ends_at_ms: 2,
        });
        assert_eq!(
            left(&r),
            " · a result not read, 2 wakes not fired: the run ended"
        );
    }
}
