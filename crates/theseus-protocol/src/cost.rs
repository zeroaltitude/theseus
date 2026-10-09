//! A turn's cost as every surface says it (theseus-c0bb): what its session has
//! cost in all, with this reply's own cost beside it. The Discord reply's footer
//! and the CLI's status line (the terminal UI's too, which shares it) print
//! [`TurnSubmitResult::cost_words`].

use crate::TurnSubmitResult;

impl TurnSubmitResult {
    /// `$47.52 total ($7.86 this reply)`. A daemon from before the session's
    /// total sends none: then the turn's cost alone, `$7.86`. None when the
    /// catalog priced none of the turn's calls.
    pub fn cost_words(&self) -> Option<String> {
        let turn = usd(self.cost_usd?);
        Some(match self.session_cost_usd {
            Some(total) => format!("{} total ({turn} this reply)", usd(total)),
            None => turn,
        })
    }
}

/// Dollars with four decimals under a dollar and two from a dollar up:
/// `$0.0031`, `$7.86`. Under is judged as four decimals round it, so
/// $0.99996 is `$1.00`, never `$1.0000`.
fn usd(dollars: f64) -> String {
    if (dollars * 1e4).round() < 1e4 {
        format!("${dollars:.4}")
    } else {
        format!("${dollars:.2}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn priced(turn: Option<f64>, total: Option<f64>) -> Option<String> {
        TurnSubmitResult {
            cost_usd: turn,
            session_cost_usd: total,
            ..Default::default()
        }
        .cost_words()
    }

    /// The session's total, then this reply's, each with two decimals from a
    /// dollar up and four under.
    #[test]
    fn the_words_give_the_sessions_total_and_this_replys_cost() {
        let words = |turn, total| priced(Some(turn), Some(total)).unwrap();
        assert_eq!(words(7.86, 47.52), "$47.52 total ($7.86 this reply)");
        assert_eq!(words(0.0031, 0.0412), "$0.0412 total ($0.0031 this reply)");
        assert_eq!(words(0.0123, 47.52), "$47.52 total ($0.0123 this reply)");
        assert_eq!(words(1.0, 1.0), "$1.00 total ($1.00 this reply)");
        assert_eq!(words(0.0, 0.0), "$0.0000 total ($0.0000 this reply)");
        // Under a dollar as four decimals round it.
        assert_eq!(words(0.99994, 0.99996), "$1.00 total ($0.9999 this reply)");
    }

    /// An older daemon's result has no total: the turn's cost alone. A turn
    /// the catalog priced nothing of says no money at all.
    #[test]
    fn without_a_total_the_words_are_the_turns_cost_alone() {
        assert_eq!(priced(Some(7.86), None).as_deref(), Some("$7.86"));
        assert_eq!(priced(Some(0.0031), None).as_deref(), Some("$0.0031"));
        assert_eq!(priced(None, Some(47.52)), None);
        assert_eq!(priced(None, None), None);
    }

    /// An older peer's result, without the field, still decodes, as none; and
    /// a result without a total writes no key, so its bytes are an older
    /// daemon's.
    #[test]
    fn an_older_peers_result_decodes_without_the_total() {
        let old = json!({"session_id": "ses_q7f3k2", "turn_id": "turn_m4p8z1", "loops": 1,
            "output": "Two files.", "stop_reason": "end_turn", "provider_stop_reason": null,
            "model": "glm-x", "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 1,
            "cost_usd": 0.0031});
        let r: TurnSubmitResult = serde_json::from_value(old).unwrap();
        assert_eq!((r.cost_usd, r.session_cost_usd), (Some(0.0031), None));
        assert_eq!(r.cost_words().as_deref(), Some("$0.0031"));
        let back = serde_json::to_value(&r).unwrap();
        assert!(back.get("session_cost_usd").is_none(), "{back}");
        let now = TurnSubmitResult {
            session_cost_usd: Some(47.52),
            ..r
        };
        assert_eq!(
            serde_json::to_value(&now).unwrap()["session_cost_usd"],
            json!(47.52)
        );
    }
}
