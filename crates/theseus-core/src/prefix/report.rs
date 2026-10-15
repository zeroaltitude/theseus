//! `theseus prefix report` (theseus-rr15), pure: the `prefix.picked` rows
//! joined with each pick's turn's first `provider.call` (did it read the
//! cache?) and with what went wrong after it in its session (a correction of
//! the route or a fact in that turn or the next, or the owner's next message
//! asking the same again). A reuse below `act` that went wrong is a labelled
//! false fit: what Jev's learning report reads to move the bands.

use std::collections::HashMap;

use theseus_protocol::prefix::{FalseFit, PrefixReport};

use super::tokens;
use crate::ledger::LedgerRow;

/// The rows a correction is.
pub const WRONG: &[&str] = &["route.corrected", "fact.corrected"];

/// A message the owner sent after a pick, and whether it asks the same again.
#[derive(Debug, Clone)]
pub struct Next {
    pub session_id: String,
    pub turn_id: String,
    pub re_ask: bool,
}

/// Whether `b` asks what `a` asked: most of their words are the same.
pub fn re_ask(a: &str, b: &str) -> bool {
    let words = |t: &str| -> std::collections::BTreeSet<String> {
        t.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .map(str::to_lowercase)
            .collect()
    };
    let (wa, wb) = (words(a), words(b));
    if wa.len() < 3 || wb.len() < 3 {
        return false;
    }
    let both = wa.intersection(&wb).count() as f64;
    both / wa.union(&wb).count() as f64 >= 0.6
}

/// The report over `picks` (oldest first), the provider calls and the
/// corrections of their sessions, and the messages after them.
pub fn build(
    picks: &[LedgerRow],
    calls: &[LedgerRow],
    wrongs: &[LedgerRow],
    nexts: &[Next],
) -> PrefixReport {
    let mut r = PrefixReport::default();
    // Each turn's first call: its cache read.
    let mut first: HashMap<(&str, &str), &LedgerRow> = HashMap::new();
    for c in calls {
        if let (Some(s), Some(t)) = (c.session_id.as_deref(), c.turn_id.as_deref()) {
            first.entry((s, t)).or_insert(c);
        }
    }
    for p in picks {
        r.picks += 1;
        let d = &p.data;
        let choice = d["choice"].as_str().unwrap_or_default();
        match choice {
            "reuse" => r.reuses += 1,
            "coarser" => r.coarser += 1,
            _ => r.mints += 1,
        }
        let (Some(sid), Some(tid)) = (p.session_id.as_deref(), p.turn_id.as_deref()) else {
            continue;
        };
        if choice != "mint" {
            let read = first
                .get(&(sid, tid))
                .and_then(|c| c.data["usage"]["cache_read_input_tokens"].as_u64())
                .unwrap_or(0);
            let header = tokens(d["header_bytes"].as_u64().unwrap_or(0));
            if read > 0 {
                r.cache_hits += 1;
                let saved = header.min(read);
                r.writes_saved_tokens += saved;
                let per = d["write_per_mtok"].as_f64().unwrap_or(0.0)
                    - d["read_per_mtok"].as_f64().unwrap_or(0.0);
                r.writes_saved_usd += saved as f64 * per.max(0.0) / 1e6;
            }
        }
        if !d["below_act"].as_bool().unwrap_or(false) {
            continue;
        }
        r.below_act += 1;
        let next = nexts.iter().find(|n| n.session_id == sid);
        let corrected = wrongs.iter().any(|w| {
            w.session_id.as_deref() == Some(sid)
                && w.at_unix_ms >= p.at_unix_ms
                && (w.turn_id.as_deref() == Some(tid)
                    || next.is_some_and(|n| w.turn_id.as_deref() == Some(n.turn_id.as_str())))
        });
        let why = if corrected {
            Some("correction")
        } else if next.is_some_and(|n| n.re_ask) {
            Some("re_ask")
        } else {
            None
        };
        if let Some(why) = why {
            r.false_fits.push(FalseFit {
                session_id: sid.into(),
                turn_id: tid.into(),
                at_unix_ms: p.at_unix_ms,
                choice: choice.into(),
                fit: d["fit"].as_f64().unwrap_or(0.0),
                header: d["header"].as_str().unwrap_or_default().into(),
                why: why.into(),
            });
        }
    }
    if r.picks > 0 {
        r.reuse_rate = (r.reuses + r.coarser) as f64 / r.picks as f64;
        r.mint_rate = r.mints as f64 / r.picks as f64;
    }
    r
}
