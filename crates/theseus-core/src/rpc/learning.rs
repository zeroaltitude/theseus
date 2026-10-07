//! `judge.label` and `learning.report` (M5 25c; design §2.9, §2.13): the
//! operator's label on a judgment, judged as an approval is
//! (`judge_act(Act::JudgeLabel)`: the owner, from a private place; the CLI
//! refuses it inside a job), and the learning report, run now or read by
//! date. The run (`Core::run_learning`) is the tender's and the on-demand
//! report's alike.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use serde_json::{json, Value};
use theseus_judge::learn::Window;
use theseus_protocol::error_code;
use theseus_protocol::learning::{
    JudgeLabelParams, JudgeLabelResult, LabelCounts, LearningReport, LearningReportParams,
    PackReport,
};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::server::{Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::learning::{self, labels, report, LabelRow, LAST_RUN};
use crate::ledger::LedgerRow;

/// A report row's key: its local day and pack version.
pub fn report_key(date: &str, pack: &str) -> String {
    format!("rpt_{date}_{pack}")
}

impl Core {
    /// `judge.label`: the operator's label on one judgment, a `judge.label`
    /// row keyed `lbl_<id>` and scoped as the judgment, at weight 1.0.
    pub fn judge_label(
        &self,
        p: &JudgeLabelParams,
        who: impl Into<Answerer>,
    ) -> anyhow::Result<JudgeLabelResult> {
        let who = who.into();
        let row = self.judgment_row(&p.judgment)?;
        let pack_name = row.data["pack"].as_str().unwrap_or_default().to_string();
        let pack = self
            .runner
            .judge
            .pack(&pack_name)
            .with_context(|| format!("this build has no pack {pack_name}"))?;
        let question = p
            .question
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty());
        // A per-item question names its answer (`helps.3`, 32d): the
        // judgment must have asked it, and the label keeps the item's key.
        let about = question.and_then(|q| learning::items::asked_about(&row.data["answers"], q));
        let label = match (question, &about) {
            (Some(q), Some(_)) => labels::check_item(q, &p.label),
            (Some(q), None) => learning::items::refuse_unasked(&pack, &row.data["answers"], q)
                .and_then(|()| labels::check(&pack, Some(q), &p.label)),
            (None, _) => labels::check(&pack, None, &p.label),
        }
        .map_err(anyhow::Error::msg)?;
        let what = format!(
            "{} on {}{}",
            label,
            p.judgment,
            question.map(|q| format!(" ({q})")).unwrap_or_default()
        );
        self.judge_act(&who, Act::JudgeLabel { what: &what })?;
        let id = crate::new_id("lbl");
        let (who_s, via) = (who.who(), who.via());
        let f = fact::judge::JudgeLabel {
            id: &id,
            judgment: &p.judgment,
            pack: &pack_name,
            question,
            about: about.as_deref(),
            label: label.clone(),
            source: "operator",
            who: &who_s,
            via: &via,
            weight: 1.0,
            note: p.note.as_deref().unwrap_or(""),
            correlation_id: row.data["context"]["call"]
                .as_str()
                .or(row.data["correlation_id"].as_str()),
            rule: None,
        };
        let mut rec = fact::row(&f, row.session_id.as_deref(), None)?;
        rec.key = Some(id.clone());
        self.store
            .append(&[rec.scoped(&super::judge::scope_of(&pack_name))])?;
        self.rec(row.session_id.as_deref()).announce(&f);
        // The ladder read first, as an RPC reads it (theseus-289c): the
        // notices' brake and the ladder's rules ask what acts, which an
        // unread ladder answers in shadow.
        if self.runner.judge.config().enabled {
            self.runner.judge.read_ladder();
        }
        // v3's notices: a noise label counts toward their brake, and a
        // noticed judgment's post shows the label.
        self.runner.judge.after_label(
            self,
            &crate::judge::notice::Labeled {
                pack: &pack_name,
                judgment: &p.judgment,
                label: &label,
                who: &who_s,
                via: &via,
                session: row.session_id.as_deref(),
            },
        );
        // The ladder's rules count a day's labels (26a: `noise`, `wrong role`),
        // after the notices' brake has read this one.
        self.runner.judge.land_label(&pack_name, &label);
        Ok(JudgeLabelResult {
            id,
            judgment: p.judgment.clone(),
            pack: pack_name,
            question: question.map(str::to_string),
            about,
            label,
            source: "operator".into(),
            weight: 1.0,
        })
    }

    /// The judgment's `judge.call` row; or, while the judge's sink has not
    /// written it yet (its window is 2 s), the `tool.notified` row of the
    /// notice that names it, which carries its pack and call, so a notice's
    /// label counts the moment the notice is seen.
    fn judgment_row(&self, judgment: &str) -> anyhow::Result<LedgerRow> {
        if let Some(r) = self.store.ledger_by_key(judgment)? {
            let row: LedgerRow = r.decode()?;
            if row.kind != LedgerKind::JudgeCall.as_str() {
                anyhow::bail!("{judgment} is not a judgment");
            }
            return Ok(row);
        }
        let key = crate::judge::notice::notice_key(judgment);
        match self.store.ledger_by_key(&key)? {
            Some(r) => Ok(r.decode()?),
            None => anyhow::bail!("no judgment is named {judgment}"),
        }
    }

    pub(super) fn rpc_judge_label(
        &self,
        p: JudgeLabelParams,
        conn: Conn<'_>,
    ) -> Result<JudgeLabelResult, RpcFailure> {
        // Only the binding names a press's place, and the core judges it.
        let who = conn.answerer(None, p.discord.clone());
        self.judge_label(&p, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "a label from {} does not count: {}. Nothing was written.",
                        r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })
    }

    /// `learning.report`: the stored report of `date`, or one run now. A
    /// run is the judge's: refused with the judge off.
    pub async fn learning_report(
        self: &std::sync::Arc<Self>,
        params: Value,
    ) -> Result<LearningReport, RpcFailure> {
        // Its params are optional: none runs the report now.
        let p: LearningReportParams = match params {
            Value::Null => LearningReportParams::default(),
            v => serde_json::from_value(v).map_err(|e| RpcFailure::invalid(e.into()))?,
        };
        let core = self.clone();
        let out = tokio::task::spawn_blocking(move || match &p.date {
            Some(date) => core.stored_report(date.trim()),
            None => core
                .run_learning(theseus_protocol::now_unix_ms(), "on_demand", |_| {})
                .map(Some),
        })
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
        .map_err(RpcFailure::invalid)?;
        let mut r = out.ok_or_else(|| {
            RpcFailure::new(
                error_code::NOT_FOUND,
                "no learning report is stored for that day".to_string(),
            )
        })?;
        if let Some(pack) = p.pack.as_deref() {
            let one = pack.contains('.');
            r.packs.retain(|x| {
                if one {
                    x.pack == pack
                } else {
                    x.pack.split('.').next() == Some(pack)
                }
            });
        }
        Ok(r)
    }

    /// One run of the learning ledger, at `now_ms`: each pack's scope read,
    /// its system labels derived, its report computed, all written in one
    /// frame with the run's mark, then the day's file. `pace` is called
    /// after each pack's work with how long it took (the tender sleeps 19
    /// times that). Nothing is written when no pack has a judgment.
    pub fn run_learning(
        &self,
        now_ms: u64,
        trigger: &str,
        pace: impl FnMut(Duration),
    ) -> anyhow::Result<LearningReport> {
        Ok(self.run_learning_read(now_ms, trigger, pace)?.0)
    }

    /// `run_learning`, and what its rules read beyond the scopes: past the
    /// last run's mark, a judgment whose windows closed before it is not
    /// read again (`learning::system::Cut`, theseus-cf5c). The mark keeps
    /// the last position this run read (`through`) for the next, and where
    /// it cuts (`cut_ms`, theseus-gf8j).
    pub fn run_learning_read(
        &self,
        now_ms: u64,
        trigger: &str,
        mut pace: impl FnMut(Duration),
    ) -> anyhow::Result<(LearningReport, learning::system::RulesRead)> {
        if !self.cfg.judge.enabled {
            anyhow::bail!(
                "the judge is off ([judge] enabled = false): nothing is judged or reported"
            );
        }
        let date = crate::judge::spend::local_day(now_ms);
        let window = Window::latest(
            learning::local_midnight(now_ms),
            self.cfg.judge.holdout_days,
        );
        let mut records: Vec<NewRecord> = Vec::new();
        let mut packs: Vec<PackReport> = Vec::new();
        let mut counts = LabelCounts::default();
        let cut = self
            .store
            .get_meta::<Value>(LAST_RUN)?
            .as_ref()
            .and_then(|m| learning::system::Cut::of(m, now_ms));
        // Every row at or before this is in the scopes read below.
        let through = self.store.last_position();
        // The newest judgment's time: where the next run cuts, so a clock
        // that read ahead never closes a window still open (theseus-gf8j).
        let mut newest = 0;
        let mut read = learning::system::RulesRead::default();
        for pack_id in learning::pack_ids() {
            let t0 = Instant::now();
            let mut scope = learning::read_scope(&self.store, &pack_id)?;
            newest = scope
                .judgments
                .values()
                .flatten()
                .map(|s| s.at_ms)
                .fold(newest, u64::max);
            let (derived, rules) = self.system_labels_after(&pack_id, &scope, now_ms, cut);
            read += rules;
            for (i, l) in derived.iter().enumerate() {
                records.push(l.record()?);
                counts.system_written += 1;
                scope.label_ids.insert(l.id.clone());
                scope
                    .labels
                    .entry(l.judgment.clone())
                    .or_default()
                    .push(LabelRow {
                        // After every stored row: the run's own are newest.
                        position: u64::MAX - (derived.len() - i) as u64,
                        at_ms: now_ms,
                        id: l.id.clone(),
                        judgment: l.judgment.clone(),
                        pack: l.pack.clone(),
                        question: Some(l.question.clone()),
                        about: l.about.clone(),
                        label: l.label.clone(),
                        source: "system".into(),
                        weight: learning::SYSTEM_WEIGHT,
                    });
            }
            for l in scope.labels.values().flatten() {
                match l.source.as_str() {
                    "system" => counts.system += 1,
                    "audit" => counts.audit += 1,
                    _ => counts.operator += 1,
                }
            }
            for (name, seen) in &scope.judgments {
                packs.push(report::pack_report(name, seen, &scope.labels, window));
            }
            pace(t0.elapsed());
        }
        let r = LearningReport {
            date,
            at_unix_ms: now_ms,
            trigger: trigger.to_string(),
            labels: counts,
            packs,
        };
        tracing::info!(
            trigger,
            sessions_read = read.sessions,
            actions_read = read.actions,
            tasks_read = read.tasks,
            judgments_closed = read.closed,
            first = cut.is_none(),
            "learning: the rules read what can still change"
        );
        if r.packs.is_empty() && records.is_empty() {
            return Ok((r, read));
        }
        self.write_run(&r, records, through, now_ms.min(newest))?;
        Ok((r, read))
    }

    /// A run's frame: its labels, its `judge.report` rows and its mark,
    /// which keeps the last position the run read (`through`) and the time
    /// the next run cuts at (`cut_ms`: the run's clock, never past the
    /// newest judgment it read; `learning::system::Cut`); then the day's
    /// file.
    fn write_run(
        &self,
        r: &LearningReport,
        mut records: Vec<NewRecord>,
        through: u64,
        cut_ms: u64,
    ) -> anyhow::Result<()> {
        let facts: Vec<fact::judge::JudgeReport<'_>> = r
            .packs
            .iter()
            .map(|p| fact::judge::JudgeReport {
                date: &r.date,
                trigger: &r.trigger,
                at_unix_ms: r.at_unix_ms,
                labels: &r.labels,
                pack: p,
            })
            .collect();
        for f in &facts {
            let mut rec = fact::row(f, None, None)?;
            rec.key = Some(report_key(&r.date, &f.pack.pack));
            records.push(rec.scoped(&super::judge::scope_of(&f.pack.pack)));
        }
        records.push(NewRecord::json(
            kinds::META,
            Some(LAST_RUN),
            &json!({"at_unix_ms": r.at_unix_ms, "date": r.date, "trigger": r.trigger,
                "through": through, learning::system::CUT_KEY: cut_ms}),
        )?);
        self.store.append(&records)?;
        let rec = self.rec(None);
        for f in &facts {
            rec.announce(f);
        }
        if let Err(e) = self.write_learning_file(r) {
            tracing::warn!(error = %format!("{e:#}"), "learning: the report's file was not written; its rows hold it");
        }
        Ok(())
    }

    /// The report stored for `date`, rebuilt from its rows (and its file
    /// written again when it is missing). None when no row is that day's.
    pub fn stored_report(&self, date: &str) -> anyhow::Result<Option<LearningReport>> {
        let names: Vec<String> = theseus_judge::pack::embedded()
            .as_ref()
            .map(|ps| ps.iter().map(|p| p.name()).collect())
            .unwrap_or_default();
        let mut found: BTreeMap<String, (u64, Value)> = BTreeMap::new();
        for name in names {
            let Some(r) = self.store.ledger_by_key(&report_key(date, &name))? else {
                continue;
            };
            let row: LedgerRow = r.decode()?;
            if row.kind == LedgerKind::JudgeReport.as_str() {
                found.insert(name, (r.position, row.data));
            }
        }
        let Some((_, (_, first))) = found.iter().max_by_key(|(_, (pos, _))| *pos) else {
            return Ok(None);
        };
        let mut r = LearningReport {
            date: date.to_string(),
            at_unix_ms: first["at_unix_ms"].as_u64().unwrap_or(0),
            trigger: first["trigger"].as_str().unwrap_or_default().to_string(),
            labels: serde_json::from_value(first["labels"].clone()).unwrap_or_default(),
            packs: Vec::new(),
        };
        for (_, (_, d)) in found {
            r.packs.push(serde_json::from_value(d["report"].clone())?);
        }
        let path = self.learning_file(date);
        if !path.exists() {
            self.write_learning_file(&r)?;
        }
        Ok(Some(r))
    }

    fn learning_file(&self, date: &str) -> std::path::PathBuf {
        self.cfg
            .state_dir()
            .join("learning")
            .join(format!("{date}.json"))
    }

    /// `<state dir>/learning/<date>.json`, written whole and renamed into
    /// place.
    fn write_learning_file(&self, r: &LearningReport) -> anyhow::Result<()> {
        let path = self.learning_file(&r.date);
        let dir = path.parent().context("the learning file has a directory")?;
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!(".{}.json.tmp", r.date));
        std::fs::write(&tmp, serde_json::to_vec_pretty(r)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}
