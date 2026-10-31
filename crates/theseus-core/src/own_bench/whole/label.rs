//! Machine labels for routing (own-benchmark part (c)): what a whole-turn
//! replay found, offered to the route pack's learning loop as a label.
//!
//! The route pack asked one deciding question of the turn, `mode`: which kind
//! of work this is, and so which profile runs it. A replay on the profile one
//! rung down that Jev judged `same` or `better` at `[route_bench] whole_label_bar`,
//! with the facts not against it (the same last test result when there was one,
//! the same paths changed, no cap ending it), says the work could run there. The
//! label is the class whose profile list leads with that profile (the first of
//! `CLASS_ORDER` that does; the class the pack gave when it is one of them),
//! written on the turn's route judgment as a `judge.label` row of source
//! `replay` and weight `REPLAY_WEIGHT`.
//!
//! It is machine-sourced and it never reaches the holdout: the learning loop
//! drops `replay` labels from the holdout it freezes and the one it scores
//! (`learning::without_machine`), so a version trained on them still has to
//! beat its baseline on the owner's own labels. The owner's label on the same
//! judgment outweighs it (weight 1.0 against 0.4).

use serde_json::{json, Value};
use theseus_protocol::own_bench::TurnReplayResult;
use theseus_protocol::LedgerKind;

use crate::fact;
use crate::learning::{labels, REPLAY_SOURCE, REPLAY_WEIGHT};
use crate::routing::model::OPTIONS;
use crate::rpc::Core;

/// The `model` option whose catalog model `model` is: what a rung's replay
/// labels the turn's route judgment with.
pub fn option_for(model: &str) -> Option<&'static str> {
    OPTIONS.iter().find(|(_, m)| *m == model).map(|(o, _)| *o)
}

impl Core {
    /// Offers the replay as a label, and says in the result what came of it.
    pub(in crate::own_bench) fn label_whole(
        &self,
        p: &theseus_protocol::own_bench::TurnReplayParams,
        profile: &str,
        result: &mut TurnReplayResult,
    ) {
        let said = match self.try_label(p, profile, result) {
            Ok(s) | Err(s) => s,
        };
        if let Some(w) = result.whole.as_mut() {
            w.label = Some(said);
        }
    }

    /// `Ok(what was written)` or `Err(why none was)`.
    fn try_label(
        &self,
        p: &theseus_protocol::own_bench::TurnReplayParams,
        profile: &str,
        result: &TurnReplayResult,
    ) -> Result<String, String> {
        let bar = self.cfg.route_bench.whole_label_bar;
        let none = |why: &str| Err(format!("none: {why}"));
        if bar <= 0.0 {
            return none("[route_bench] whole_label_bar is 0");
        }
        let (Some(v), Some(w)) = (result.verdict.as_ref(), result.whole.as_ref()) else {
            return none("the replay was not judged");
        };
        if v.same_or_better < bar {
            return none(&format!(
                "compare.v1 put same or better at {:.2}, under {bar:.2}",
                v.same_or_better
            ));
        }
        if let Some(c) = &w.capped {
            return none(&format!("the replay ended at its {c} cap"));
        }
        if w.same.tests == Some(false) {
            return none("the last test run differs from the original's");
        }
        if !w.same.files {
            return none("the paths changed differ from the original's");
        }
        let rows = self
            .ledger_rows(LedgerKind::RouteDecided, Some(&p.session), None, None)
            .map_err(|e| format!("none: the route rows were not read: {e}"))?;
        // The turn's route decision whose judgment the ledger can read (the
        // sink writes a judgment a moment after its turn).
        let mut found = None;
        for row in rows
            .iter()
            .filter(|r| r.turn_id.as_deref() == Some(p.turn.as_str()))
        {
            if let Some(j) = row.data["judgment"].as_str() {
                if let Ok(jrow) = self.judgment_row(j) {
                    found = Some((j.to_string(), jrow));
                    break;
                }
            }
        }
        let Some((judgment, jrow)) = found else {
            return none("the turn has no route judgment the ledger can read");
        };
        let judgment = judgment.as_str();
        let Some(option) = self
            .cfg
            .profile(profile)
            .ok()
            .and_then(|p| option_for(&p.model))
        else {
            return none(&format!("no `model` option names the model {profile} runs"));
        };
        let pack_name = jrow.data["pack"].as_str().unwrap_or_default().to_string();
        let Some(pack) = self.runner.judge.pack(&pack_name) else {
            return none(&format!("this build has no pack {pack_name}"));
        };
        let label: Value =
            labels::check(&pack, Some("model"), &json!(option)).map_err(|e| format!("none: {e}"))?;
        let id = format!(
            "lbl_wt_{}",
            &crate::blobs::digest(format!("whole|{judgment}|model|{}", p.turn).as_bytes())[..24]
        );
        if self.store.ledger_by_key(&id).ok().flatten().is_some() {
            return none("this turn's replay already labeled it");
        }
        let note = format!("whole-turn replay of {} on {profile}", p.turn);
        let f = fact::judge::JudgeLabel {
            id: &id,
            judgment,
            pack: &pack_name,
            question: Some("model"),
            about: None,
            label,
            source: REPLAY_SOURCE,
            who: REPLAY_SOURCE,
            via: REPLAY_SOURCE,
            weight: REPLAY_WEIGHT,
            note: &note,
            correlation_id: None,
            rule: Some("whole_turn"),
        };
        let mut rec =
            fact::row(&f, jrow.session_id.as_deref(), None).map_err(|e| format!("none: {e}"))?;
        rec.key = Some(id.clone());
        self.store
            .append(&[rec.scoped(&crate::rpc::judge::scope_of(&pack_name))])
            .map_err(|e| format!("none: the label was not written: {e}"))?;
        Ok(format!("{pack_name} model={option} on {judgment}"))
    }
}
