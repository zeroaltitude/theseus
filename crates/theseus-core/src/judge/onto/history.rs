//! The owner's history filed under topics, once (issue 46; `theseus ledger
//! topics`, the owner's act in person, `rpc/exchanges.rs`). Topics touch an
//! exchange only for messages since the build that added them
//! (`names::topics`), so a topic's timeline began then. This reads the
//! exchanges from before, oldest first, and asks the ontology's judge the
//! live point's question once about each one's opening message.
//!
//! - **Which exchanges**: those that touch no category of a kind `onto` asks,
//!   were not asked by this job before, open with a person's own typed
//!   message with words (never a task's, never Theseus's own, never a slash
//!   command), and whose session `onto` acts on now (`[ontology] judge` on,
//!   the pack live on the ladder): the live point writes touches only then.
//! - **Touches only.** The ask goes in shadow, so the sink writes its row
//!   and nothing else: no membership, no category minted, no card, no
//!   `still_*` answer acted on. The job writes the touches itself, with the
//!   live path's own writer (`names::topics::touches`) and no plan, so a
//!   `new` answer names nothing; each touch takes its exchange's own time.
//! - **The state** is the live point's (`inbound::input` and `onto_input`)
//!   over the opening message alone: no message before it, no reply, no
//!   attachment. Its text goes to Jev through the judge's one client and its
//!   scrubber, and nowhere else: the row's context leaves out the names
//!   mined from it, no state blob is kept, and nothing here logs it or puts
//!   it in an error.
//! - **Resume** without a record of its own: each judgment's context says
//!   `purpose: history` (an additive key of the `judge.call` row's free
//!   data), and an exchange whose answered judgment says so is not asked
//!   again; its touches are written again from that row where missing.
//! - **Spend**: each exchange's worst case is reserved before its call
//!   against the judge's day budget, which holds on the daemon's day ceiling,
//!   as the people backfill's judgments are; the run stops before an exchange
//!   whose worst case would pass its cap (`cap_usd`, default $10); each call
//!   settles at its price. Paced by the machine's quiet between exchanges;
//!   the daemon's stop is looked for there. A dry run reads and prices, and
//!   asks nothing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::json;
use theseus_judge::builders::ONTO_KINDS;
use theseus_judge::price::Micros;
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Judgment, Mode, Urgency};
use theseus_kernel::{micros_to_usd, usd_to_micros};
use theseus_ontology::Ontology;
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
}

/// The ledger read once: what to ask, this job's earlier answers, and the
/// counts of the rest.
#[derive(Default)]
struct Plan {
    todo: Vec<Old>,
    again: Vec<Judgment>,
    r: ExchangeTopicsResult,
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

/// This job's answered judgments, by turn: the rows of `onto`'s scope whose
/// context says `purpose: history`.
fn asked_before(store: &Store) -> anyhow::Result<HashMap<String, Judgment>> {
    let id = ONTO_PACK.split('.').next().unwrap_or(ONTO_PACK);
    let mut out = HashMap::new();
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
        if let (true, Some(turn)) = (j.actionable(), j.context["turn"].as_str()) {
            out.insert(turn.to_string(), j);
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
        // The names mined from the message are its words: they reach Jev in
        // the state, never the row.
        if let Some(c) = answers.as_object_mut() {
            c.remove("names");
            c.remove("described");
        }
        let state = theseus_judge::prepare(&pack, &Input::Inbound(input), &self.scrub()).ok()?;
        let mut context = json!({
            "session": old.session, "turn": old.turn, "node": n.id, "baseline": "no_membership",
            "place_kind": msg.place_kind, "on_path_ms": 0, "class": inbound::class(false),
            "purpose": PURPOSE, "event_at_ms": n.created_at_ms, "onto": answers,
        });
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
        let judgments = built
            .judge
            .judge(DecisionPoint {
                asks,
                urgency: Urgency::Shadow,
            })
            .await;
        let (_, spent) = self.settle_round(&today, need, &judgments);
        let j = judgments.into_iter().next().ok_or(Stop::Client)?;
        Ok((j, spent))
    }
}

impl Core {
    /// `exchange.topics`: see the module's doc. The caller has judged it the
    /// owner's act.
    pub(crate) async fn topics_history(
        self: &Arc<Self>,
        p: &ExchangeTopicsParams,
    ) -> anyhow::Result<ExchangeTopicsResult> {
        let cap_usd = p.cap_usd.unwrap_or(CAP_USD);
        if !(cap_usd.is_finite() && cap_usd > 0.0) {
            anyhow::bail!("--cap-usd is a positive number of dollars, not {cap_usd}");
        }
        let core = self.clone();
        let Plan { todo, again, mut r } = theseus_store::blocking(move || core.topics_plan())?;
        (r.dry_run, r.cap_usd, r.to_ask) = (p.dry_run, cap_usd, todo.len() as u64);
        if p.dry_run {
            r.worst_usd = micros_to_usd(self.topics_worst(&todo)?);
            return Ok(r);
        }
        let o = self.onto_now()?;
        for j in &again {
            self.topics_write(&o, j, &mut r).await?;
        }
        let cap = usd_to_micros(cap_usd);
        let mut spent: Micros = 0;
        for (i, old) in todo.iter().enumerate() {
            if i > 0 {
                crate::pace::pace().await;
            }
            let left = (todo.len() - i) as u64;
            if self.outbox.stopping() {
                (r.left, r.stopped) = (left, Some(format!(
                    "the daemon is stopping: {left} exchanges left; run the same command again to go on"
                )));
                break;
            }
            match self
                .topics_one(old, cap.saturating_sub(spent), &mut r)
                .await?
            {
                Ok(cost) => spent += cost,
                Err(stop) => {
                    (r.left, r.stopped) = (left, Some(stopped(&stop, spent, cap_usd, left)));
                    break;
                }
            }
        }
        r.spent_usd = micros_to_usd(spent);
        Ok(r)
    }

    /// The ledger read once, oldest exchange first: see the module's doc.
    fn topics_plan(&self) -> anyhow::Result<Plan> {
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
    fn topics_worst(&self, todo: &[Old]) -> anyhow::Result<Micros> {
        let o = self.onto_now()?;
        let mut worst: Micros = 0;
        for old in todo {
            if let Some(ask) = self.topics_ask(&o, old)? {
                worst += self.runner.judge.history_worst(&ask).unwrap_or(0);
            }
        }
        Ok(worst)
    }

    /// One old exchange asked under `left` of the cap, and its touches
    /// written: its price, or why the run stops before it.
    async fn topics_one(
        self: &Arc<Self>,
        old: &Old,
        left: Micros,
        r: &mut ExchangeTopicsResult,
    ) -> anyhow::Result<Result<Micros, Stop>> {
        let o = self.onto_now()?;
        let Some(ask) = self.topics_ask(&o, old)? else {
            return Ok(Ok(0));
        };
        let (j, cost) = match self.runner.judge.history_judge(ask, left).await {
            Ok(done) => done,
            Err(stop) => return Ok(Err(stop)),
        };
        r.asked += 1;
        if j.actionable() {
            r.answered += 1;
            self.topics_write(&o, &j, r).await?;
        }
        Ok(Ok(cost))
    }

    /// The touches `j`'s answers come to at the act band, with no plan (so a
    /// `new` names nothing), in one frame between turns.
    async fn topics_write(
        &self,
        o: &Ontology,
        j: &Judgment,
        r: &mut ExchangeTopicsResult,
    ) -> anyhow::Result<()> {
        let act = self.cfg.ontology.act;
        let records = crate::names::topics::touches(&self.store, o, (&[j], act, &[]));
        if !records.is_empty() {
            self.people_write(&records).await?;
            (r.filed, r.touches) = (r.filed + 1, r.touches + records.len() as u64);
        }
        Ok(())
    }
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
    }
}
