//! Health's `cache` block: each profile's TTL and where it comes from. Nothing is kept warm any more (issue
//! 44: not between turns, and since 2026-11-22 not during a wait), so the fields of the reads (`kept`,
//! `stopped`, `reads_today`, `usd_today`, a profile's `keep_warm_hours`, `keep_warm_minutes`) are sent as
//! zero: the CLI's `theseus health` reads `reads_today` and `usd_today` (`render/cache.rs`, shown only above
//! zero), and a cockpit page left open since the upgrade reads the rest.

impl super::Core {
    /// Health's `cache` block.
    pub fn cache_health(&self) -> theseus_protocol::PromptCacheHealth {
        let profiles = self
            .cfg
            .all_profiles()
            .iter()
            .filter_map(|(name, p)| {
                let (ttl, from) = self.cfg.ttl_of(name)?;
                Some(theseus_protocol::ProfileTtl {
                    profile: name.clone(),
                    model: p.model.clone(),
                    ttl: ttl.as_str().into(),
                    inherited: from == "cache" && name != "default",
                    keep_warm_hours: 0.0,
                })
            })
            .collect();
        theseus_protocol::PromptCacheHealth {
            profiles,
            ..Default::default()
        }
    }
}
