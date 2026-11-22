//! The prompt cache (theseus-ev1, theseus-ezeg): one section, `[cache]`. A cache
//! entry's lifetime, `CacheTtl`, is `[cache] ttl`, and a profile's own
//! `cache_ttl` when it says one: a profile that says none inherits `[cache]`'s
//! (theseus-ezeg; before it, a profile without one cached for 5 minutes
//! whatever the default said). A profile that must stay at 5 minutes under a
//! 1-hour `[cache] ttl` says `cache_ttl = "5m"`, which serializes.
//!
//! `[cache]` holds that default, and what is the daemon's, not a profile's:
//!
//! - `ttl` (`5m`): a profile's default.
//! - `keep_warm_every_minutes` (55): how often a job wait reads a one-hour
//!   cache again while it waits (`crate::keep_warm_wait`). Under the 1-hour
//!   TTL by 5 minutes: the provider's clock starts at its own receipt of the
//!   last call, and a read that lands after the entry expired writes it whole
//!   again at 2x instead of reading it at 0.025x of input.
//! - `keep_warm_min_tokens` (20,000): a request estimated under this is not
//!   kept warm during a wait, and the sticky effort rule holds only above it
//!   (`turn::effort_step`). A read and a rewrite both scale with the prefix
//!   (on Fable 5.1 one 1-hour write costs as much as 80 reads), so the floor
//!   is for what does not: a small prompt's cold write is cents.
//!
//! The keep-warm reads BETWEEN turns, their windows (`keep_warm_hours`,
//! `threads_keep_warm_hours`) and the stubs of a cold rewrite
//! (`stub_min_tokens`, `stub_after_turns`) went with the transcript (issue 44).

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::ProfileConfig;

/// A prompt cache entry's lifetime (theseus-ev1). Each read restarts it.
/// `1h` writes cost 2 × input against 1.25 × for `5m`, and pay when the
/// prefix is read again after a pause of 5 to 60 minutes. It applies to
/// every breakpoint of a conversation's requests, the automatic one
/// included; a task's own conversation keeps `5m` (its loops run seconds
/// apart), after the header's `1h` entries, as the order rule asks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CacheTtl {
    #[default]
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl CacheTtl {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The lifetime in seconds.
    pub fn secs(self) -> u64 {
        match self {
            Self::FiveMinutes => 300,
            Self::OneHour => 3600,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinutes => "5m",
            Self::OneHour => "1h",
        }
    }

    /// The `cache_control` marker. Five minutes is the API's default, so its
    /// marker names no `ttl`: the bytes every request carried before 13c.
    pub fn marker(self) -> serde_json::Value {
        match self {
            Self::FiveMinutes => serde_json::json!({"type": "ephemeral"}),
            Self::OneHour => serde_json::json!({"type": "ephemeral", "ttl": "1h"}),
        }
    }

    /// How long an entry lives, in ms.
    pub fn ms(self) -> u64 {
        match self {
            Self::FiveMinutes => 5 * 60_000,
            Self::OneHour => 60 * 60_000,
        }
    }
}

/// A profile's settings that come from `[cache]` when it says none: the
/// resolved profiles carry the value in force, and `[profiles]` as written
/// keeps `None`, which is how health tells an inherited one.
pub(super) fn inherit(profiles: &mut BTreeMap<String, ProfileConfig>, c: &CacheConfig) {
    for p in profiles.values_mut() {
        p.cache_ttl.get_or_insert(c.ttl);
    }
}

impl ProfileConfig {
    /// The cache TTL in force: the profile's, else `[cache]`'s, which a
    /// resolved profile always carries.
    pub fn ttl(&self) -> CacheTtl {
        self.cache_ttl.unwrap_or_default()
    }
}

impl super::Config {
    /// A profile's TTL in force, and where it comes from: `profile` when
    /// it says its own, `cache` when it inherits `[cache]`'s (the implicit
    /// default profile is `[cache]`'s own).
    pub fn ttl_of(&self, name: &str) -> Option<(CacheTtl, &'static str)> {
        let p = self.all_profiles().get(name)?;
        let own = self
            .profiles
            .get(name)
            .is_some_and(|raw| raw.cache_ttl.is_some());
        Some((p.ttl(), if own { "profile" } else { "cache" }))
    }
}

/// `[cache]`: the cache's lifetime, and the keep-warm read of a job wait (theseus-ezeg).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default, skip_serializing_if = "CacheTtl::is_default")]
    pub ttl: CacheTtl,
    #[serde(default = "default_keep_warm_minutes")]
    pub keep_warm_every_minutes: f64,
    #[serde(default = "default_keep_warm_min_tokens")]
    pub keep_warm_min_tokens: u64,
}

fn default_keep_warm_minutes() -> f64 {
    55.0
}
fn default_keep_warm_min_tokens() -> u64 {
    20_000
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl: CacheTtl::default(),
            keep_warm_every_minutes: default_keep_warm_minutes(),
            keep_warm_min_tokens: default_keep_warm_min_tokens(),
        }
    }
}

impl CacheConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The keep-warm interval, in ms.
    pub fn interval_ms(&self) -> u64 {
        (self.keep_warm_every_minutes * 60_000.0) as u64
    }

    /// Each value in range, and the interval under the 1-hour TTL: a read
    /// past it finds the entry gone and writes it whole.
    pub(crate) fn validate(&self, _cfg: &super::Config) -> Result<()> {
        let m = self.keep_warm_every_minutes;
        if !(m > 0.0 && m < 60.0) {
            bail!("[cache] keep_warm_every_minutes must be above 0 and under 60 (the 1-hour cache's life), not {m}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn with(from: &str, to: &str) -> Result<Config> {
        assert!(Config::EXAMPLE_TOML.contains(from), "{from}");
        Ok(Config::parse(&Config::EXAMPLE_TOML.replace(from, to))?.0)
    }

    /// The template's `[cache]` is the defaults, and a value out of range
    /// is refused with its key.
    #[test]
    fn the_cache_section_has_its_defaults_and_its_range() {
        let cfg = Config::example();
        assert_eq!(cfg.cache, CacheConfig::default());
        assert_eq!(cfg.cache.interval_ms(), 55 * 60_000);
        for bad in ["60", "0", "-1"] {
            let e = with(
                "keep_warm_every_minutes = 55",
                &format!("keep_warm_every_minutes = {bad}"),
            );
            let e = format!("{:#}", e.unwrap_err());
            assert!(e.contains("keep_warm_every_minutes"), "{e}");
        }
    }

    /// A profile that must stay at 5 minutes under a 1-hour `[cache] ttl` says
    /// so, and it holds: the profile's own "5m" wins, it serializes (it is
    /// no longer the value nothing says), and health names it the
    /// profile's own.
    #[test]
    fn an_explicit_five_minutes_holds_under_a_one_hour_model() {
        let cfg = with("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n").unwrap();
        let text = Config::EXAMPLE_TOML
            .replace("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n")
            .replace(
                "[profiles.opus]\nprovider = \"anthropic\"\n",
                "[profiles.opus]\nprovider = \"anthropic\"\ncache_ttl = \"5m\"\n",
            );
        let (five, _) = Config::parse(&text).unwrap();
        assert_eq!(cfg.profile("opus").unwrap().ttl(), CacheTtl::OneHour);
        assert_eq!(five.profile("opus").unwrap().ttl(), CacheTtl::FiveMinutes);
        assert_eq!(
            five.ttl_of("opus"),
            Some((CacheTtl::FiveMinutes, "profile"))
        );
        assert_eq!(five.ttl_of("fable"), Some((CacheTtl::OneHour, "cache")));
        assert_eq!(five.ttl_of("default"), Some((CacheTtl::OneHour, "cache")));
        let raw = toml::to_string(&five.profiles["opus"]).unwrap();
        assert!(raw.contains("cache_ttl = \"5m\""), "{raw}");
        // Written back and read again, it is still the profile's own.
        let back = toml::to_string(&five).unwrap();
        let (again, _) = Config::parse(&back).unwrap();
        assert_eq!(
            again.ttl_of("opus"),
            Some((CacheTtl::FiveMinutes, "profile"))
        );
    }
}
