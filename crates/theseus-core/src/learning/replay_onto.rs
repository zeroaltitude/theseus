//! `onto.v1`'s labelled replay over the store (theseus-nngor; `theseus judge
//! replay --onto`): the sessions the import labelled go through the wired
//! pack with the labels hidden, and each kind's precision and recall are
//! scored against them (`theseus_judge::replay`'s pure half), beside the
//! share of Jev's live writes the owner took back (`onto_undo`).
//!
//! - **The cases**: every private, non-task session the held snapshot has an
//!   `Origin::Import` membership for, in a kind `onto.v1` asks, newest first,
//!   at most `limit`. Each is its first human message as the live point
//!   would have judged it (`judge::inbound::input` and `onto::onto_input`)
//!   against the ontology as it stands; its labels are the import's
//!   memberships, which `hide_labels` keeps out of the state.
//! - **The state is the session's at its first message** (theseus-bpsnj),
//!   when it held no category: every membership of the session is hidden,
//!   the import's, Jev's and the operator's alike (no pool item says
//!   `session`), a category's last use (`used_ms`) leaves out the session's
//!   own lists, and the naming call's suggested names, all made after a
//!   judgment of the session's, are not offered (`as_at_first`). The other
//!   sessions' lists stay as they stand now.
//! - **The run** is [`replay`](super::replay)'s: each call a `judge.call` row
//!   (`purpose: replay`) scoped `judge.replay:<pack id>`, the run a
//!   `judge.replay` row in the same frame, spend priced and reserved against
//!   `[judge] replay_limit_usd` (refused with the numbers before any call),
//!   never the shadow day budget. It sends the owner's history to Jev, so it
//!   runs only under `[judge] backfill_consent`, as a backfill does.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::json;
use theseus_judge::builders::{OntoInput, ONTO_KINDS};
use theseus_judge::price::{micros_to_usd, usd_to_micros};
use theseus_judge::replay::{
    onto_asks, onto_markdown, onto_report, onto_scored, KindScore, LabelledCase, OntoReplay,
};
use theseus_judge::{Judgment, Outcome};
use theseus_ontology::Origin;
use theseus_protocol::judge_runs::{
    JudgeReplayParams, JudgeReplayResult, OntoKindScore, OntoReplayReport, ReplayLeftOut, UndoRate,
};
use theseus_store::NewRecord;

use super::onto_undo::{rows_since, undo_rates, WINDOW_MS};
use super::replay::{candidate_of, scope, Caller};
use crate::config::PackMode;
use crate::fact;
use crate::import::Integrity;
use crate::judge::inbound::{self, Inbound};
use crate::node::{Body, Node, Origin as NodeOrigin};
use crate::rpc::Core;
use crate::session::SessionRecord;

/// How many sessions a run replays when `limit` names none.
const LIMIT: usize = 50;

/// The sessions to replay, and those left out with why.
pub(crate) struct Cases {
    pub cases: Vec<LabelledCase>,
    pub left_out: Vec<ReplayLeftOut>,
}

/// A node's text when it is a human's own message: an imported operator
/// line, or a live session's operator message.
fn human_text(n: &Node) -> Option<&str> {
    match &n.body {
        Body::Imported {
            text,
            integrity: Integrity::Operator,
            ..
        } => Some(text),
        Body::UserMessage { text, .. } if n.origin == NodeOrigin::Operator => Some(text),
        _ => None,
    }
}

impl Core {
    /// `judge.replay { onto }`: the owner's run, from a private place, on a
    /// learning thread as `judge_replay`'s are.
    pub(super) async fn onto_replay_run(
        self: &Arc<Self>,
        p: JudgeReplayParams,
        who: crate::approval::Answerer,
    ) -> anyhow::Result<JudgeReplayResult> {
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::JUDGE_REPLAY,
                what: "replay of onto.v1 over the import's labels",
            },
        )?;
        let core = Arc::downgrade(self);
        let rt = tokio::runtime::Handle::current();
        let (who_s, via) = (who.who(), who.via());
        let rx = super::tender::on_low_thread(move || match core.upgrade() {
            Some(c) => c.onto_replay(&rt, &p, &who_s, &via),
            None => Err(anyhow::anyhow!("the daemon stopped before the replay ran")),
        })?;
        rx.await.context("the replay's thread ended")?.1
    }

    /// The run itself: see the module's doc.
    pub(crate) fn onto_replay(
        &self,
        rt: &tokio::runtime::Handle,
        p: &JudgeReplayParams,
        who: &str,
        via: &str,
    ) -> anyhow::Result<JudgeReplayResult> {
        let _rt = rt.enter();
        if !self.cfg.judge.backfill_consent {
            bail!(
                "the labelled replay sends your recorded history to Jev, so it runs only under \
                 your consent: `backfill_consent = true` under [judge] in your config note, which \
                 agents can't write. Nothing was sent."
            );
        }
        let mut named = p.clone();
        if named.candidate.is_none() && named.pack_text.is_none() {
            named.candidate = Some("onto.v1".into());
        }
        let (pack, text) = candidate_of(&named)?;
        let id = crate::new_id("rpl");
        let limit = p.limit.map_or(LIMIT, |n| n as usize).clamp(1, 1000);
        let Cases { cases, left_out } = self.onto_cases(limit)?;
        let asks = onto_asks(&pack, &cases, &json!({"purpose": "replay", "run": id}))
            .map_err(anyhow::Error::msg)?;
        let built = self.runner.judge.jev()?;
        let mut caller = Caller::new(rt, built, usd_to_micros(self.cfg.judge.replay_limit_usd))
            .with_ceiling(&self.runner);
        let estimate = caller.estimate(asks.iter().map(|a| &a.ask))?;
        if estimate > caller.limit {
            bail!(
                "the replay of {} over {} sessions would reserve {}, past [judge] \
                 replay_limit_usd ({}); nothing was sent",
                pack.name(),
                cases.len(),
                crate::narrative::dollars(estimate),
                crate::narrative::dollars(caller.limit)
            );
        }
        let mut left_out = left_out;
        let mut judgments: Vec<Judgment> = Vec::new();
        for a in &asks {
            let mut ask = a.ask.clone();
            ask.id = Some(theseus_judge::new_id());
            match caller.ask(ask) {
                Some(j) => judgments.push(j),
                None => break,
            }
        }
        // A run cut short by its limit scores the sessions it reached, and
        // leaves out every one after, each with the limit's words.
        let reached = judgments.len();
        let why = caller.left_reason();
        left_out.extend(cases[reached..].iter().map(|case| ReplayLeftOut {
            judgment: case.session.clone(),
            reason: why.clone(),
        }));
        let scored = onto_scored(&cases[..reached], &asks[..reached], &judgments);
        let now = theseus_protocol::now_unix_ms();
        let undo = match rows_since(&self.store, now.saturating_sub(WINDOW_MS))? {
            Some(rows) => undo_rates(&rows, now.saturating_sub(WINDOW_MS)),
            None => Vec::new(),
        };
        let unreadable = left_out.len() as u32;
        let report = onto_report(&pack.name(), reached, &scored);
        let result = JudgeReplayResult {
            id,
            candidate: pack.name(),
            candidate_sha256: pack.sha256.clone(),
            incumbent: pack.name(),
            change: "asks".into(),
            set: "onto_labels".into(),
            labels: "import".into(),
            judgments: cases.len() as u32,
            called: judgments
                .iter()
                .filter(|j| j.outcome == Outcome::Answered)
                .count() as u32,
            left_out,
            estimate_usd: micros_to_usd(estimate),
            limit_usd: micros_to_usd(caller.limit),
            cost_usd: micros_to_usd(caller.spent),
            onto: Some(onto_result(&report, unreadable, undo)),
            ..JudgeReplayResult::default()
        };
        self.write_onto_replay(&pack.id, &text, &result, &caller.called, who, via)?;
        Ok(result)
    }

    /// The labelled sessions as cases: see the module's doc.
    pub(crate) fn onto_cases(&self, limit: usize) -> anyhow::Result<Cases> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        // Per session, the import's memberships by kind.
        let mut labels: BTreeMap<&str, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
        for l in o.lists() {
            if !ONTO_KINDS.contains(&l.kind.as_str()) {
                continue;
            }
            for m in l.members.iter().filter(|m| m.origin == Origin::Import) {
                labels
                    .entry(&l.session)
                    .or_default()
                    .entry(l.kind.clone())
                    .or_default()
                    .insert(m.category.to_string());
            }
        }
        let mut sessions: Vec<(u64, &str)> = labels
            .keys()
            .filter_map(|s| {
                let rec = self.store.get_session::<SessionRecord>(s).ok()??;
                let private = matches!(
                    self.runner.place_of(s),
                    theseus_memory::recall::Place::Private
                );
                (private && rec.task.is_none()).then_some((rec.last_active_ms, *s))
            })
            .collect();
        sessions.sort_by(|a, b| b.cmp(a));
        let mut out = Cases {
            cases: Vec::new(),
            left_out: Vec::new(),
        };
        for (_, sid) in sessions {
            if out.cases.len() == limit {
                break;
            }
            match self.onto_case(&o, sid, labels[sid].clone()) {
                Ok(c) => out.cases.push(c),
                Err(reason) => out.left_out.push(ReplayLeftOut {
                    judgment: sid.into(),
                    reason,
                }),
            }
        }
        Ok(out)
    }

    /// One session's case: its first human message, as the live point would
    /// have judged it now.
    fn onto_case(
        &self,
        o: &theseus_ontology::Ontology,
        sid: &str,
        labels: BTreeMap<String, BTreeSet<String>>,
    ) -> Result<LabelledCase, String> {
        let nodes = self
            .store
            .session_nodes(sid)
            .map_err(|e| format!("{e:#}"))?;
        let at = nodes
            .iter()
            .position(|(_, n)| human_text(n).is_some())
            .ok_or("no first message from a person can be read")?;
        let first = &nodes[at].1;
        let text = human_text(first).unwrap_or_default().to_string();
        let upto: Vec<Node> = nodes[..=at].iter().map(|(_, n)| n.clone()).collect();
        let msg = Inbound {
            session_id: sid.into(),
            execution_id: String::new(),
            turn_id: String::new(),
            node_id: first.id.clone(),
            text,
            place_kind: "import (private)".into(),
            author: "operator".into(),
            kernel: self.kernel.clone(),
            threads: Vec::new(),
            // An imported session is replayed as no thread's own session.
            in_thread: false,
            route: PackMode::Shadow,
            chosen: None,
            task: false,
            persona: Default::default(),
            capture: false,
            data_names: Default::default(),
            attachments: Vec::new(),
            restricted: Default::default(),
        };
        let mut input = inbound::input(&upto, &msg, Vec::new());
        let mut onto = crate::judge::onto::onto_input(self, o, &msg);
        as_at_first(o, sid, &mut onto);
        input.onto = Some(Box::new(onto));
        Ok(LabelledCase {
            session: sid.into(),
            input,
            labels,
        })
    }

    /// The run's frame: each call's `judge.call` row and the `judge.replay`
    /// row, scoped `judge.replay:<pack id>`, as `replay`'s are.
    pub(super) fn write_onto_replay(
        &self,
        pack: &str,
        text: &str,
        r: &JudgeReplayResult,
        called: &[Judgment],
        who: &str,
        via: &str,
    ) -> anyhow::Result<()> {
        let sc = scope(pack);
        let text_blob = self.store.blobs().put(text.as_bytes())?;
        let mut records: Vec<NewRecord> = Vec::new();
        for j in called {
            let f = fact::judge::JudgeCall {
                judgment: j,
                budget: "replay",
            };
            let mut rec = fact::row(&f, None, None)?;
            rec.key = Some(j.id.clone());
            records.push(rec.scoped(&sc));
        }
        let f = fact::judge_runs::JudgeReplayed {
            result: r,
            text_blob: &text_blob,
            who,
            via,
        };
        let mut rec = fact::row(&f, None, None)?;
        rec.key = Some(r.id.clone());
        records.push(rec.scoped(&sc));
        self.store
            .append(&records)
            .context("writing the replay's rows")?;
        self.rec(None).announce(&f);
        Ok(())
    }
}

/// What `--onto` refuses, since its run would ignore it (theseus-hdqkt): the
/// set of a report or of ids. `candidate` and `pack_text` it honours. A
/// client that calls the method itself is told as the CLI's parse tells.
pub(crate) fn ignored_with_onto(p: &JudgeReplayParams) -> Option<String> {
    let (flag, what) = if p.split.is_some() {
        ("--split", "is for a report's holdout")
    } else if p.errors {
        (
            "--errors",
            "is for a report's judgments the incumbent got wrong",
        )
    } else if !p.judgments.is_empty() {
        ("--judgments", "names judgments by id")
    } else if p.report.is_some() {
        ("--report", "names a report whose holdout is the set")
    } else {
        return None;
    };
    Some(format!(
        "--onto replays the import's labelled sessions; {flag} {what} and is not used with \
         it. Nothing was sent."
    ))
}

/// The state as it stood at the session's first message, when the session
/// held no category: no pool item is the session's, whatever the
/// membership's origin; a category's last use leaves out the session's own
/// lists; and no name a naming call suggested (each made after a judgment of
/// the session's) is offered.
pub(crate) fn as_at_first(o: &theseus_ontology::Ontology, sid: &str, input: &mut OntoInput) {
    let own: BTreeSet<String> = o
        .lists()
        .filter(|l| l.session == sid)
        .flat_map(|l| l.members.iter().map(|m| m.category.to_string()))
        .collect();
    let mut used: BTreeMap<String, u64> = BTreeMap::new();
    for l in o.lists().filter(|l| l.session != sid) {
        for m in &l.members {
            let at = used.entry(m.category.to_string()).or_default();
            *at = (*at).max(m.as_of_ms);
        }
    }
    for item in input.kinds.iter_mut().flat_map(|k| k.pool.iter_mut()) {
        item.session = false;
        if own.contains(&item.id) {
            item.used_ms = used.get(&item.id).copied().unwrap_or(0);
        }
    }
    input.suggested.clear();
}

fn score(k: &KindScore) -> OntoKindScore {
    OntoKindScore {
        kind: k.kind.clone(),
        sessions: k.sessions as u32,
        labelled: k.labelled as u32,
        picked: k.picked as u32,
        hits: k.hits as u32,
        wrong: k.wrong as u32,
        missed: k.missed as u32,
        minted: k.minted as u32,
        precision: k.precision,
        recall: k.recall,
    }
}

/// The wire report: the replay's numbers, what was left out, the undo rates,
/// and the Markdown the +onto report quotes.
pub(crate) fn onto_result(r: &OntoReplay, left_out: u32, undo: Vec<UndoRate>) -> OntoReplayReport {
    let mut markdown = onto_markdown(r);
    if left_out > 0 {
        markdown.push_str(&format!(
            "\n{left_out} labelled sessions were left out (no readable first message, or the \
             run's limit).\n"
        ));
    }
    markdown.push_str(&undo_markdown(&undo));
    OntoReplayReport {
        pack: r.pack.clone(),
        sessions: r.sessions as u32,
        unanswered: r.unanswered as u32,
        left_out,
        at_act: r.at_act.iter().map(score).collect(),
        at_confirm: r.at_confirm.iter().map(score).collect(),
        undo,
        markdown,
    }
}

/// The undo rates as a Markdown section, for the report.
fn undo_markdown(undo: &[UndoRate]) -> String {
    let mut out = String::from(
        "\n## Undone by the owner (Jev's writes, the last 7 days)\n\n| kind | writes | undone | rate |\n|---|---|---|---|\n",
    );
    for u in undo {
        let rate = u.rate.map_or_else(|| "n/a".into(), |r| format!("{r:.3}"));
        out.push_str(&format!(
            "| {} | {} | {} | {rate} |\n",
            u.kind, u.writes, u.undone
        ));
    }
    if undo.is_empty() {
        out.push_str("| (none) | 0 | 0 | n/a |\n");
    }
    out
}
