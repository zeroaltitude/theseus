//! `pack.list`, `pack.promote` and `pack.rollback` (M5 26a; design §2.7,
//! §2.13): the ladder as the operator sees and moves it.
//!
//! - **Moves are the owner's**, from a private place: `judge_act(Act::Ladder)`
//!   judges each one, and the CLI refuses them inside a job (`OPERATORS`).
//!   A job's process that could roll a pack back could silence security's
//!   notices; one that could promote could make Jev act.
//! - **The owner may promote at any time.** Short of the bar
//!   (`ladder::promote`), his row says `forced`, with the numbers. An
//!   automatic promotion (`promote_automatic`, the learning loop's) is
//!   refused one short of the bar.
//! - **A `security.*` promotion is an approval card**, his or automatic: a
//!   question answered as every question is (`action.confirm`: `theseus
//!   confirm`, Discord's buttons, the cockpit), judged by `judge_act`.
//!   Approval writes the mode; a decline or no answer writes a declined row
//!   and no mode.
//!
//! **Where the card's question lives.** A question is a planned action on an
//! execution, and a promotion has none: the owner's CLI and cockpit open no
//! session. Its question is planned on the ladder's own session, `the
//! ladder` (META `ladder.session`), opened once, at the first such card, as
//! a conversation whose execution waits on input: each card takes one short
//! turn on it to plan its question (`plan_confirm_with`) and parks it again,
//! so the question is listed and expires as any other (`confirm.list` reads
//! parked executions) and its answer is the kernel's bind or decline. It
//! never runs a model turn: nothing sends it input, and an answer wakes
//! nothing, as a proposed extension's ack does not (`extend::answer`). A
//! session of its own keeps the cards off every conversation's transcript
//! and its spend, and a card from the CLI needs no session to name.

use std::sync::atomic::Ordering::SeqCst;

use anyhow::{bail, Context as _, Result};
use serde_json::json;
use theseus_kernel::{Action, RetryClass, TurnEnd, Wake};
use theseus_protocol::error_code;
use theseus_protocol::packs::{
    PackInfo, PackListResult, PackModeRow, PackPromoteParams, PackPromoteResult, PackRollbackParams,
};
use theseus_protocol::{ConfirmRequest, SessionOpenParams};

use super::server::{Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::judge::ladder::{self, promote, rules, Rung};
use crate::outbox::Closed;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;

/// The tool a promotion's card asks under.
pub const PROMOTE: &str = "pack.promote";

/// The META key naming the ladder's session.
pub const LADDER_SESSION: &str = "ladder.session";

/// The rows `pack.list` gives per pack.
const LAST_ROWS: usize = 5;

/// A promotion as asked, before the bar.
struct Ask {
    pack: String,
    to: Rung,
    share: Option<f64>,
}

/// A promotion, its pack known (compiled in, or learned: 25f). A learned
/// version may also go to `shadow`: in its root's place, recording.
fn ask_of(p: &PackPromoteParams, known: bool, learned: bool) -> Result<Ask> {
    if !known {
        bail!(
            "this build has no pack {} (packs are named as `loop.v1`)",
            p.pack
        );
    }
    let to = match p.to.as_str() {
        "canary" => Rung::Canary,
        "live" => Rung::Live,
        "shadow" if learned => Rung::Shadow,
        other => bail!("a promotion goes to `canary` or `live`, not {other:?}"),
    };
    let share = match (to, p.share) {
        (Rung::Canary, Some(s)) if s > 0.0 && s <= 1.0 => Some(s),
        (Rung::Canary, _) => bail!("a canary takes a share above 0 and at most 1 (--canary 0.2)"),
        (_, Some(_)) => bail!("live takes no share: every session acts"),
        (_, None) => None,
    };
    Ok(Ask {
        pack: p.pack.clone(),
        to,
        share,
    })
}

fn what(a: &Ask) -> String {
    format!(
        "{} to {}",
        a.pack,
        crate::fact::ladder::mode_words(a.to.as_str(), a.share)
    )
}

/// Whether a pack's promotion is a card: every `security.*` version.
pub fn needs_card(pack: &str) -> bool {
    ladder::id_of(pack) == "security"
}

fn refused(e: anyhow::Error, what: &str) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "{what} from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    /// The ladder's first read, after serving (never on the start path), on
    /// the blocking pool: the adoptions it lacks are written then. Nothing
    /// with the judge off.
    pub fn warm_ladder(self: &std::sync::Arc<Self>) {
        if !self.runner.judge.config().enabled {
            return;
        }
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            if let Some(core) = core.upgrade() {
                core.runner.judge.ladder().standing(crate::judge::LOOP_PACK);
                // The learned versions (25f), and their files from their
                // rows.
                core.runner.judge.write_pack_files(&core.cfg.state_dir());
            }
        });
    }

    /// `pack.list`: every pack this build wires, on the ladder.
    pub fn pack_list(&self) -> PackListResult {
        let cfg = self.runner.judge.config();
        let j = &self.runner.judge;
        let l = j.ladder();
        let info = |pack: String, wired: crate::config::PackMode| {
            let s = l.standing(&pack);
            let rows = l.rows_of(&pack);
            let rows = rows[rows.len().saturating_sub(LAST_ROWS)..].to_vec();
            PackInfo {
                acts: cfg.mode_of(&pack, s.rung.acts_as()).as_str().into(),
                mode: s.rung.as_str().into(),
                share: s.share,
                wired: wired.as_str().into(),
                why: s.words(),
                until_ms: s.until_ms.filter(|_| s.rung == Rung::RolledBack),
                rules: rules::rules_of(&pack)
                    .iter()
                    .map(|r| r.name().to_string())
                    .collect(),
                rows,
                source: "compiled".into(),
                parent: None,
                root: None,
                standing: j.placed(&j.root_of(&pack), "") == pack,
                text: theseus_judge::pack::EMBEDDED
                    .iter()
                    .find(|(n, _)| *n == pack)
                    .map(|(_, t)| (*t).to_string())
                    .unwrap_or_default(),
                pack,
            }
        };
        let mut packs: Vec<PackInfo> = l
            .wired_packs()
            .into_iter()
            .map(|(pack, wired)| info(pack, wired))
            .collect();
        // Learned versions (25f): off until a row places them.
        for v in j.lineage().all(&self.store) {
            let mut p = info(v.name(), crate::config::PackMode::Off);
            if l.rows_of(&p.pack).iter().all(|r| r.declined) {
                p.mode = "off".into();
                p.acts = "off".into();
                p.why = format!("learned; not placed ({})", v.proposal);
            }
            p.source = "learned".into();
            p.parent = Some(v.parent.clone());
            p.root = Some(v.root.clone());
            p.text = v.text.clone();
            packs.push(p);
        }
        PackListResult {
            enabled: cfg.enabled,
            max_mode: cfg.max_mode.as_str().into(),
            packs,
        }
    }

    /// `pack.promote`: the owner's move up. Short of the bar it is forced,
    /// with the numbers; a security pack's is a card.
    pub fn pack_promote(
        &self,
        p: &PackPromoteParams,
        who: impl Into<Answerer>,
    ) -> Result<PackPromoteResult> {
        let who = who.into();
        let ask = self.ask_of(p)?;
        self.judge_act(
            &who,
            Act::Ladder {
                method: theseus_protocol::method::PACK_PROMOTE,
                what: &what(&ask),
            },
        )?;
        let row = self.promotion_row(&ask, p.report.as_deref(), "owner", &who)?;
        self.promote_with(row)
    }

    /// An automatic promotion (the learning loop's, theseus-0j2.12): refused
    /// one short of the bar, with the numbers ("work_state: labeled 37 of
    /// 200"). A security pack's is the owner's card all the same.
    pub fn promote_automatic(&self, p: &PackPromoteParams) -> Result<PackPromoteResult> {
        let ask = self.ask_of(p)?;
        let system = Answerer {
            label: rules::SYSTEM.into(),
            surface: crate::approval::Surface::Cli,
            discord: None,
        };
        let row = self.promotion_row(&ask, p.report.as_deref(), rules::SYSTEM, &system)?;
        if row.forced {
            bail!(
                "{} refused: short of the bar: {}",
                what(&ask),
                row.numbers.as_deref().unwrap_or("no qualifying report")
            );
        }
        self.promote_with(row)
    }

    fn ask_of(&self, p: &PackPromoteParams) -> Result<Ask> {
        let j = &self.runner.judge;
        let learned = theseus_judge::pack::by_name(&p.pack).is_none();
        ask_of(p, j.pack(&p.pack).is_some(), learned)
    }

    /// A learned version's move (25f), the learning loop's: through the
    /// ladder's own act, citing its proposal. Where 26a's bar would refuse
    /// it (no report of the new version, or one short of the minimum), the
    /// proposal's replay is its evidence: the row is the system's, never
    /// `forced`, its report the proposal, its numbers the replay's. A
    /// security version's is the owner's card all the same.
    pub fn promote_learned(
        &self,
        pack: &str,
        to: &str,
        share: Option<f64>,
        proposal: &str,
        why: &str,
        numbers: &str,
    ) -> Result<PackPromoteResult> {
        let ask = self.ask_of(&PackPromoteParams {
            pack: pack.into(),
            to: to.into(),
            share,
            ..PackPromoteParams::default()
        })?;
        let l = self.runner.judge.ladder();
        let s = l.standing(&ask.pack);
        let row = PackModeRow {
            pack: ask.pack.clone(),
            mode: ask.to.as_str().into(),
            from: s.rung.as_str().into(),
            share: ask.share,
            who: rules::SYSTEM.into(),
            by: "learning".into(),
            via: "learning".into(),
            why: format!("learned ({proposal}): {why}"),
            forced: false,
            numbers: Some(numbers.into()),
            report: Some(proposal.into()),
            ..PackModeRow::default()
        };
        self.promote_with(row)
    }

    /// The row a promotion writes, the bar read: `forced` with the numbers
    /// when short of it.
    fn promotion_row(
        &self,
        ask: &Ask,
        report: Option<&str>,
        who_word: &str,
        who: &Answerer,
    ) -> Result<PackModeRow> {
        let l = self.runner.judge.ladder();
        let s = l.standing(&ask.pack);
        let since = s.rolled_back_at.filter(|_| s.rung == Rung::RolledBack);
        let bar = promote::bar(&self.store, &ask.pack, report, since, l.now());
        let why = match (&bar.report, bar.cleared()) {
            (Some(r), true) => format!("{r} clears the bar"),
            _ => format!("forced by the {who_word}"),
        };
        Ok(PackModeRow {
            pack: ask.pack.clone(),
            mode: ask.to.as_str().into(),
            from: s.rung.as_str().into(),
            share: ask.share,
            who: who_word.into(),
            by: who.who(),
            via: who.via(),
            why,
            forced: !bar.cleared(),
            numbers: bar.short,
            report: bar.report,
            holdout: bar.holdout,
            ..PackModeRow::default()
        })
    }

    /// Write the row, or for a security pack ask its card.
    fn promote_with(&self, row: PackModeRow) -> Result<PackPromoteResult> {
        if needs_card(&row.pack) {
            let q = self.ask_promotion(&row)?;
            return Ok(PackPromoteResult {
                row: None,
                said: format!(
                    "{}'s promotion to {} is the owner's card: `theseus confirm {q} --approve` \
                     writes it. Nothing is written until then.",
                    row.pack,
                    crate::fact::ladder::mode_words(&row.mode, row.share)
                ),
                question: Some(q),
            });
        }
        let row = self.runner.judge.ladder().write(row)?;
        Ok(PackPromoteResult {
            said: format!(
                "{} is {}{}.",
                row.pack,
                crate::fact::ladder::mode_words(&row.mode, row.share),
                if row.forced {
                    format!(
                        ", forced short of the bar ({})",
                        row.numbers.as_deref().unwrap_or("")
                    )
                } else {
                    String::new()
                }
            ),
            row: Some(row),
            question: None,
        })
    }

    /// `pack.rollback`: the owner moves a version down to `rolled_back`,
    /// where it stands until a promotion.
    pub fn pack_rollback(
        &self,
        p: &PackRollbackParams,
        who: impl Into<Answerer>,
    ) -> Result<PackModeRow> {
        let who = who.into();
        if self.runner.judge.pack(&p.pack).is_none() {
            bail!("this build has no pack {}", p.pack);
        }
        self.judge_act(
            &who,
            Act::Ladder {
                method: theseus_protocol::method::PACK_ROLLBACK,
                what: &format!("{} rolled back", p.pack),
            },
        )?;
        let l = self.runner.judge.ladder();
        let s = l.standing(&p.pack);
        let why = p
            .why
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .unwrap_or(if p.off {
                "the owner rejected it"
            } else {
                "the owner rolled it back"
            });
        let to = if p.off { Rung::Off } else { Rung::RolledBack };
        l.write(PackModeRow {
            pack: p.pack.clone(),
            mode: to.as_str().into(),
            from: s.rung.as_str().into(),
            share: s.share,
            who: "owner".into(),
            by: who.who(),
            via: who.via(),
            why: why.into(),
            words: Some(why.into()),
            ..PackModeRow::default()
        })
    }

    /// The ladder's three methods (`pack.*`), from one arm of `dispatch`.
    pub(super) fn rpc_packs(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        let out = match name {
            theseus_protocol::method::PACK_LIST => serde_json::to_value(self.pack_list()),
            theseus_protocol::method::PACK_PROMOTE => {
                serde_json::to_value(self.rpc_pack_promote(super::server::parse(params)?, conn)?)
            }
            theseus_protocol::method::PACK_ROLLBACK => {
                serde_json::to_value(self.rpc_pack_rollback(super::server::parse(params)?, conn)?)
            }
            other => {
                return Err(RpcFailure::new(
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown method {other:?}"),
                ))
            }
        };
        out.map_err(|e| RpcFailure::invalid(e.into()))
    }

    pub(super) fn rpc_pack_promote(
        &self,
        p: PackPromoteParams,
        conn: Conn<'_>,
    ) -> Result<PackPromoteResult, RpcFailure> {
        let who = conn.answerer(None, None);
        self.pack_promote(&p, who)
            .map_err(|e| refused(e, "a promotion"))
    }

    pub(super) fn rpc_pack_rollback(
        &self,
        p: PackRollbackParams,
        conn: Conn<'_>,
    ) -> Result<PackModeRow, RpcFailure> {
        let who = conn.answerer(None, None);
        self.pack_rollback(&p, who)
            .map_err(|e| refused(e, "a rollback"))
    }

    /// The ladder's session, opened at its first card.
    fn ladder_session(&self) -> Result<SessionRecord> {
        if let Some(id) = self.store.get_meta::<String>(LADDER_SESSION)? {
            if let Some(rec) = self.store.get_session::<SessionRecord>(&id)? {
                let open = rec
                    .execution_id
                    .as_deref()
                    .and_then(|x| self.kernel.execution(x).ok().flatten())
                    .is_some_and(|e| !e.state.is_terminal());
                if open {
                    return Ok(rec);
                }
            }
        }
        let rec = self.open_session(SessionOpenParams {
            kind: None,
            label: Some("the ladder: promotions waiting on the owner".into()),
            opened_from: None,
        })?;
        self.store.put_meta(LADDER_SESSION, &rec.session_id)?;
        Ok(rec)
    }

    /// A security pack's card: its question planned on the ladder's
    /// session, in one short turn that parks it again. Returns the
    /// question's id.
    fn ask_promotion(&self, row: &PackModeRow) -> Result<String> {
        let rec = self.ladder_session()?;
        let exec = rec
            .execution_id
            .clone()
            .context("the ladder's session has no execution")?;
        let mode = crate::fact::ladder::mode_words(&row.mode, row.share);
        let question = format!(
            "Promote {} to {mode}?{} A security pack moves up only with your approval.",
            row.pack,
            match (&row.forced, &row.numbers) {
                (true, Some(n)) => format!(" It is short of the bar ({n}): it would be forced."),
                (true, None) => " It is short of the bar: it would be forced.".into(),
                _ => String::new(),
            }
        );
        let proposal = theseus_protocol::Proposal {
            tool: PROMOTE.into(),
            args: json!({"row": row, "question": question, "pack": row.pack, "to": mode}),
            resource: Some(ladder::scope(&row.pack)),
            policy_context: json!({}),
        };
        let guard = self.kernel.admit_input(&exec)?;
        let planned = self.kernel.plan_confirm_with(
            &guard,
            &proposal,
            RetryClass::NonRepeatable,
            None,
            |_| Ok(vec![]),
        );
        let ended = self
            .kernel
            .end_turn(guard, TurnEnd::Wait { wake: Wake::Input });
        let a = planned?;
        ended?;
        let ttl = self.kernel.config().confirm_ttl_ms;
        self.tools
            .question_due
            .fetch_min(a.planned_at_ms + ttl, SeqCst);
        self.session_rec(&rec.session_id)
            .record(&fact::tool::CallAsked {
                request: &promotion_request(&a, &rec, ttl),
            });
        Ok(a.correlation_id)
    }

    /// The answer to a promotion's card (`action.confirm`, judged by the
    /// place rule before it gets here), or its expiry: approval writes the
    /// mode, a decline or no answer a declined row. One frame: the bind or
    /// the decline, the answer's row, and the `pack.mode` row. Nothing
    /// wakes.
    pub(crate) fn answer_promotion(
        &self,
        a: &Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let proposal = a.proposal.clone().unwrap_or_default();
        let mut row: PackModeRow = serde_json::from_value(proposal.args["row"].clone())
            .context("the card names no promotion")?;
        let corr = a.correlation_id.as_str();
        row.question = Some(corr.into());
        row.by = by.into();
        row.via = via.into();
        if approve {
            row.who = "owner".into();
            row.why = format!("{} (approved)", row.why);
        } else {
            row.declined = true;
            row.why = note.unwrap_or("the owner declined").into();
        }
        let l = self.runner.judge.ladder();
        let record = l.row_record(&row)?;
        let answered = fact::answer::CallAnswered {
            action: a,
            approve,
            note,
            by,
            via,
            trust: false,
        };
        let session = Some(a.session_id.as_str());
        let rows = vec![fact::row(&answered, session, None)?, record];
        self.kernel.frame(&[&a.execution_id], |k| {
            match approve {
                true => {
                    k.bind_confirm(corr, OPERATOR, &proposal)?;
                }
                false => {
                    k.decline_action(corr, by, note.unwrap_or("the owner declined"))?;
                }
            }
            k.stage(&rows)
        })?;
        // Read again from the store, now: the row rode in the answer's frame.
        l.reload();
        l.say(&row);
        self.session_rec(&a.session_id).announce(&answered);
        let closed = match (approve, by) {
            (true, _) => Closed::new("approved", Some(by)),
            (false, super::confirms::EXPIRY) => Closed {
                note: note.map(str::to_string),
                ..Closed::new("expired", None)
            },
            (false, _) => Closed::new("declined", Some(by)),
        };
        self.card_closed(corr, closed);
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: corr.into(),
            approved: approve,
            session_id: a.session_id.clone(),
            execution_id: a.execution_id.clone(),
            resumes: false,
        })
    }
}

/// A promotion's card as the operator sees it: its card, `confirm.list`.
pub fn promotion_request(a: &Action, session: &SessionRecord, ttl_ms: u64) -> ConfirmRequest {
    let args = a
        .proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default();
    ConfirmRequest {
        correlation_id: a.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: a.execution_id.clone(),
        tool: PROMOTE.into(),
        reason: args["question"].as_str().unwrap_or_default().to_string(),
        input: args,
        resource: a.resource.clone(),
        by: OPERATOR.into(),
        requested_at_ms: a.planned_at_ms,
        expires_at_ms: a.planned_at_ms + ttl_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        external_text: None,
    }
}
