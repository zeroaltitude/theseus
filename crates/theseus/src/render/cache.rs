//! Health's lines for the prompt cache (theseus-ezeg): each profile's TTL,
//! and whether it inherited `[cache]`'s; then today's keep-warm reads, which a
//! job wait makes (the reads between turns went with the transcript, issue
//! 44). Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::PromptCacheHealth;

use super::{plural, Tag};

/// `cache: default 1h, fable 1h (from [cache]), glm 5m (from [cache]), …`,
/// then `  keep-warm: …` when a wait read today. Nothing from a daemon before it.
pub fn cache_lines(c: Option<&PromptCacheHealth>) -> Vec<(Tag, String)> {
    let Some(c) = c else { return Vec::new() };
    let ttls: Vec<String> = c
        .profiles
        .iter()
        .map(|p| {
            let from = if p.inherited { " (from [cache])" } else { "" };
            format!("{} {}{from}", p.profile, p.ttl)
        })
        .collect();
    let mut out = vec![(Tag::Plain, format!("cache: {}", ttls.join(", ")))];
    if c.reads_today > 0 {
        out.push((
            Tag::Plain,
            format!(
                "  keep-warm: reads during job waits · {} today, ${:.2}",
                plural(c.reads_today, "read", "reads"),
                c.usd_today
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ProfileTtl;

    fn ttl(profile: &str, ttl: &str, inherited: bool) -> ProfileTtl {
        ProfileTtl {
            profile: profile.into(),
            model: "claude-fable-5-1".into(),
            ttl: ttl.into(),
            inherited,
            ..ProfileTtl::default()
        }
    }

    /// Each profile's TTL, said inherited where it is, and today's reads of
    /// job waits; no keep-warm line on a day with none.
    #[test]
    fn health_lists_each_profiles_ttl_and_the_reads_of_waits() {
        let c = PromptCacheHealth {
            profiles: vec![
                ttl("default", "1h", false),
                ttl("fable", "1h", true),
                ttl("glm", "5m", false),
                ttl("opus", "1h", false),
            ],
            reads_today: 14,
            usd_today: 2.1875,
            ..PromptCacheHealth::default()
        };
        let lines = cache_lines(Some(&c));
        assert_eq!(
            lines[0].1,
            "cache: default 1h, fable 1h (from [cache]), glm 5m, opus 1h"
        );
        assert_eq!(
            lines[1].1,
            "  keep-warm: reads during job waits · 14 reads today, $2.19"
        );
        let quiet = PromptCacheHealth {
            profiles: vec![ttl("glm", "5m", true)],
            ..PromptCacheHealth::default()
        };
        let lines = cache_lines(Some(&quiet));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, "cache: glm 5m (from [cache])");
        assert!(cache_lines(None).is_empty());
    }
}
