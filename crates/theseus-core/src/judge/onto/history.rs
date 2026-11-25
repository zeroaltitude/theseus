//! The owner's history filed under topics, once (issue 46; `theseus ledger
//! topics`, the owner's act in person, `rpc/exchanges.rs`). Topics touch an
//! exchange only for messages since the build that added them
//! (`names::topics`), so a topic's timeline began then. This reads the
//! exchanges from before, oldest first, and asks the ontology's judge the
//! live point's question once about each one's opening message.
//!
//! - **Which exchanges**: those that touch no category of a kind `onto` asks,
//!   were opened by the owner (the exchange's person is `owner`, as his mark
//!   or the rebuild proved it: a guest's message in a shared place is never
//!   sent, nor one whose person no record proves), were not asked by this
//!   job before, open with his own typed message with words (never a task's,
//!   never Theseus's own, never a slash command, never an imported or an
//!   erased one), and whose session `onto` acts on now (`[ontology] judge`
//!   on, the pack live on the ladder): the live point writes touches only
//!   then. A message he retracted and did not erase is kept: a retraction
//!   says a fact was wrong, and a touch says only what the exchange was about.
//!   What is not his is counted as someone else's only when a person's
//!   message opened it; of those, the ones whose author no record proves (no
//!   person, no mark) are counted apart, with how many are where `onto` does
//!   not act now. Counts only: none of them is asked.
//! - **His declaration** (issue 51; `include_unmarked`): no mark proves
//!   who typed a message from before marks were recorded, so none of his
//!   older history is proven his. A run that carries his declaration counts
//!   as his an exchange whose opening message has no mark at all and
//!   arrived, by the place code stored on it as it arrived
//!   (`surface::code_of`), in one of his own places: his terminal (its ask
//!   and its TUI), the web UI, his DM, a channel bound private. Never a
//!   shared channel, voice, a Discord place no record names, or a message
//!   with no recorded place (from before places were), which is counted
//!   apart; never a message whose mark proves someone else. The exchange's
//!   stored person is not written (its words stay "author unknown"
//!   wherever they are shown); it is asked as a private place's, and its
//!   row's context says `declared: unmarked`, so what the declaration filed
//!   can be found and undone. Only a run that carries the declaration
//!   writes a declared answer's touches again from its row; the undo
//!   (`undo_declared`, `history_undo.rs`) ends every touch they came to.
//! - **Touches only.** The ask goes in shadow, so the sink writes its row
//!   and nothing else: no membership, no category minted, no card, no
//!   `still_*` answer acted on. The job writes the touches itself, with the
//!   live path's own writer (`names::topics::touches`) and no plan, so a
//!   `new` answer names nothing; each touch takes its exchange's own time.
//!   Its categories are read as the ontology stands when the touch is
//!   written, under the ontology writers' lock (a category retired while the
//!   call was out gets none), and whether `onto` acts on the session is
//!   asked again before each call, and before any answer's touches once the
//!   writer's turn has come.
//! - **The state** is the live point's (`inbound::input` and `onto_input`)
//!   over the opening message alone: no message before it, no reply, no
//!   attachment. Its text goes to Jev through the judge's one client and its
//!   scrubber, and nowhere else: the row's context leaves out the names
//!   mined from it and the candidate projects' titles (their ids stay), no
//!   state blob is kept, and nothing here logs it or puts it in an error.
//!   Nor does a word Jev or its server chose reach a row:
//!   the judgment is recorded only once [`withheld`] has kept its class and
//!   its accounting and dropped the rest (an error's message, a malformed
//!   answer's reason, a model that is not the pinned one, the rate headers).
//! - **Resume** without a record of its own: each judgment's context says
//!   `purpose: history` (an additive key of the `judge.call` row's free
//!   data), and an exchange whose answered judgment says so is not asked
//!   again; its touches are written again from that row where missing.
//! - **Once**: one run at a time (a second is refused, [`Busy`], and asks
//!   nothing), and each answer's row is written by the sink before the next
//!   exchange is asked and before the run returns ([`Core::history_written`]),
//!   so the next run's plan reads every answer, one that touched nothing
//!   included. A run that returns before its answer's row is written (its
//!   wait ran out, its touches failed, the daemon is stopping) leaves that
//!   answer known to the judge until the row is written, queued or in a
//!   frame the sink has taken: a new run waits a little for it, and is
//!   refused if it is still not written. What no run can promise: a daemon
//!   crash, or an ordinary stop, while a call is out or its answer is not
//!   yet written may pay for that one exchange again.
//! - **As it stands** when touches are written: after the wait for a moment
//!   between turns, which a stop ends, whether `onto` acts on the session
//!   and whether the daemon is stopping are asked again, for a fresh answer
//!   and a recorded one.
//! - **Spend**: each exchange's worst case is reserved before its call
//!   against the judge's day budget, which holds on the daemon's day ceiling,
//!   as the people backfill's judgments are; the run stops before an exchange
//!   whose worst case would pass its cap (`cap_usd`, default $10); each call
//!   settles at its price. Paced by the machine's quiet between exchanges;
//!   the daemon's stop is looked for there. A dry run reads and prices, and
//!   asks nothing: its figure is the worst case of every exchange it would
//!   ask, which a run's cap may stop short of; and where it cannot price one
//!   (the judge's model has no price, its client is not built) it says so
//!   and fails, never a figure of zero.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::json;
use theseus_judge::builders::ONTO_KINDS;
use theseus_judge::judge::UNPINNED;
use theseus_judge::price::Micros;
use theseus_judge::{
    Ask, DecisionPoint, Input, JevError, Judge, Judgment, JudgmentSink, Mode, Outcome, Urgency,
};
use theseus_kernel::{micros_to_usd, usd_to_micros};
use theseus_ontology::{Ontology, Origin as OntoOrigin};
use theseus_protocol::exchange::{ExchangeTopicsParams, ExchangeTopicsResult};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, Store as _};

use super::ONTO_PACK;
use crate::config::PackMode;
use crate::exchange::{Exchange, Opener, EXCHANGE_PREFIX, SEP, TOUCH_PREFIX};
use crate::judge::inbound::{self, Inbound};
use crate::judge::{spend, JudgeService};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin};
use crate::places::PlaceClass;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::store::Store;

/// The `purpose` this job's judgments carry in their context.
pub const PURPOSE: &str = "history";
/// What a judgment's context says under `declared` when his declaration,
/// not a mark, counted its exchange as his.
pub const DECLARED: &str = "unmarked";
/// The run's cap when the owner names none.
pub const CAP_USD: f64 = 10.0;

/// One old exchange to ask about.
#[derive(Debug, Clone)]
pub(crate) struct Old {
    pub turn: String,
    pub session: String,
    pub place: String,
    pub private: bool,
    /// The opening message's node.
    pub node: String,
    /// Counted as his by his declaration, not by a mark.
    pub declared: bool,
}

/// Whether the place code a message stored as it arrived
/// (`surface::code_of`) is one of his own places: his terminal, its ask and
/// its TUI, the web UI, his DM, a channel bound private. A shared channel,
/// voice, MCP and a Discord place no record names are not.
pub(crate) fn own_place(code: &str) -> bool {
    matches!(
        code,
        "terminal" | "terminal:ask" | "terminal:tui" | "cockpit" | "discord:dm"
    ) || code.starts_with("discord:channel:private:")
}

/// The ledger read once: what to ask, this job's earlier answers, and the
/// counts of the rest.
#[derive(Default)]
struct Plan {
    todo: Vec<Old>,
    again: Vec<Judgment>,
    r: ExchangeTopicsResult,
}

/// A run refused before it reads anything (issue 46): it asks nothing.
#[derive(Debug)]
pub enum Busy {
    /// Another run goes.
    Running,
    /// An earlier run's answer is still not written, after a short wait.
    Writing,
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Busy::Running => {
                "your history is already being filed under topics: this one asked nothing and \
                 spent nothing. Run it again once that one has ended"
            }
            Busy::Writing => {
                "an earlier answer of your history's filing is still being written: this one asked \
                 nothing and spent nothing. Run it again shortly"
            }
        })
    }
}

impl std::error::Error for Busy {}

/// The longest a run waits for the sink to write an answer's row: the
/// sink's own longest wait for a moment between turns, and a minute.
const WRITTEN_BOUND: std::time::Duration =
    crate::memory_pass::BUSY_BOUND.saturating_add(std::time::Duration::from_secs(60));

/// The longest a new run waits for an earlier run's answers to be written
/// before it is refused: a quiet daemon's sink writes in a few seconds.
const DRAIN_BOUND: std::time::Duration = std::time::Duration::from_secs(10);

/// What became of an answer's row while the run waited for it.
enum Row {
    Written,
    /// The daemon is stopping: the row may be written by its flush, or not.
    Stopping,
    /// Not written in [`WRITTEN_BOUND`].
    Late,
}

/// Why a run stops before an exchange.
enum Stop {
    /// Its worst case would pass what is left of the cap.
    Cap(Micros),
    /// The judge's day budget, or the daemon's day ceiling, would not hold it.
    Day,
    /// The judge's model has no price.
    Unpriced,
    /// The judge's client could not be built.
    Client,
    /// The sink did not write the last answer's row in [`WRITTEN_BOUND`].
    Unwritten,
    /// The daemon is stopping after an exchange was asked, its touches or
    /// its row not yet written.
    Stopping,
}

/// The turns whose exchange already touches a category of a kind `onto`
/// asks: one read of each kind's touches.
fn onto_touched(store: &Store) -> anyhow::Result<HashSet<String>> {
    let mut out = HashSet::new();
    for kind in ONTO_KINDS {
        let prefix = format!("{TOUCH_PREFIX}thing:{kind}:");
        for r in store.inner().latest_with_prefix(kinds::META, &prefix)? {
            let turn = r.key.as_deref().and_then(|k| k.rsplit(SEP).next());
            out.extend(turn.map(str::to_string));
        }
    }
    Ok(out)
}

/// This job's answered judgments, by turn.
fn asked_before(store: &Store) -> anyhow::Result<HashMap<String, Judgment>> {
    let mut out = HashMap::new();
    for j in history_rows(store)? {
        if let Some(turn) = j.context["turn"].as_str() {
            out.insert(turn.to_string(), j);
        }
    }
    Ok(out)
}

/// This job's answered judgments, oldest first: the rows of `onto`'s scope
/// whose context says `purpose: history`, each one.
pub(super) fn history_rows(store: &Store) -> anyhow::Result<Vec<Judgment>> {
    let id = ONTO_PACK.split('.').next().unwrap_or(ONTO_PACK);
    let mut out = Vec::new();
    for r in store.scope_after(&format!("judge:{id}"), 0)? {
        let Ok(row) = r.decode::<LedgerRow>() else {
            continue;
        };
        if row.kind != LedgerKind::JudgeCall.as_str()
            || row.data["context"]["purpose"].as_str() != Some(PURPOSE)
        {
            continue;
        }
        let Ok(j) = serde_json::from_value::<Judgment>(row.data) else {
            continue;
        };
        if j.actionable() {
            out.push(j);
        }
    }
    Ok(out)
}

/// The opening message of `ex`, when the live point would have asked `onto`
/// of it: a person's own typed message with words, not a slash command, in
/// a session that is not a task's.
fn opening(store: &Store, ex: &Exchange) -> Option<Node> {
    if !matches!(ex.opened_by, Opener::Message | Opener::Unknown) {
        return None;
    }
    let (_, n) = store.get_node(ex.ask.as_deref()?).ok().flatten()?;
    let Body::UserMessage { text, .. } = &n.body else {
        return None;
    };
    let own = n.author.as_deref().is_some_and(|a| a.starts_with("self:"));
    if n.origin != Origin::Operator || own || text.trim().is_empty() || inbound::slash_command(text)
    {
        return None;
    }
    let rec: SessionRecord = store.get_session(&ex.session_id).ok().flatten()?;
    rec.task.is_none().then_some(n)
}

impl JudgeService {
    /// Whether `onto` acts on `session` now, as the live point decides it:
    /// `[ontology] judge` on, and the version in the root's place live.
    fn onto_acts(&self, core: &Core, session: &str) -> bool {
        let pack = self.placed(ONTO_PACK, session);
        core.cfg.ontology.judge && self.mode_for(&pack, session).mode == PackMode::Live
    }

    /// The ask about `old`'s opening message `n`: see the module's doc. None
    /// when `onto` would ask it nothing.
    pub(crate) fn history_ask(
        &self,
        core: &Core,
        o: &Ontology,
        old: &Old,
        n: &Node,
    ) -> Option<Ask> {
        let Body::UserMessage { text, .. } = &n.body else {
            return None;
        };
        let class = match old.private {
            true => PlaceClass::Private,
            false => PlaceClass::Shared,
        };
        let msg = Inbound {
            session_id: old.session.clone(),
            execution_id: String::new(),
            turn_id: old.turn.clone(),
            node_id: n.id.clone(),
            text: text.clone(),
            place_kind: inbound::place_kind(Some(&old.place), "", class),
            author: inbound::author_of(class).into(),
            kernel: core.kernel.clone(),
            route: PackMode::Shadow,
            chosen: None,
            task: false,
            own: false,
            persona: Default::default(),
            capture: false,
            data_names: Default::default(),
            attachments: Vec::new(),
        };
        let mut input = inbound::input(std::slice::from_ref(n), &msg, Vec::new());
        let mut onto = super::onto_input(core, o, &msg);
        onto.shared = !old.private;
        input.onto = Some(Box::new(onto));
        let pack = self.pack(&self.placed(ONTO_PACK, &old.session))?;
        let mut answers = super::context(core, &old.session, &input);
        // The names mined from the message are its words, and a candidate
        // project's title is the owner's: they reach Jev in the state, never
        // the row. The candidates' ids stay, in `kinds`.
        if let Some(c) = answers.as_object_mut() {
            c.remove("names");
            c.remove("described");
            c.remove("projects");
        }
        let state = theseus_judge::prepare(&pack, &Input::Inbound(input), &self.scrub()).ok()?;
        let mut context = json!({
            "session": old.session, "turn": old.turn, "node": n.id, "baseline": "no_membership",
            "place_kind": msg.place_kind, "on_path_ms": 0, "class": inbound::class(false),
            "purpose": PURPOSE, "event_at_ms": n.created_at_ms, "onto": answers,
        });
        if old.declared {
            context["declared"] = json!(DECLARED);
        }
        // The arm is recorded; the ask is shadow whatever it says, so the
        // sink files nothing of it.
        let _ = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack, &state, Mode::Shadow, context);
        ask.id = Some(theseus_judge::judge::new_id());
        (!ask.asked.is_empty()).then_some(ask)
    }

    /// What `ask` reserves; none when its model has no price.
    fn history_worst(&self, ask: &Ask) -> Result<Micros, Stop> {
        let built = self.built().map_err(|_| Stop::Client)?;
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(ask));
        need.ok_or(Stop::Unpriced)
    }

    /// `ask`, under `left` of the run's cap: refused before the call when its
    /// worst case would pass it, or the day's budget will not hold it; else
    /// its judgment, settled at its price, and that price.
    async fn history_judge(&self, ask: Ask, left: Micros) -> Result<(Judgment, Micros), Stop> {
        let need = self.history_worst(&ask)?;
        if need > left {
            return Err(Stop::Cap(need));
        }
        let built = self.built().map_err(|_| Stop::Client)?;
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let asks = vec![ask];
        if !self.reserve(&today, need, spend::Class::of_asks(&asks)) {
            return Err(Stop::Day);
        }
        // Asked unrecorded, and recorded once withheld: the shared sink never
        // holds a word the answer brought back.
        let judgments = built
            .jev()
            .judge(DecisionPoint {
                asks,
                urgency: Urgency::Shadow,
            })
            .await;
        let (_, spent) = self.settle_round(&today, need, &judgments);
        let j = judgments.into_iter().next().ok_or(Stop::Client)?;
        let j = withheld(j);
        // Its id stays pending until the sink has written its row
        // (`Core::history_written` waits on it), as the gate's ids do; and
        // its turn is unwritten until then, whatever the run does next.
        self.pending_insert([j.id.clone()]);
        let turn = j.context["turn"].as_str().unwrap_or_default();
        self.unwritten_answers()
            .insert(turn.to_string(), j.id.clone());
        built.judge.sink().record(&j);
        Ok((j, spent))
    }

    fn unwritten_answers(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        (self.history_unwritten.lock()).unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The history's answers received and not yet written: an answer's
    /// entry goes once its row is (its id has left `pending`).
    fn answers_unwritten(&self) -> usize {
        let mut u = self.unwritten_answers();
        u.retain(|_, id| self.pending_has(id));
        u.len()
    }
}

/// How long a run waits for its answer's row, and a new run for an earlier
/// run's.
#[cfg(not(test))]
fn bounds(_: &JudgeService) -> (std::time::Duration, std::time::Duration) {
    (WRITTEN_BOUND, DRAIN_BOUND)
}

/// How long a run waits for its answer's row, and a new run for an earlier
/// run's: a test's shorter bounds, else the daemon's.
#[cfg(test)]
fn bounds(judge: &JudgeService) -> (std::time::Duration, std::time::Duration) {
    (judge.history_bounds.get().copied()).unwrap_or((WRITTEN_BOUND, DRAIN_BOUND))
}

/// The words this job's rows put where Jev's or its server's own went.
pub const WITHHELD: &str = "withheld";

/// What this job records of `j`: its class and its accounting, and no word
/// Jev or its server chose. An error keeps its class and its numbers (a
/// status, a retry's wait, a timeout's phase, a malformed answer's usage)
/// and its message or reason says [`WITHHELD`]; a malformed answer's model
/// is dropped; the model that answered is kept only when it is the pinned
/// one; the rate headers are dropped. The answers stay: the client takes a
/// choice only among the options asked, by their ids.
fn withheld(mut j: Judgment) -> Judgment {
    let w = || WITHHELD.to_string();
    if let Outcome::Failed { error, .. } = &mut j.outcome {
        *error = match error.clone() {
            JevError::Network { .. } => JevError::Network { message: w() },
            JevError::RateLimited {
                retry_after_secs, ..
            } => JevError::RateLimited {
                message: w(),
                retry_after_secs,
            },
            JevError::Server { status, .. } => JevError::Server {
                status,
                message: w(),
            },
            JevError::Auth { status, .. } => JevError::Auth {
                status,
                message: w(),
            },
            JevError::InvalidRequest { status, .. } => JevError::InvalidRequest {
                status,
                message: w(),
            },
            JevError::Malformed { usage, .. } => JevError::Malformed {
                reason: w(),
                usage,
                model: None,
            },
            kept @ (JevError::Timeout { .. } | JevError::OverState { .. }) => kept,
        };
    }
    let pinned = j.model != UNPINNED;
    j.answered_by = j.answered_by.filter(|m| pinned && *m == j.model);
    j.rate_limit.clear();
    j
}

impl Core {
    /// `exchange.topics`: see the module's doc. The caller has judged it the
    /// owner's act.
    pub(crate) async fn topics_history(
        self: &Arc<Self>,
        p: &ExchangeTopicsParams,
    ) -> anyhow::Result<ExchangeTopicsResult> {
        let cap_usd = match p.cap_usd {
            None => CAP_USD,
            Some(usd) => theseus_protocol::exchange::topics_cap(usd).map_err(anyhow::Error::msg)?,
        };
        // One run at a time: a second would plan the same exchanges, and
        // spend a second cap on them.
        let Ok(_one) = self.runner.judge.history.try_lock() else {
            return Err(Busy::Running.into());
        };
        // And none while an earlier run's answer is still not written: its
        // plan would not see it, and would ask that exchange again.
        if !self.history_drained().await {
            return Err(Busy::Writing.into());
        }
        if p.undo_declared {
            anyhow::ensure!(
                !p.include_unmarked,
                "--undo-declared undoes your declaration and --include-unmarked makes it: run one \
                 of them. Nothing was asked or ended"
            );
            let core = self.clone();
            let dry_run = p.dry_run;
            return theseus_store::blocking(move || core.topics_undo(dry_run));
        }
        let core = self.clone();
        let declare = p.include_unmarked;
        let Plan { todo, again, mut r } =
            theseus_store::blocking(move || core.topics_plan(declare))?;
        (r.dry_run, r.cap_usd, r.to_ask) = (p.dry_run, cap_usd, todo.len() as u64);
        r.include_unmarked = declare;
        if p.dry_run {
            r.worst_usd = micros_to_usd(self.topics_worst(&todo)?);
            return Ok(r);
        }
        for (i, j) in again.iter().enumerate() {
            if self.outbox.stopping() {
                let left = (again.len() - i + todo.len()) as u64;
                (r.left, r.stopped) = (left, Some(stopping(left)));
                return Ok(r);
            }
            let session = j.context["session"].as_str().unwrap_or_default();
            if self.runner.judge.onto_acts(self, session) && !self.topics_write(j, &mut r).await? {
                let left = (again.len() - i + todo.len()) as u64;
                (r.left, r.stopped) = (left, Some(stopping(left)));
                return Ok(r);
            }
        }
        let cap = usd_to_micros(cap_usd);
        let mut spent: Micros = 0;
        for (i, old) in todo.iter().enumerate() {
            if i > 0 {
                crate::pace::pace().await;
            }
            let left = (todo.len() - i) as u64;
            if self.outbox.stopping() {
                (r.left, r.stopped) = (left, Some(stopping(left)));
                break;
            }
            if let Some(stop) = self.topics_one(old, (cap, &mut spent), &mut r).await? {
                // An answer not written was asked: it is not left.
                let left = match stop {
                    Stop::Unwritten | Stop::Stopping => left - 1,
                    _ => left,
                };
                (r.left, r.stopped) = (left, Some(stopped(&stop, spent, cap_usd, left)));
                break;
            }
        }
        r.spent_usd = micros_to_usd(spent);
        Ok(r)
    }

    /// The ledger read once, oldest exchange first: see the module's doc.
    /// `declare`: the run carries his declaration.
    fn topics_plan(&self, declare: bool) -> anyhow::Result<Plan> {
        let touched = onto_touched(&self.store)?;
        let mut before = asked_before(&self.store)?;
        let mut all: Vec<Exchange> = (self.store.inner())
            .latest_with_prefix(kinds::META, EXCHANGE_PREFIX)?
            .into_iter()
            .filter_map(|r| r.decode::<Exchange>().ok())
            .collect();
        all.sort_by(|a, b| (a.start_ms, &a.turn_id).cmp(&(b.start_ms, &b.turn_id)));
        let mut p = Plan::default();
        p.r.exchanges = all.len() as u64;
        let judge = &self.runner.judge;
        for ex in all {
            if touched.contains(&ex.turn_id) {
                p.r.touched_before += 1;
            } else if ex.person.as_deref() != Some(crate::exchange::OWNER) {
                // Whose it is counts only where a person's message opened
                // it: a wake, a task's brief and Theseus's own turn are none.
                let Some(n) = opening(&self.store, &ex) else {
                    p.r.no_message += 1;
                    continue;
                };
                p.r.not_owner += 1;
                if ex.person.is_some() || crate::exchange::mark(&self.store, &n.id)?.is_some() {
                    continue;
                }
                p.r.unproven += 1;
                let acts = judge.onto_acts(self, &ex.session_id);
                if !acts {
                    p.r.unproven_not_acting += 1;
                }
                match n.surface.as_deref() {
                    None => p.r.unplaced += 1,
                    Some(code) if own_place(code) => p.r.declared += 1,
                    Some(_) => {}
                }
                if !declare || !n.surface.as_deref().is_some_and(own_place) {
                    continue;
                }
                if let Some(j) = before.remove(&ex.turn_id) {
                    p.r.asked_before += 1;
                    p.again.push(j);
                } else if acts {
                    p.todo.push(Old {
                        turn: ex.turn_id,
                        session: ex.session_id,
                        place: ex.place,
                        private: true,
                        node: n.id,
                        declared: true,
                    });
                } else {
                    p.r.not_acting += 1;
                }
            } else if let Some(j) = before.remove(&ex.turn_id) {
                p.r.asked_before += 1;
                p.again.push(j);
            } else if let Some(n) = opening(&self.store, &ex) {
                match judge.onto_acts(self, &ex.session_id) {
                    true => p.todo.push(Old {
                        turn: ex.turn_id,
                        session: ex.session_id,
                        place: ex.place,
                        private: ex.private,
                        node: n.id,
                        declared: false,
                    }),
                    false => p.r.not_acting += 1,
                }
            } else {
                p.r.no_message += 1;
            }
        }
        Ok(p)
    }

    /// The ontology as it stands, read through the store's blocking wait.
    fn onto_now(&self) -> anyhow::Result<Arc<Ontology>> {
        theseus_store::blocking(|| self.runner.ontology.snapshot(&self.store))
    }

    /// The old exchange's ask, from its stored node; none when there is
    /// nothing to ask.
    fn topics_ask(&self, o: &Ontology, old: &Old) -> anyhow::Result<Option<Ask>> {
        let node = theseus_store::blocking(|| self.store.get_node(&old.node))?;
        Ok(node.and_then(|(_, n)| self.runner.judge.history_ask(self, o, old, &n)))
    }

    /// The dry run's worst case: every ask's reservation, summed. No call.
    /// An ask it cannot price fails it: an estimate that failed is no zero.
    fn topics_worst(&self, todo: &[Old]) -> anyhow::Result<Micros> {
        let o = self.onto_now()?;
        let mut worst: Micros = 0;
        for old in todo {
            if let Some(ask) = self.topics_ask(&o, old)? {
                match self.runner.judge.history_worst(&ask) {
                    Ok(need) => worst += need,
                    Err(stop) => anyhow::bail!("{}", no_estimate(&stop)),
                }
            }
        }
        Ok(worst)
    }

    /// One old exchange asked under what is left of the cap, its price
    /// added to `spent`, its touches written and its row written: or why
    /// the run stops (before it, or after it for an unwritten row).
    async fn topics_one(
        self: &Arc<Self>,
        old: &Old,
        (cap, spent): (Micros, &mut Micros),
        r: &mut ExchangeTopicsResult,
    ) -> anyhow::Result<Option<Stop>> {
        // The pack may have been rolled back, or the setting turned off,
        // since the plan: asked again before the money is spent.
        if !self.runner.judge.onto_acts(self, &old.session) {
            r.not_acting += 1;
            return Ok(None);
        }
        let o = self.onto_now()?;
        let Some(ask) = self.topics_ask(&o, old)? else {
            return Ok(None);
        };
        let left = cap.saturating_sub(*spent);
        let (j, cost) = match self.runner.judge.history_judge(ask, left).await {
            Ok(done) => done,
            Err(stop) => return Ok(Some(stop)),
        };
        (r.asked, *spent) = (r.asked + 1, *spent + cost);
        if j.actionable() {
            r.answered += 1;
            if self.runner.judge.onto_acts(self, &old.session) && !self.topics_write(&j, r).await? {
                return Ok(Some(Stop::Stopping));
            }
        }
        Ok(match self.history_written(&j.id).await {
            Row::Written => None,
            Row::Stopping => Some(Stop::Stopping),
            Row::Late => Some(Stop::Unwritten),
        })
    }

    /// Wait until the sink has written judgment `id`'s row: its id leaves
    /// the judge's `pending` once its frame is written, as the gate's
    /// `of_call` reads it. A stop ends the wait, and is no proof of the row:
    /// its flush writes what is queued, and nothing after it.
    async fn history_written(&self, id: &str) -> Row {
        let judge = &self.runner.judge;
        let until = tokio::time::Instant::now() + bounds(judge).0;
        while judge.pending_has(id) {
            if self.outbox.stopping() || judge.queue.is_closed() {
                return Row::Stopping;
            }
            if tokio::time::Instant::now() >= until {
                return Row::Late;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Row::Written
    }

    /// Wait a little for an earlier run's answers to be written (one that
    /// stopped, or whose wait for its row ran out, or whose touches failed):
    /// true once none is left unwritten; false at [`DRAIN_BOUND`], or on a
    /// stop.
    async fn history_drained(&self) -> bool {
        let judge = &self.runner.judge;
        let until = tokio::time::Instant::now() + bounds(judge).1;
        while judge.answers_unwritten() > 0 {
            if self.outbox.stopping() || tokio::time::Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        true
    }

    /// The touches `j`'s answers come to at the act band, with no plan (so a
    /// `new` names nothing), in one frame between turns. They are made from
    /// the ontology as it stands under its writers' lock, and written before
    /// it is let go, so a category retired while the call was out, or before
    /// a recorded answer is touched, gets no touch. The wait for a moment
    /// between turns ends on a stop, and once it is granted, which may be
    /// minutes on, the pack's mode and the stop are asked again: rolled back
    /// meanwhile, no touch. False: the daemon is stopping, and nothing was
    /// written.
    async fn topics_write(
        &self,
        j: &Judgment,
        r: &mut ExchangeTopicsResult,
    ) -> anyhow::Result<bool> {
        let act = self.cfg.ontology.act;
        let w = tokio::select! {
            w = self.runner.pass.writing() => w,
            () = self.outbox.stopped() => return Ok(false),
        };
        if self.outbox.stopping() {
            return Ok(false);
        }
        let session = j.context["session"].as_str().unwrap_or_default();
        if !self.runner.judge.onto_acts(self, session) {
            return Ok(true);
        }
        let written = theseus_store::blocking(|| -> anyhow::Result<u64> {
            let board = &self.runner.ontology;
            let staged = board.stage(&self.store, OntoOrigin::Jev, |o| {
                let touches = crate::names::topics::touches(&self.store, o, (&[j], act, &[]));
                Ok((Vec::new(), touches))
            })?;
            let Some(staged) = staged else {
                return Ok(0);
            };
            self.store.append(staged.frame())?;
            Ok(staged.frame().len() as u64)
        });
        drop(w);
        let n = written?;
        if n > 0 {
            (r.filed, r.touches) = (r.filed + 1, r.touches + n);
        }
        Ok(true)
    }
}

/// The words of a run the daemon's stop ended.
fn stopping(left: u64) -> String {
    format!("the daemon is stopping: {left} exchanges left; run the same command again to go on")
}

/// The words of a run the daemon's stop ended after an exchange was asked,
/// before its row was known written or its touches were.
fn stopping_asked(left: u64) -> String {
    format!(
        "the daemon is stopping, and the last answer's row or touches may not be written (a \
         rerun may ask that exchange again); {left} exchanges left; run the same command again \
         to go on"
    )
}

/// The words of a dry run that could not price an exchange.
fn no_estimate(stop: &Stop) -> String {
    let why = match stop {
        Stop::Unpriced => "the judge's model has no price",
        _ => "the judge's client could not be built",
    };
    format!("no estimate: {why}, so a run would ask nothing. Nothing was asked")
}

/// The words of a stop before an exchange, and how to go on.
fn stopped(stop: &Stop, spent: Micros, cap_usd: f64, left: u64) -> String {
    let go_on = "run it again to go on from where it stopped";
    match stop {
        Stop::Cap(worst) => format!(
            "stopped at the cap: the next exchange's worst case ({}) would pass ${cap_usd} with {} \
             spent; {left} exchanges left. Go on with: theseus ledger topics --cap-usd <dollars>",
            crate::narrative::dollars(*worst),
            crate::narrative::dollars(spent),
        ),
        Stop::Day => format!(
            "the judge's budget for today would not hold the next exchange; {left} exchanges left: \
             {go_on} tomorrow"
        ),
        Stop::Unpriced => format!("the judge's model has no price; {left} exchanges left"),
        Stop::Client => format!("the judge's client was not built; {left} exchanges left: {go_on}"),
        Stop::Unwritten => format!(
            "the judge's last answer was not written within {} minutes; {left} exchanges left: \
             {go_on} once the daemon is quieter",
            WRITTEN_BOUND.as_secs() / 60
        ),
        Stop::Stopping => stopping_asked(left),
    }
}
