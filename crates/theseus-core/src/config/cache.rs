//! The prompt cache (theseus-ev1, theseus-ezeg). A cache entry's lifetime,
//! `CacheTtl`, is `[model]`'s `cache_ttl`, and a profile's own when it says
//! one: a profile that says none inherits `[model]`'s (theseus-ezeg; before
//! it, a profile without one cached for 5 minutes whatever `[model]` said).
//! So does `keep_warm_hours`. A profile that must stay at 5 minutes under a
//! 1-hour `[model]` says `cache_ttl = "5m"`, which now serializes.
//!
//! `[cache]` holds what is the daemon's, not a profile's:
//!
//! - `keep_warm_minutes` (55): how long after a conversation's last call its
//!   keep-warm read goes (`crate::keep_warm`). Under the 1-hour TTL by 5
//!   minutes: the provider's clock starts at its own receipt of the last
//!   call, and a read that lands after the entry expired writes it whole
//!   again at 2x instead of reading it at 0.025x of input.
//! - `keep_warm_min_tokens` (20,000): a conversation whose prefix is
//!   estimated under this is not kept warm. A read and a rewrite both scale
//!   with the prefix (on Fable 5.1 one 1-hour write costs as much as 80
//!   reads), so the floor is for what does not: a small prompt's cold write
//!   is cents, and each read carries the reply's own small write and its
//!   request.
//! - `stub_min_tokens` (20,000) and `stub_after_turns` (3): at a cold
//!   rewrite, an attachment's extracted text over the first, older than the
//!   last this many turns, is sent as a one-line stub instead
//!   (`crate::compiler::stubs`). 0 for `stub_min_tokens` turns it off.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::{ModelConfig, ProfileConfig};

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

/// `keep_warm_hours`' default: a day after the last message.
pub(super) fn default_keep_warm_hours() -> f64 {
    24.0
}

pub(super) fn is_default_keep_warm_hours(h: &f64) -> bool {
    *h == default_keep_warm_hours()
}

/// A profile's settings that come from `[model]` when it says none: the
/// resolved profiles carry the value in force, and `[profiles]` as written
/// keeps `None`, which is how health tells an inherited one.
pub(super) fn inherit(profiles: &mut BTreeMap<String, ProfileConfig>, m: &ModelConfig) {
    for p in profiles.values_mut() {
        p.cache_ttl.get_or_insert(m.cache_ttl);
        p.keep_warm_hours.get_or_insert(m.keep_warm_hours);
    }
}

impl ProfileConfig {
    /// The cache TTL in force: the profile's, else `[model]`'s, which a
    /// resolved profile always carries.
    pub fn ttl(&self) -> CacheTtl {
        self.cache_ttl.unwrap_or_default()
    }

    /// The keep-warm window in force, in hours (0: none).
    pub fn keep_warm_hours(&self) -> f64 {
        self.keep_warm_hours.unwrap_or_else(default_keep_warm_hours)
    }
}

impl super::Config {
    /// A profile's TTL in force, and where it comes from: `profile` when
    /// it says its own, `model` when it inherits `[model]`'s (the implicit
    /// default profile is `[model]`'s own).
    pub fn ttl_of(&self, name: &str) -> Option<(CacheTtl, &'static str)> {
        let p = self.all_profiles().get(name)?;
        let own = self
            .profiles
            .get(name)
            .is_some_and(|raw| raw.cache_ttl.is_some());
        Some((p.ttl(), if own { "profile" } else { "model" }))
    }
}

/// `[cache]`: the keep-warm read and the cold rewrite's stubs (theseus-ezeg).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default = "default_keep_warm_minutes")]
    pub keep_warm_minutes: f64,
    #[serde(default = "default_keep_warm_min_tokens")]
    pub keep_warm_min_tokens: u64,
    #[serde(default = "default_stub_min_tokens")]
    pub stub_min_tokens: u64,
    #[serde(default = "default_stub_after_turns")]
    pub stub_after_turns: u32,
}

fn default_keep_warm_minutes() -> f64 {
    55.0
}
fn default_keep_warm_min_tokens() -> u64 {
    20_000
}
fn default_stub_min_tokens() -> u64 {
    20_000
}
fn default_stub_after_turns() -> u32 {
    3
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            keep_warm_minutes: default_keep_warm_minutes(),
            keep_warm_min_tokens: default_keep_warm_min_tokens(),
            stub_min_tokens: default_stub_min_tokens(),
            stub_after_turns: default_stub_after_turns(),
        }
    }
}

impl CacheConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The keep-warm interval, in ms.
    pub fn interval_ms(&self) -> u64 {
        (self.keep_warm_minutes * 60_000.0) as u64
    }

    /// Each value in range, and the interval under the 1-hour TTL: a read
    /// past it finds the entry gone and writes it whole.
    pub(crate) fn validate(&self, cfg: &super::Config) -> Result<()> {
        let m = self.keep_warm_minutes;
        if !(m > 0.0 && m < 60.0) {
            bail!("[cache] keep_warm_minutes must be above 0 and under 60 (the 1-hour cache's life), not {m}");
        }
        for (name, p) in cfg.all_profiles() {
            let h = p.keep_warm_hours();
            if !(h >= 0.0 && h.is_finite()) {
                bail!("profile {name:?}: keep_warm_hours must be 0 or more, not {h}");
            }
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
                "keep_warm_minutes = 55",
                &format!("keep_warm_minutes = {bad}"),
            );
            let e = format!("{:#}", e.unwrap_err());
            assert!(e.contains("keep_warm_minutes"), "{e}");
        }
        let e = format!(
            "{:#}",
            with("keep_warm_hours = 24\n", "keep_warm_hours = -2\n").unwrap_err()
        );
        assert!(e.contains("keep_warm_hours"), "{e}");
    }

    /// `keep_warm_hours` is inherited as `cache_ttl` is: a profile without
    /// one takes `[model]`'s, and 0 on a profile turns its keep-warm off.
    #[test]
    fn keep_warm_hours_is_inherited_like_the_ttl() {
        let cfg = with(
            "[profiles.opus]\nprovider = \"anthropic\"\n",
            "[profiles.opus]\nprovider = \"anthropic\"\nkeep_warm_hours = 0\n",
        )
        .unwrap();
        assert_eq!(cfg.profile("fable").unwrap().keep_warm_hours(), 24.0);
        assert_eq!(cfg.profile("opus").unwrap().keep_warm_hours(), 0.0);
        assert_eq!(cfg.profile("default").unwrap().keep_warm_hours(), 24.0);
        assert_eq!(cfg.profiles["fable"].keep_warm_hours, None);
        let cfg = with("keep_warm_hours = 24\n", "keep_warm_hours = 12\n").unwrap();
        assert_eq!(cfg.profile("fable").unwrap().keep_warm_hours(), 12.0);
    }

    /// A profile that must stay at 5 minutes under a 1-hour `[model]` says
    /// so, and it holds: the profile's own "5m" wins, it serializes (it is
    /// no longer the value nothing says), and health names it the
    /// profile's own.
    #[test]
    fn an_explicit_five_minutes_holds_under_a_one_hour_model() {
        let cfg = with("live = \"sonnet\"", "live = \"sonnet\"\ncache_ttl = \"1h\"").unwrap();
        let text = Config::EXAMPLE_TOML
            .replace("live = \"sonnet\"", "live = \"sonnet\"\ncache_ttl = \"1h\"")
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
        assert_eq!(five.ttl_of("fable"), Some((CacheTtl::OneHour, "model")));
        assert_eq!(five.ttl_of("default"), Some((CacheTtl::OneHour, "model")));
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
