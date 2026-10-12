//! The pending wakes, as `theseus wakes` and health's `wakes:` line show them
//! (DD8; moved out of render.rs at theseus-lherl).

use theseus_protocol::WakeInfo;

use super::time::fmt_date;
use super::until_due;

/// What people call a wake: the daemon's `title` (its note's first line,
/// clipped), or for an older daemon the note's own first line.
fn title_of(w: &WakeInfo, chars: usize) -> String {
    match &w.title {
        Some(t) => t.clone(),
        None => w
            .note
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(chars)
            .collect(),
    }
}

/// How often it runs: `once`, or a series' span, its weekdays and its end,
/// and the occurrence due: `every 1d on mon,fri until 2026-10-30 18:00 #4`.
fn cadence(w: &WakeInfo) -> String {
    let Some(e) = &w.every else {
        return "once".into();
    };
    let mut out = format!("every {e}");
    if !w.days.is_empty() {
        out.push_str(&format!(" on {}", w.days.join(",")));
    }
    if let Some(u) = w.until_ms {
        out.push_str(&format!(" until {}", fmt_date(u)));
    }
    if let Some(n) = w.occurrence {
        out.push_str(&format!(" #{n}"));
    }
    out
}

/// One pending wake, as `theseus wakes` lists it (DD8): its short id, when
/// it is due, how often (`once`, or a series' span, weekdays and end, and the
/// occurrence due, `every 1d on mon,fri #4`; 37a), its session, or the task
/// that set it (`task a1b2c3`; 37b), and its state, and its title.
pub fn wake_line(w: &WakeInfo, now_ms: u64) -> String {
    let whose = match &w.task {
        Some(t) => format!("task {t}"),
        None => format!("session {}", w.session_id),
    };
    format!(
        "{}\t{} ({})\t{}\t{whose} ({}){}\t{}",
        w.short,
        w.due_local,
        until_due(w.due_at_ms, now_ms),
        cadence(w),
        w.state,
        w.target
            .as_deref()
            .map(|t| format!(" → {t}"))
            .unwrap_or_default(),
        title_of(w, 100)
    )
}

/// `wakes: 2 pending · next 3f9a1c 2026-09-30 13:15:00 -07:00 (in 9m): check
/// the build`, or nothing when none is pending.
pub fn wakes_line(wakes: &[WakeInfo], now_ms: u64) -> Option<String> {
    let next = wakes.iter().min_by_key(|w| w.due_at_ms)?;
    let title: String = title_of(next, 60).chars().take(60).collect();
    Some(format!(
        "wakes: {} pending · next {} {} ({}): {title} · theseus wakes, theseus cancel <id>",
        wakes.len(),
        next.short,
        next.due_local,
        until_due(next.due_at_ms, now_ms)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> WakeInfo {
        WakeInfo {
            wake_id: "wak_0199aaaa3f9a1c".into(),
            short: "3f9a1c".into(),
            session_id: "ses_1".into(),
            execution_id: "exe_1".into(),
            due_at_ms: 1_000_000 + 540_000,
            due_local: "2026-09-30 13:15:00 -07:00".into(),
            note: "check the build\nand the tests".into(),
            set_at_ms: 1_000_000,
            target: Some("discord:dm:42".into()),
            state: "waiting".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_wake_lists_its_due_time_session_and_note() {
        let w = base();
        assert_eq!(
            wake_line(&w, 1_000_000),
            "3f9a1c\t2026-09-30 13:15:00 -07:00 (in 9m)\tonce\tsession ses_1 (waiting) → \
             discord:dm:42\tcheck the build"
        );
        let daily = WakeInfo {
            every: Some("1d".into()),
            occurrence: Some(4),
            next: Some("13:15 Wed".into()),
            ..w.clone()
        };
        assert_eq!(
            wake_line(&daily, 1_000_000),
            "3f9a1c\t2026-09-30 13:15:00 -07:00 (in 9m)\tevery 1d #4\tsession ses_1 (waiting) → \
             discord:dm:42\tcheck the build"
        );
        // A task's wake names the task (37b).
        let tasks = WakeInfo {
            task: Some("a1b2c3".into()),
            ..w.clone()
        };
        assert_eq!(
            wake_line(&tasks, 1_000_000),
            "3f9a1c\t2026-09-30 13:15:00 -07:00 (in 9m)\tonce\ttask a1b2c3 (waiting) → \
             discord:dm:42\tcheck the build"
        );
        assert_eq!(until_due(1_000, 181_000), "due 3m ago");
        assert_eq!(wakes_line(&[], 0), None);
        let later = WakeInfo {
            short: "b4f566".into(),
            due_at_ms: w.due_at_ms + 3_600_000,
            ..w.clone()
        };
        assert_eq!(
            wakes_line(&[later, w], 1_000_000).unwrap(),
            "wakes: 2 pending · next 3f9a1c 2026-09-30 13:15:00 -07:00 (in 9m): check the build · \
             theseus wakes, theseus cancel <id>"
        );
    }

    /// The daemon's title, a series' weekdays and its end (theseus-lherl).
    #[test]
    fn a_wake_lists_its_title_days_and_until() {
        let w = WakeInfo {
            every: Some("1d".into()),
            occurrence: Some(2),
            days: vec!["mon".into(), "fri".into()],
            // 2026-10-30 18:00:00 -07:00.
            until_ms: Some(1_793_408_400_000),
            title: Some("evening haiku".into()),
            note: "write the evening haiku, and post it\nto the channel".into(),
            ..base()
        };
        assert_eq!(
            wake_line(&w, 1_000_000),
            "3f9a1c\t2026-09-30 13:15:00 -07:00 (in 9m)\tevery 1d on mon,fri until 2026-10-30 18:00 #2\t\
             session ses_1 (waiting) → discord:dm:42\tevening haiku"
        );
        assert!(wakes_line(&[w], 1_000_000)
            .unwrap()
            .contains("): evening haiku · theseus wakes"));
    }
}
