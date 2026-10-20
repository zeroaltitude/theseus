//! The prefix library (theseus-rr15, T13; the owner's idea of 2026-10-09):
//! canonical layered headers kept by digest, picked at compile points and
//! reused byte for byte across threads and sessions, so the provider's prompt
//! cache (an exact byte prefix per model, alive five minutes or an hour after
//! its last read) serves the same project to every session that works it.
//!
//! - **What is kept.** Each project layer (layer 3 of `compiler::layers`: the
//!   system-level context files, then the topics' guidance) a session minted,
//!   as a [`Header`]: its sections, its text, its sketch (`sketch`), its place
//!   class and the digest of the layers before it (`lower`: the tools, the
//!   header and the persona layer, which the cache reads first). A META record
//!   `prefix:<digest>` (store format 41), written in the frame of the
//!   compilation that minted it; never edited. Its hit log is the META record
//!   `prefix.hits.<digest>` (reuses, last read), written in the frame of each
//!   compilation that reuses it; a retired one is `prefix.retired.<digest>`.
//!   Loaded once into memory (`Library::loaded`) by the first compile point
//!   after a start, and held under `[prefix] max_bytes` (10 MiB): past it the
//!   header read least recently retires.
//! - **The pick** (`fit`), only at a compile point (`turn`): reuse the best
//!   header verbatim and put what it lacks after the cached prefix, in the
//!   session's block (layer 4); or the coarser one with a bigger delta; or
//!   mint the fresh layer as a new header. The compilation's manifest names the
//!   header it used and the fresh layer it stood for (`PrefixUse`), so every
//!   append renders the same bytes (`applied`) with nothing read.
//! - **Warmth**, per model, in memory: a request that carries a header reads
//!   or writes its cache entry, so each compile touches it (`touch`), and it
//!   is warm until its TTL passes. A start forgets it, so a header reads cold
//!   until its next use: the estimate errs to the write price.
//! - **The guard** (`report`): every pick is a `prefix.picked` row with its
//!   fit and choice; `theseus prefix report` joins it with the turn's cache
//!   read and with what went wrong after it (a correction, a re-ask), and a
//!   reuse below `act` that went wrong is a labelled false fit.
//!
//! A source change (a context file, a guidance section, a persona file, a
//! standing record) changes a section's bytes: the stale header no longer
//! fits, the next compile point mints the new one under its own digest, and
//! the stale one retires once it is cold (`retire_stale`).

pub mod fit;
pub mod report;
pub mod sketch;
pub mod turn;

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::compiler::{sha, Compiled, RequestSpec};
use crate::store::Store;
use fit::{Bands, Best, Choice, Odds, Prices, Section, Sums};

/// The META key prefixes of a header, its hit log, and its retirement.
pub const HEADER: &str = "prefix:";
pub const HITS: &str = "prefix.hits.";
pub const RETIRED: &str = "prefix.retired.";

/// The layer the library picks.
pub const PROJECT: &str = crate::compiler::layers::PROJECT;

/// A header, as stored (META `prefix:<digest>`, store format 41).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub digest: String,
    /// `project` (the third layer).
    pub layer: String,
    /// `private` or `shared`: a shared place reuses only a shared place's.
    pub class: String,
    /// The digest of what the cache reads before it: the tools, the header
    /// and the persona layer.
    pub lower: String,
    pub sections: Vec<Section>,
    pub text: String,
    pub sketch: Vec<i8>,
    pub created_at_ms: u64,
}

/// A header's hit log (META `prefix.hits.<digest>`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hits {
    pub reuses: u64,
    pub last_read_ms: u64,
}

/// Why a header retired (META `prefix.retired.<digest>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Retired {
    pub at_ms: u64,
    /// `evicted` (past `max_bytes`) or `source_changed`.
    pub why: String,
}

/// What a compilation's manifest records of the library (store format 41):
/// the header its third layer is, and the digest of the fresh layer it
/// stands for. An append renders the same bytes while the fresh layer is
/// unchanged; when it changes, the system does, and the recompile picks again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixUse {
    pub header: String,
    pub fresh: String,
}

/// What a spec carries for the compile: the use, the header's text, and the
/// delta that goes after the cached prefix.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub used: PrefixUse,
    pub header: Arc<str>,
    pub delta: Arc<str>,
}

/// A pick at a compile point, for its row and its compilation's frame.
#[derive(Debug, Clone)]
pub struct Picked {
    pub plan: Plan,
    pub choice: Choice,
    pub fit: Option<f64>,
    pub below_act: bool,
    pub candidates: usize,
    pub class: String,
    pub model: String,
    pub odds: Odds,
    pub sums: Sums,
    pub prices: Prices,
    pub header_bytes: u64,
    pub delta_bytes: u64,
    /// The header's record (a mint), its hit log (a reuse), and retirements.
    pub records: Vec<(String, serde_json::Value)>,
}

/// `spec` with its plan applied: the third layer is the header's text, and
/// the delta heads the session's block. Borrowed when there is no plan, or
/// when the spec's third layer is not the fresh layer the plan stands for.
pub fn applied(spec: &RequestSpec) -> Cow<'_, RequestSpec> {
    let Some(p) = spec.prefix.as_deref() else {
        return Cow::Borrowed(spec);
    };
    if sha(&spec.context_text) != p.used.fresh {
        return Cow::Borrowed(spec);
    }
    let mut s = spec.clone();
    s.context_text = p.header.to_string();
    if !p.delta.is_empty() {
        s.session_text = fit::join(
            [&*p.delta, s.session_text.as_str()]
                .into_iter()
                .filter(|t| !t.is_empty()),
        );
    }
    s.prefix_used = Some(p.used.clone());
    s.prefix = None;
    Cow::Owned(s)
}

/// The records a new compilation's frame carries for its pick: the header,
/// the hit log, the retirements and the `prefix.picked` row.
pub fn records(compiled: &Compiled, turn_id: &str) -> Result<Vec<NewRecord>> {
    let Some(p) = compiled
        .prefix
        .as_deref()
        .filter(|_| compiled.new_compilation)
    else {
        return Ok(vec![]);
    };
    let mut out = Vec::with_capacity(p.records.len() + 1);
    for (key, value) in &p.records {
        out.push(NewRecord::json(kinds::META, Some(key), value)?);
    }
    let sid = &compiled.compilation.session_id;
    out.push(crate::fact::row(
        &crate::fact::prefix::PrefixPicked { picked: p },
        Some(sid),
        Some(turn_id),
    )?);
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
struct Warm {
    at_ms: u64,
    ttl_ms: u64,
}

#[derive(Debug, Clone)]
struct Entry {
    header: Arc<Header>,
    hits: Hits,
    /// By model.
    warm: HashMap<String, Warm>,
}

impl Entry {
    fn bytes(&self) -> u64 {
        let h = &self.header;
        (h.text.len() + h.sketch.len() + h.sections.len() * 96 + 256) as u64
    }

    fn warm_on(&self, model: &str, now_ms: u64) -> bool {
        self.warm
            .get(model)
            .is_some_and(|w| now_ms < w.at_ms + w.ttl_ms)
    }

    fn warm_anywhere(&self, now_ms: u64) -> bool {
        self.warm.values().any(|w| now_ms < w.at_ms + w.ttl_ms)
    }

    fn last_read(&self) -> u64 {
        let warm = self.warm.values().map(|w| w.at_ms).max().unwrap_or(0);
        warm.max(self.hits.last_read_ms)
            .max(self.header.created_at_ms)
    }
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<String, Entry>,
    bytes: u64,
    /// The delta of each (header, fresh) pair a pick made in this run.
    deltas: HashMap<(String, String), Arc<str>>,
}

/// The library in memory: none until its first reader loads it.
#[derive(Default)]
pub struct Library(Mutex<Option<State>>);

/// What a pick is asked: the fresh layer, and where it would be read.
pub struct Query<'a> {
    pub sections: &'a [(Section, &'a str)],
    pub lower: String,
    pub class: &'a str,
    pub model: &'a str,
    pub sketch: Vec<i8>,
    pub prices: Prices,
    pub bands: Bands,
    pub candidates: usize,
    pub max_bytes: u64,
    pub now_ms: u64,
}

/// What `stats` reports of the library.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub headers: u64,
    pub bytes: u64,
    pub warm: u64,
}

/// Tokens of a text, as the compiler's estimate counts them.
pub fn tokens(bytes: u64) -> u64 {
    bytes.div_ceil(4)
}

impl Library {
    /// Load the records once: every header not retired, with its hit log.
    pub fn loaded(&self, store: &Store) -> Result<()> {
        if self.0.lock().unwrap().is_some() {
            return Ok(());
        }
        let state = theseus_store::blocking(|| read(store))?;
        let mut g = self.0.lock().unwrap();
        if g.is_none() {
            *g = Some(state);
        }
        Ok(())
    }

    /// The plan an append renders for `used`: the header's text and the delta
    /// this run's pick made. None when either is not held (a start forgets the
    /// deltas: the compile point after it picks again, the same bytes).
    pub fn replay(&self, used: &PrefixUse) -> Option<Plan> {
        let g = self.0.lock().unwrap();
        let s = g.as_ref()?;
        let e = s.entries.get(&used.header)?;
        let delta = if used.header == used.fresh {
            Arc::from("")
        } else {
            s.deltas
                .get(&(used.header.clone(), used.fresh.clone()))?
                .clone()
        };
        Some(Plan {
            used: used.clone(),
            header: Arc::from(e.header.text.as_str()),
            delta,
        })
    }

    /// A request on `model` carried the header `digest`: its cache entry was
    /// read or written now, alive `ttl_ms` from here.
    pub fn touch(&self, digest: &str, model: &str, ttl_ms: u64, now_ms: u64) {
        let mut g = self.0.lock().unwrap();
        if let Some(e) = g.as_mut().and_then(|s| s.entries.get_mut(digest)) {
            e.warm.insert(
                model.to_string(),
                Warm {
                    at_ms: now_ms,
                    ttl_ms,
                },
            );
        }
    }

    /// The biggest header on `lower` and `class` that fits `fresh` at
    /// `confirm` or above and is warm on `model` now: its bytes, which a
    /// request there reads from the cache. None while the library is not
    /// loaded, or with none.
    pub fn warm_fit(
        &self,
        fresh: &[Section],
        (lower, class): (&str, &str),
        model: &str,
        confirm: f64,
        now_ms: u64,
    ) -> Option<u64> {
        let g = self.0.lock().unwrap();
        g.as_ref()?
            .entries
            .values()
            .filter(|e| {
                let h = &e.header;
                h.layer == PROJECT && h.class == class && h.lower == lower
            })
            .filter(|e| e.warm_on(model, now_ms))
            .filter(|e| fit::fit(fresh, &e.header.sections).is_some_and(|f| f >= confirm))
            .map(|e| e.header.text.len() as u64)
            .max()
    }

    pub fn stats(&self, now_ms: u64) -> Stats {
        let g = self.0.lock().unwrap();
        let Some(s) = g.as_ref() else {
            return Stats::default();
        };
        Stats {
            headers: s.entries.len() as u64,
            bytes: s.bytes,
            warm: s
                .entries
                .values()
                .filter(|e| e.warm_anywhere(now_ms))
                .count() as u64,
        }
    }

    /// Undo what a pick did in memory when its compilation is not the one the
    /// call uses (routing moved the turn, `route_step`): a mint's header is
    /// forgotten, so the compile the call uses mints it, with its record and
    /// its `prefix.picked` row, instead of reusing a header that was never
    /// stored; a reuse gives back its hit (theseus-i4y7l).
    pub fn discard(&self, p: &Picked) {
        let mut g = self.0.lock().unwrap();
        let Some(s) = g.as_mut() else { return };
        let digest = &p.plan.used.header;
        let minted = p
            .records
            .iter()
            .any(|(k, _)| k.strip_prefix(HEADER) == Some(digest.as_str()));
        if minted {
            if let Some(e) = s.entries.remove(digest) {
                s.bytes -= e.bytes();
            }
        } else if let Some(e) = s.entries.get_mut(digest) {
            e.hits.reuses = e.hits.reuses.saturating_sub(1);
        }
        s.deltas.retain(|(h, _), _| h != digest);
    }

    /// Pick the third layer for `q` (`fit`'s rule). None while the library is
    /// not loaded, or for an empty layer.
    pub fn pick(&self, q: &Query<'_>) -> Option<Picked> {
        let fresh: Vec<Section> = q.sections.iter().map(|(s, _)| s.clone()).collect();
        if fresh.is_empty() {
            return None;
        }
        let fresh_text = fit::join(q.sections.iter().map(|(_, t)| *t));
        let fresh_digest = sha(&fresh_text);
        let fresh_bytes = fresh_text.len() as u64;
        let mut g = self.0.lock().unwrap();
        let s = g.as_mut()?;
        let (candidates, best, odds) = s.nearest(q, &fresh);
        let judged = best.as_ref().map(|(f, h, warm)| Best {
            fit: *f,
            warm: *warm,
            tokens: tokens(h.text.len() as u64),
        });
        let (choice, sums) = fit::decide(judged, tokens(fresh_bytes), q.bands, odds, q.prices);
        let mut records = Vec::new();
        let (header, fit) = match (&best, choice) {
            (Some((f, h, _)), Choice::Reuse | Choice::Coarser) => {
                let e = s.entries.get_mut(&h.digest)?;
                e.hits.reuses += 1;
                e.hits.last_read_ms = q.now_ms;
                records.push((
                    format!("{HITS}{}", h.digest),
                    serde_json::to_value(&e.hits).ok()?,
                ));
                (h.clone(), Some(*f))
            }
            _ => {
                let h = Arc::new(Header {
                    digest: fresh_digest.clone(),
                    layer: PROJECT.into(),
                    class: q.class.into(),
                    lower: q.lower.clone(),
                    sections: fresh.clone(),
                    text: fresh_text,
                    sketch: sketch::of(q.sections.iter().map(|(_, t)| *t)),
                    created_at_ms: q.now_ms,
                });
                records.push((
                    format!("{HEADER}{}", h.digest),
                    serde_json::to_value(&*h).ok()?,
                ));
                records.extend(s.insert(h.clone(), &fresh, q));
                (h, best.as_ref().map(|b| b.0))
            }
        };
        let missing = fit::missing(&fresh, &header.sections);
        let delta: String = fit::join(
            q.sections
                .iter()
                .filter(|(sec, _)| missing.iter().any(|m| m.digest == sec.digest))
                .map(|(_, t)| *t),
        );
        let used = PrefixUse {
            header: header.digest.clone(),
            fresh: fresh_digest,
        };
        let delta: Arc<str> = Arc::from(delta);
        if used.header != used.fresh {
            if s.deltas.len() > 4096 {
                s.deltas.clear();
            }
            s.deltas
                .insert((used.header.clone(), used.fresh.clone()), delta.clone());
        }
        Some(Picked {
            choice,
            below_act: choice != Choice::Mint && fit.is_some_and(|f| f < q.bands.act),
            fit,
            candidates,
            class: q.class.into(),
            model: q.model.into(),
            odds,
            sums,
            prices: q.prices,
            header_bytes: header.text.len() as u64,
            delta_bytes: delta.len() as u64,
            plan: Plan {
                used,
                header: Arc::from(header.text.as_str()),
                delta,
            },
            records,
        })
    }
}

/// The best fitting header among the nearest: its fit, the header, and
/// whether it is warm on the query's model.
type Fitted = Option<(f64, Arc<Header>, bool)>;

impl State {
    /// The headers on the query's lower layers and place class, the nearest
    /// `candidates` by sketch: how many, the best fitting one, and the odds
    /// their hit log gives a mint.
    fn nearest(&self, q: &Query<'_>, fresh: &[Section]) -> (usize, Fitted, Odds) {
        let mut near: Vec<&Entry> = self
            .entries
            .values()
            .filter(|e| {
                let h = &e.header;
                h.layer == PROJECT && h.class == q.class && h.lower == q.lower
            })
            .collect();
        near.sort_by(|a, b| {
            let (ca, cb) = (
                sketch::cosine(&a.header.sketch, &q.sketch),
                sketch::cosine(&b.header.sketch, &q.sketch),
            );
            cb.total_cmp(&ca)
                .then(a.header.digest.cmp(&b.header.digest))
        });
        near.truncate(q.candidates);
        let warm = |e: &Entry| e.warm_on(q.model, q.now_ms);
        let best = near
            .iter()
            .filter_map(|e| fit::fit(fresh, &e.header.sections).map(|f| (f, *e)))
            .max_by(|(fa, a), (fb, b)| {
                fa.total_cmp(fb)
                    .then(warm(a).cmp(&warm(b)))
                    .then(a.header.text.len().cmp(&b.header.text.len()))
                    .then(b.header.digest.cmp(&a.header.digest))
            })
            .map(|(f, e)| (f, e.header.clone(), warm(e)));
        let reused = near.iter().filter(|e| e.hits.reuses > 0).count() as u64;
        let reads: u64 = near.iter().map(|e| e.hits.reuses).sum();
        (near.len(), best, Odds::of(near.len() as u64, reused, reads))
    }

    /// Hold `h`, then retire what it makes stale and what the cap pushes out:
    /// the records of each retirement.
    fn insert(
        &mut self,
        h: Arc<Header>,
        fresh: &[Section],
        q: &Query<'_>,
    ) -> Vec<(String, serde_json::Value)> {
        let e = Entry {
            header: h.clone(),
            hits: Hits {
                reuses: 0,
                last_read_ms: q.now_ms,
            },
            warm: HashMap::new(),
        };
        self.bytes += e.bytes();
        if let Some(old) = self.entries.insert(h.digest.clone(), e) {
            self.bytes -= old.bytes();
        }
        let mut out = Vec::new();
        // A header one of whose sections changed since: retired once cold.
        let stale: Vec<String> = self
            .entries
            .values()
            .filter(|e| {
                let o = &e.header;
                o.digest != h.digest
                    && o.class == h.class
                    && o.lower == h.lower
                    && !e.warm_anywhere(q.now_ms)
                    && o.sections.iter().any(|s| {
                        fresh
                            .iter()
                            .any(|f| f.name == s.name && f.digest != s.digest)
                    })
            })
            .map(|e| e.header.digest.clone())
            .collect();
        for d in stale {
            out.extend(self.retire(&d, "source_changed", q.now_ms));
        }
        // Past the cap, the header read least recently first; never `h`.
        while self.bytes > q.max_bytes {
            let Some(d) = self
                .entries
                .values()
                .filter(|e| e.header.digest != h.digest)
                .min_by_key(|e| (e.last_read(), e.header.digest.clone()))
                .map(|e| e.header.digest.clone())
            else {
                break;
            };
            out.extend(self.retire(&d, "evicted", q.now_ms));
        }
        out
    }

    fn retire(
        &mut self,
        digest: &str,
        why: &str,
        now_ms: u64,
    ) -> Option<(String, serde_json::Value)> {
        let e = self.entries.remove(digest)?;
        self.bytes -= e.bytes();
        self.deltas.retain(|(h, _), _| h != digest);
        let r = Retired {
            at_ms: now_ms,
            why: why.into(),
        };
        Some((format!("{RETIRED}{digest}"), serde_json::to_value(r).ok()?))
    }
}

/// The library as the store holds it.
fn read(store: &Store) -> Result<State> {
    let inner = store.inner();
    let mut retired = std::collections::HashSet::new();
    for r in inner.latest_with_prefix(kinds::META, RETIRED)? {
        if let Some(d) = r.key.as_deref().and_then(|k| k.strip_prefix(RETIRED)) {
            retired.insert(d.to_string());
        }
    }
    let mut hits = HashMap::new();
    for r in inner.latest_with_prefix(kinds::META, HITS)? {
        let Some(d) = r.key.as_deref().and_then(|k| k.strip_prefix(HITS)) else {
            continue;
        };
        if let Ok(h) = r.decode::<Hits>() {
            hits.insert(d.to_string(), h);
        }
    }
    let mut state = State::default();
    for r in inner.latest_with_prefix(kinds::META, HEADER)? {
        match r.decode::<Header>() {
            Ok(h) if !retired.contains(&h.digest) => {
                let e = Entry {
                    hits: hits.remove(&h.digest).unwrap_or_default(),
                    header: Arc::new(h),
                    warm: HashMap::new(),
                };
                state.bytes += e.bytes();
                state.entries.insert(e.header.digest.clone(), e);
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(key = ?r.key, error = %e,
                "a prefix header does not read: left out"),
        }
    }
    Ok(state)
}
