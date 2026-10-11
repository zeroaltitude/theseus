//! Earned autonomy's methods and its weekly card (theseus-uttu). The pure
//! parts are `policy::earned` (the finder, the grant, the number); this is
//! what reads the ledger for them and writes what they decide:
//!
//! - `policy.earned`: the number (interrupts per active hour over four
//!   weeks, and what the grants would have saved), the grants, the open
//!   card; with `propose_now` a card now (the owner's act), with `show` one
//!   group's record.
//! - `policy.earned.answer`: yes (a grant: a loosening, so the owner's, from
//!   a private place, judged by `judge_act` as an untighten is) or not this
//!   one (the group is not proposed for 90 days).
//! - `policy.revoke`: a grant ends. It tightens, so anyone may.
//!
//! Each move is one frame: the state's META record, its ledger row
//! (`policy.earned_proposed`, `_granted`, `_passed`, `_revoked`) and, for a
//! card, the DM post. The card is posted by a tender after serving, beside
//! the nightly ones, on a thread at nice 19 (`learning::tender`), never on a
//! turn's path.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::{json, Value};
use theseus_protocol::earned::{
    EarnedCard, EarnedGrant, EarnedGroup, PolicyEarnedAnswerParams, PolicyEarnedAnswerResult,
    PolicyEarnedParams, PolicyEarnedResult, PolicyRevokeParams, PolicyRevokeResult,
};
use theseus_protocol::{error_code, method, LedgerKind};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::ledger::LedgerRow;
use crate::narrative::narrate;
use crate::policy::earned::find::{find, FindParams};
use crate::policy::earned::measure::{measure, WEEKS, WEEK_MS};
use crate::policy::earned::{day, Passed, FOREVER, PASS_MS};

/// The methods routed here.
pub(super) const OWN: [&str; 3] = [
    method::POLICY_EARNED,
    method::POLICY_EARNED_ANSWER,
    method::POLICY_REVOKE,
];

/// The most candidates one card carries.
pub const CARD_MAX: usize = 5;

/// The most rows of one kind a read takes (a bound, said when it cuts).
const ROWS_MAX: usize = 400_000;

/// A day before the window: a question's row is read when its answer is.
const LOOKBACK_MS: u64 = 2 * 86_400_000;

/// The rows of `kind` from `since_ms` on, oldest first, and whether the cap
/// cut them. `None` while the ledger's index is being built.
fn rows_of(core: &Core, kind: &str, since_ms: u64) -> Result<Option<(Vec<LedgerRow>, bool)>> {
    let mut out: Vec<LedgerRow> = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(kind)],
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: None,
            limit: 512,
        };
        let Some(got) = core.store.ledger_page(&page)? else {
            return Ok(None);
        };
        out.extend(
            got.records
                .iter()
                .filter_map(|r| r.decode::<LedgerRow>().ok())
                .filter(|r| r.at_unix_ms >= since_ms),
        );
        if out.len() >= ROWS_MAX {
            out.sort_by_key(|r| r.at_unix_ms);
            return Ok(Some((out, true)));
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    out.sort_by_key(|r| r.at_unix_ms);
    Ok(Some((out, false)))
}

/// The four kinds the finder reads, from `since_ms`, in time order.
fn finder_rows(core: &Core, since_ms: u64) -> Result<Option<Vec<LedgerRow>>> {
    let mut all = Vec::new();
    for k in [
        "tool.confirm_requested",
        "action.confirmed",
        "action.declined",
        "policy.tightened",
    ] {
        let Some((rows, _)) = rows_of(core, k, since_ms)? else {
            return Ok(None);
        };
        all.extend(rows);
    }
    all.sort_by_key(|r| r.at_unix_ms);
    Ok(Some(all))
}

impl Core {
    /// The candidates now, from the record: none while the ledger's index is
    /// being built (a card waits for it).
    pub fn earned_candidates(&self, now_ms: u64) -> Result<Option<Vec<EarnedGroup>>> {
        let c = &self.cfg.policy.earned;
        let window = c.window_days.max(1) * 86_400_000;
        let Some(rows) = finder_rows(self, now_ms.saturating_sub(window + LOOKBACK_MS))? else {
            return Ok(None);
        };
        let fresh = c.every_days.max(1) * 86_400_000;
        let skip = |id: &str| self.tools.earned.skips(id, now_ms, fresh);
        Ok(Some(find(
            &rows,
            &FindParams {
                now_ms,
                window_ms: window,
                min_approvals: c.min_approvals,
                roots: &self.tools.policy.roots,
                skip: &skip,
            },
        )))
    }

    /// When the next card may be posted: `every_days` after the last.
    pub fn earned_next_card_ms(&self) -> u64 {
        let last = self.tools.earned.snapshot().last_card_ms;
        match last {
            0 => 0,
            l => l.saturating_add(self.cfg.policy.earned.every_days.max(1) * 86_400_000),
        }
    }

    /// Post a card: the candidates (at most `CARD_MAX`), the state and its
    /// row `policy.earned_proposed`, and the DM post, one frame. `force` is
    /// the owner's `--propose-now`: it passes the week's limit. None: no
    /// candidate, nothing due, or no DM takes it (and not forced).
    pub fn earned_propose(&self, force: bool) -> Result<Option<EarnedCard>> {
        let c = &self.cfg.policy.earned;
        if !c.enabled {
            bail!("earned autonomy is off: [policy.earned] enabled = false");
        }
        let now = theseus_protocol::now_unix_ms();
        if !force && (now < self.earned_next_card_ms() || !self.runner.place_rule.owner_dm_bound())
        {
            return Ok(None);
        }
        let Some(mut found) = self.earned_candidates(now)? else {
            if force {
                bail!("the ledger's index is still being built after the start; ask again in a minute");
            }
            return Ok(None);
        };
        found.truncate(CARD_MAX);
        if found.is_empty() {
            return Ok(None);
        }
        let card = EarnedCard {
            proposed_ms: now,
            candidates: found,
            answered: Vec::new(),
        };
        let body = json!({"kind": "earned_card", "text": card_text(&card), "card": card});
        let (post, more) = self.outbox.stage_to_operator(None, body)?;
        let row = LedgerRow::new(
            LedgerKind::PolicyEarnedProposed,
            None,
            None,
            json!({"candidates": card.candidates.iter().map(|g| json!({
                "id": g.id, "tool": g.tool, "label": g.label, "approvals": g.record.approvals,
                "first_ms": g.record.first_ms, "last_ms": g.record.last_ms,
                "digest": g.record.digest})).collect::<Vec<_>>(),
                   "forced": force}),
        );
        let held = card.clone();
        let wrote = self
            .tools
            .earned
            .update(&self.store, &row, more, move |s| {
                s.last_card_ms = now;
                s.card = Some(held);
                true
            })?;
        if wrote {
            self.outbox.posted(&post);
            narrate!(
                self.narrator,
                Approval,
                None,
                None,
                "A card of {} earned grants went to the owner's DM.",
                card.candidates.len()
            );
        }
        Ok(Some(card))
    }

    /// The owner's answer to a candidate: yes makes the grant, no passes on
    /// the group for 90 days. Only a candidate on the open card is answered.
    pub fn earned_answer(
        &self,
        id: &str,
        accept: bool,
        who: &Answerer,
    ) -> Result<PolicyEarnedAnswerResult> {
        let state = self.tools.earned.snapshot();
        if let Some(g) = state.grants.iter().find(|g| g.group.id == id) {
            return Ok(PolicyEarnedAnswerResult {
                id: id.into(),
                accepted: true,
                grant: Some(g.clone()),
                already: true,
            });
        }
        let on_card = state
            .card
            .as_ref()
            .and_then(|c| c.candidates.iter().find(|g| g.id == id).cloned());
        let what = format!("the {} of {id}", if accept { "grant" } else { "pass" });
        self.judge_act(
            who,
            Act::Earned {
                method: method::POLICY_EARNED_ANSWER,
                what: &what,
                tightens: false,
            },
        )?;
        let Some(group) = on_card else {
            if state.passed.contains_key(id) && !accept {
                return Ok(PolicyEarnedAnswerResult {
                    id: id.into(),
                    accepted: false,
                    grant: None,
                    already: true,
                });
            }
            bail!(
                "{id} is on no open card, so there is nothing to answer: `theseus policy earned` \
                 shows the card, and `--propose-now` posts one"
            );
        };
        let now = theseus_protocol::now_unix_ms();
        let grant = EarnedGrant {
            group: group.clone(),
            by: who.label.clone(),
            who: who.who(),
            via: who.via(),
            at_ms: now,
        };
        let r = &group.record;
        let row = answer_row(&group, accept, who, now);
        let (g, wid, by, via) = (grant.clone(), id.to_string(), who.label.clone(), who.via());
        self.tools
            .earned
            .update(&self.store, &row, Vec::new(), move |s| {
                if accept {
                    s.grants.push(g);
                } else {
                    s.passed.insert(
                        wid.clone(),
                        Passed {
                            until_ms: now + PASS_MS,
                            why: format!("not this one, by {by} via {via}"),
                            at_ms: now,
                        },
                    );
                }
                if let Some(c) = s.card.as_mut() {
                    c.answered.push(wid);
                    if c.candidates.iter().all(|x| c.answered.contains(&x.id)) {
                        s.card = None;
                    }
                }
                true
            })?;
        if accept {
            narrate!(
                self.narrator,
                Approval,
                None,
                None,
                "{} now runs with a notice instead of asking: earned by {} approvals, {} to {}; {} gave it.",
                group.label,
                r.approvals,
                day(r.first_ms),
                day(r.last_ms),
                who.label
            );
        }
        Ok(PolicyEarnedAnswerResult {
            id: id.into(),
            accepted: accept,
            grant: accept.then_some(grant),
            already: false,
        })
    }

    /// End a grant. `press`: it was a "should have asked" on one of its
    /// notices, which counts as a decline from then on, so its group is
    /// never proposed again; else 90 days.
    pub fn earned_revoke(
        &self,
        id: &str,
        who: &Answerer,
        press: Option<&str>,
    ) -> Result<EarnedGrant> {
        let Some(g) = self
            .tools
            .earned
            .grants()
            .into_iter()
            .find(|g| g.group.id == id)
        else {
            bail!("{id} is no earned grant; `theseus policy` lists them");
        };
        let what = format!("the revoke of {id}");
        self.judge_act(
            who,
            Act::Earned {
                method: method::POLICY_REVOKE,
                what: &what,
                tightens: true,
            },
        )?;
        let now = theseus_protocol::now_unix_ms();
        let row = LedgerRow::new(
            LedgerKind::PolicyEarnedRevoked,
            None,
            None,
            json!({"id": id, "label": g.group.label, "digest": g.group.record.digest,
                   "by": who.label, "who": who.who(), "via": who.via(),
                   "should_have_asked": press.is_some(), "correlation_id": press,
                   "granted_ms": g.at_ms}),
        );
        let (wid, via, label) = (id.to_string(), who.via(), who.label.clone());
        let until = if press.is_some() {
            FOREVER
        } else {
            now + PASS_MS
        };
        let pressed = press.is_some();
        self.tools
            .earned
            .update(&self.store, &row, Vec::new(), move |s| {
                s.grants.retain(|x| x.group.id != wid);
                s.passed.insert(
                    wid,
                    Passed {
                        until_ms: until,
                        why: format!(
                            "{} by {label} via {via}",
                            if pressed {
                                "should have asked"
                            } else {
                                "revoked"
                            }
                        ),
                        at_ms: now,
                    },
                );
                true
            })?;
        narrate!(
            self.narrator,
            Approval,
            None,
            None,
            "{} asks first again: {} ended its earned grant.",
            g.group.label,
            who.label
        );
        Ok(g)
    }

    /// "Should have asked" on the notice of a call an earned grant covered:
    /// the grant ends instead of the tool tightening, and its group counts
    /// as declined. None: no grant's notice was that call's. A notified call
    /// keeps no proposal, so the notice's own row names the grant: its
    /// `setting` (`earned grant ern_…`), read from the session's newest.
    pub(crate) fn earned_press(
        &self,
        action: &theseus_kernel::Action,
        who: &Answerer,
    ) -> Result<Option<EarnedGrant>> {
        if !self.tools.earned.any() {
            return Ok(None);
        }
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![theseus_store::pages::ledger_kind_session(
                "tool.notified",
                &action.session_id,
            )],
            after: None,
            before: None,
            since_ms: None,
            until_ms: None,
            limit: 200,
        };
        let Some(got) = self.store.ledger_page(&page)? else {
            return Ok(None);
        };
        let setting = got
            .records
            .iter()
            .rev()
            .filter_map(|r| r.decode::<LedgerRow>().ok())
            .find(|r| r.data["correlation_id"] == action.correlation_id.as_str())
            .and_then(|r| r.data["setting"].as_str().map(str::to_string));
        let Some(id) = setting
            .as_deref()
            .and_then(crate::policy::earned::grant_of_setting)
        else {
            return Ok(None);
        };
        if !self.tools.earned.grants().iter().any(|g| g.group.id == id) {
            return Ok(None);
        }
        self.earned_revoke(id, who, Some(&action.correlation_id))
            .map(Some)
    }

    /// "Should have asked" on a call an earned grant's notice reported: the
    /// grant ends (`earned_press`) and the tool keeps the posture the config
    /// gave it, answered as a press is. None: it was no grant's notice.
    pub(super) fn tighten_by_earned(
        &self,
        tool: &str,
        call: Option<&theseus_kernel::Action>,
        who: &Answerer,
    ) -> Result<Option<theseus_protocol::TightenResult>> {
        let Some(a) = call else { return Ok(None) };
        let Some(g) = self.earned_press(a, who)? else {
            return Ok(None);
        };
        let now = self.tools.posture_now(tool);
        let t = theseus_protocol::Tightening {
            tool: tool.into(),
            posture: now.posture.as_str().into(),
            by: who.label.clone(),
            who: who.who(),
            via: who.via(),
            at_ms: theseus_protocol::now_unix_ms(),
            correlation_id: Some(a.correlation_id.clone()),
            session_id: Some(a.session_id.clone()),
            digest: Some(a.args_digest.clone()),
        };
        let mut r = super::policy::result(tool, &who.label, t, now, true, false);
        r.setting = format!(
            "earned grant {} ended: {} asks again",
            g.group.id, g.group.label
        );
        Ok(Some(r))
    }

    /// The number, the grants, the card; the owner's `propose_now`; `show`.
    pub fn earned_read(
        &self,
        p: &PolicyEarnedParams,
        who: &Answerer,
    ) -> Result<PolicyEarnedResult> {
        let now = theseus_protocol::now_unix_ms();
        let state = self.tools.earned.snapshot();
        let mut out = PolicyEarnedResult {
            enabled: self.cfg.policy.earned.enabled,
            grants: state.grants.clone(),
            card: state.card.clone(),
            next_card_ms: self
                .earned_next_card_ms()
                .max(if self.cfg.policy.earned.enabled {
                    0
                } else {
                    u64::MAX
                }),
            ..Default::default()
        };
        if p.propose_now {
            let what = "a card of earned grants, now".to_string();
            self.judge_act(
                who,
                Act::Earned {
                    method: method::POLICY_EARNED,
                    what: &what,
                    tightens: false,
                },
            )?;
            out.proposed = self.earned_propose(true)?;
            if out.proposed.is_none() {
                out.note = Some(format!(
                    "no card: nothing was approved at least {} times in {} days without a decline",
                    self.cfg.policy.earned.min_approvals, self.cfg.policy.earned.window_days
                ));
            }
            out.card = self.tools.earned.snapshot().card;
            out.next_card_ms = self.earned_next_card_ms();
        }
        if let Some(id) = &p.show {
            out.shown = state
                .grants
                .iter()
                .map(|g| &g.group)
                .chain(out.card.iter().flat_map(|c| c.candidates.iter()))
                .find(|g| &g.id == id)
                .cloned()
                .or_else(|| {
                    self.earned_candidates(now)
                        .ok()
                        .flatten()
                        .and_then(|v| v.into_iter().find(|g| &g.id == id))
                });
            if out.shown.is_none() {
                bail!(
                    "{id} is no grant, no candidate on the card, and no group the record holds now"
                );
            }
        }
        if p.brief {
            return Ok(out);
        }
        let since = now.saturating_sub(WEEKS * WEEK_MS);
        let (confirms, turns) = (
            rows_of(self, "tool.confirm_requested", since)?,
            rows_of(self, "turn.started", since)?,
        );
        let (Some((confirms, cut)), Some((turns, _))) = (confirms, turns) else {
            out.note = Some(
                "the ledger's index is still being built after the start: the number waits for it"
                    .into(),
            );
            return Ok(out);
        };
        let turn_times: Vec<u64> = turns.iter().map(|r| r.at_unix_ms).collect();
        let m = measure(&confirms, &turn_times, &state.grants, now);
        out.weeks = m.weeks;
        out.interrupts = m.interrupts;
        out.active_hours = m.active_hours;
        out.per_hour = m.per_hour;
        out.saved = m.saved;
        out.per_hour_with_grants = m.per_hour_with_grants;
        if cut {
            out.note = Some(format!(
                "the questions of the four weeks were cut at {ROWS_MAX} rows"
            ));
        }
        Ok(out)
    }

    /// The card's tender, after serving: once a day at the learning hour (a
    /// missed day once, never within ten minutes of a start), it asks
    /// whether a card is due and posts one, on a thread at nice 19.
    pub fn earned_after_serving(self: &Arc<Self>) {
        if !self.cfg.policy.earned.enabled {
            return;
        }
        let core = Arc::downgrade(self);
        let started = tokio::time::Instant::now();
        tokio::spawn(async move {
            let ran = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let here = ran_at.load(std::sync::atomic::Ordering::Relaxed);
                let now = theseus_protocol::now_unix_ms();
                let (at, trigger) = crate::learning::tender::due(
                    now,
                    c.cfg.judge.learning_hour,
                    (here > 0).then_some(here),
                );
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |_trigger: &'static str| {
                let core = core.clone();
                ran.store(
                    theseus_protocol::now_unix_ms(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                async move {
                    let Some(c) = core.upgrade() else { return };
                    // Nothing to read until a card is due: no ledger read most days.
                    if theseus_protocol::now_unix_ms() < c.earned_next_card_ms() {
                        return;
                    }
                    let worker = c.clone();
                    drop(c);
                    let Ok(rx) = crate::learning::tender::on_idle_thread(move || {
                        worker.earned_propose(false)
                    }) else {
                        return;
                    };
                    match rx.await {
                        Ok((_, Ok(Some(card)))) => tracing::info!(
                            candidates = card.candidates.len(),
                            "earned autonomy: a card was posted"
                        ),
                        Ok((_, Ok(None))) => {}
                        Ok((_, Err(e))) => {
                            tracing::warn!(error = %format!("{e:#}"), "earned autonomy: no card was posted")
                        }
                        Err(_) => {}
                    }
                }
            };
            crate::learning::tender::tend(started, next, run).await;
        });
    }

    pub(super) fn rpc_earned(
        &self,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let params = super::server::or_empty(params);
        match m {
            method::POLICY_EARNED => route(params, |p: PolicyEarnedParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                theseus_store::blocking(|| self.earned_read(&p, &who)).map_err(refused("the card"))
            }),
            method::POLICY_EARNED_ANSWER => route(params, |p: PolicyEarnedAnswerParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                theseus_store::blocking(|| self.earned_answer(&p.id, p.accept, &who))
                    .map_err(refused("the answer"))
            }),
            _ => route(params, |p: PolicyRevokeParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                let grant = self
                    .earned_revoke(&p.id, &who, None)
                    .map_err(refused("the revoke"))?;
                Ok(PolicyRevokeResult { id: p.id, grant })
            }),
        }
    }
}

/// The ledger row of an answer: the grant with its record's digest, or the pass.
fn answer_row(group: &EarnedGroup, accept: bool, who: &Answerer, now: u64) -> LedgerRow {
    let r = &group.record;
    let (kind, data) = if accept {
        (
            LedgerKind::PolicyEarnedGranted,
            json!({"id": group.id, "tool": group.tool, "prefix": group.prefix, "root": group.root,
                   "label": group.label, "approvals": r.approvals, "first_ms": r.first_ms,
                   "last_ms": r.last_ms, "sessions": r.sessions, "digest": r.digest,
                   "by": who.label, "who": who.who(), "via": who.via()}),
        )
    } else {
        (
            LedgerKind::PolicyEarnedPassed,
            json!({"id": group.id, "label": group.label, "digest": r.digest,
                   "until_ms": now + PASS_MS, "by": who.label, "who": who.who(), "via": who.via()}),
        )
    };
    LedgerRow::new(kind, None, None, data)
}

/// A refusal is `REFUSED` with who, via and why; anything else is the caller's.
fn refused(what: &'static str) -> impl Fn(anyhow::Error) -> RpcFailure {
    move |e| match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "{what} from {} does not count: {}. Nothing changed.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

/// The card's words, for the DM and the CLI: each candidate with its record,
/// and the answers.
pub fn card_text(card: &EarnedCard) -> String {
    let mut t = format!(
        "Earned autonomy: {} thing{} you approve every time.\n",
        card.candidates.len(),
        if card.candidates.len() == 1 { "" } else { "s" }
    );
    for (i, g) in card.candidates.iter().enumerate() {
        let r = &g.record;
        let after = if g.open_ended && g.root.is_none() {
            " (and what follows it)"
        } else {
            ""
        };
        t.push_str(&format!(
            "{}. `{}`{after}: you approved all {} since {}, never declined. Let it run with a notice?\n   e.g. `{}`\n   yes: `theseus policy earned accept {}` · not this one: `theseus policy earned pass {}` · the record: `theseus policy earned show {}`\n",
            i + 1,
            g.label,
            r.approvals,
            day(r.first_ms),
            r.samples.first().map_or("", String::as_str),
            g.id, g.id, g.id
        ));
    }
    t
}
