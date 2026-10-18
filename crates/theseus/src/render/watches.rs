//! Watches as `theseus watch-for` lists them (theseus-f2r2b): one line each,
//! what it looks at, its condition, how often, what it last saw, and when it
//! gives up. Pure: the caller passes the clock.

use theseus_protocol::WatchInfo;

use super::time::fmt_hm;
use super::until_due;

/// The watch's line: its short id, its title, then what it does.
pub fn watch_line(w: &WatchInfo, now_ms: u64) -> String {
    let seen = match (&w.value, w.checks) {
        (Some(v), n) => format!(
            "{} ({n} {}{})",
            clip(v, 40),
            if w.kind == "reply" {
                "replies"
            } else {
                "checks"
            },
            w.last_checked_ms
                .map(|t| format!(", last {}", fmt_hm(t)))
                .unwrap_or_default()
        ),
        (None, _) if w.kind == "reply" => "waiting for a reply".into(),
        (None, _) => "not checked yet".into(),
    };
    let next = w
        .next_check_ms
        .map(|n| format!(" · next {}", until_due(n, now_ms)))
        .unwrap_or_default();
    let failing = w
        .failing
        .as_deref()
        .map(|f| format!(" · FAILING: {}", clip(f, 60)))
        .unwrap_or_default();
    format!(
        "{}\t{}\t{} {} → {} · every {} · then {}\t{seen}{next}{failing}\tgives up {}",
        w.short,
        w.title,
        w.kind,
        clip(&w.target, 60),
        w.until,
        w.every,
        w.then,
        until_due(w.expires_at_ms, now_ms),
    )
}

fn clip(s: &str, n: usize) -> String {
    let one = s.lines().next().unwrap_or("");
    let t: String = one.chars().take(n).collect();
    if t.chars().count() < one.chars().count() || s.lines().count() > 1 {
        format!("{t}…")
    } else {
        t
    }
}

/// A watch that ended, as a watching terminal says it: the message as it is.
pub fn noticed_line(note: &str) -> String {
    format!("  {}", note.lines().next().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> WatchInfo {
        WatchInfo {
            wake_id: "wak_c3d4e5".into(),
            short: "c3d4e5".into(),
            session_id: "ses_1".into(),
            execution_id: "exe_1".into(),
            title: "PR 123 merged?".into(),
            kind: "run".into(),
            target: "gh pr view 123 --json state".into(),
            until: "equals MERGED".into(),
            every: "5m".into(),
            then: "notice".into(),
            set_at_ms: 0,
            expires_at_ms: 3 * 86_400_000,
            value: Some("OPEN".into()),
            checks: 611,
            last_checked_ms: None,
            next_check_ms: Some(240_000),
            failing: None,
        }
    }

    #[test]
    fn a_line_says_what_it_does_and_what_it_saw() {
        assert_eq!(
            watch_line(&info(), 0),
            "c3d4e5\tPR 123 merged?\trun gh pr view 123 --json state → equals MERGED · every 5m · then notice\t\
             OPEN (611 checks) · next in 4m\tgives up in 3d"
        );
        let mut w = info();
        w.value = None;
        w.checks = 0;
        w.next_check_ms = None;
        w.failing = Some("gh: not logged in".into());
        assert!(watch_line(&w, 0).contains("not checked yet · FAILING: gh: not logged in"));
    }
}
