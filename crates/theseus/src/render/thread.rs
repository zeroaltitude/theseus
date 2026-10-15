//! Threads in a terminal (theseus-j0yh): the status line that names the
//! current thread, the reply's tag, and the picker's rows. Pure functions of
//! `thread.list`'s rows and the clock the caller passes; the CLI's interactive
//! loop and the TUI both draw from them.

use theseus_protocol::threads::{ThreadRow, ThreadState};

/// What the status line calls no thread: the place's home conversation.
pub const HOME: &str = "home";

/// Whether the thread's prompt cache is still alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warmth {
    Warm,
    Cold,
}

impl Warmth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "warm",
            Self::Cold => "cold",
        }
    }
}

/// Warm when the session's last activity is within its profile's cache TTL.
/// None before a first turn (no session, or no profile to read a TTL from):
/// the line says nothing rather than guess.
pub fn warmth(row: &ThreadRow, now_ms: u64) -> Option<Warmth> {
    let ttl = row.cache_ttl_secs?;
    row.thread.session_id.as_ref()?;
    let age = now_ms.saturating_sub(row.thread.last_active_ms);
    Some(if age < ttl.saturating_mul(1000) {
        Warmth::Warm
    } else {
        Warmth::Cold
    })
}

/// A context size as `41K`.
pub fn ctx(tokens: u64) -> String {
    match tokens {
        t if t >= 1_000_000 => format!("{:.1}M", t as f64 / 1e6),
        t if t >= 1_000 => format!("{}K", (t + 500) / 1_000),
        t => t.to_string(),
    }
}

/// `text` cut to `room` characters, with a `…` where it was cut.
fn fit(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let cut: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The status line: `bh-ai pr · active · 41K ctx · warm`, or `home` for no
/// thread. Within `width` columns it gives up, in order, the state, the
/// context, the warmth, and then cuts the title.
pub fn status_line(current: Option<&ThreadRow>, now_ms: u64, width: usize) -> String {
    let Some(row) = current else {
        return HOME.to_string();
    };
    let title = row.thread.title.as_str();
    let state = row.thread.state.as_str();
    let ctx = row.context_tokens.map(|t| format!("{} ctx", self::ctx(t)));
    let warm = warmth(row, now_ms).map(|w| w.as_str().to_string());
    let join = |parts: Vec<&str>| parts.join(" · ");
    let tries: [Vec<&str>; 4] = [
        [Some(title), Some(state), ctx.as_deref(), warm.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        [Some(title), ctx.as_deref(), warm.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        [Some(title), warm.as_deref()]
            .into_iter()
            .flatten()
            .collect(),
        vec![title],
    ];
    for parts in tries {
        let line = join(parts);
        if line.chars().count() <= width {
            return line;
        }
    }
    fit(title, width)
}

/// The reply's tag, dim, in the header line of a reply made in a thread.
pub fn reply_tag(title: &str) -> String {
    format!("→ {title}")
}

/// One picker row, numbered from 1 as `/t N` takes it: title, persona, state,
/// last activity (`when`, in the caller's local time), context, warmth, and
/// 🔒 when sealed. `mark` shows the current thread.
pub fn picker_line(n: usize, row: &ThreadRow, when: &str, now_ms: u64, current: bool) -> String {
    let t = &row.thread;
    let mut parts = vec![t.title.clone()];
    if let Some(p) = &t.persona {
        parts.push(p.clone());
    }
    parts.push(
        match t.state {
            ThreadState::Active => "active",
            ThreadState::Paused => "paused",
            ThreadState::Done => "done",
        }
        .to_string(),
    );
    parts.push(when.to_string());
    parts.push(
        row.context_tokens
            .map_or("-".into(), |c| format!("{} ctx", ctx(c))),
    );
    if let Some(w) = warmth(row, now_ms) {
        parts.push(w.as_str().to_string());
    }
    let lock = if t.sealed.is_some() { " 🔒" } else { "" };
    format!(
        "{n:>2} {}{}{lock}",
        if current { "▸ " } else { "  " },
        parts.join(" · ")
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use theseus_protocol::threads::{ThreadInfo, WorkingSet};

    pub(crate) fn row(slug: &str, title: &str) -> ThreadRow {
        ThreadRow {
            thread: ThreadInfo {
                slug: slug.into(),
                title: title.into(),
                goal: String::new(),
                topics: vec![],
                working_set: WorkingSet::default(),
                card: vec![],
                persona: None,
                state: ThreadState::Active,
                sealed: None,
                seal_pending: None,
                surfaces: vec![],
                parent: None,
                merged_into: None,
                split_from: None,
                session_id: Some("ses_a".into()),
                last_active_ms: 1_000_000,
                created_at_ms: 0,
            },
            context_tokens: Some(41_200),
            today_usd: 0.0,
            cache_ttl_secs: Some(3600),
            cache: None,
            merged_from: vec![],
            split_to: vec![],
        }
    }

    #[test]
    fn warm_inside_the_ttl_cold_after_and_silent_without_one() {
        let r = row("pr", "bh-ai pr");
        assert_eq!(warmth(&r, 1_000_000 + 3_599_000), Some(Warmth::Warm));
        assert_eq!(warmth(&r, 1_000_000 + 3_600_000), Some(Warmth::Cold));
        let mut none = r.clone();
        none.cache_ttl_secs = None;
        assert_eq!(warmth(&none, 2_000_000), None);
        let mut fresh = r;
        fresh.thread.session_id = None;
        assert_eq!(warmth(&fresh, 2_000_000), None);
    }

    #[test]
    fn the_status_line_names_the_thread_and_shrinks_with_the_width() {
        let r = row("pr", "bh-ai pr");
        let now = 1_000_000 + 60_000;
        assert_eq!(status_line(None, now, 80), "home");
        assert_eq!(
            status_line(Some(&r), now, 80),
            "bh-ai pr · active · 41K ctx · warm"
        );
        assert_eq!(status_line(Some(&r), now, 30), "bh-ai pr · 41K ctx · warm");
        assert_eq!(status_line(Some(&r), now, 18), "bh-ai pr · warm");
        assert_eq!(status_line(Some(&r), now, 6), "bh-ai…");
    }

    #[test]
    fn a_cold_thread_says_cold_and_a_sealed_one_wears_the_lock() {
        let mut r = row("pr", "bh-ai pr");
        r.thread.sealed = Some(theseus_protocol::threads::Seal {
            by: "owner".into(),
            at_ms: 0,
            reason: "x".into(),
        });
        r.thread.persona = Some("surveyor".into());
        let line = picker_line(2, &r, "09:12", 10_000_000, true);
        assert_eq!(
            line,
            " 2 ▸ bh-ai pr · surveyor · active · 09:12 · 41K ctx · cold 🔒"
        );
        assert_eq!(reply_tag("bh-ai pr"), "→ bh-ai pr");
    }
}
