//! A deferred task on `/tasks` (theseus-rcgph): it says when it starts, in
//! place of what it waits on.

use theseus_protocol::TaskInfo;

/// A task's state on its `/tasks` line: `▷ starts Fri 09:00` while a deferred
/// one sleeps, else its state and what it waits on.
pub(super) fn state_word(t: &TaskInfo) -> String {
    match (&t.waiting_on, t.starts_at_ms) {
        (_, Some(at)) => format!("▷ starts {}", theseus_core::push::hm(at)),
        (Some(w), None) => format!("{} on {w}", t.state),
        (None, None) => t.state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_tasks_says_when_a_deferred_task_starts() {
        let at = theseus_protocol::now_unix_ms() + 3_600_000;
        let t = TaskInfo {
            short: "a1b2c3".into(),
            title: Some("Draft the notes".into()),
            state: "waiting".into(),
            waiting_on: Some("a due time".into()),
            starts_at_ms: Some(at),
            limit_usd: 2.5,
            created_at_ms: 1_000,
            ..Default::default()
        };
        let text = super::super::tasks(std::slice::from_ref(&t), 181_000);
        assert!(
            text.contains(&format!(
                "• `a1b2c3` ▷ starts {} · $0 of $2.50",
                theseus_core::push::hm(at)
            )),
            "{text}"
        );
        let started = TaskInfo {
            starts_at_ms: None,
            ..t
        };
        assert_eq!(state_word(&started), "waiting on a due time");
    }
}
