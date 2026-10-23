//! `standing.list`, `standing.add` and `standing.revoke` (theseus-a7vi): the
//! owner's standing orders, preferences and facts. The writes are the owner's
//! acts, judged as the ontology's are (`judge_act(Act::Standing)`: the owner,
//! from a private place) before anything is read, and refused by the CLI
//! inside a job. A write is one frame: its record and its ledger row. The
//! list reads memory, and tallies the manifests: of the compilations in each
//! live record's scope since it began, how many listed it.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_protocol::{
    error_code, method, StandingAddParams, StandingAnswerParams, StandingChanged, StandingInfo,
    StandingKind, StandingListParams, StandingListResult, StandingProposalAnswer,
    StandingProposalsParams, StandingRecord, StandingRevokeParams,
};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::session::SessionRecord;
use crate::standing::{self, Scope};

impl Core {
    /// The three methods, each as the connection's answerer.
    pub(super) fn rpc_standing(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::STANDING_LIST => route(params, |p: StandingListParams| {
                self.standing_list(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::STANDING_ADD => route(params, |p: StandingAddParams| {
                self.standing_add(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::STANDING_PROPOSALS => route(params, |p: StandingProposalsParams| {
                self.standing_proposals(&p, conn.answerer(None, None))
            }),
            method::STANDING_KEEP => route(params, |p: StandingProposalAnswer| {
                self.standing_keep(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::STANDING_DISMISS => route(params, |p: StandingProposalAnswer| {
                self.standing_dismiss(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            method::STANDING_ANSWER => route(params, |p: StandingAnswerParams| {
                self.standing_answer(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
            _ => route(params, |p: StandingRevokeParams| {
                self.standing_revoke(&p, conn.answerer(p.author.clone(), p.discord.clone()))
            }),
        }
    }

    /// Read the records now, off the start path: after serving, on the
    /// blocking pool. A turn that comes sooner reads them itself, once.
    pub fn warm_standing(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.standing.all(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "the standing records cannot be read");
            }
        });
    }

    /// `standing.list`: the live records (or all), each with the manifests'
    /// tally, and the totals.
    pub fn standing_list(
        &self,
        p: &StandingListParams,
        who: impl Into<Answerer>,
    ) -> Result<StandingListResult, RpcFailure> {
        let all = self
            .runner
            .standing
            .all(&self.store)
            .map_err(RpcFailure::invalid)?;
        // A preference's or a fact's text is the owner's, from a private place.
        let who = who.into();
        let owner =
            crate::places::owner_in_private(&who, &self.runner.place_rule, &self.cfg).is_ok();
        let now = theseus_protocol::now_unix_ms();
        let place = p
            .session_id
            .as_deref()
            .and_then(|s| self.runner.target_of(s));
        let persona = p.session_id.as_deref().and_then(|s| {
            let rec = self.store.get_session::<SessionRecord>(s).ok().flatten()?;
            self.runner.persona_of(&rec)
        });
        let scope = p.session_id.as_deref().map(|session| Scope {
            session,
            place: place.as_deref(),
            surface: p.surface.as_deref(),
            persona: persona.as_deref(),
        });
        let shown: Vec<(&StandingRecord, &'static str)> = all
            .iter()
            .map(|r| (r, standing::state(&all, r, now)))
            .filter(|(r, s)| {
                *s != "ending" && (p.all || *s == "live") && scope.is_none_or(|sc| sc.holds(r))
            })
            .collect();
        let live: Vec<&StandingRecord> = shown
            .iter()
            .filter(|(_, s)| *s == "live")
            .map(|(r, _)| *r)
            .collect();
        // A scan of the sessions' compilations: off the async worker.
        let tally =
            theseus_store::blocking(|| self.standing_tally(&live)).map_err(RpcFailure::invalid)?;
        // What capture holds: the week's repeats and the open asks.
        let held = theseus_store::blocking(|| self.runner.standing.capture.peek(&self.store))
            .map_err(RpcFailure::invalid)?;
        let asks = self.capture_asks(owner).map_err(RpcFailure::invalid)?;
        let records = shown
            .into_iter()
            .map(|(r, state)| {
                let (compiles, missing) = tally.per.get(r.id.as_str()).copied().unwrap_or((0, 0));
                let mut record = r.clone();
                if !owner && record.kind != StandingKind::Order {
                    record.text = "(private)".into();
                }
                let captured_by = match record.source.starts_with("capture:") {
                    true => "capture".to_string(),
                    false => String::new(),
                };
                StandingInfo {
                    repeats: held.week_of(&record.id, now) as u64,
                    captured_by,
                    record,
                    state: state.into(),
                    compiles,
                    missing,
                }
            })
            .collect();
        Ok(StandingListResult {
            records,
            compiles: tally.compiles,
            missing: tally.missing,
            asks,
        })
    }

    /// For each live record, the compilations of its scope since it began
    /// (each session's, read by the scan of its scope) and those that did
    /// not list it.
    fn standing_tally(&self, live: &[&StandingRecord]) -> Result<Tally> {
        let mut t = Tally::default();
        let Some(earliest) = live.iter().map(|r| r.since_ms).min() else {
            return Ok(t);
        };
        let mut counted = BTreeSet::new();
        for s in self.store.live_sessions::<SessionRecord>()? {
            if s.last_active_ms < earliest {
                continue;
            }
            let place = self.runner.target_of(&s.session_id);
            let persona = self.runner.persona_of(&s);
            let scope = Scope {
                session: &s.session_id,
                place: place.as_deref(),
                surface: None,
                persona: persona.as_deref(),
            };
            // A surface's records ride the request's tail, which no manifest
            // lists: the tally has nothing to count them against.
            let mine: Vec<&&StandingRecord> = live.iter().filter(|r| scope.holds(r)).collect();
            if mine.is_empty() {
                continue;
            }
            for c in self.store.session_compilations(&s.session_id)? {
                for r in &mine {
                    if c.created_at_ms < r.since_ms {
                        continue;
                    }
                    let e = t.per.entry(r.id.clone()).or_default();
                    e.0 += 1;
                    if !c.manifest.standing.iter().any(|u| u.id == r.id) {
                        e.1 += 1;
                        t.missing += 1;
                    }
                    counted.insert(c.id.clone());
                }
            }
        }
        t.compiles = counted.len() as u64;
        Ok(t)
    }

    /// The owner said a live record's words again, in `node` of `session`
    /// (theseus-slvl.4): a `standing.repeated` row, `{record, node}`, scoped
    /// to the session. A record that is not live is refused, so a count is
    /// only ever of records that held. The capture row calls it; nothing on a
    /// turn's path does.
    pub fn standing_repeated(&self, session: &str, record: &str, node: &str) -> Result<()> {
        let all = self.runner.standing.all(&self.store)?;
        let live = standing::live(&all, None, theseus_protocol::now_unix_ms());
        if !live.iter().any(|r| r.id == record) {
            bail!("no live standing record {record}");
        }
        self.rec(Some(session))
            .record(&fact::standing::StandingRepeated { record, node });
        Ok(())
    }

    /// `standing.add`: a record, alone or replacing a live one.
    pub fn standing_add(
        &self,
        p: &StandingAddParams,
        who: impl Into<Answerer>,
    ) -> Result<StandingChanged, RpcFailure> {
        let who = who.into();
        let what = format!("a standing {}", p.kind.name());
        self.judge_act(
            &who,
            Act::Standing {
                method: method::STANDING_ADD,
                what: &what,
            },
        )
        .map_err(failure)?;
        let now = theseus_protocol::now_unix_ms();
        let text = self.standing_text(p, now)?;
        let (who_s, via) = (who.who(), who.via());
        let mut same: Option<StandingRecord> = None;
        let written = self
            .runner
            .standing
            .write(
                &self.store,
                |all| {
                    let live = standing::live(all, None, now);
                    let old = match &p.replaces {
                        Some(id) => Some(resolve(&live, id)?.clone()),
                        None => None,
                    };
                    if let Some(r) = live.iter().find(|r| {
                        r.kind == p.kind
                            && r.scope == p.scope
                            && r.text == text
                            && old.as_ref().is_none_or(|o| o.id != r.id)
                    }) {
                        same = Some((*r).clone());
                        return Ok(vec![]);
                    }
                    let held = live
                        .iter()
                        .filter(|r| r.scope == p.scope && old.as_ref().is_none_or(|o| o.id != r.id))
                        .count();
                    if held >= standing::MAX_LIVE {
                        bail!(
                            "{} records already hold {}: revoke one first",
                            held,
                            p.scope.words()
                        );
                    }
                    Ok(vec![StandingRecord {
                        id: standing::mint(all),
                        kind: p.kind,
                        text: text.clone(),
                        scope: p.scope.clone(),
                        since_ms: now,
                        until_ms: p.until_ms,
                        source: p.source.clone().unwrap_or_else(|| via.clone()),
                        ends: old.map(|o| o.id),
                    }])
                },
                |_, records| {
                    records
                        .iter()
                        .map(|r| {
                            fact::row(
                                &fact::standing::StandingAdded {
                                    record: r,
                                    who: &who_s,
                                    via: &via,
                                },
                                None,
                                None,
                            )
                        })
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        if let Some(r) = same {
            return Ok(StandingChanged {
                record: r,
                ended: None,
            });
        }
        let record = written.into_iter().next().expect("one record");
        self.rec(None).announce(&fact::standing::StandingAdded {
            record: &record,
            who: &who_s,
            via: &via,
        });
        Ok(StandingChanged {
            ended: record.ends.clone(),
            record,
        })
    }

    /// The text of an add, read as the daemon keeps it (one line), once the
    /// add's other parts check: its length, its lapse, and its scope.
    fn standing_text(&self, p: &StandingAddParams, now: u64) -> Result<String, RpcFailure> {
        let text = p.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err(RpcFailure::invalid(anyhow!(
                "a standing record needs its text"
            )));
        }
        if text.chars().count() > standing::MAX_TEXT {
            return Err(RpcFailure::invalid(anyhow!(
                "a standing record is at most {} characters: it rides every prompt of its scope",
                standing::MAX_TEXT
            )));
        }
        if p.until_ms.is_some_and(|u| u <= now) {
            return Err(RpcFailure::invalid(anyhow!("`until` is already past")));
        }
        match &p.scope {
            theseus_protocol::StandingScope::Session(s) => {
                let known = self
                    .store
                    .get_session::<SessionRecord>(s)
                    .map_err(RpcFailure::invalid)?;
                if known.is_none() {
                    return Err(RpcFailure::invalid(anyhow!("no session {s}")));
                }
            }
            theseus_protocol::StandingScope::Surface(name)
                if !theseus_protocol::standing::SURFACES.contains(&name.as_str()) =>
            {
                return Err(RpcFailure::invalid(anyhow!(
                    "`{name}` is no surface: write {}",
                    theseus_protocol::standing::SURFACES.join(", ")
                )));
            }
            theseus_protocol::StandingScope::Persona(name) if !self.personas().exists(name) => {
                return Err(RpcFailure::invalid(anyhow!(
                    "`{name}` is no persona: {}",
                    match self.personas().names() {
                        known if known.is_empty() =>
                            "none is defined (`theseus persona add`)".into(),
                        known => format!("the personas are {known}"),
                    }
                )));
            }
            theseus_protocol::StandingScope::Place(pl) if !pl.starts_with("discord:") => {
                return Err(RpcFailure::invalid(anyhow!(
                    "a place scope names a bound place's target (`discord:dm:<user id>` or \
                     `discord:channel:<id>`, as `theseus places` lists them); the CLI and the web \
                     UI have none: use everywhere, or a session"
                )));
            }
            _ => {}
        }
        Ok(text)
    }

    /// `standing.revoke`: a record named by its id (or the start of it).
    pub fn standing_revoke(
        &self,
        p: &StandingRevokeParams,
        who: impl Into<Answerer>,
    ) -> Result<StandingChanged, RpcFailure> {
        let who = who.into();
        let what = format!("the revoke of {}", p.id);
        self.judge_act(
            &who,
            Act::Standing {
                method: method::STANDING_REVOKE,
                what: &what,
            },
        )
        .map_err(failure)?;
        let now = theseus_protocol::now_unix_ms();
        let (who_s, via) = (who.who(), who.via());
        let written = self
            .runner
            .standing
            .write(
                &self.store,
                |all| {
                    let live = standing::live(all, None, now);
                    let old = resolve(&live, &p.id)?;
                    Ok(vec![StandingRecord {
                        id: standing::mint(all),
                        kind: old.kind,
                        text: String::new(),
                        scope: old.scope.clone(),
                        since_ms: now,
                        until_ms: None,
                        source: via.clone(),
                        ends: Some(old.id.clone()),
                    }])
                },
                |_, records| {
                    records
                        .iter()
                        .map(|r| {
                            fact::row(
                                &fact::standing::StandingRevoked {
                                    record: r,
                                    who: &who_s,
                                    via: &via,
                                },
                                None,
                                None,
                            )
                        })
                        .collect()
                },
            )
            .map_err(RpcFailure::invalid)?;
        let record = written.into_iter().next().expect("one record");
        self.rec(None).announce(&fact::standing::StandingRevoked {
            record: &record,
            who: &who_s,
            via: &via,
        });
        // An undo of a record `directive.v1` kept is a wrong label on it.
        if let Some(ended) = &record.ends {
            self.capture_undone(ended, &who);
        }
        Ok(StandingChanged {
            ended: record.ends.clone(),
            record,
        })
    }
}

#[derive(Default)]
struct Tally {
    per: BTreeMap<String, (u64, u64)>,
    compiles: u64,
    missing: u64,
}

/// The live record `id` names: exactly, or by the start of it that no other
/// live record shares.
fn resolve<'a>(live: &[&'a StandingRecord], id: &str) -> Result<&'a StandingRecord> {
    let id = id.trim();
    if let Some(r) = live.iter().find(|r| r.id == id) {
        return Ok(r);
    }
    let starts: Vec<&&StandingRecord> = live
        .iter()
        .filter(|r| !id.is_empty() && r.id.starts_with(id))
        .collect();
    match starts.as_slice() {
        [r] => Ok(**r),
        [] => bail!("no live standing record {id}"),
        _ => bail!("{id} starts more than one live standing record: name more of it"),
    }
}

/// A write's error on the wire: a judged refusal is `REFUSED`.
pub(crate) fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a standing write from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
