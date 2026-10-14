//! An extension's ask for hands (theseus-pw1q.14; rsi-plan §3, wave 2):
//! `extend.propose { hands: { usd_cap, backends, max_group } }`, every field
//! required when it is there. It is a capability grant, so the owner's Load
//! card names it in plain words and only the owner's ack grants it: an
//! extension, a hand, or the self loop's `"self"` principal answers nothing.
//!
//! The ask is frozen with the tree: its canonical JSON is folded into the
//! digest (`freeze::with_caps`), so an edit to it is another digest, another
//! copy, and another card. An extension with no ask is exactly as before:
//! its digest is its tree's, and the board gives it no channel
//! (`broker.rs`).

use serde::{Deserialize, Serialize};

use crate::aws::hands::launch::{Backend, MAX_HANDS};

/// The most a day's cap may be: an extension's batch work, not a budget.
pub const MAX_USD_CAP: f64 = 100.0;

/// What an extension may spend on hands, where, and how many at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandsCap {
    /// Its day's line: every hand's worst case reserves against it, and the
    /// local day's settled cost counts.
    pub usd_cap: f64,
    /// The backends its groups may run on.
    pub backends: Vec<Backend>,
    /// The most hands it runs at a time, all its groups together.
    pub max_group: u32,
}

impl HandsCap {
    /// Why the ask cannot be frozen, if it cannot.
    pub fn check(&self) -> Result<(), String> {
        if !self.usd_cap.is_finite() || self.usd_cap <= 0.0 || self.usd_cap > MAX_USD_CAP {
            return Err(format!(
                "hands.usd_cap is the day's dollars, above 0 and at most {MAX_USD_CAP}"
            ));
        }
        if self.backends.is_empty() {
            return Err("hands.backends names lambda, fargate, or both".into());
        }
        if (1..self.backends.len()).any(|i| self.backends[..i].contains(&self.backends[i])) {
            return Err("hands.backends names a backend twice".into());
        }
        if self.max_group == 0 || self.max_group as usize > MAX_HANDS {
            return Err(format!("hands.max_group is 1 to {MAX_HANDS}"));
        }
        Ok(())
    }

    /// The bytes the digest folds in: the same ask, the same bytes.
    pub fn canonical(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The day's line, in micro-dollars.
    pub fn cap_micros(&self) -> u64 {
        (self.usd_cap * 1e6).round() as u64
    }

    /// Whether its groups may run on `b`.
    pub fn allows(&self, b: Backend) -> bool {
        self.backends.contains(&b)
    }

    /// The Load card's sentence: "This extension may run up to 20 hands at
    /// a time on Lambda, at most $2.00 a day."
    pub fn words(&self) -> String {
        let on: Vec<&str> = self
            .backends
            .iter()
            .map(|b| match b {
                Backend::Lambda => "Lambda",
                Backend::Fargate => "Fargate",
            })
            .collect();
        format!(
            "This extension may run up to {} at a time on {}, at most ${:.2} a day.",
            crate::narrative::count(u64::from(self.max_group), "hand", "hands"),
            on.join(" or "),
            self.usd_cap
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(v: serde_json::Value) -> Result<HandsCap, String> {
        let c: HandsCap = serde_json::from_value(v).map_err(|e| e.to_string())?;
        c.check().map(|()| c)
    }

    #[test]
    fn the_ask_parses_with_every_field_and_says_itself_plainly() {
        let c = ask(serde_json::json!({"usd_cap": 2.0, "backends": ["lambda"], "max_group": 20}))
            .unwrap();
        assert_eq!(
            c.words(),
            "This extension may run up to 20 hands at a time on Lambda, at most $2.00 a day."
        );
        assert_eq!(c.cap_micros(), 2_000_000);
        assert!(c.allows(Backend::Lambda) && !c.allows(Backend::Fargate));
        assert_eq!(
            c.canonical(),
            r#"{"usd_cap":2.0,"backends":["lambda"],"max_group":20}"#
        );
        let both = ask(
            serde_json::json!({"usd_cap": 0.5, "backends": ["lambda", "fargate"], "max_group": 1}),
        )
        .unwrap();
        assert_eq!(
            both.words(),
            "This extension may run up to 1 hand at a time on Lambda or Fargate, at most $0.50 a day."
        );
    }

    #[test]
    fn a_missing_field_or_a_wrong_value_is_refused() {
        for v in [
            serde_json::json!({"usd_cap": 2.0, "backends": ["lambda"]}),
            serde_json::json!({"usd_cap": 2.0, "max_group": 3}),
            serde_json::json!({"backends": ["lambda"], "max_group": 3}),
            serde_json::json!({"usd_cap": 2.0, "backends": ["lambda"], "max_group": 3, "more": 1}),
            serde_json::json!({"usd_cap": 2.0, "backends": ["ec2"], "max_group": 3}),
            serde_json::json!({"usd_cap": 0.0, "backends": ["lambda"], "max_group": 3}),
            serde_json::json!({"usd_cap": 101.0, "backends": ["lambda"], "max_group": 3}),
            serde_json::json!({"usd_cap": 2.0, "backends": [], "max_group": 3}),
            serde_json::json!({"usd_cap": 2.0, "backends": ["lambda", "lambda"], "max_group": 3}),
            serde_json::json!({"usd_cap": 2.0, "backends": ["lambda"], "max_group": 0}),
            serde_json::json!({"usd_cap": 2.0, "backends": ["lambda"], "max_group": 101}),
        ] {
            assert!(ask(v.clone()).is_err(), "{v} was taken");
        }
    }
}
