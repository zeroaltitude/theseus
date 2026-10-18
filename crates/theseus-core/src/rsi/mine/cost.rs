//! Cost per occurrence, and the saving (§2.4). For an occurrence of k calls
//! emitted by m model loops:
//!
//! - **Tokens**: each loop's usage is kept as its four parts (uncached input,
//!   cache reads, cache writes, output) and its dollars, never collapsed into
//!   one number. A loop's cost is shared among the calls it emitted, so a
//!   window holding two of a loop's four calls is charged half of it.
//! - **Seconds**: the loops' model time plus the calls' own duration.
//! - **Decisions**: m, the model's choice points. A tool replaces m loops with
//!   one, plus one for each hand-back taken.
//! - **The saving** per occurrence is the cost of the loops removed: m − 1 of
//!   m of what the window was charged. Tool time stays, because the flow runs
//!   the same commands. The projection is that times the occurrences a day in
//!   the scope.
//!
//! The score (§2.6) puts a motif beside the backlog's fixes on one scale:
//!
//! ```text
//! 1000 × ln(1 + occurrences) × (1 + usd_saved/2 + secs_saved/600 + decisions/2)
//!      × (0.5 + recency) ÷ work
//! ```

use serde::{Deserialize, Serialize};

use super::{Motif, Run};

const DAY_MS: f64 = 86_400_000.0;

/// A cost in tokens, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}

impl Tokens {
    pub fn total(&self) -> u64 {
        (self.input + self.cache_read + self.cache_write + self.output).round() as u64
    }
    fn add(&mut self, t: &[u64; 4], share: f64) {
        self.input += t[0] as f64 * share;
        self.cache_read += t[1] as f64 * share;
        self.cache_write += t[2] as f64 * share;
        self.output += t[3] as f64 * share;
    }
    fn scale(&mut self, k: f64) {
        self.input *= k;
        self.cache_read *= k;
        self.cache_write *= k;
        self.output *= k;
    }
    fn plus(&mut self, o: &Tokens) {
        self.input += o.input;
        self.cache_read += o.cache_read;
        self.cache_write += o.cache_write;
        self.output += o.output;
    }
}

/// What one occurrence cost, on average.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub tokens: Tokens,
    pub usd: f64,
    pub model_secs: f64,
    pub tool_secs: f64,
    /// Model loops: the choice points.
    pub decisions: f64,
}

/// What a tool would save, per occurrence and projected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Saving {
    pub tokens: Tokens,
    pub usd: f64,
    pub model_secs: f64,
    /// Loops removed: m − 1.
    pub decisions: f64,
    /// Occurrences a day in the scope over the window.
    pub per_day: f64,
    pub usd_per_day: f64,
    pub secs_per_day: f64,
}

/// One window of a run, costed.
pub fn occurrence(run: &Run, calls: std::ops::Range<usize>) -> (Cost, Saving) {
    let mut c = Cost::default();
    let mut seen = std::collections::BTreeSet::new();
    for call in &run.calls[calls] {
        c.tool_secs += f64::from(call.ms) / 1000.0;
        let Some(l) = run.loops.get(call.loop_ix as usize) else {
            continue;
        };
        seen.insert(call.loop_ix);
        let share = 1.0 / f64::from(l.calls.max(1));
        c.tokens.add(&l.tokens, share);
        c.usd += l.usd_micros as f64 / 1e6 * share;
        c.model_secs += l.model_ms as f64 / 1000.0 * share;
    }
    let m = seen.len() as f64;
    c.decisions = m;
    let mut s = Saving::default();
    if m >= 2.0 {
        let k = (m - 1.0) / m;
        s.tokens = c.tokens;
        s.tokens.scale(k);
        s.usd = c.usd * k;
        s.model_secs = c.model_secs * k;
        s.decisions = m - 1.0;
    }
    (c, s)
}

/// The mean of costs and savings, over `n` occurrences, projected over
/// `window_days`.
pub fn mean(parts: &[(Cost, Saving)], window_days: u32) -> (Cost, Saving) {
    let n = parts.len().max(1) as f64;
    let (mut c, mut s) = (Cost::default(), Saving::default());
    for (pc, ps) in parts {
        c.tokens.plus(&pc.tokens);
        c.usd += pc.usd;
        c.model_secs += pc.model_secs;
        c.tool_secs += pc.tool_secs;
        c.decisions += pc.decisions;
        s.tokens.plus(&ps.tokens);
        s.usd += ps.usd;
        s.model_secs += ps.model_secs;
        s.decisions += ps.decisions;
    }
    c.tokens.scale(1.0 / n);
    c.usd /= n;
    c.model_secs /= n;
    c.tool_secs /= n;
    c.decisions /= n;
    s.tokens.scale(1.0 / n);
    s.usd /= n;
    s.model_secs /= n;
    s.decisions /= n;
    s.per_day = parts.len() as f64 / f64::from(window_days.max(1));
    s.usd_per_day = s.usd * s.per_day;
    s.secs_per_day = s.model_secs * s.per_day;
    (c, s)
}

/// The score's work term: a flow is 1, an MCP extension 2, a Rust built-in 3.
/// The miner writes flow skeletons, the cheapest kind that works; the mint step
/// chooses the kind.
pub const WORK_FLOW: u8 = 1;

impl Motif {
    /// The motif's score in the backlog's milli-points (§2.6).
    pub fn score(&self, now_ms: u64, window_days: u32) -> u64 {
        score(
            self.support.occurrences,
            &self.saving,
            self.last_ms,
            now_ms,
            window_days,
            WORK_FLOW,
        )
    }
}

/// The §2.6 score, from its terms.
pub fn score(
    occurrences: u32,
    saving: &Saving,
    last_ms: u64,
    now_ms: u64,
    window_days: u32,
    work: u8,
) -> u64 {
    let _ = window_days;
    let n = f64::from(occurrences);
    let usd = saving.usd * n;
    let secs = saving.model_secs * n;
    let gain = 1.0 + usd / 2.0 + secs / 600.0 + saving.decisions / 2.0;
    let age_days = now_ms.saturating_sub(last_ms) as f64 / DAY_MS;
    let recency = 1.0 / (1.0 + age_days);
    let work = f64::from(work.clamp(1, 4));
    (1000.0 * (1.0 + n).ln() * gain * (0.5 + recency) / work).round() as u64
}

#[cfg(test)]
mod tests;
