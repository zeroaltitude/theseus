//! The owner's runs over the learning ledger (M5 25d; design §2.9): a
//! replay, an audit, a backfill. Each run is one row, keyed by its id and
//! scoped as what it wrote (`judge.replay:<pack id>` for a replay, beside
//! its calls; `judge:<pack id>` for an audit and a backfill, beside the
//! labels and judgments the report reads), written in the frame that ends
//! the run, and one sentence. The runs are outside every turn: no span.

use serde_json::{json, Value};
use theseus_protocol::judge_runs::{JudgeAuditResult, JudgeBackfillResult, JudgeReplayResult};
use theseus_protocol::{LedgerKind, NarrativePart};

use super::{Fact, Say};

fn usd(x: f64) -> String {
    format!("${x:.4}")
}

/// A replay ran (`judge.replay`): the candidate's name, sha256, and the
/// blob that holds its text; the set; and the result whole.
pub struct JudgeReplayed<'a> {
    pub result: &'a JudgeReplayResult,
    /// The blob holding the candidate's text.
    pub text_blob: &'a str,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for JudgeReplayed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeReplay);

    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.result).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("text_blob".into(), json!(self.text_blob));
            o.insert("who".into(), json!(self.who));
            o.insert("via".into(), json!(self.via));
        }
        v
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.result;
        say.line(
            NarrativePart::Session,
            format!(
                "{} replayed {} beside {} over {} judgments of {} ({} asked, {} left out): {} \
                 fixed, {} broken, {}.",
                self.who,
                r.candidate,
                r.incumbent,
                r.judgments,
                r.set,
                r.called + r.rebanded,
                r.left_out.len(),
                r.fixed,
                r.broken,
                usd(r.cost_usd)
            ),
        );
    }
}

/// An audit ran (`judge.audit`): its profile, model, seed, counts, and cost.
pub struct JudgeAudited<'a> {
    pub result: &'a JudgeAuditResult,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for JudgeAudited<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeAudit);

    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.result).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("who".into(), json!(self.who));
            o.insert("via".into(), json!(self.via));
        }
        v
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.result;
        let stopped = r
            .stopped
            .as_deref()
            .map(|s| format!("; it stopped early: {s}"))
            .unwrap_or_default();
        say.line(
            NarrativePart::Session,
            format!(
                "{} audited {} with {} ({}): {} of {} judgments asked, {} audit labels, {} \
                 dropped, {}{stopped}.",
                self.who,
                r.pack,
                r.profile,
                r.model,
                r.asked,
                r.sampled,
                r.labels,
                r.dropped,
                usd(r.cost_usd)
            ),
        );
    }
}

/// A backfill ran (`judge.backfill`), under the consent of the config whose
/// digest it names.
pub struct JudgeBackfilled<'a> {
    pub result: &'a JudgeBackfillResult,
    pub who: &'a str,
    pub via: &'a str,
}

impl Fact for JudgeBackfilled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeBackfill);

    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.result).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("who".into(), json!(self.who));
            o.insert("via".into(), json!(self.via));
        }
        v
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.result;
        say.line(
            NarrativePart::Session,
            format!(
                "{} backfilled {} from its recorded history: {} events, {} judged now, {} \
                 judged before, {} left out, {}.",
                self.who,
                r.pack,
                r.events,
                r.judged,
                r.already,
                r.left_out.len(),
                usd(r.cost_usd)
            ),
        );
    }
}
