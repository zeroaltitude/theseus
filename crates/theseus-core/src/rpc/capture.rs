//! The owner's side of capture (theseus-slvl.5): `standing.answer` keeps or
//! drops an ask, an undo of a kept record labels its judgment, and the lists
//! (`standing.list`) show the open asks and the week's repeats.

use crate::approval::Surface;
use anyhow::{anyhow, bail};
use serde_json::json;
use theseus_protocol::learning::JudgeLabelParams;
use theseus_protocol::TurnSubmitParams;
use theseus_protocol::{
    method, StandingAnswerParams, StandingAnswerResult, StandingAsk, StandingKind, StandingRecord,
    StandingScope,
};

use super::confirms::Act;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::capture::act::refuse_if_full;
use crate::capture::state::Kept;
use crate::fact;
use crate::standing;

/// The label an undo of a kept record puts on its judgment: the pack's
/// rollback counts it (three in a day).
pub const WRONG: &str = "wrong capture";

impl Core {
    /// `standing.answer`: an ask kept (where it was judged to hold, or
    /// everywhere) or dropped. The owner's act, from a private place, as
    /// `standing.add` is.
    pub fn standing_answer(
        &self,
        p: &StandingAnswerParams,
        who: impl Into<Answerer>,
    ) -> Result<StandingAnswerResult, RpcFailure> {
        let who = who.into();
        if !matches!(p.answer.as_str(), "keep" | "everywhere" | "not") {
            return Err(RpcFailure::invalid(anyhow!(
                "an answer is keep, everywhere, or not: {:?} is none of them",
                p.answer
            )));
        }
        let what = format!("the answer {} to {}", p.answer, p.id);
        self.judge_act(
            &who,
            Act::Standing {
                method: method::STANDING_ANSWER,
                what: &what,
            },
        )
        .map_err(super::standing::failure)?;
        let (who_s, via) = (who.who(), who.via());
        theseus_store::blocking(|| self.answer_standing_ask(p, &who_s, &via))
            .map_err(RpcFailure::invalid)
    }

    fn answer_standing_ask(
        &self,
        p: &StandingAnswerParams,
        who: &str,
        via: &str,
    ) -> anyhow::Result<StandingAnswerResult> {
        let now = theseus_protocol::now_unix_ms();
        let mut held = self.runner.standing.capture.lock(&self.store)?;
        let mut next = (*held).clone();
        next.trim(now);
        let id = p.id.trim();
        let hits: Vec<usize> = next
            .asks
            .iter()
            .enumerate()
            .filter(|(_, o)| o.ask.id == id || (!id.is_empty() && o.ask.id.starts_with(id)))
            .map(|(i, _)| i)
            .collect();
        let at = match hits.as_slice() {
            [i] => *i,
            [] => bail!("no open ask {id}"),
            _ => bail!("{id} starts more than one open ask: name more of it"),
        };
        let open = next.asks.remove(at);
        let ask = &open.ask;
        let keep = p.answer != "not";
        let scope = match p.answer.as_str() {
            "everywhere" => StandingScope::Everywhere,
            _ => ask.scope.clone(),
        };
        let all = self.runner.standing.all(&self.store)?;
        let mut kept: Option<StandingRecord> = None;
        if keep {
            let live = standing::live(&all, None, now);
            refuse_if_full(&live, &scope)?;
            let rec = StandingRecord {
                id: standing::mint(&all),
                kind: ask.kind,
                text: ask.text.clone(),
                scope,
                since_ms: now,
                until_ms: None,
                source: format!("capture:{}", ask.node),
                ends: None,
            };
            next.kept.push(Kept {
                record: rec.id.clone(),
                judgment: open.judgment.clone(),
                at_ms: now,
            });
            kept = Some(rec);
        }
        let answered = fact::capture::StandingAnswered {
            ask: &ask.id,
            answer: &p.answer,
            record: kept.as_ref().map(|r| r.id.as_str()),
            who,
            via,
        };
        let mut rows = vec![fact::row(&answered, Some(&open.session), None)?];
        if let Some(rec) = &kept {
            let added = fact::standing::StandingAdded {
                record: rec,
                who,
                via,
            };
            rows.push(fact::row(&added, Some(&open.session), None)?);
        }
        rows.push(next.row()?);
        let records: Vec<StandingRecord> = kept.iter().cloned().collect();
        self.runner
            .standing
            .write(&self.store, |_| Ok(records), |_, _| Ok(rows))?;
        held.set(next);
        drop(held);
        if let Some(rec) = &kept {
            self.rec(Some(&open.session))
                .announce(&fact::standing::StandingAdded {
                    record: rec,
                    who,
                    via,
                });
        }
        Ok(StandingAnswerResult {
            id: ask.id.clone(),
            answer: p.answer.clone(),
            record: kept,
        })
    }

    /// A record was revoked: if `directive.v1` kept it, its judgment is
    /// labeled `wrong capture` (one a day-count away from the pack's
    /// rollback) and the record is forgotten as kept. The undo has already
    /// happened; a label that cannot be written is logged.
    pub(crate) fn capture_undone(&self, record: &str, who: &Answerer) {
        let found = theseus_store::blocking(|| -> anyhow::Result<Option<String>> {
            let mut held = self.runner.standing.capture.lock(&self.store)?;
            let Some(at) = held.kept.iter().position(|k| k.record == record) else {
                return Ok(None);
            };
            let mut next = (*held).clone();
            let judgment = next.kept.remove(at).judgment;
            self.store.append(&[next.row()?])?;
            held.set(next);
            Ok(Some(judgment))
        });
        let judgment = match found {
            Ok(Some(j)) => j,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "capture: an undo was not noted");
                return;
            }
        };
        let label = JudgeLabelParams {
            judgment,
            question: None,
            label: json!(WRONG),
            note: Some(format!("the owner undid {record}")),
            discord: None,
        };
        if let Err(e) = self.judge_label(&label, who.clone()) {
            tracing::warn!(error = %format!("{e:#}"), "capture: the undo's label was not written");
        }
    }

    /// The open asks, for `standing.list`: the words go to the owner alone.
    pub(crate) fn capture_asks(&self, owner: bool) -> anyhow::Result<Vec<StandingAsk>> {
        let now = theseus_protocol::now_unix_ms();
        let mut state = self.runner.standing.capture.peek(&self.store)?;
        state.trim(now);
        Ok(state
            .asks
            .into_iter()
            .map(|o| {
                let mut a = o.ask;
                if !owner && a.kind != StandingKind::Order {
                    a.text = "(private)".into();
                }
                a
            })
            .collect())
    }
}

impl Core {
    /// `turn.submit`'s part: the message noted as the owner's own, when it is
    /// one and capture is on. A message a job sent (`opened_from`), one
    /// through the MCP server or an unnamed surface, a prompt's, or one with
    /// files is never noted, so the pack is not asked about it: the rule the
    /// owner's route corrections follow (`rpc/route_correct.rs`).
    pub(super) fn expect_capture(&self, p: &TurnSubmitParams, conn: Conn<'_>, session: &str) {
        if !self.cfg.standing.capture
            || !self.cfg.judge.enabled
            || p.opened_from.is_some()
            || p.prompt.is_some()
            || !p.attachments.is_empty()
            || matches!(conn.surface, Surface::Mcp | Surface::Unnamed)
        {
            return;
        }
        self.runner.standing.expected.note(session, &p.input);
    }
}
