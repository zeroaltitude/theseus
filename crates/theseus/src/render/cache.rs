//! Health's lines for the prompt cache (theseus-ezeg): each profile's TTL,
//! and whether it inherited `[model]`'s; then the keep-warm's state. Apart
//! from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::PromptCacheHealth;

use super::{plural, Tag};

/// `cache: default 1h, fable 1h (from [model]), glm 5m (from [model]), …`,
/// then `  keep-warm: …`. Nothing from a daemon before it.
pub fn cache_lines(c: Option<&PromptCacheHealth>) -> Vec<(Tag, String)> {
    let Some(c) = c else { return Vec::new() };
    let ttls: Vec<String> = c
        .profiles
        .iter()
        .map(|p| {
            let from = if p.inherited { " (from [model])" } else { "" };
            format!("{} {}{from}", p.profile, p.ttl)
        })
        .collect();
    let mut out = vec![(Tag::Plain, format!("cache: {}", ttls.join(", ")))];
    let warm: Vec<String> = c
        .profiles
        .iter()
        .filter(|p| p.keep_warm_hours > 0.0)
        .map(|p| format!("{} {}h", p.profile, trim(p.keep_warm_hours)))
        .collect();
    if warm.is_empty() {
        out.push((
            Tag::Plain,
            "  keep-warm: off (no profile caches for an hour and keeps warm)".into(),
        ));
        return out;
    }
    let stopped = match c.stopped {
        0 => String::new(),
        n => format!(", {} stopped until a message", n),
    };
    let tag = if c.stopped > 0 { Tag::Warn } else { Tag::Plain };
    out.push((
        tag,
        format!(
            "  keep-warm: a read every {} min after a session's last call, for {} after its last \
             message · {} warm now{stopped} · {} today, ${:.2}",
            trim(c.keep_warm_minutes),
            warm.join(", "),
            plural(c.kept, "session", "sessions"),
            plural(c.reads_today, "read", "reads"),
            c.usd_today
        ),
    ));
    out
}

/// A whole number without its decimals.
fn trim(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::ProfileTtl;

    fn ttl(profile: &str, ttl: &str, inherited: bool, hours: f64) -> ProfileTtl {
        ProfileTtl {
            profile: profile.into(),
            model: "claude-fable-5-1".into(),
            ttl: ttl.into(),
            inherited,
            keep_warm_hours: hours,
        }
    }

    /// Each profile's TTL, said inherited where it is, and the keep-warm's
    /// sessions, its stops (a warning) and today's reads.
    #[test]
    fn health_lists_each_profiles_ttl_and_the_keep_warm() {
        let c = PromptCacheHealth {
            profiles: vec![
                ttl("default", "1h", false, 24.0),
                ttl("fable", "1h", true, 24.0),
                ttl("glm", "5m", false, 0.0),
                ttl("opus", "1h", false, 12.0),
            ],
            keep_warm_minutes: 55.0,
            kept: 2,
            stopped: 1,
            reads_today: 14,
            usd_today: 2.1875,
        };
        let lines = cache_lines(Some(&c));
        assert_eq!(
            lines[0].1,
            "cache: default 1h, fable 1h (from [model]), glm 5m, opus 1h"
        );
        assert_eq!(
            lines[1].1,
            "  keep-warm: a read every 55 min after a session's last call, for default 24h, \
             fable 24h, opus 12h after its last message · 2 sessions warm now, 1 stopped until \
             a message · 14 reads today, $2.19"
        );
        assert!(matches!(lines[1].0, Tag::Warn));
        let off = PromptCacheHealth {
            profiles: vec![ttl("glm", "5m", true, 0.0)],
            ..PromptCacheHealth::default()
        };
        let lines = cache_lines(Some(&off));
        assert_eq!(lines[0].1, "cache: glm 5m (from [model])");
        assert!(lines[1].1.contains("keep-warm: off"), "{}", lines[1].1);
        assert!(cache_lines(None).is_empty());
    }
}
