//! The pick on the turn's path (theseus-rr15): only at a compile point. The
//! compile step asks `prefix_spec` at each loop; it answers the spec with the
//! library's plan: at a compile point (a new session, `/new`, a thread's
//! birth, a cold resume, a forced recompile, or a system that changed under
//! the session, which recompiles anyway) a fresh pick, and otherwise the
//! replay of the plan the current compilation used, from memory. A warm
//! session never picks, and its appends read nothing from the store.

use std::borrow::Cow;
use std::sync::Arc;

use super::fit::{self, Bands, Prices};
use super::{applied, Library, Picked, Plan, Query};
use crate::compiler::{layers::system_digest, Compilation, RequestSpec};
use crate::config::CacheTtl;
use crate::node::Body;
use crate::places::PlaceClass;
use crate::stub::{Kind, Stub};
use crate::turn::TurnRunner;

/// A cache entry's life on the provider at `ttl`.
pub fn ttl_ms(ttl: CacheTtl) -> u64 {
    match ttl {
        CacheTtl::FiveMinutes => 5 * 60_000,
        CacheTtl::OneHour => 60 * 60_000,
    }
}

/// `spec` carrying `plan`.
fn with_plan(spec: &RequestSpec, plan: Plan) -> RequestSpec {
    let mut s = spec.clone();
    s.prefix = Some(Arc::new(plan));
    s
}

/// What the compile step tells `prefix_spec` of its loop.
pub struct At<'a> {
    pub current: Option<&'a Compilation>,
    pub nodes: &'a [(u64, Stub)],
    /// A compile point the step knows of: no compilation yet, a forced
    /// recompile, a cold resume.
    pub point: bool,
    /// The turn's first loop: the only one that may pick.
    pub first: bool,
    pub class: PlaceClass,
}

impl TurnRunner {
    /// The spec this loop compiles, with the library's plan for its third
    /// layer, and the pick when this loop made one. None, None: the library
    /// is off, or has nothing to say (the spec stands as it is).
    pub(crate) fn prefix_spec(
        &self,
        spec: &RequestSpec,
        at: At<'_>,
    ) -> (Option<RequestSpec>, Option<Arc<Picked>>) {
        let cfg = &self.cfg.prefix;
        if !cfg.enabled {
            return (None, None);
        }
        let replay = at
            .current
            .and_then(|c| c.manifest.prefix.as_ref())
            .and_then(|u| self.prefix.replay(u));
        let point = at.first && (at.point || changes(spec, at.current, replay.as_ref()));
        if !point {
            return (replay.map(|p| with_plan(spec, p)), None);
        }
        if let Err(e) = self.prefix.loaded(&self.store) {
            tracing::warn!(error = %format!("{e:#}"),
                "the prefix library cannot be read: this compile renders its own layers");
            return (replay.map(|p| with_plan(spec, p)), None);
        }
        let guidance = spec
            .walk
            .as_deref()
            .map(|w| w.compose(&w.current).sections)
            .unwrap_or_default();
        let sections = fit::sections_of(&spec.context_text, &guidance);
        let Some(picked) = self.pick_for(&self.prefix, spec, &sections, &at) else {
            return (replay.map(|p| with_plan(spec, p)), None);
        };
        let picked = Arc::new(picked);
        (Some(with_plan(spec, picked.plan.clone())), Some(picked))
    }

    /// `spec` with the plan `current` used, from memory: what an append
    /// renders (the keep-warm read's compile). None: no plan to replay.
    pub(crate) fn prefix_replayed(
        &self,
        spec: &RequestSpec,
        current: Option<&Compilation>,
    ) -> Option<RequestSpec> {
        if !self.cfg.prefix.enabled {
            return None;
        }
        let used = current?.manifest.prefix.as_ref()?;
        self.prefix.replay(used).map(|p| with_plan(spec, p))
    }

    fn pick_for(
        &self,
        library: &Library,
        spec: &RequestSpec,
        sections: &[(fit::Section, &str)],
        at: &At<'_>,
    ) -> Option<Picked> {
        let cfg = &self.cfg.prefix;
        let entry = self.catalog.get(&spec.model)?;
        if !entry.caches {
            return None;
        }
        let prices = Prices {
            write: match spec.cache_ttl {
                CacheTtl::FiveMinutes => entry.cache_write_per_mtok,
                CacheTtl::OneHour => entry.cache_write_1h_per_mtok,
            },
            read: entry.cache_read_per_mtok,
        };
        let lower = lower_of(spec);
        // What the new work is about: the newest message, the thread's card
        // and the layer's own section names.
        let message = at
            .nodes
            .iter()
            .rev()
            .find(|(_, n)| n.kind == Kind::UserMessage)
            .and_then(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let card = spec
            .card
            .as_ref()
            .map(|c| c.now.clone())
            .unwrap_or_default();
        let names: Vec<&str> = sections.iter().map(|(s, _)| s.name.as_str()).collect();
        let q = Query {
            sections,
            lower,
            class: class_name(at.class),
            model: &spec.model,
            sketch: super::sketch::of(
                [message.as_str(), card.as_str()]
                    .into_iter()
                    .chain(names.iter().copied()),
            ),
            prices,
            bands: Bands {
                act: cfg.act,
                confirm: cfg.confirm,
            },
            candidates: cfg.candidates as usize,
            max_bytes: cfg.max_bytes,
            now_ms: theseus_protocol::now_unix_ms(),
        };
        library.pick(&q)
    }

    /// The tokens of a request on `target` for `session_id` that the cache
    /// holds warm through the library (theseus-rr15): the tools and the system
    /// through a warm header that fits the session's third layer there, under
    /// that target's own lower layers. 0 with none, or with the library off.
    /// Routing's cold estimate takes them off the request's (`route_step`);
    /// asked only for a switch the estimate holds back, never on a warm turn.
    pub(crate) fn warm_prefix_tokens(
        &self,
        target: &crate::turn::Target,
        kind: theseus_protocol::SessionKind,
        session_id: &str,
        place: crate::ceiling::PlaceView,
    ) -> u64 {
        if !self.cfg.prefix.enabled {
            return 0;
        }
        let (mut spec, _) = self.request_spec(target, kind, place);
        spec.walk = self.walk(session_id, place.class);
        if let Some(b) = self.standing_block(
            session_id,
            place.class,
            self.restricted_profile(&target.profile),
        ) {
            b.apply(&mut spec);
        }
        self.thread_spec(&mut spec, session_id, None);
        let guidance = spec
            .walk
            .as_deref()
            .map(|w| w.compose(&w.current).sections)
            .unwrap_or_default();
        let sections: Vec<fit::Section> = fit::sections_of(&spec.context_text, &guidance)
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        let warm = self.prefix.warm_fit(
            &sections,
            (&lower_of(&spec), class_name(place.class)),
            &spec.model,
            self.cfg.prefix.confirm,
            theseus_protocol::now_unix_ms(),
        );
        let Some(header) = warm else { return 0 };
        let tools = spec.tools.json_len();
        let text = (spec.system_text.len() + spec.persona_text.len()) as u64 + header;
        let rates = self.catalog.get(&spec.model).map_or_else(
            || crate::catalog::TokenRates::of(&spec.model),
            |e| e.bytes_per_token,
        );
        (tools as f64 / rates.json.max(1.0) + text as f64 / rates.text.max(1.0)) as u64
    }

    /// The loop's request carried the header its compilation names: its cache
    /// entry on the model is read or written now.
    pub(crate) fn prefix_touched(&self, compiled: &crate::compiler::Compiled, spec: &RequestSpec) {
        if let Some(u) = &compiled.compilation.manifest.prefix {
            let now = theseus_protocol::now_unix_ms();
            self.prefix
                .touch(&u.header, &spec.model, ttl_ms(spec.cache_ttl), now);
        }
    }
}

/// The digest of what the cache reads before the third layer: the tools,
/// the header and the persona layer.
fn lower_of(spec: &RequestSpec) -> String {
    spec.tools
        .digest_with(&[&spec.system_text, &spec.persona_text])
}

/// The class's name in a header.
pub fn class_name(class: PlaceClass) -> &'static str {
    match class {
        PlaceClass::Shared => "shared",
        _ => "private",
    }
}

/// Whether the append would change the system under `current`, so the
/// compile recompiles anyway (`system_changed`): then it is a compile point.
fn changes(spec: &RequestSpec, current: Option<&Compilation>, replay: Option<&Plan>) -> bool {
    let Some(c) = current else { return true };
    let appended = match spec.walk.as_deref() {
        Some(w) => crate::ontology::guided(
            spec,
            w.compose(&crate::ontology::recorded(&c.manifest.memberships)),
        ),
        None => Cow::Borrowed(spec),
    };
    let digest = match replay {
        Some(p) => system_digest(&applied(&with_plan(&appended, p.clone()))),
        None => system_digest(&appended),
    };
    digest != c.manifest.system_digest
}
