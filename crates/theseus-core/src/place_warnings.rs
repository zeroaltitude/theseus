//! What a binding's start finds wrong with its places (theseus-ext.11): a
//! place left unbound (its ceiling names a profile the config lacks, so it
//! alone is read as a channel the file does not name), a ceiling's tool
//! family this daemon has no tool of, and a spend limit below one call's
//! output reservation on its profile. Each is a warning in the log, a
//! `place.warned` row with its narrative line, recorded once a start (as
//! `place.viewed` is), and health's `places.warnings`. Nothing here refuses:
//! the binding binds every other place.

use theseus_protocol::PlaceWarning;

use crate::places::BoundPlace;
use crate::rpc::Core;

/// A place left unbound for want of its ceiling's profile.
pub const UNBOUND: &str = "unbound";
/// A ceiling's `tools` entry that names no tool here.
pub const UNKNOWN_FAMILY: &str = "unknown_family";
/// A spend limit below one call's output reservation.
pub const LIMIT_BELOW_CALL: &str = "limit_below_call";

/// Money as health and the log say it: `$1.28`.
fn dollars(micros: u64) -> String {
    format!("${:.2}", theseus_kernel::micros_to_usd(micros))
}

/// The warning for a place whose ceiling names a profile the config lacks:
/// the binding leaves it alone unbound.
pub fn unbound(place: &str, name: &str, profile: &str, configured: &[String]) -> PlaceWarning {
    PlaceWarning {
        place: place.into(),
        name: name.into(),
        kind: UNBOUND.into(),
        detail: format!(
            "{name} is not bound: its ceiling names profile {profile:?}, which the config does \
             not have (configured: {}); fix the bindings file or the config, and restart",
            configured.join(", ")
        ),
    }
}

impl Core {
    /// The binding's start (theseus-ext.11): the places it left unbound, and
    /// what it finds of the ones it bound, said in the log, ledgered once
    /// each, and kept for health. Returns every warning.
    pub fn place_warnings(
        &self,
        bound: &[BoundPlace],
        unbound: Vec<PlaceWarning>,
    ) -> Vec<PlaceWarning> {
        let mut all = unbound;
        for p in bound {
            for (_, family) in self.unknown_families(std::slice::from_ref(p)) {
                all.push(PlaceWarning {
                    place: p.target.clone(),
                    name: p.name.clone(),
                    kind: UNKNOWN_FAMILY.into(),
                    detail: format!(
                        "{}'s ceiling names tool family {family:?}, which this daemon has no \
                         tool of: it offers nothing there",
                        p.name
                    ),
                });
            }
            all.extend(self.limit_below_call(p));
        }
        for w in &all {
            tracing::warn!(place = %w.place, name = %w.name, kind = %w.kind, "{}", w.detail);
            self.rec(None)
                .record(&crate::fact::place::PlaceWarned { warning: w });
        }
        self.runner.place_rule.warn(all.clone());
        all
    }

    /// A place whose spend limit is below one call's output reservation on
    /// its profile (its ceiling's, else the live one): no call of the
    /// profile's output cap fits under it, before its input. None for a
    /// place with no limit, or a model the catalog does not price, which
    /// has no figure (the log says so).
    fn limit_below_call(&self, p: &BoundPlace) -> Option<PlaceWarning> {
        let c = p.ceiling.as_ref()?;
        let limit = c.spend_limit_usd?;
        let profile = match &c.profile {
            Some(name) => name.clone(),
            None => self.live_profile().0,
        };
        let prof = match self.cfg.profile(&profile) {
            Ok(p) => p,
            Err(e) => {
                tracing::info!(place = %p.target, error = %format!("{e:#}"),
                    "a place's spend limit was not weighed against its profile");
                return None;
            }
        };
        let Some(price) = self.catalog.get(&prof.model) else {
            tracing::info!(place = %p.target, profile, model = %prof.model,
                "a place's spend limit has no figure to weigh: the catalog does not price its model");
            return None;
        };
        let call = price.reserve_micros(prof.effective_max_tokens(&self.catalog), 0);
        let limit_micros = theseus_kernel::usd_to_micros(limit);
        (limit_micros < call).then(|| PlaceWarning {
            place: p.target.clone(),
            name: p.name.clone(),
            kind: LIMIT_BELOW_CALL.into(),
            detail: format!(
                "{}'s {} limit is below one call's {} on {profile} (before its input)",
                p.name,
                dollars(limit_micros),
                dollars(call)
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use theseus_protocol::PlaceCeiling;

    use super::*;
    use crate::provider::FakeProvider;

    fn core(dir: &std::path::Path) -> Arc<Core> {
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.to_string_lossy().into_owned();
        let store = crate::store::Store::open(&dir.join("store")).unwrap();
        Core::build(crate::rpc::Parts::for_tests(
            cfg,
            Arc::new(FakeProvider::scripted(vec![])),
            store,
        ))
        .unwrap()
    }

    fn pier(limit: f64, tools: Option<&[&str]>) -> BoundPlace {
        BoundPlace {
            target: "discord:channel:223456789012345681".into(),
            name: "#pier".into(),
            private: false,
            guild: Some("100000000000000002".into()),
            ceiling: Some(PlaceCeiling {
                spend_limit_usd: Some(limit),
                tools: tools.map(|t| t.iter().map(|s| s.to_string()).collect()),
                ..Default::default()
            }),
        }
    }

    /// The template's sonnet reserves $1.28 of output a call: a $1 limit
    /// warns with both figures, a $2 one does not; an unknown family warns;
    /// each warning is health's and a `place.warned` row with its line.
    #[tokio::test]
    async fn a_limit_below_one_call_and_an_unknown_family_warn() {
        let d = tempfile::tempdir().unwrap();
        let c = core(d.path());
        let w = c.place_warnings(&[pier(1.0, Some(&["web", "nosuch"]))], vec![]);
        let details: Vec<&str> = w.iter().map(|w| w.detail.as_str()).collect();
        assert_eq!(w.len(), 2, "{details:?}");
        assert_eq!(w[0].kind, UNKNOWN_FAMILY);
        assert!(w[0].detail.contains("\"nosuch\""), "{details:?}");
        assert_eq!(w[1].kind, LIMIT_BELOW_CALL);
        assert_eq!(
            w[1].detail,
            "#pier's $1.00 limit is below one call's $1.28 on sonnet (before its input)"
        );
        let h = c.runner.place_rule.health(&c.cfg);
        assert_eq!(h.warnings, w);
        let rows: Vec<_> = c
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(1000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "place.warned")
            .collect();
        assert_eq!(rows.len(), 2);

        // $2 is above one call's reservation: no warning, and health's
        // warnings are this start's alone.
        assert!(c.place_warnings(&[pier(2.0, None)], vec![]).is_empty());
        assert!(c.runner.place_rule.health(&c.cfg).warnings.is_empty());
    }

    /// A model the catalog does not price has no figure: no warning.
    #[tokio::test]
    async fn an_unpriced_model_has_no_figure() {
        let d = tempfile::tempdir().unwrap();
        let text = format!(
            "{}\n[profiles.plain]\nprovider = \"anthropic\"\nmodel = \"unpriced-model-7\"\n",
            crate::Config::EXAMPLE_TOML
        );
        let (mut cfg, _) = crate::Config::parse(&text).unwrap();
        cfg.server.state_dir = d.path().to_string_lossy().into_owned();
        let store = crate::store::Store::open(&d.path().join("store")).unwrap();
        let c = Core::build(crate::rpc::Parts::for_tests(
            cfg,
            Arc::new(FakeProvider::scripted(vec![])),
            store,
        ))
        .unwrap();
        let mut p = pier(0.01, None);
        p.ceiling.as_mut().unwrap().profile = Some("plain".into());
        assert!(c.place_warnings(&[p], vec![]).is_empty());
    }
}
