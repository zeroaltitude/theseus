//! What a Jev call costs (design §2.6). A catalog-shaped row per pinned Jev
//! model: kind `judge`, provider `typesafe`, input and output prices per
//! million tokens, and no cache prices. The core's catalog gets the same row
//! at the wire-in (23a); a model with no row is never called (`unpriced`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::client::Usage;

/// Micro-dollars, the budget's unit.
pub type Micros = u64;
pub const MICROS_PER_USD: u64 = 1_000_000;

/// The output tokens a reservation allows per question. Observed: 22 to 57
/// a question on 2026-09-30, the most for a Choice of four options.
pub const OUTPUT_ALLOWANCE_PER_QUESTION: u64 = 64;

/// Input tokens Jev bills on every call beyond the request itself. Fitted
/// on the lane's live calls (2026-09-30): about 260 tokens a call, once per
/// call however many questions it carries; reserved with room to spare.
pub const CALL_OVERHEAD_TOKENS: u64 = 320;

/// Request bytes per billed input token, for the reservation. The live
/// calls billed about one token per 3 bytes of question text; the state's
/// own estimate (bytes / 4) is kept for its cap, not for money.
pub const RESERVE_BYTES_PER_TOKEN: u64 = 3;

/// The model every pack in this crate pins.
pub const JEV_MODEL: &str = "jev-1.13.0";

/// Dollars to micro-dollars, as the kernel converts them (rounded; zero for
/// anything not a positive finite number).
pub fn usd_to_micros(usd: f64) -> Micros {
    if usd.is_finite() && usd > 0.0 {
        (usd * MICROS_PER_USD as f64).round() as u64
    } else {
        0
    }
}

pub fn micros_to_usd(m: Micros) -> f64 {
    m as f64 / MICROS_PER_USD as f64
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevPrice {
    /// Always `judge`: the core's catalog tells a judge row from a model row.
    pub kind: String,
    pub provider: String,
    /// Input and output together, in tokens.
    pub context_window: u64,
    /// The longest state Jev takes, in tokens.
    pub state_limit_tokens: u64,
    /// US dollars per million tokens.
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    /// Where the figures came from, and whether they still need checking.
    pub source: String,
}

impl JevPrice {
    /// `jev-1.13.0`, from `refs/jev.md`'s observed price (2026-09-21). To
    /// re-verify against TypeSafe's console before anyone quotes it.
    pub fn jev_1_13_0() -> Self {
        Self {
            kind: "judge".into(),
            provider: "typesafe".into(),
            context_window: 64_000,
            state_limit_tokens: 32_000,
            input_per_mtok: 0.042,
            output_per_mtok: 0.042,
            source:
                "refs/jev.md, observed 2026-09-21 (about $0.042 per million tokens); to re-verify"
                    .into(),
        }
    }

    /// What a call's usage costs, rounded up to the next micro-dollar.
    pub fn cost_micros(&self, u: &Usage) -> Micros {
        micros_of(&[
            (u.input_tokens, self.input_per_mtok),
            (u.output_tokens, self.output_per_mtok),
        ])
    }

    pub fn cost_usd(&self, u: &Usage) -> f64 {
        (u.input_tokens as f64 * self.input_per_mtok
            + u.output_tokens as f64 * self.output_per_mtok)
            / 1_000_000.0
    }

    /// What a call reserves before it runs, rounded up per call: its whole
    /// request (the state and every question's text, at one token per 3
    /// bytes) plus Jev's per-call overhead at the input price, and
    /// [`OUTPUT_ALLOWANCE_PER_QUESTION`] per question at the output price.
    ///
    /// §2.6 reserved the state's estimate and the output allowance only. The
    /// live calls billed 395 to 479 input tokens for a 72-token state and one
    /// question, so that would have reserved about a third of the cost; this
    /// covers every live call the lane made (see the lane report).
    pub fn reserve_micros(&self, request_bytes: usize, questions: usize) -> Micros {
        let input = CALL_OVERHEAD_TOKENS + (request_bytes as u64).div_ceil(RESERVE_BYTES_PER_TOKEN);
        micros_of(&[
            (input, self.input_per_mtok),
            (
                OUTPUT_ALLOWANCE_PER_QUESTION * questions as u64,
                self.output_per_mtok,
            ),
        ])
    }

    /// What a request reserves (see [`JevPrice::reserve_micros`]).
    pub fn reserve_request(&self, req: &crate::client::Request) -> Micros {
        self.reserve_micros(req.body().len(), req.questions.len())
    }
}

/// The built-in rows, by model.
pub fn builtin() -> BTreeMap<String, JevPrice> {
    BTreeMap::from([(JEV_MODEL.to_string(), JevPrice::jev_1_13_0())])
}

/// Σ tokens × price, rounded up to the next micro-dollar: a price in dollars
/// per million tokens is micro-dollars per token, so scaled by a million it is
/// an exact integer, and the sum is exact before the one rounding (the
/// core's catalog rounds the same way).
fn micros_of(terms: &[(u64, f64)]) -> Micros {
    let scaled: u128 = terms
        .iter()
        .map(|&(tokens, per_mtok)| tokens as u128 * usd_to_micros(per_mtok) as u128)
        .sum();
    u64::try_from(scaled.div_ceil(MICROS_PER_USD as u128)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_2000_token_state_costs_about_84_micro_dollars_and_rounds_up() {
        let p = JevPrice::jev_1_13_0();
        let u = Usage {
            input_tokens: 2_000,
            output_tokens: 0,
        };
        assert_eq!(p.cost_micros(&u), 84);
        // One token is a fraction of a micro-dollar, charged as a whole one.
        let one = Usage {
            input_tokens: 1,
            output_tokens: 0,
        };
        assert_eq!(p.cost_micros(&one), 1);
        assert_eq!(p.cost_micros(&Usage::default()), 0);
        assert!((p.cost_usd(&u) - 0.000_084).abs() < 1e-12);
    }

    #[test]
    fn the_reservation_covers_the_whole_request_and_the_call_overhead() {
        let p = JevPrice::jev_1_13_0();
        // 320 + 900 / 3 = 620 input tokens, and 3 × 64 output tokens: 812
        // tokens at 0.042 is 34.104, rounded up.
        assert_eq!(p.reserve_micros(900, 3), 35);
        // Every live call of 2026-09-30 (request bytes, questions, billed in
        // and out) fits inside its reservation.
        for (bytes, questions, input_tokens, output_tokens) in [
            (1_205, 3, 586, 98),
            (788, 1, 479, 57),
            (618, 1, 424, 22),
            (539, 1, 395, 26),
        ] {
            let billed = p.cost_micros(&Usage {
                input_tokens,
                output_tokens,
            });
            assert!(
                p.reserve_micros(bytes, questions) >= billed,
                "{bytes} bytes"
            );
        }
    }

    #[test]
    fn the_builtin_table_prices_the_pinned_model_and_is_marked_to_reverify() {
        let t = builtin();
        let row = &t[JEV_MODEL];
        assert_eq!(row.kind, "judge");
        assert_eq!(row.provider, "typesafe");
        assert!(row.source.contains("re-verify"));
        assert!(!t.contains_key("jev-latest"));
    }
}
