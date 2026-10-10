//! One session's pass (theseus-wy7y): the extraction, the exclusions, and
//! Jev's judgment of each kept candidate, shared by the live point and the
//! backfill. The extraction's row (and the caller's mark, when it has one)
//! is one frame, written between turns; the judgments are the sink's rows.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::json;
use theseus_judge::builders::{PeopleInput, PersonCandidate};
use theseus_judge::price::Micros;
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Outcome, Urgency};
use theseus_ontology::{handles_of, Ontology};
use theseus_store::{kinds, NewRecord};

use super::{extract, fold, nearest, Line, NotPeople, PACK, SCOPE};
use crate::config::PackMode;
use crate::judge::{spend, JudgeService, ScrubWith};
use crate::ledger::LedgerRow;
use crate::provider::Provider;
use crate::rpc::Core;

/// One session to pass over.
pub struct Session<'a> {
    pub sid: &'a str,
    pub title: &'a str,
    pub lines: Vec<Line>,
    /// `live`, or `backfill:<tag>`.
    pub purpose: &'a str,
    /// A record the pass's frame carries (the caller's mark).
    pub mark: Option<NewRecord>,
    /// The names this session must not be asked about again: proposed
    /// before (and so answered, or waiting), or held by it.
    pub known: HashSet<String>,
}

/// What one pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pass {
    pub candidates: u64,
    pub excluded: u64,
    pub judged: u64,
    /// The extractor's cost and Jev's.
    pub spent: Micros,
    pub failed: bool,
    /// The names it asked Jev about, folded.
    pub asked: Vec<String>,
}

impl Core {
    /// The extractor: `[people] extract_profile`'s target and provider.
    pub(crate) fn people_extractor(
        &self,
    ) -> Result<(crate::turn::Target, Arc<dyn Provider>), String> {
        let profile = self.runner.cfg.people.extract_profile.clone();
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&profile), None, None)
            .map_err(|e| format!("[people] extract_profile {profile:?} does not resolve: {e:#}"))?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| format!("no provider {:?} for {profile}", target.provider))?;
        Ok((target, provider))
    }

    /// The extraction's request for `lines`, scrubbed.
    pub(crate) fn people_request(
        &self,
        model: &str,
        title: &str,
        lines: &[Line],
    ) -> crate::provider::ProviderRequest {
        let scrubber = self.runner.judge.scrubber.clone();
        extract::request(model, title, lines, &|t| scrubber.scrub(t).0)
    }

    /// The names each session was asked about before (`people.v1`'s
    /// judgments in its scope), folded, by session: one scan.
    pub(crate) fn people_known(&self) -> anyhow::Result<HashMap<String, HashSet<String>>> {
        let mut out: HashMap<String, HashSet<String>> = HashMap::new();
        for r in self.store.scope_after(SCOPE, 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind != "judge.call" {
                continue;
            }
            let (Some(sid), Some(name)) = (
                row.session_id.clone(),
                row.data["context"]["candidate"]["name"].as_str(),
            ) else {
                continue;
            };
            out.entry(sid).or_default().insert(fold(name));
        }
        Ok(out)
    }

    /// The people a session holds now, as names: each one's name and its
    /// `name:` handles, folded.
    pub(crate) fn people_held_by(o: &Ontology, sid: &str) -> HashSet<String> {
        o.memberships(sid)
            .iter()
            .filter(|m| m.kind() == theseus_ontology::person::KIND)
            .filter_map(|m| o.category(&m.category))
            .flat_map(|c| {
                let mut v = vec![fold(&c.name)];
                v.extend(
                    handles_of(c)
                        .iter()
                        .filter_map(|h| h.strip_prefix("name:"))
                        .map(fold),
                );
                v
            })
            .collect()
    }

    /// One session's pass. `Err` says why nothing was asked (no extractor,
    /// no price, the day's ceiling); a failed call is a pass that failed,
    /// its row written.
    pub(crate) async fn propose_people(&self, s: Session<'_>) -> Result<Pass, String> {
        let (target, provider) = self.people_extractor()?;
        let request = self.people_request(&target.model, s.title, &s.lines);
        let need = self
            .audit_reserve(&request)
            .ok_or_else(|| format!("no price for {}", target.model))?;
        let hold = self.runner.day_hold(need, "people")?;
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        let mut pass = Pass {
            spent: cost,
            ..Pass::default()
        };
        let id = crate::new_id("ppl");
        let (model, usage, found, failed) = match &answered {
            Ok(r) => match extract::parse(&r.content, &s.lines) {
                Some(found) => (r.model.clone(), Some(r.usage.clone()), found, None),
                None => (
                    r.model.clone(),
                    Some(r.usage.clone()),
                    Vec::new(),
                    Some("the answer held no propose_people call".to_string()),
                ),
            },
            Err(e) => (
                target.model.clone(),
                None,
                Vec::new(),
                Some(format!("{e:#}")),
            ),
        };
        pass.failed = failed.is_some();
        pass.candidates = found.len() as u64;
        let o = match self.runner.ontology.held() {
            Some(o) => o,
            None => self
                .runner
                .ontology
                .snapshot(&self.store)
                .map_err(|e| format!("the ontology was not read: {e:#}"))?,
        };
        let not = NotPeople::of(&self.runner.cfg, &s.lines).with_held(&o);
        let mut known = s.known.clone();
        known.extend(Self::people_held_by(&o, s.sid));
        let (mut kept, mut excluded) = (Vec::new(), Vec::new());
        for (c, nodes) in found.iter().cloned() {
            let folded = fold(&c.name);
            if not.excludes(&c) || known.contains(&folded) {
                excluded.push(c.name.clone());
            } else {
                known.insert(folded.clone());
                pass.asked.push(folded);
                kept.push((c, nodes));
            }
        }
        pass.excluded = excluded.len() as u64;
        let (judgments, jev) = self
            .runner
            .judge
            .judge_people(s.sid, s.title, s.purpose, &id, kept, &o)
            .await;
        pass.judged = judgments.len() as u64;
        pass.spent += jev;
        let names: Vec<String> = found.iter().map(|(c, _)| c.name.clone()).collect();
        let f = crate::fact::people::PeopleExtracted {
            id: &id,
            purpose: s.purpose,
            profile: &target.profile,
            model: &model,
            cost_usd: theseus_kernel::micros_to_usd(cost),
            input_tokens: usage.as_ref().map_or(0, |u| u.input_tokens),
            output_tokens: usage.as_ref().map_or(0, |u| u.output_tokens),
            candidates: &names,
            excluded: &excluded,
            judgments: &judgments,
            failed: failed.as_deref(),
        };
        let mut records = Vec::new();
        let mut row = crate::fact::row(&f, Some(s.sid), None).map_err(|e| e.to_string())?;
        row.key = Some(id.clone());
        records.push(row.scoped(SCOPE));
        records.extend(s.mark);
        self.people_write(&records)
            .await
            .map_err(|e| format!("the extraction's row was not written: {e:#}"))?;
        self.rec(Some(s.sid)).announce(&f);
        Ok(pass)
    }

    /// One frame, written between turns (the memory pass's writer
    /// handshake), as consolidation's are.
    pub(crate) async fn people_write(&self, records: &[NewRecord]) -> anyhow::Result<()> {
        let w = self.runner.pass.writing().await;
        let written = theseus_store::blocking(|| self.store.append(records));
        drop(w);
        written?;
        Ok(())
    }
}

impl JudgeService {
    /// `people.v1` on each kept candidate, one judgment each, in one
    /// decision point: their ids and Jev's cost. None asked when the pack is
    /// off, the client cannot be built, or the day's budget is spent.
    pub(crate) async fn judge_people(
        &self,
        sid: &str,
        title: &str,
        purpose: &str,
        extraction: &str,
        kept: Vec<(PersonCandidate, Vec<String>)>,
        o: &Ontology,
    ) -> (Vec<String>, Micros) {
        if kept.is_empty() || self.mode_for(PACK, sid).mode == PackMode::Off {
            return (Vec::new(), 0);
        }
        let Some(pack) = theseus_judge::pack::by_name(PACK) else {
            return (Vec::new(), 0);
        };
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let built = match self.built() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built; people not judged");
                return (Vec::new(), 0);
            }
        };
        let scrub = ScrubWith(self.scrubber.clone());
        let mut asks = Vec::new();
        for (candidate, nodes) in kept {
            let input = PeopleInput {
                session_title: title.to_string(),
                held: nearest(o, &candidate),
                candidate: candidate.clone(),
            };
            let Ok(state) = theseus_judge::prepare(&pack, &Input::People(input), &scrub) else {
                continue;
            };
            let blob = match theseus_store::blocking(|| {
                self.store.blobs().put(state.state.json.as_bytes())
            }) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, "judge: the state's blob was not written; not judged");
                    continue;
                }
            };
            let mut context = json!({
                "session": sid, "purpose": purpose, "extraction": extraction,
                "baseline": "no_membership", "decision": "no_membership", "blob": blob,
                "on_path_ms": 0, "candidate": candidate, "evidence_nodes": nodes,
            });
            let mode = self.ask_mode(&pack.name(), &mut context);
            let mut ask = Ask::new(pack.clone(), &state, mode, context);
            ask.id = Some(format!("jdg_{}", uuid::Uuid::now_v7().simple()));
            asks.push(ask);
        }
        let need = built.judge.inner().reserve_micros(&asks).unwrap_or(0);
        if asks.is_empty() || !self.reserve(&today, need) {
            return (Vec::new(), 0);
        }
        let judgments = built
            .judge
            .judge(DecisionPoint {
                asks,
                urgency: Urgency::Shadow,
            })
            .await;
        let mut spent_all = 0;
        let mut ids = Vec::new();
        let share = need / judgments.len().max(1) as u64;
        for j in &judgments {
            let (called, failed, unknown) = match &j.outcome {
                Outcome::Answered => (true, false, false),
                Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
                Outcome::Skipped { .. } => (false, false, false),
            };
            let spent = j.cost_micros.unwrap_or(if unknown { share } else { 0 });
            self.budget.settle(&today, share, spent, called, failed);
            spent_all += spent;
            ids.push(j.id.clone());
        }
        (ids, spent_all)
    }
}

/// A record of `kind` META, keyed.
pub(crate) fn meta<T: serde::Serialize>(key: &str, value: &T) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(key), value)
}
