//! The prefix library's pick (theseus-rr15; `crate::prefix`): one
//! `prefix.picked` row a compile point, in the frame of the compilation it
//! made, with the fit, the choice and the economics it read. The report
//! (`prefix::report`) joins it with the turn's cache read and with what went
//! wrong after it; nothing of the header's text is in it.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;
use crate::prefix::Picked;

pub struct PrefixPicked<'a> {
    pub picked: &'a Picked,
}

impl Fact for PrefixPicked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PrefixPicked);

    fn row(&self) -> Value {
        let p = self.picked;
        json!({
            "choice": p.choice.as_str(),
            "fit": p.fit,
            "below_act": p.below_act,
            "header": p.plan.used.header,
            "fresh": p.plan.used.fresh,
            "class": p.class,
            "model": p.model,
            "candidates": p.candidates,
            "header_bytes": p.header_bytes,
            "delta_bytes": p.delta_bytes,
            "p_reuse": p.odds.p_reuse,
            "reads": p.odds.reads,
            "gain_usd": p.sums.gain_usd,
            "premium_usd": p.sums.premium_usd,
            "write_per_mtok": p.prices.write,
            "read_per_mtok": p.prices.read,
        })
    }
}
