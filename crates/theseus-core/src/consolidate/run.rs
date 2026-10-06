//! Consolidation's run (M6 31b): the owner's `memory.consolidate`, or the
//! nightly tender's, on a `learning` thread at nice 19.
//!
//! - **Clusters** from the newest `recall.shadow` and `recall.ran` rows, read
//!   by kind through the store's pages (`theseus_memory::consolidate`): a
//!   cluster synthesized before (its digest in a `synthesis.proposed` row)
//!   is not proposed again, unless its one answer was rejected for its form
//!   (the deterministic checks, not Jev): it is proposed once more, on a
//!   later run ([`FORM_TRIES`]).
//! - **The profile.** `[memory] synth_profile = "session"` (the default) is
//!   the profile every source's session last used (`SessionRecord.last_target`,
//!   which routing may move turn by turn): no second provider reads a
//!   session's text. Sources whose sessions disagree, or have used none, wait,
//!   counted. A named profile is the operator's choice.
//! - **External text** (DD5): a cluster with an external source is never
//!   synthesized; it is counted (`external`).
//! - **Money.** Each call reserves its worst case at the model's prices
//!   before it is sent; the run stops before one would pass
//!   `synth_limit_usd_per_day`, the local day's spend read from its own
//!   `synthesis.proposed` rows, so a restart keeps it.
//! - **The checks**: the deterministic ones, then `citation.v1`
//!   (`judge::citation`); without Jev a synthesis stays `unchecked`. Both
//!   read the answer's entry (`theseus_memory::consolidate::entry`), a
//!   leading heading set aside, and the node keeps the entry; the
//!   `synthesis.proposed` row keeps the answer whole, as it was said.
//! - **Frames.** Each cluster's records (its node and edges, its rows) are
//!   one frame, written only between turns, through the memory pass's
//!   writer handshake (`memory_pass::turns`, which counts its writers).
//!   A dry run writes nothing.
//! - **Where it runs.** The plan (the rows read, the clusters, their
//!   sources) on a `learning` thread at nice 19; the calls and the frames'
//!   waits on the runtime, as tasks, never a thread blocked on it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{bail, Context as _};
use serde_json::{json, Value};
use theseus_judge::builders::{CitationInput, CitedSentence};
use theseus_judge::price::{micros_to_usd, usd_to_micros, Micros};
use theseus_memory::consolidate::{self as pure, CoRecall};
use theseus_protocol::memory::{
    MemoryConsolidateParams, MemoryConsolidateResult, RecallManifest, SynthesisReport,
};
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, NewRecord, Page, Store as _};

use super::{CitationCheck, Stage};
use crate::config::memory::SUMMARY_SESSION;
use crate::fact::synthesis::{SynthesisChecked, SynthesisProposed, SynthesisScored, SCOPE};
use crate::graph::{Edge, EdgeKind, VIA_SYNTHESIS};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, RecalledRef};
use crate::provider::ProviderRequest;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The META key naming the memory's harness session.
pub const MEMORY_SESSION: &str = "memory.session";
/// The META key of the nightly run's last time.
pub const LAST_RUN: &str = "memory.consolidated";
/// The newest recall rows a run reads.
pub const ROWS: usize = 5000;
/// A source's text in the request, at most, in characters.
pub const SOURCE_CHARS: usize = 1500;
/// A synthesis's answer, at most: 120 words and their citations.
pub const MAX_TOKENS: u32 = 600;
/// A cluster whose every answer was rejected for its form is proposed until
/// it has this many: one more run, at one more call.
pub const FORM_TRIES: usize = 2;

/// What the profile is told, before the sources.
pub const INSTRUCTIONS: &str = "You write one short encyclopedia entry from numbered notes. \
Use only what the notes say. Write at most 120 words, in plain sentences. End every sentence \
with the numbers of the notes that support it, in brackets, as [1] or [2][3]. Reply with the \
entry alone.";

/// One cluster's sources, read.
struct Source {
    node: Arc<Node>,
    header: String,
    text: String,
}

impl Core {
    /// `memory.consolidate`: the owner's run, from a private place
    /// (`judge_act(Act::JudgeRun)`: it spends money, and sends a session's
    /// text to a profile), on a thread of its own at low priority.
    pub async fn memory_consolidate(
        self: &Arc<Self>,
        p: MemoryConsolidateParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<MemoryConsolidateResult> {
        let who = who.into();
        if !self.runner.memory.on() {
            bail!("memory is off ([memory] mode = \"off\"): nothing is consolidated");
        }
        let dry = p.dry_run.unwrap_or(false);
        let what = if dry {
            "a dry run of consolidation"
        } else {
            "consolidation"
        };
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::MEMORY_CONSOLIDATE,
                what,
            },
        )?;
        self.consolidate_now(dry, "operator").await
    }

    /// Consolidation now: its plan (the rows, the clusters, their sources)
    /// on a `learning` thread at nice 19; then each cluster's calls and
    /// frames on the runtime, so a stop during a wait between turns ends the
    /// run as any task's, never a blocked thread's.
    pub async fn consolidate_now(
        self: &Arc<Self>,
        dry: bool,
        trigger: &'static str,
    ) -> anyhow::Result<MemoryConsolidateResult> {
        let core = Arc::downgrade(self);
        let nightly = trigger == "nightly";
        let plan = move || {
            // The nightly run starts once the machine is not busy, never
            // past a stop, as the nightly learning run does (theseus-tood).
            if let Some(c) = core.upgrade().filter(|_| nightly) {
                let stopping = || c.outbox.stopping();
                let bound = theseus_store::pressure::BOUND;
                theseus_store::pressure::quiet_blocking_unless(bound, stopping);
            }
            let t0 = std::time::Instant::now();
            let plan = match core.upgrade() {
                Some(c) => c.consolidate_plan(dry),
                None => Err(anyhow::anyhow!(
                    "the daemon stopped before consolidation ran"
                )),
            };
            // The nightly run keeps to about 5% of a core: its thread sleeps
            // 19 times as long as it worked, as the learning tender's does.
            if nightly {
                std::thread::sleep(t0.elapsed() * 19);
            }
            plan
        };
        // The nightly run's thread is in SCHED_IDLE too: it answers no one.
        // The owner's run is waited for, as main's owner's runs are.
        let rx = if nightly {
            crate::learning::tender::on_idle_thread(plan)
        } else {
            crate::learning::tender::on_low_thread(plan)
        }?;
        let plan = rx.await.context("consolidation's thread ended")?.1?;
        self.consolidate_run(plan, trigger).await
    }

    /// The newest recall rows, by kind through the store's pages.
    fn recall_rows(&self) -> anyhow::Result<Vec<RecallManifest>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![
                ledger_kind(LedgerKind::RecallShadow.as_str()),
                ledger_kind(LedgerKind::RecallRan.as_str()),
            ],
            limit: ROWS,
            ..Page::default()
        };
        let Some(out) = self.store.inner().page(&page)? else {
            bail!("the store's index is being built after the start: consolidation waits for it");
        };
        Ok(out
            .records
            .iter()
            .filter_map(|r| r.decode::<LedgerRow>().ok())
            .filter_map(|row| serde_json::from_value(row.data).ok())
            .collect())
    }

    /// Consolidation's own rows of `kind`, oldest first, through the pages
    /// (or its scope, while the index's shape is built); with `since_ms`,
    /// only those from then.
    pub(crate) fn synthesis_rows(
        &self,
        kind: LedgerKind,
        since_ms: Option<u64>,
    ) -> anyhow::Result<Vec<LedgerRow>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(kind.as_str())],
            since_ms,
            limit: usize::MAX / 2,
            ..Page::default()
        };
        let records = match self.store.inner().page(&page)? {
            Some(out) => out.records,
            None => self
                .store
                .scope_after(SCOPE, 0)?
                .into_iter()
                .filter(|r| r.kind == kinds::LEDGER)
                .collect(),
        };
        Ok(records
            .iter()
            .filter_map(|r| r.decode::<LedgerRow>().ok())
            .filter(|r| r.kind == kind.as_str())
            .filter(|r| since_ms.is_none_or(|s| r.at_unix_ms >= s))
            .collect())
    }

    /// The memory's harness session, opened at the first synthesis: found
    /// by its META key, as the ladder's is. Never compiled: no turn runs
    /// there, and its place reads private (no target), so a shared place
    /// never draws on it.
    pub fn memory_session(&self) -> anyhow::Result<Option<String>> {
        self.store.get_meta::<String>(MEMORY_SESSION)
    }

    /// The harness session, opened now if it is not: callers hold the
    /// pass's writer guard (`open_memory_session_between`).
    pub(crate) fn open_memory_session(&self) -> anyhow::Result<String> {
        if let Some(id) = self.memory_session()? {
            return Ok(id);
        }
        let rec = self.open_session(theseus_protocol::SessionOpenParams {
            kind: None,
            label: Some("memory: consolidation's syntheses (never compiled)".into()),
            opened_from: None,
        })?;
        self.store.put_meta(MEMORY_SESSION, &rec.session_id)?;
        // Every arm but `+synthesis` leaves it out from now on: a first
        // read finds it by its META key, and a read made before it learns it
        // as its first synthesis is kept (`Memory::kept_synthesis`).
        self.runner.memory.syntheses(&self.store);
        Ok(rec.session_id)
    }

    /// The profile a cluster's synthesis is written with, or why it waits.
    fn synth_profile(&self, sources: &[Source]) -> Result<String, (&'static str, String)> {
        let word = &self.runner.memory.cfg().synth_profile;
        if word != SUMMARY_SESSION {
            return Ok(word.clone());
        }
        let mut seen = BTreeSet::new();
        for s in sources {
            let last = self
                .store
                .get_session::<SessionRecord>(&s.node.session_id)
                .ok()
                .flatten()
                .and_then(|r| r.last_target);
            match last {
                Some(t) => {
                    seen.insert(t.profile);
                }
                None => {
                    return Err((
                        "no_profile",
                        format!("{} has used no profile yet", s.node.session_id),
                    ))
                }
            }
        }
        match seen.len() {
            1 => Ok(seen.into_iter().next().unwrap_or_default()),
            _ => Err((
                "profiles_disagree",
                format!(
                    "the sources' sessions last used {}: synth_profile = \"session\" waits",
                    seen.into_iter().collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }

    /// The run's plan, read and computed off the runtime: the recall rows,
    /// the day's spend, the clusters, and each one's sources and profile.
    fn consolidate_plan(&self, dry: bool) -> anyhow::Result<Plan> {
        let cfg = self.runner.memory.cfg().clone();
        let now = theseus_protocol::now_unix_ms();
        let rows = self.recall_rows()?;
        let proposed = self.synthesis_rows(LedgerKind::SynthesisProposed, None)?;
        let midnight = crate::learning::local_midnight(now);
        let spent: Micros = proposed
            .iter()
            .filter(|r| r.at_unix_ms >= midnight)
            .map(|r| usd_to_micros(r.data["cost_usd"].as_f64().unwrap_or(0.0)))
            .sum();
        let checked = self.synthesis_rows(LedgerKind::SynthesisChecked, None)?;
        let (done, again) = done_clusters(&proposed, &checked);
        // Where each admitted node is, and the nodes that are never sources.
        let mut at: BTreeMap<String, (String, u64)> = BTreeMap::new();
        let mut excluded = BTreeSet::new();
        for m in &rows {
            for a in &m.admitted {
                at.insert(a.node_id.clone(), (a.session_id.clone(), a.position));
                if a.kind == "synthesis" || a.kind == "recall" {
                    excluded.insert(a.node_id.clone());
                }
            }
        }
        let co: Vec<CoRecall> = rows
            .iter()
            .map(|m| CoRecall {
                turn: m.turn_id.clone().unwrap_or_else(|| m.recall_id.clone()),
                nodes: m.admitted.iter().map(|a| a.node_id.clone()).collect(),
            })
            .collect();
        let (clusters, skipped) = pure::clusters(&co, &excluded, &done);
        let limit = usd_to_micros(cfg.synth_limit_usd_per_day);
        let mut todo = Vec::new();
        let mut r = MemoryConsolidateResult {
            dry_run: dry,
            recalls: rows.len() as u64,
            skipped: skipped
                .into_iter()
                .map(|(k, n)| (k.as_str().to_string(), n as u64))
                .collect(),
            limit_usd: micros_to_usd(limit),
            ..MemoryConsolidateResult::default()
        };
        let skip = |r: &mut MemoryConsolidateResult, why: &str| {
            *r.skipped.entry(why.to_string()).or_default() += 1;
        };
        for c in clusters {
            let Some(sources) = self.read_sources(&c.nodes, &at) else {
                skip(&mut r, "unreadable");
                continue;
            };
            if sources.iter().any(|s| external(&s.node)) {
                skip(&mut r, "external");
                continue;
            }
            let mut report = SynthesisReport {
                cluster: c.digest.clone(),
                sources: c.nodes.clone(),
                turns: c.turns as u64,
                why: again.get(&c.digest).map(|w| {
                    format!("proposed again, once: its last answer was rejected for its form ({w})")
                }),
                ..SynthesisReport::default()
            };
            let profile = match self.synth_profile(&sources) {
                Ok(p) => p,
                Err((why, words)) => {
                    skip(&mut r, why);
                    tracing::debug!(cluster = %c.digest, why = %words, "consolidation: a cluster waits");
                    continue;
                }
            };
            report.profile.clone_from(&profile);
            if dry {
                report.outcome = "would_propose".into();
                r.clusters.push(report);
                continue;
            }
            todo.push((c, sources, profile, report));
        }
        r.spent_today_usd = micros_to_usd(spent);
        Ok(Plan {
            result: r,
            todo,
            rows,
            spent,
            limit,
            now,
        })
    }

    /// The plan's clusters, each synthesized, checked, and written between
    /// turns, until the day's limit.
    async fn consolidate_run(
        &self,
        plan: Plan,
        trigger: &str,
    ) -> anyhow::Result<MemoryConsolidateResult> {
        let Plan {
            result: mut r,
            todo,
            rows,
            mut spent,
            limit,
            now,
        } = plan;
        for (c, sources, profile, report) in todo {
            let step = self
                .synthesize(
                    &c, &sources, &profile, report, &mut spent, limit, trigger, &rows,
                )
                .await?;
            match step {
                Step::Skip(why) => *r.skipped.entry(why.to_string()).or_default() += 1,
                Step::Stop(why) => {
                    r.stopped = Some(why);
                    break;
                }
                Step::Done(report) => r.clusters.push(report),
            }
        }
        r.spent_today_usd = micros_to_usd(spent);
        if !r.dry_run && trigger == "nightly" {
            let mark = NewRecord::json(
                kinds::META,
                Some(LAST_RUN),
                &json!({"at_unix_ms": now, "clusters": r.clusters.len()}),
            )?;
            self.write_between(&[mark]).await?;
        }
        Ok(r)
    }

    /// One cluster's synthesis: the call under the day's limit, the checks,
    /// and its frame.
    #[allow(clippy::too_many_arguments)]
    async fn synthesize(
        &self,
        c: &pure::Cluster,
        sources: &[Source],
        profile: &str,
        mut report: SynthesisReport,
        spent: &mut Micros,
        limit: Micros,
        trigger: &str,
        rows: &[RecallManifest],
    ) -> anyhow::Result<Step> {
        let (live, _) = self.live_profile();
        let target = match self.runner.resolve_target(&live, Some(profile), None, None) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(profile, error = %format!("{e:#}"), "consolidation: the profile does not resolve");
                return Ok(Step::Skip("profile_unresolved"));
            }
        };
        let Some(provider) = self.runner.providers.get(&target.provider).cloned() else {
            return Ok(Step::Skip("no_provider"));
        };
        let request = request_for(&target.model, sources);
        let Some(need) = self.audit_reserve(&request) else {
            return Ok(Step::Skip("no_price"));
        };
        if spent.saturating_add(need) > limit {
            return Ok(Step::Stop(format!(
                "the next synthesis would reserve {}, past [memory] synth_limit_usd_per_day ({}) \
                 with {} spent today",
                crate::narrative::dollars(need),
                crate::narrative::dollars(limit),
                crate::narrative::dollars(*spent)
            )));
        }
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let (text, model, cost) = match provider.stream_message(&request, &mut quiet).await {
            Ok(resp) => {
                let cost = self
                    .runner
                    .catalog
                    .get(&resp.model)
                    .or_else(|| self.runner.catalog.get(&target.model))
                    .map_or(need, |e| e.cost_micros(&resp.usage));
                (resp.text.trim().to_string(), resp.model, cost)
            }
            Err(e) => {
                // A failed request may have been billed: booked at its
                // reservation, as the audit's are.
                *spent += need;
                report.outcome = "failed".into();
                report.why = Some(format!("{e:#}"));
                report.cost_usd = micros_to_usd(need);
                self.write_failed(c, profile, &target.model, need, trigger)
                    .await?;
                return Ok(Step::Done(report));
            }
        };
        *spent += cost;
        let id = crate::new_id("syn");
        report.synthesis_id = Some(id.clone());
        report.cost_usd = micros_to_usd(cost);
        let entry = pure::entry(&text);
        let verdict = self.verdict(&id, entry.text, sources).await;
        let session = match &verdict {
            Verdict::Checked(_) => Some(self.open_memory_session_between().await?),
            Verdict::Rejected(..) => None,
        };
        report.outcome = verdict.word().into();
        report.why = verdict.why();
        // What was kept, or the answer refused, as it was said.
        report.text = Some(match &verdict {
            Verdict::Checked(_) => entry.text.to_string(),
            Verdict::Rejected(..) => text.clone(),
        });
        let records = self.records(
            &id,
            c,
            &text,
            &entry,
            profile,
            &model,
            cost,
            trigger,
            &verdict,
            rows,
            session.as_deref(),
        )?;
        self.write_between(&records).await?;
        if let (Verdict::Checked(check), Some(session)) = (&verdict, session) {
            self.runner
                .memory
                .kept_synthesis(&session, &id, check.checked());
        }
        Ok(Step::Done(report))
    }

    /// A cluster's nodes, each read by position; `None` when one cannot be.
    fn read_sources(
        &self,
        ids: &[String],
        at: &BTreeMap<String, (String, u64)>,
    ) -> Option<Vec<Source>> {
        ids.iter()
            .map(|id| {
                let (session_id, position) = at.get(id)?.clone();
                // Its testimony header names its place, as a recalled item's
                // does (35a).
                let place = self.runner.place_name(&session_id);
                let probe = RecalledRef {
                    node_id: id.clone(),
                    session_id,
                    position,
                    chunk: (0, 0),
                    header: String::new(),
                    tokens: 0,
                };
                let node = self.runner.memory.read_source(&self.store, &probe)?;
                Some(Source {
                    header: crate::recall::render::header(&node, position, &place),
                    text: crate::recall::text_of(&node),
                    node,
                })
            })
            .collect()
    }

    /// The deterministic checks, then Jev's, on an entry's text: its
    /// sentences are numbered from 1 after its heading.
    async fn verdict(&self, id: &str, text: &str, sources: &[Source]) -> Verdict {
        let sentences = match pure::check(text, sources.len()) {
            Ok(s) => s,
            Err(f) => return Verdict::Rejected(f.to_string(), Vec::new(), None),
        };
        let session = theseus_store::blocking(|| self.memory_session())
            .ok()
            .flatten()
            .unwrap_or_default();
        let input = CitationInput {
            sentences: sentences
                .into_iter()
                .map(|s| CitedSentence {
                    text: s.text,
                    cites: s.cites,
                })
                .collect(),
            sources: sources.iter().map(|s| s.text.clone()).collect(),
        };
        let checked = self.runner.judge.check_citations(&session, id, input).await;
        match checked {
            Err(why) => Verdict::Checked(CitationCheck::Unchecked { why }),
            Ok(c) => {
                let bad: Vec<String> = c.unsupported().iter().map(|s| s.to_string()).collect();
                match c.least() {
                    Some(least) if bad.is_empty() => Verdict::Checked(CitationCheck::Supported {
                        least,
                        judgment: c.judgment,
                        mode: c.mode,
                    }),
                    Some(least) => Verdict::Rejected(
                        format!("Jev found {} of its citations unsupported", bad.len()),
                        bad,
                        Some((least, c.judgment)),
                    ),
                    None => Verdict::Checked(CitationCheck::Unchecked {
                        why: format!("Jev left {} of its pairs unanswered", c.unanswered.len()),
                    }),
                }
            }
        }
    }

    /// One synthesis's frame: its node and edges (unless rejected), its
    /// `synthesis.proposed` (the answer whole), `.checked` (the heading set
    /// aside, if any), and `.scored` rows; the node and the score are the
    /// entry's.
    #[allow(clippy::too_many_arguments)]
    fn records(
        &self,
        id: &str,
        c: &pure::Cluster,
        answer: &str,
        entry: &pure::Entry<'_>,
        profile: &str,
        model: &str,
        cost: Micros,
        trigger: &str,
        verdict: &Verdict,
        rows: &[RecallManifest],
        session: Option<&str>,
    ) -> anyhow::Result<Vec<NewRecord>> {
        let mut out = Vec::new();
        let key = |mut rec: NewRecord| {
            rec.key = Some(id.to_string());
            rec.scoped(SCOPE)
        };
        let proposed = SynthesisProposed {
            synthesis_id: id,
            cluster: &c.digest,
            sources: &c.nodes,
            turns: c.turns as u64,
            text: answer,
            profile,
            model,
            cost_usd: micros_to_usd(cost),
            trigger,
        };
        out.push(key(crate::fact::row(&proposed, None, None)?));
        let (word, least, judgment, mode, why, bad) = match verdict {
            Verdict::Checked(CitationCheck::Supported {
                least,
                judgment,
                mode,
            }) => (
                "supported",
                Some(*least),
                Some(judgment.as_str()),
                Some(mode.as_str()),
                None,
                &[][..],
            ),
            Verdict::Checked(CitationCheck::Unchecked { why }) => {
                ("unchecked", None, None, None, Some(why.as_str()), &[][..])
            }
            Verdict::Rejected(why, bad, jev) => (
                "rejected",
                jev.as_ref().map(|(l, _)| *l),
                jev.as_ref().map(|(_, j)| j.as_str()),
                None,
                Some(why.as_str()),
                bad.as_slice(),
            ),
        };
        let checked = SynthesisChecked {
            synthesis_id: id,
            cluster: &c.digest,
            verdict: word,
            least,
            judgment,
            mode,
            why,
            unsupported: bad,
            heading: entry.heading,
        };
        out.push(key(crate::fact::row(&checked, None, None)?));
        let (Verdict::Checked(check), Some(session)) = (verdict, session) else {
            return Ok(out);
        };
        let body = Body::Synthesis {
            text: entry.text.to_string(),
            sources: c.nodes.clone(),
            check: check.clone(),
            stage: Stage::of(check),
            cluster: c.digest.clone(),
            profile: profile.to_string(),
            model: model.to_string(),
            cost_usd: Some(micros_to_usd(cost)),
        };
        let mut node = Node::synthesis(session, body);
        node.id = id.to_string();
        out.push(node.record()?);
        for s in &c.nodes {
            out.push(Edge::new(EdgeKind::DerivedFrom, id, s, VIA_SYNTHESIS).record()?);
        }
        let scored = score(&c.nodes, entry.text, rows, self.runner.memory.cfg());
        if !scored.is_empty() {
            let f = SynthesisScored {
                synthesis_id: id,
                recalls: &scored,
            };
            out.push(key(crate::fact::row(&f, None, None)?));
        }
        Ok(out)
    }

    /// The harness session, opened between turns at the first synthesis.
    async fn open_memory_session_between(&self) -> anyhow::Result<String> {
        if let Some(id) = theseus_store::blocking(|| self.memory_session())? {
            return Ok(id);
        }
        let w = self.runner.pass.writing().await;
        let id = theseus_store::blocking(|| self.open_memory_session());
        drop(w);
        id
    }

    /// A failed call's row: its cluster and its cost, so the day's spend
    /// counts it.
    async fn write_failed(
        &self,
        c: &pure::Cluster,
        profile: &str,
        model: &str,
        cost: Micros,
        trigger: &str,
    ) -> anyhow::Result<()> {
        let id = crate::new_id("syn");
        let f = SynthesisProposed {
            synthesis_id: &id,
            cluster: &c.digest,
            sources: &c.nodes,
            turns: c.turns as u64,
            text: "",
            profile,
            model,
            cost_usd: micros_to_usd(cost),
            trigger,
        };
        let mut rec = crate::fact::row(&f, None, None)?;
        rec.key = Some(id);
        self.write_between(&[rec.scoped(SCOPE)]).await
    }

    /// One frame, written only between turns (the memory pass's writer
    /// handshake), and its facts announced.
    async fn write_between(&self, records: &[NewRecord]) -> anyhow::Result<()> {
        let w = self.runner.pass.writing().await;
        let written = theseus_store::blocking(|| self.store.append(records));
        drop(w);
        written?;
        for rec in records {
            if rec.kind != kinds::LEDGER {
                continue;
            }
            if let Ok(row) = serde_json::from_slice::<LedgerRow>(&rec.payload) {
                tracing::info!(kind = %row.kind, data = %row.data, "consolidation");
            }
        }
        Ok(())
    }
}

/// A run's plan: what the nice thread read and computed.
struct Plan {
    /// The result so far: the skipped, and a dry run's clusters.
    result: MemoryConsolidateResult,
    /// Each cluster to synthesize: its sources, its profile, its report.
    todo: Vec<(pure::Cluster, Vec<Source>, String, SynthesisReport)>,
    rows: Vec<RecallManifest>,
    spent: Micros,
    limit: Micros,
    now: u64,
}

/// What became of one cluster.
enum Step {
    /// Left out, and why.
    Skip(&'static str),
    /// The run stops here, and why.
    Stop(String),
    Done(SynthesisReport),
}

/// What the checks made of a synthesis.
enum Verdict {
    /// Kept: supported, or unchecked.
    Checked(CitationCheck),
    /// Not kept: why, the unsupported pairs, and Jev's least and judgment.
    Rejected(String, Vec<String>, Option<(f64, String)>),
}

impl Verdict {
    fn word(&self) -> &'static str {
        match self {
            Verdict::Checked(c) => c.as_str(),
            Verdict::Rejected(..) => "rejected",
        }
    }

    fn why(&self) -> Option<String> {
        match self {
            Verdict::Checked(CitationCheck::Unchecked { why }) | Verdict::Rejected(why, ..) => {
                Some(why.clone())
            }
            Verdict::Checked(CitationCheck::Supported { .. }) => None,
        }
    }
}

/// The clusters synthesized before, and those proposed again with the fault
/// of their last answer. A cluster is done once a call of it answered (a
/// failed call's row has no text, and counts as nothing), unless every
/// answer was rejected for its form (its `synthesis.checked` row `rejected`
/// with no `judgment`: the deterministic checks, never Jev) and there are
/// fewer than [`FORM_TRIES`]. Jev's rejection, a kept synthesis, or an
/// answer with no checked row leaves it done.
fn done_clusters(
    proposed: &[LedgerRow],
    checked: &[LedgerRow],
) -> (BTreeSet<String>, BTreeMap<String, String>) {
    let form: BTreeMap<&str, &str> = checked
        .iter()
        .filter(|r| r.data["verdict"] == "rejected" && r.data["judgment"].is_null())
        .filter_map(|r| {
            let id = r.data["synthesis_id"].as_str()?;
            Some((id, r.data["why"].as_str().unwrap_or("its form")))
        })
        .collect();
    // Each cluster's answers, and the fault of its last one if every one
    // was rejected for its form (oldest first, so the last is the newest).
    let mut answers: BTreeMap<&str, (usize, Option<&str>)> = BTreeMap::new();
    for r in proposed
        .iter()
        .filter(|r| r.data["text"].as_str().is_some_and(|t| !t.is_empty()))
    {
        let Some(cluster) = r.data["cluster"].as_str() else {
            continue;
        };
        let fault = r.data["synthesis_id"].as_str().and_then(|id| form.get(id));
        let (n, last) = answers.entry(cluster).or_insert((0, None));
        *last = if *n == 0 || last.is_some() {
            fault.copied()
        } else {
            None
        };
        *n += 1;
    }
    let mut done = BTreeSet::new();
    let mut again = BTreeMap::new();
    for (cluster, (n, last)) in answers {
        match last {
            Some(why) if n < FORM_TRIES => {
                again.insert(cluster.to_string(), why.to_string());
            }
            _ => {
                done.insert(cluster.to_string());
            }
        }
    }
    (done, again)
}

/// Whether a node is external text (DD5).
fn external(n: &Node) -> bool {
    matches!(
        &n.body,
        Body::ToolResult {
            external: Some(_),
            ..
        }
    )
}

/// The request for one cluster: the instructions, then its sources,
/// numbered, each under its header and clipped.
fn request_for(model: &str, sources: &[Source]) -> ProviderRequest {
    let notes: Vec<String> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let text: String = s.text.chars().take(SOURCE_CHARS).collect();
            format!("[{}] {}\n{}", i + 1, s.header, text.trim())
        })
        .collect();
    let content = format!(
        "Notes:\n\n{}\n\nWrite the entry, every sentence ending with its notes' numbers.",
        notes.join("\n\n")
    );
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: Vec::new(),
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra: Default::default(),
        image_tokens: 0,
    }
}

/// The shadow score (§2.7) over the recalls that admitted two or more of
/// its sources: scored as its best admitted source, ranked just ahead of it
/// (rows keep no query).
pub(crate) fn score(
    sources: &[String],
    text: &str,
    rows: &[RecallManifest],
    cfg: &crate::config::MemoryConfig,
) -> Vec<Value> {
    let tokens = theseus_memory::recall::tokens_of(text);
    rows.iter()
        .filter_map(|m| {
            let admitted: Vec<pure::Admitted> = m
                .admitted
                .iter()
                .map(|a| pure::Admitted {
                    node_id: a.node_id.clone(),
                    rank: a.rank as usize,
                    tokens: a.tokens,
                })
                .collect();
            let s = pure::score(
                sources,
                tokens,
                &admitted,
                cfg.recall_max_items,
                m.budget_tokens,
            )?;
            Some(
                json!({"recall": m.recall_id, "would_select": s.would_select,
                        "rank": s.rank, "sources": s.sources}),
            )
        })
        .collect()
}
