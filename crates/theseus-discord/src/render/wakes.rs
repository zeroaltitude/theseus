//! The pending wakes in a place (DD8; moved out of render.rs at
//! theseus-lherl): `/wakes`. Pure: wakes in, text out.

use super::clip;

/// This place's pending wakes (DD8), as `/wakes` lists them: the soonest
/// ten, each with its time in the reader's own zone and its title. A series
/// says its span, its weekdays and its end around its next time: `🔁 every 1d
/// on mon,fri · next 21:00 · until Oct 30` (37a; theseus-lherl).
pub fn wakes(wakes: &[theseus_protocol::WakeInfo]) -> String {
    if wakes.is_empty() {
        return "No wakes pending here. Ask for one: \"remind me in 10 minutes to …\".".into();
    }
    let mut lines = vec![format!("**Wakes here** ({})", wakes.len())];
    for w in wakes.iter().take(10) {
        let at = w.due_at_ms / 1000;
        // The daemon's title, else (an older daemon) the note's first line.
        let note = w
            .title
            .as_deref()
            .unwrap_or_else(|| w.note.lines().next().unwrap_or_default());
        let busy = match w.state.as_str() {
            "running" | "queued" => " · runs when the current turn ends",
            _ => "",
        };
        let days = if w.days.is_empty() {
            String::new()
        } else {
            format!(" on {}", w.days.join(","))
        };
        let series = w
            .every
            .as_deref()
            .map_or(String::new(), |e| format!("🔁 every {e}{days} · next "));
        let until = w
            .until_ms
            .map_or(String::new(), |u| format!(" · until <t:{}:d>", u / 1000));
        // A task's wake names the task (37b).
        let task = w
            .task
            .as_deref()
            .map_or(String::new(), |t| format!(" · task `{t}`"));
        lines.push(format!(
            "• `{}` {series}<t:{at}:t> (<t:{at}:R>){until}{task}{busy} · {}",
            w.short,
            clip(note, 120)
        ));
    }
    if wakes.len() > 10 {
        lines.push(format!(
            "-# and {} later; `theseus wakes` lists them all",
            wakes.len() - 10
        ));
    }
    lines.push("-# `/cancel <id>` cancels one.".into());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/wakes` (DD8): each wake by its short id, its time in the reader's
    /// zone, and its note's first line (its title); one waiting for a busy
    /// turn says so.
    #[test]
    fn slash_wakes_lists_each_with_its_time_and_note() {
        let w = |short: &str, state: &str| theseus_protocol::WakeInfo {
            short: short.into(),
            due_at_ms: 1_790_798_700_000,
            note: "check the build\nthen the tests".into(),
            state: state.into(),
            ..Default::default()
        };
        assert_eq!(
            wakes(&[w("3f9a1c", "waiting"), w("b4f566", "running")]),
            "**Wakes here** (2)\n\
             • `3f9a1c` <t:1790798700:t> (<t:1790798700:R>) · check the build\n\
             • `b4f566` <t:1790798700:t> (<t:1790798700:R>) · runs when the current turn ends · \
             check the build\n\
             -# `/cancel <id>` cancels one."
        );
        assert!(wakes(&[]).starts_with("No wakes pending here"));
        let daily = theseus_protocol::WakeInfo {
            every: Some("1d".into()),
            occurrence: Some(4),
            next: Some("21:00 Thu".into()),
            ..w("c5d6e7", "waiting")
        };
        assert_eq!(
            wakes(&[daily]),
            "**Wakes here** (1)\n\
             • `c5d6e7` 🔁 every 1d · next <t:1790798700:t> (<t:1790798700:R>) · check the build\n\
             -# `/cancel <id>` cancels one."
        );
        let tasks = theseus_protocol::WakeInfo {
            task: Some("a1b2c3".into()),
            ..w("d7e8f9", "waiting")
        };
        assert_eq!(
            wakes(&[tasks]),
            "**Wakes here** (1)\n\
             • `d7e8f9` <t:1790798700:t> (<t:1790798700:R>) · task `a1b2c3` · check the build\n\
             -# `/cancel <id>` cancels one."
        );
    }

    /// A series says its weekdays and its end, and a wake its daemon's title
    /// (theseus-lherl).
    #[test]
    fn slash_wakes_lists_a_titles_days_and_until() {
        let w = theseus_protocol::WakeInfo {
            short: "a41d04".into(),
            due_at_ms: 1_790_798_700_000,
            note: "write the evening haiku, and post it\nto the channel".into(),
            title: Some("evening haiku".into()),
            state: "waiting".into(),
            every: Some("1d".into()),
            occurrence: Some(2),
            days: vec!["mon".into(), "fri".into()],
            until_ms: Some(1_793_408_400_000),
            ..Default::default()
        };
        assert_eq!(
            wakes(&[w]),
            "**Wakes here** (1)\n\
             • `a41d04` 🔁 every 1d on mon,fri · next <t:1790798700:t> (<t:1790798700:R>) · \
             until <t:1793408400:d> · evening haiku\n\
             -# `/cancel <id>` cancels one."
        );
    }
}
