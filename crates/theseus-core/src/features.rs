//! Health's first line (theseus-is8t): which of Theseus's killer features
//! are on, worded from the config and the judge's own health, in one place
//! so health, `theseusd check` and the tests agree.
//!
//! Memory: recall's mode and arm. Jev: routing live or in shadow, or off
//! and why (no key). Outbound substitution: the vault's values replaced on what
//! leaves, on unless `[privacy] redact_outbound = false`. Budgets: the daemon's daily
//! ceiling, and what a session's limit does.

use theseus_protocol::features::Features;

use crate::config::{MemoryMode, PackMode, RoutingMode};
use crate::Config;

/// The features `cfg` turns on. `key` is the Jev key's state when it is
/// known (health's `judge.key`: `ready`, `resolving`, `failed: …`); `theseusd
/// check` passes none, having settled the board.
pub fn of(cfg: &Config, key: Option<&str>) -> Features {
    let memory = memory_words(cfg);
    let jev = jev_words(cfg, key);
    let scrub = if cfg.privacy.redact_outbound {
        "outbound substitution on"
    } else {
        "outbound substitution OFF (the vault's values leave as written)"
    }
    .to_string();
    let budgets = format!(
        "budgets: daily ceiling ${}",
        cfg.kernel.daily_spend_ceiling_usd
    );
    Features {
        line: format!("features: {memory} · {jev} · {scrub} · {budgets}"),
        memory,
        jev,
        scrub,
        budgets,
    }
}

fn memory_words(cfg: &Config) -> String {
    let m = &cfg.memory;
    match m.mode {
        MemoryMode::Off => "memory off".into(),
        MemoryMode::Live => format!("memory live ({})", m.arm.as_str()),
        MemoryMode::Shadow => format!("memory shadow ({})", m.arm.as_str()),
        MemoryMode::Canary => format!("memory canary ({}, {})", m.arm.as_str(), m.canary_fraction),
    }
}

fn jev_words(cfg: &Config, key: Option<&str>) -> String {
    let j = &cfg.judge;
    if !j.enabled {
        return format!("Jev off ({})", j.off_why());
    }
    let routing_acts = cfg.routing.enabled
        && cfg.routing.mode == RoutingMode::Live
        && j.mode_of(crate::judge::inbound::ROUTE_PACK, PackMode::Live) == PackMode::Live;
    let routing = if routing_acts {
        "routing live"
    } else {
        "routing shadow"
    };
    match key {
        Some(k) if k.starts_with("failed") => format!("Jev on, {routing}, key {k}"),
        Some("resolving") => format!("Jev on, {routing}, key resolving"),
        _ => format!("Jev on, {routing}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(extra: &str, rest: &str) -> Config {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n{extra}\n{rest}\n"
        ))
        .unwrap()
        .0
    }

    const SET: &str = "jev_api_key = \"env:PATH\"";

    /// The line in each state: a fresh install with a key, one without,
    /// `enabled = false`, memory off, routing in shadow, and a key that did
    /// not resolve.
    #[test]
    fn the_line_names_each_feature_in_each_state() {
        let f = of(&cfg(SET, ""), Some("ready"));
        assert_eq!(
            f.line,
            "features: memory live (baseline) · Jev on, routing live · outbound substitution on \
             · budgets: daily ceiling $200"
        );
        assert_eq!(f.line.lines().count(), 1);
        assert_eq!(
            (f.memory.as_str(), f.jev.as_str()),
            ("memory live (baseline)", "Jev on, routing live")
        );
        let none = of(&cfg("", ""), None);
        assert_eq!(none.jev, "Jev off (no jev_api_key entry under [secrets])");
        let off = of(&cfg(SET, "[judge]\nenabled = false"), None);
        assert_eq!(off.jev, "Jev off ([judge] enabled = false)");
        let shadow = of(&cfg(SET, "[routing]\nrung = \"shadow\""), None);
        assert_eq!(shadow.jev, "Jev on, routing shadow");
        let pack = crate::judge::inbound::ROUTE_PACK;
        let packed = of(
            &cfg(SET, &format!("[judge.packs.\"{pack}\"]\nrung = \"shadow\"")),
            None,
        );
        assert_eq!(packed.jev, "Jev on, routing shadow");
        let mem = of(&cfg(SET, "[memory]\nmode = \"off\""), None);
        assert_eq!(mem.memory, "memory off");
        let canary = of(&cfg(SET, "[memory]\nmode = \"canary\""), None);
        assert_eq!(canary.memory, "memory canary (baseline, 0.5)");
        let failed = of(&cfg(SET, ""), Some("failed: the vault said no"));
        assert_eq!(
            failed.jev,
            "Jev on, routing live, key failed: the vault said no"
        );
        let open = of(&cfg(SET, "[privacy]\nredact_outbound = false"), None);
        assert_eq!(
            open.scrub,
            "outbound substitution OFF (the vault's values leave as written)"
        );
        assert!(open.line.contains(&open.scrub), "{}", open.line);
        let ceiling = of(&cfg(SET, "[kernel]\ndaily_spend_ceiling_usd = 50"), None);
        assert!(ceiling.budgets.ends_with("$50"), "{}", ceiling.budgets);
    }
}
