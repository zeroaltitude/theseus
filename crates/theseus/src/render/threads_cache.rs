//! Health's lines for threads' keep-warm and the day's cold resumes
//! (theseus-ytt8). Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::thread_cache::ThreadsHealth;

use super::{plural, Tag};

/// `threads: kept warm 4h after the owner's last message · 2 held now`, then
/// the day's numbers. Nothing from a daemon before it, or one with no thread.
pub fn threads_lines(t: Option<&ThreadsHealth>) -> Vec<(Tag, String)> {
    let Some(t) = t else { return Vec::new() };
    let warm = if t.keep_warm_hours > 0.0 {
        format!(
            "kept warm {}h after the owner's last message",
            t.keep_warm_hours
        )
    } else {
        "keep-warm off ([threads] keep_warm_hours = 0)".to_string()
    };
    let Some(c) = &t.cache else {
        return vec![(Tag::Plain, format!("threads: {warm}"))];
    };
    let skipped = match c.skipped_today {
        0 => String::new(),
        n => format!(
            " · {} skipped at the day's spend line",
            plural(n, "read", "reads")
        ),
    };
    let tag = if c.skipped_today > 0 {
        Tag::Warn
    } else {
        Tag::Plain
    };
    let mut out = vec![(
        tag,
        format!(
            "threads: {warm} · {} held now{skipped}",
            plural(c.kept, "session", "sessions")
        ),
    )];
    if !c.total.is_empty() {
        out.push((
            Tag::Plain,
            format!("  today ({}): {}", c.day, c.total.line()),
        ));
    }
    for e in c.threads.iter().filter(|e| !e.stats.is_empty()) {
        out.push((Tag::Plain, format!("    {}: {}", e.slug, e.stats.line())));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::thread_cache::{ThreadCacheEntry, ThreadCacheStats, ThreadsCache};

    fn stats() -> ThreadCacheStats {
        ThreadCacheStats {
            cold_resumes: 2,
            rewrite_tokens: 180_000,
            rewrite_usd: 1.2,
            warm_hits: 5,
            avoided: 3,
            keep_warm_reads: 14,
            keep_warm_usd: 0.09,
            saved_usd: 0.8,
        }
    }

    /// The window, what is held, the day's total and each thread's own line.
    #[test]
    fn the_lines_say_the_window_the_held_and_the_days_numbers() {
        assert!(threads_lines(None).is_empty());
        let t = ThreadsHealth {
            keep_warm_hours: 4.0,
            cache: Some(ThreadsCache {
                day: "2026-10-14".into(),
                total: stats(),
                threads: vec![ThreadCacheEntry {
                    slug: "tide-gauge".into(),
                    stats: stats(),
                    kept: true,
                }],
                kept: 1,
                skipped_today: 2,
            }),
        };
        let lines = threads_lines(Some(&t));
        assert_eq!(lines[0].0, Tag::Warn);
        assert_eq!(
            lines[0].1,
            "threads: kept warm 4h after the owner's last message · 1 session held now · \
             2 reads skipped at the day's spend line"
        );
        assert_eq!(
            lines[1].1,
            "  today (2026-10-14): 2 cold resumes (180k tokens rewritten, $1.20) · 5 warm hits \
             (3 kept by keep-warm, saving about $0.80) · 14 keep-warm reads, $0.09"
        );
        assert!(lines[2].1.starts_with("    tide-gauge: 2 cold resumes"));
        let off = ThreadsHealth {
            keep_warm_hours: 0.0,
            cache: None,
        };
        assert_eq!(
            threads_lines(Some(&off))[0].1,
            "threads: keep-warm off ([threads] keep_warm_hours = 0)"
        );
    }
}
