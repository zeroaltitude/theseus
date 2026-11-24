//! Health's prompt-cache TTLs, including profiles that inherit `[cache]`.

use theseus_protocol::PromptCacheHealth;

use super::Tag;

/// `cache: default 1h, fable 1h (from [cache]), glm 5m (from [cache]), …`,
/// Nothing from a daemon before prompt-cache health.
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
    vec![(Tag::Plain, format!("cache: {}", ttls.join(", ")))]
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

    /// TTLs remain current; an older daemon's keep-warm counts are ignored.
    #[test]
    fn health_lists_ttls_without_retired_keep_warm_reads() {
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
            lines.len(),
            1,
            "retired keep-warm reads are not current health"
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
