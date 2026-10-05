//! The heat cache (M6 step 33, theseus-6fn.13; design §2.10): decoded nodes,
//! shared across turns and readers, so a turn decodes only what no reader
//! decoded before it. §6.1's arena in its first cut.
//!
//! - **By position.** A node is written once and never changes, so its WAL
//!   position names its bytes for good: the cache keeps `Arc<Node>` by
//!   position, and every reader of the store's nodes (a turn's transcript, a
//!   reader outside a turn, a recall's source) asks it before it decodes.
//! - **Bounded.** At most `[memory] node_cache_mb` (64; 0 turns it off) of
//!   the nodes' record bytes, the size a node is counted at. A node larger
//!   than the bound is never kept. Past the bound it evicts down to seven
//!   eighths of it, so a sweep is paid once per many inserts.
//! - **By heat.** Each entry's heat is its last touch (ms) and its count of
//!   touches. Eviction asks the science first: `decay_sweep`'s hints (the
//!   nodes idle past its threshold, coldest first) go first, the one place
//!   the science touches tiering; then the coldest by last touch, then by
//!   count, then by order of touch.
//! - **The store's read path**, not memory's: it serves with `[memory]`
//!   off too, and nothing on the start path fills it (it fills as reads
//!   happen).
//!
//! Its counts are health's `store.node_cache` and the metrics
//! `theseus.node_cache.*`: hits, misses, decodes, evictions, failed
//! rehydrations, and the bytes and entries held.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, PoisonError};

use theseus_memory::science::Heat;
use theseus_memory::{Baseline, MemoryScience};
use theseus_protocol::NodeCacheHealth;

use crate::node::Node;

/// The bound's default, in MB: `[memory] node_cache_mb`.
pub const DEFAULT_MB: u64 = 64;

/// What an entry costs beyond its record's bytes: the `Arc`, the map's slot.
const OVERHEAD: u64 = 128;

/// One kept node and its heat.
struct Slot {
    node: Arc<Node>,
    size: u64,
    /// The last touch, in ms since the epoch, and the touch's order.
    last_ms: u64,
    tick: u64,
    count: u64,
}

#[derive(Default)]
struct Slots {
    map: HashMap<u64, Slot>,
    bytes: u64,
    tick: u64,
}

/// The cache. One per store, shared by every handle (`Store::clone`,
/// `Store::for_turn`).
pub struct NodeCache {
    cap: AtomicU64,
    slots: Mutex<Slots>,
    science: Mutex<Arc<dyn MemoryScience>>,
    hits: AtomicU64,
    misses: AtomicU64,
    decodes: AtomicU64,
    evictions: AtomicU64,
    failed: AtomicU64,
}

impl Default for NodeCache {
    fn default() -> Self {
        Self::new(DEFAULT_MB * 1024 * 1024)
    }
}

impl NodeCache {
    /// A cache bounded at `cap` bytes (0: off).
    pub fn new(cap: u64) -> Self {
        Self {
            cap: AtomicU64::new(cap),
            slots: Mutex::default(),
            science: Mutex::new(Arc::new(Baseline::default())),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            decodes: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        }
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The bound, from `[memory] node_cache_mb` (0 turns it off and empties
    /// it). A smaller bound evicts at once.
    pub fn set_mb(&self, mb: u64) {
        self.cap.store(mb.saturating_mul(1024 * 1024), Relaxed);
        self.evict(now_ms());
    }

    /// The science whose `decay_sweep` orders eviction.
    pub fn set_science(&self, science: Arc<dyn MemoryScience>) {
        *self.science.lock().unwrap_or_else(PoisonError::into_inner) = science;
    }

    pub fn cap(&self) -> u64 {
        self.cap.load(Relaxed)
    }

    /// Whether the node at `position` is kept, without touching it.
    pub fn holds(&self, position: u64) -> bool {
        self.slots().map.contains_key(&position)
    }

    /// The node at `position` when it is kept, untouched: what a stub reads
    /// its fields from at a transcript's read.
    pub fn peek(&self, position: u64) -> Option<Arc<Node>> {
        self.slots().map.get(&position).map(|s| s.node.clone())
    }

    /// The node at `position`, touched, when it is kept.
    pub fn get(&self, position: u64) -> Option<Arc<Node>> {
        self.get_at(position, now_ms())
    }

    fn get_at(&self, position: u64, now: u64) -> Option<Arc<Node>> {
        let mut s = self.slots();
        s.tick += 1;
        let tick = s.tick;
        let slot = s.map.get_mut(&position)?;
        slot.last_ms = now;
        slot.tick = tick;
        slot.count += 1;
        self.hits.fetch_add(1, Relaxed);
        Some(slot.node.clone())
    }

    /// The node at `position`: kept, or decoded from `payload` (its record's
    /// bytes) and kept. Every decode of a node the store reads is here, so
    /// `decodes` counts them.
    pub fn node(&self, position: u64, payload: &[u8]) -> anyhow::Result<Arc<Node>> {
        if let Some(n) = self.get(position) {
            return Ok(n);
        }
        let n = Arc::new(self.decode(payload)?);
        self.keep(position, n.clone(), payload.len() as u64);
        Ok(n)
    }

    /// A decode the cache counts, of a node it was asked for and lacks.
    pub fn decode(&self, payload: &[u8]) -> anyhow::Result<Node> {
        self.misses.fetch_add(1, Relaxed);
        self.decodes.fetch_add(1, Relaxed);
        Ok(serde_json::from_slice(payload)?)
    }

    /// Keep `node`, `bytes` long on the record, at `position`.
    pub fn keep(&self, position: u64, node: Arc<Node>, bytes: u64) {
        self.keep_at(position, node, bytes, now_ms());
    }

    fn keep_at(&self, position: u64, node: Arc<Node>, bytes: u64, now: u64) {
        let size = bytes + OVERHEAD;
        if size > self.cap() {
            return;
        }
        {
            let mut s = self.slots();
            s.tick += 1;
            let tick = s.tick;
            let slot = Slot {
                node,
                size,
                last_ms: now,
                tick,
                count: 1,
            };
            if let Some(old) = s.map.insert(position, slot) {
                s.bytes -= old.size;
            }
            s.bytes += size;
            if s.bytes <= self.cap() {
                return;
            }
        }
        self.evict(now);
    }

    /// Past the bound: evict down to seven eighths of it, `decay_sweep`'s
    /// hints first, then the coldest.
    fn evict(&self, now: u64) {
        let cap = self.cap();
        let mut s = self.slots();
        if s.bytes <= cap {
            return;
        }
        let low = cap / 8 * 7;
        let view: Vec<Heat> = s
            .map
            .values()
            .map(|slot| Heat {
                node_id: slot.node.id.clone(),
                written_ms: slot.node.created_at_ms,
                last_read_ms: slot.last_ms,
            })
            .collect();
        let science = self
            .science
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let hints = science.decay_sweep(now, &view);
        let by_id: HashMap<&str, u64> = s
            .map
            .iter()
            .map(|(p, slot)| (slot.node.id.as_str(), *p))
            .collect();
        let mut order: Vec<u64> = hints
            .iter()
            .filter_map(|d| by_id.get(d.node_id.as_str()).copied())
            .collect();
        let hinted: std::collections::HashSet<u64> = order.iter().copied().collect();
        let mut rest: Vec<(u64, u64, u64, u64)> = s
            .map
            .iter()
            .filter(|(p, _)| !hinted.contains(p))
            .map(|(p, slot)| (slot.last_ms, slot.count, slot.tick, *p))
            .collect();
        rest.sort_unstable();
        order.extend(rest.into_iter().map(|(.., p)| p));
        drop(by_id);
        for p in order {
            if s.bytes <= low {
                break;
            }
            if let Some(slot) = s.map.remove(&p) {
                s.bytes -= slot.size;
                self.evictions.fetch_add(1, Relaxed);
            }
        }
    }

    /// A stub whose node could not be read back (§6): counted here, and
    /// logged where it failed.
    pub fn failed(&self) {
        self.failed.fetch_add(1, Relaxed);
    }

    /// Decodes so far: what a bench reads around a turn.
    pub fn decodes(&self) -> u64 {
        self.decodes.load(Relaxed)
    }

    /// Health's block.
    pub fn health(&self) -> NodeCacheHealth {
        let s = self.slots();
        NodeCacheHealth {
            cap_bytes: self.cap(),
            bytes: s.bytes,
            entries: s.map.len() as u64,
            hits: self.hits.load(Relaxed),
            misses: self.misses.load(Relaxed),
            decodes: self.decodes.load(Relaxed),
            evictions: self.evictions.load(Relaxed),
            failed: self.failed.load(Relaxed),
        }
    }
}

fn now_ms() -> u64 {
    theseus_protocol::now_unix_ms()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn node(i: u64, created_ms: u64) -> Arc<Node> {
        let mut n = Node::user("ses_tern", None, "web", &format!("note {i}"));
        n.id = format!("msg_{i:04}");
        n.created_at_ms = created_ms;
        Arc::new(n)
    }

    const DAY: u64 = 86_400_000;

    /// A node decoded once is served by position after, counted a hit, and
    /// a bound of 0 keeps nothing.
    #[test]
    fn a_node_decoded_once_is_served_after_and_zero_keeps_none() {
        let n = Node::user("ses_tern", None, "web", "the heron left at dawn");
        let bytes = serde_json::to_vec(&n).unwrap();
        let c = NodeCache::default();
        let a = c.node(7, &bytes).unwrap();
        let b = c.node(7, &bytes).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "one decode, shared");
        let h = c.health();
        assert_eq!((h.decodes, h.hits, h.misses, h.entries), (1, 1, 1, 1));
        let off = NodeCache::new(0);
        off.node(7, &bytes).unwrap();
        off.node(7, &bytes).unwrap();
        let h = off.health();
        assert_eq!((h.decodes, h.entries, h.bytes), (2, 0, 0));
        c.set_mb(0);
        assert_eq!(c.health().entries, 0, "turning it off empties it");
    }

    /// Hints first: a node `decay_sweep` names goes before a colder one it
    /// does not, and the hottest is kept.
    #[test]
    fn decay_sweeps_hints_go_first() {
        let now = 400 * DAY;
        let c = NodeCache::new(3 * (100 + OVERHEAD));
        // Written long ago and touched long ago: idle past the baseline's 30 days.
        c.keep_at(1, node(1, 0), 100, now - 40 * DAY);
        // Colder by last touch, but written today: no hint.
        c.keep_at(2, node(2, now - 50 * DAY), 100, now - 50 * DAY);
        c.slots().map.get_mut(&2).unwrap().last_ms = now - 50 * DAY;
        c.keep_at(3, node(3, now), 100, now);
        c.keep_at(4, node(4, now), 100, now);
        let kept: Vec<u64> = {
            let mut k: Vec<u64> = c.slots().map.keys().copied().collect();
            k.sort_unstable();
            k
        };
        assert!(!kept.contains(&1), "the hinted node went first: {kept:?}");
        assert!(
            kept.contains(&4) && kept.contains(&3),
            "the hottest stay: {kept:?}"
        );
        assert_eq!(c.health().evictions as usize, 4 - kept.len());
    }

    #[derive(Debug, Clone)]
    enum Op {
        Keep { pos: u64, bytes: u64 },
        Get { pos: u64 },
        Tick { ms: u64 },
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0u64..64, 1u64..3_000).prop_map(|(pos, bytes)| Op::Keep { pos, bytes }),
            (0u64..64).prop_map(|pos| Op::Get { pos }),
            (1u64..5 * DAY).prop_map(|ms| Op::Tick { ms }),
        ]
    }

    proptest! {
        /// Under any run of keeps, touches and time: never over the bound;
        /// and every eviction took a node no hotter than every node kept
        /// unless the science hinted it.
        #[test]
        fn eviction_by_heat_stays_under_the_bound(cap in 0u64..20_000, ops in prop::collection::vec(op(), 1..200)) {
            let c = NodeCache::new(cap);
            let mut now = 100 * DAY;
            for o in ops {
                match o {
                    Op::Tick { ms } => now += ms,
                    Op::Get { pos } => { c.get_at(pos, now); }
                    Op::Keep { pos, bytes } => {
                        let before: HashMap<u64, (u64, u64, u64, u64)> = c.slots().map.iter()
                            .map(|(p, s)| (*p, (s.last_ms, s.count, s.tick, s.node.created_at_ms))).collect();
                        c.keep_at(pos, node(pos, now), bytes, now);
                        let s = c.slots();
                        prop_assert!(s.bytes <= cap, "{} bytes over a bound of {cap}", s.bytes);
                        prop_assert_eq!(s.bytes, s.map.values().map(|v| v.size).sum::<u64>());
                        let gone: Vec<u64> = before.keys().filter(|p| !s.map.contains_key(p) && **p != pos).copied().collect();
                        for g in gone {
                            let (gl, gc, gt, gw) = before[&g];
                            let hinted = now.saturating_sub(gl.max(gw)) >= 30 * DAY;
                            if hinted { continue; }
                            for (p, k) in s.map.iter().filter(|(p, _)| **p != pos) {
                                let kh = now.saturating_sub(k.last_ms.max(k.node.created_at_ms)) >= 30 * DAY;
                                prop_assert!(!kh, "kept hinted {} while evicting {} unhinted", p, g);
                                prop_assert!((gl, gc, gt) <= (k.last_ms, k.count, k.tick),
                                    "evicted {} ({}, {}) while keeping colder {} ({}, {})", g, gl, gc, p, k.last_ms, k.count);
                            }
                        }
                    }
                }
            }
        }
    }
}
