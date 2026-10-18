//! A night of the pattern miner (theseus-pw1q.21.3): `self.mine.ran`, written
//! in the frame that moves the miner's cursor. Its fields are `SELF_ROWS`'s
//! (`window`, `runs`, `calls_read`, `motifs`, `top`) beside every `self.*`
//! row's `what`, `why`, `numbers` and `undo`. It holds digests and numbers,
//! never a value.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// One motif of the night's top: its digest, its support and what it would save.
pub struct Top {
    pub digest: String,
    pub support: u32,
    pub saving_usd_per_day: f64,
}

/// The miner read the window and found motifs.
pub struct MineRan {
    pub window_days: u32,
    pub runs: u32,
    pub calls_read: u32,
    pub motifs: u32,
    pub top: Vec<Top>,
}

impl Fact for MineRan {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfMineRan);

    fn row(&self) -> Value {
        let top: Vec<Value> = self
            .top
            .iter()
            .map(|t| {
                json!([
                    t.digest,
                    t.support,
                    (t.saving_usd_per_day * 1e4).round() / 1e4
                ])
            })
            .collect();
        json!({"window": self.window_days, "runs": self.runs, "calls_read": self.calls_read,
               "motifs": self.motifs, "top": top,
               "what": format!("the pattern miner found {} recurring sequences", self.motifs),
               "why": format!("{} runs, {} new calls, over {} days", self.runs, self.calls_read, self.window_days),
               "numbers": {"runs": self.runs, "calls_read": self.calls_read, "motifs": self.motifs},
               "undo": "set [self.mint] mine = false"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "The pattern miner read {} runs over {} days and found {} recurring sequences.",
                self.runs, self.window_days, self.motifs
            ),
        );
    }
}
