//! The daemon's own work on the work board (theseus-jn17m): one root,
//! `Theseus`, whose children are the system jobs (an import, an erase,
//! topics and people, consolidation, an index rebuild) running or ended
//! within the last hour. Types and pure functions only: no clock (the caller
//! passes `now_ms`).
//!
//! A system job's view carries its words in the fields every view has: the
//! `title` is what it is doing (`Importing Claude Code history`), the
//! `attention.label` how far it is (`batch 41 of 120`, from `doing`) or how it
//! ended (`done · 412 sessions`, `failed: …`), `progress` its counts
//! (`steps_done` of `steps_total`, 0 when the total is unknown, `current` the
//! unit), and `started_at_ms` / `ended_at_ms` its time. `line` writes them as
//! the one line every surface shows. System work is the owner's: a shared
//! place's watch never sees the root.

use crate::work::{WorkKind, WorkView};

/// The root's id, and every system job's `root`.
pub const ROOT: &str = "system";

/// The root's title.
pub const ROOT_TITLE: &str = "Theseus";

/// How long an ended system job stays on the board: an hour.
pub const KEEP_MS: u64 = 3_600_000;

/// How long a running system job may go without a row before it is taken
/// to have ended with nothing said (a client that quit, a run that stopped
/// at a limit): 5 minutes. The board has no timer, so the next frame settles
/// it.
pub const QUIET_MS: u64 = 300_000;

/// Whether `id` is the system root's.
pub fn is_root(id: &str) -> bool {
    id == ROOT
}

/// A time as a clock: `3:12`, `1:02:09`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `batch 41 of 120`, `batch 41`, `14 clusters`, `400 of 1200 records`, or
/// `41%`: a job's count, from its progress (`unit` what it counts, `total`
/// 0 while unknown).
pub fn count_words(done: u32, total: u32, unit: &str) -> String {
    let of = total.max(done);
    match (unit, total) {
        ("%", _) => format!("{done}%"),
        ("batch", 0) => format!("batch {done}"),
        ("batch", _) => format!("batch {done} of {of}"),
        (_, 0) => format!("{done} {unit}"),
        (_, _) => format!("{done} of {of} {unit}"),
    }
}

/// A system job as one line: `Importing Claude Code history · batch 41 of
/// 120 · 3:12`. A running job's time is how long it has run (to `now_ms`), an
/// ended one's how long it took. The root is its title alone.
pub fn line(v: &WorkView, now_ms: u64) -> String {
    if v.kind != WorkKind::System || is_root(&v.id) {
        return v.title.clone();
    }
    let end = v.ended_at_ms.unwrap_or(now_ms);
    let mut s = v.title.clone();
    // A job with no count says nothing but its time.
    let bare = v.state.working() && v.attention.label == "running";
    if !v.attention.label.is_empty() && !bare {
        s.push_str(" · ");
        s.push_str(&v.attention.label);
    }
    if v.started_at_ms > 0 {
        s.push_str(" · ");
        s.push_str(&clock(end.saturating_sub(v.started_at_ms)));
    }
    s
}

/// The running system jobs in a few words, for the first line of a status:
/// `⚙ importing 41/120, rebuilding 40%`. None when none runs. Each job is
/// its title's first word and its count (`41/120`, `41`, `40%`).
pub fn status_words(views: &[WorkView]) -> Option<String> {
    let parts: Vec<String> = views
        .iter()
        .filter(|v| v.kind == WorkKind::System && v.parent.is_some() && v.state.working())
        .map(|v| {
            let verb = v.title.split_whitespace().next().unwrap_or("working");
            let count = match &v.progress {
                Some(p) if p.current.as_deref() == Some("%") => format!(" {}%", p.steps_done),
                Some(p) if p.steps_total > 0 => {
                    format!(" {}/{}", p.steps_done, p.steps_total.max(p.steps_done))
                }
                Some(p) if p.steps_done > 0 => format!(" {}", p.steps_done),
                _ => String::new(),
            };
            format!("{}{count}", verb.to_lowercase())
        })
        .collect();
    (!parts.is_empty()).then(|| format!("⚙ {}", parts.join(", ")))
}

/// The root collapsed to one phrase for a tree that shows it closed: the
/// running jobs (`status_words`), else `⚙ Theseus · 2 done, 1 failed` from the
/// root's count. None when the board holds no system work.
pub fn collapsed(views: &[WorkView]) -> Option<String> {
    if let Some(running) = status_words(views) {
        return Some(running);
    }
    let root = views.iter().find(|v| is_root(&v.id))?;
    let b = root.below.unwrap_or_default();
    let mut parts = Vec::new();
    if b.done > 0 {
        parts.push(format!("{} done", b.done));
    }
    if b.failed > 0 {
        parts.push(format!("{} failed", b.failed));
    }
    (!parts.is_empty()).then(|| format!("⚙ {ROOT_TITLE} · {}", parts.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_is_minutes_and_seconds_then_hours() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(192_000), "3:12");
        assert_eq!(clock(3_729_000), "1:02:09");
    }

    fn view(v: serde_json::Value) -> WorkView {
        let mut base = serde_json::json!({
            "id": "sys_import_1", "kind": "system", "position": 5, "at_ms": 9, "root": "system",
            "parent": "system", "title": "Importing Claude Code history", "state": "running",
            "attention": {"level": "working", "label": "batch 41 of 120", "since_ms": 1},
            "started_at_ms": 1_000, "state_since_ms": 1_000
        });
        for (k, x) in v.as_object().unwrap() {
            base[k] = x.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    /// The first line of a status says each running job's verb and count, and
    /// nothing while none runs; a tree that shows the root closed says its
    /// count of what ended.
    #[test]
    fn a_status_line_and_a_closed_root_say_what_runs_or_ended() {
        let import = view(serde_json::json!({
            "progress": {"steps_done": 41, "steps_total": 120, "current": "batch", "at_ms": 9}}));
        let rebuild = view(serde_json::json!({
            "id": "sys_rebuild_1", "title": "Rebuilding the index",
            "progress": {"steps_done": 40, "steps_total": 100, "current": "%", "at_ms": 9}}));
        let bare = view(serde_json::json!({"id": "sys_c", "title": "Consolidating memory"}));
        assert_eq!(
            status_words(&[import.clone(), rebuild]).as_deref(),
            Some("⚙ importing 41/120, rebuilding 40%")
        );
        assert_eq!(status_words(&[bare]).as_deref(), Some("⚙ consolidating"));
        let root = view(serde_json::json!({
            "id": "system", "parent": null, "title": "Theseus", "state": "ready",
            "below": {"needs_you": 0, "working": 0, "done": 2, "failed": 1, "total": 3}}));
        let mut done = import;
        done.state = crate::work::WorkState::Done;
        assert_eq!(status_words(&[root.clone(), done.clone()]), None);
        assert_eq!(
            collapsed(&[root, done]).as_deref(),
            Some("⚙ Theseus · 2 done, 1 failed")
        );
        assert_eq!(collapsed(&[]), None);
    }

    #[test]
    fn a_count_names_its_unit_and_its_total_when_known() {
        assert_eq!(count_words(41, 120, "batch"), "batch 41 of 120");
        assert_eq!(count_words(41, 0, "batch"), "batch 41");
        assert_eq!(count_words(14, 0, "clusters"), "14 clusters");
        assert_eq!(count_words(14, 40, "clusters"), "14 of 40 clusters");
        assert_eq!(count_words(41, 100, "%"), "41%");
        // A total below the count is the count: never "41 of 40".
        assert_eq!(count_words(41, 40, "batch"), "batch 41 of 41");
    }
}
