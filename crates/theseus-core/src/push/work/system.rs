//! The daemon's own work on the work board (theseus-jn17m): one root,
//! `Theseus`, and under it the system jobs, each made from rows the daemon
//! already writes:
//!
//! | job | rows | progress |
//! |---|---|---|
//! | an import (Claude Code, Codex, OpenClaw) | `import.batch` | `batch 41 of 120`, from the batch's number and its run's total |
//! | the OpenClaw sync | `import.batch`, ended by `import.synced` | `batch 41`; the end says its counts or its error |
//! | an erase | `import.erasing` (a frame's), then the receipt, `import.erased` | sessions erased of the run's |
//! | topics, people (placed or taken back) | `import.topics`, `import.people` | records written of the run's |
//! | consolidation | `synthesis.proposed` (one per cluster, with its place in the run) | `14 clusters` |
//! | an index rebuild | `index.rebuild` (`requested`, `progress`, `finished`, `failed`) | the share of the WAL |
//!
//! No row is written for the board: each is a row its job wrote already (the
//! new fields ride in its frame), so a job writes the frames it wrote before.
//! A job ends on its last batch, its receipt, or its end row; one whose rows
//! stop (a client that quit, a run that stopped at a limit) is `cancelled`
//! once `QUIET_MS` has passed, at the next frame (the board has no timer),
//! and an ended one leaves the board an hour after it ended (`KEEP_MS`).
//! After a restart the last hour's rows are read again (`warm_system`).

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::work::{
    Doing, DoingKind, Progress, WorkKind, WorkReport, WorkState, WorkView,
};
use theseus_protocol::work_system::{count_words, KEEP_MS, QUIET_MS, ROOT, ROOT_TITLE};

use super::views::blank;
use crate::ledger::LedgerRow;

/// The sync's source: its job is ended by `import.synced`.
const SYNC_SOURCE: &str = "openclaw-sync";

/// A rebuild may go this long between two rows (a fifth of a big WAL).
const REBUILD_QUIET_MS: u64 = 30 * 60_000;

/// Ledger kinds that make system jobs: the rows the board's allow-list
/// takes for them (`work::ROWS`, by prefix).
pub(super) const KINDS: &[&str] = &[
    "import.batch",
    "import.synced",
    "import.erased",
    "import.erasing",
    "import.topics",
    "import.people",
    "synthesis.proposed",
    "index.rebuild",
];

/// One system job, as its rows left it.
#[derive(Debug, Clone)]
struct Job {
    id: String,
    title: String,
    source: Option<String>,
    unit: &'static str,
    started_ms: u64,
    last_ms: u64,
    state: WorkState,
    since_ms: u64,
    ended_ms: Option<u64>,
    done: u64,
    total: u64,
    last_batch: u64,
    file: String,
    quiet_ms: u64,
    /// What it has done, summed over its rows.
    imported: u64,
    appended: u64,
    made: u64,
    joined: u64,
    taken: u64,
    retired: u64,
    summary: Option<String>,
    error: Option<String>,
}

impl Job {
    fn new(id: String, title: &str, unit: &'static str, at: u64) -> Self {
        Self {
            id,
            title: title.to_string(),
            source: None,
            unit,
            started_ms: at,
            last_ms: at,
            state: WorkState::Running,
            since_ms: at,
            ended_ms: None,
            done: 0,
            total: 0,
            last_batch: 0,
            file: String::new(),
            quiet_ms: QUIET_MS,
            imported: 0,
            appended: 0,
            made: 0,
            joined: 0,
            taken: 0,
            retired: 0,
            summary: None,
            error: None,
        }
    }

    fn running(&self) -> bool {
        self.state == WorkState::Running
    }

    fn end(&mut self, state: WorkState, at: u64) {
        self.state = state;
        self.since_ms = at;
        self.ended_ms = Some(at);
        self.last_ms = at;
    }
}

/// The system jobs the board holds.
#[derive(Debug, Default)]
pub(super) struct SystemJobs {
    jobs: BTreeMap<String, Job>,
    /// Each run key's newest job.
    open: BTreeMap<String, String>,
}

/// A job's place in the dirty set, and the root's.
fn touch(dirty: &mut BTreeSet<String>, id: &str) {
    dirty.insert(id.to_string());
    dirty.insert(ROOT.to_string());
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What an import's source is called.
fn source_name(source: &str) -> String {
    match source {
        "claude-code" => "Claude Code".into(),
        "codex" => "Codex".into(),
        s if s.starts_with("openclaw") => "OpenClaw".into(),
        s => s.to_string(),
    }
}

fn clip(s: &str) -> String {
    let t: String = s.chars().take(200).collect();
    t.lines().next().unwrap_or("").trim().to_string()
}

impl SystemJobs {
    /// The job for `key`: its running one, or (a new run, or none running) a
    /// new one made by `make`.
    fn job(
        &mut self,
        key: &str,
        new_run: bool,
        at: u64,
        make: impl FnOnce(String) -> Job,
    ) -> &mut Job {
        let running = self
            .open
            .get(key)
            .and_then(|id| self.jobs.get(id))
            .is_some_and(Job::running);
        if !running || new_run {
            let id = format!("sys_{}_{at}", key.replace(':', "_"));
            self.open.insert(key.to_string(), id.clone());
            self.jobs.insert(id.clone(), make(id));
        }
        let id = &self.open[key];
        self.jobs.get_mut(id).expect("made above")
    }

    /// One row the board reads. Rows of other kinds, and ones that do not
    /// say what a job needs, are left alone.
    pub(super) fn take(&mut self, r: &LedgerRow, dirty: &mut BTreeSet<String>) {
        let at = r.at_unix_ms;
        let id = match r.kind.as_str() {
            "import.batch" => self.batch(r, at),
            "import.synced" => self.synced(r, at),
            "import.erased" | "import.erasing" => self.erased(r, at),
            "import.topics" | "import.people" => self.placed(r, at),
            "synthesis.proposed" => self.cluster(r, at),
            "index.rebuild" => self.rebuild(r, at),
            _ => None,
        };
        if let Some(id) = id {
            touch(dirty, &id);
        }
    }

    fn batch(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let tag = d["tag"].as_str()?;
        let (batch, batches) = (d["batch"].as_u64(), d["batches"].as_u64());
        let source = d["source"].as_str().map(str::to_string);
        let file = d["file"].as_str().unwrap_or("").to_string();
        let key = format!("import:{tag}");
        // A run starts again at batch 1: the one running was cut short (it
        // reached a later batch), or is another file's.
        let new_run = batch == Some(1)
            && self
                .open
                .get(&key)
                .and_then(|id| self.jobs.get(id))
                .is_some_and(|j| j.running() && (j.last_batch >= 2 || j.file != file));
        let title = match source.as_deref() {
            Some(SYNC_SOURCE) => "Syncing OpenClaw history".to_string(),
            Some(s) => format!("Importing {} history", source_name(s)),
            None => format!("Importing {tag}"),
        };
        let j = self.job(&key, new_run, at, |id| Job::new(id, &title, "batch", at));
        j.source = source;
        j.file = file;
        j.last_ms = at;
        j.imported += d["imported"].as_u64().unwrap_or(0);
        j.appended += d["appended"].as_u64().unwrap_or(0);
        match batch {
            Some(b) => {
                j.last_batch = j.last_batch.max(b);
                j.done = j.done.max(b);
            }
            None => j.done += 1,
        }
        if let Some(t) = batches {
            j.total = t.max(j.done);
        }
        if batch.is_some() && batch == batches {
            let s = import_summary(j);
            j.summary = Some(s);
            j.end(WorkState::Done, at);
        }
        Some(j.id.clone())
    }

    /// The sync's end: it ends the sync's running import and says its
    /// counts, or its error; a sync that sent nothing makes its own job.
    fn synced(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let error = d["error"].as_str().filter(|e| !e.is_empty()).map(clip);
        let summary = format!(
            "{} new, {} appended to",
            plural(d["imported"].as_u64().unwrap_or(0), "session", "sessions"),
            d["appended"].as_u64().unwrap_or(0)
        );
        let running: Vec<String> = self
            .jobs
            .values()
            .filter(|j| j.running() && j.source.as_deref() == Some(SYNC_SOURCE))
            .map(|j| j.id.clone())
            .collect();
        let started = d["started_ms"].as_u64().filter(|s| *s > 0).unwrap_or(at);
        let ids = if running.is_empty() {
            let id = format!("sys_sync_{at}");
            let mut j = Job::new(id.clone(), "Syncing OpenClaw history", "batch", started);
            j.source = Some(SYNC_SOURCE.into());
            self.jobs.insert(id.clone(), j);
            vec![id]
        } else {
            running
        };
        for id in &ids {
            let j = self.jobs.get_mut(id).expect("listed");
            match &error {
                Some(e) => {
                    j.error = Some(e.clone());
                    j.end(WorkState::Failed, at);
                }
                None => {
                    j.summary = Some(summary.clone());
                    j.end(WorkState::Done, at);
                }
            }
        }
        // The last one stands for the caller's dirty set; the rest are
        // rare (two syncs open at once) and settle with the next row.
        ids.into_iter().next()
    }

    fn erased(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let tag = d["tag"].as_str()?;
        let key = format!("erase:{tag}");
        let title = format!("Erasing the import {tag}");
        let partial = r.kind == "import.erasing";
        let j = self.job(&key, false, at, |id| Job::new(id, &title, "sessions", at));
        j.last_ms = at;
        if partial {
            j.done = d["done"].as_u64().unwrap_or(j.done);
            j.total = d["of"].as_u64().unwrap_or(j.total);
        } else {
            let (s, n) = (
                d["sessions"].as_u64().unwrap_or(0),
                d["nodes"].as_u64().unwrap_or(0),
            );
            j.done = j.done.max(s);
            j.summary = Some(format!(
                "{}, {} tombstoned",
                plural(s, "session", "sessions"),
                plural(n, "node", "nodes")
            ));
            j.end(WorkState::Done, at);
        }
        Some(j.id.clone())
    }

    /// Topics or people, placed or taken back: a run of frames, each saying
    /// how many of its records are written.
    fn placed(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let tag = d["tag"].as_str()?;
        let people = r.kind == "import.people";
        let (taken, retired) = (
            d["taken"].as_u64().unwrap_or(0),
            d["retired"].as_u64().unwrap_or(0),
        );
        let back = taken > 0 || retired > 0;
        let what = if people { "people" } else { "topics" };
        let (key, title) = if back {
            (
                format!("un{what}:{tag}"),
                format!("Taking back the import {tag}'s {what}"),
            )
        } else if people {
            (
                format!("{what}:{tag}"),
                format!("Placing the import {tag} with its people"),
            )
        } else {
            (
                format!("{what}:{tag}"),
                format!("Placing the import {tag} in its topics"),
            )
        };
        let j = self.job(&key, false, at, |id| Job::new(id, &title, "records", at));
        j.last_ms = at;
        j.made += d["made"].as_u64().unwrap_or(0);
        j.joined += d["joined"].as_u64().unwrap_or(0);
        j.taken += taken;
        j.retired += retired;
        let (done, of) = (d["done"].as_u64(), d["of"].as_u64());
        j.done = done.unwrap_or(j.done + 1);
        if let Some(of) = of {
            j.total = of.max(j.done);
        }
        // Rows that say no total are one frame's: that job is done.
        if of.is_none() || done >= of {
            j.summary = Some(match (back, people) {
                (true, _) => format!(
                    "{} emptied, {} taken away",
                    plural(j.taken, "list", "lists"),
                    plural(j.retired, "entry", "entries")
                ),
                (false, true) => format!(
                    "{} placed, {} declared",
                    plural(j.joined, "session", "sessions"),
                    plural(j.made, "person", "people")
                ),
                (false, false) => format!(
                    "{} placed, {} declared",
                    plural(j.joined, "session", "sessions"),
                    plural(j.made, "topic", "topics")
                ),
            });
            j.end(WorkState::Done, at);
        }
        Some(j.id.clone())
    }

    /// One cluster's frame, with its place in its run.
    fn cluster(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let (run, n, of) = (d["run"].as_u64()?, d["n"].as_u64()?, d["of"].as_u64()?);
        let key = format!("consolidate:{run}");
        let j = self.job(&key, false, at, |id| {
            Job::new(id, "Consolidating memory", "clusters", run.min(at))
        });
        j.last_ms = at;
        j.done = j.done.max(n);
        j.total = of.max(j.done);
        if n >= of {
            let nightly = d["trigger"].as_str() == Some("nightly");
            j.summary = Some(format!(
                "{}{}",
                plural(j.done, "cluster", "clusters"),
                if nightly { ", nightly" } else { "" }
            ));
            j.end(WorkState::Done, at);
        }
        Some(j.id.clone())
    }

    fn rebuild(&mut self, r: &LedgerRow, at: u64) -> Option<String> {
        let d = &r.data;
        let run = d["run"].as_u64()?;
        let key = format!("rebuild:{run}");
        let j = self.job(&key, false, at, |id| {
            let mut j = Job::new(id, "Rebuilding the index", "%", run.min(at));
            j.quiet_ms = REBUILD_QUIET_MS;
            j
        });
        j.last_ms = at;
        match d["event"].as_str()? {
            "progress" => {
                if let (Some(done), Some(total)) =
                    (d["done_bytes"].as_u64(), d["total_bytes"].as_u64())
                {
                    if let Some(pct) = (done * 100).checked_div(total) {
                        j.done = pct.min(100);
                        j.total = 100;
                    }
                }
            }
            "finished" => {
                j.done = 100;
                j.total = 100;
                j.end(WorkState::Done, at);
            }
            "failed" => {
                j.error = d["why"].as_str().map(clip);
                j.end(WorkState::Failed, at);
            }
            _ => {}
        }
        Some(j.id.clone())
    }

    /// Jobs whose rows stopped end (cancelled), and jobs that ended an hour
    /// ago leave. Run on every frame; a board with no system job does
    /// nothing.
    pub(super) fn sweep(&mut self, now: u64, dirty: &mut BTreeSet<String>) {
        if self.jobs.is_empty() {
            return;
        }
        let mut gone = Vec::new();
        for j in self.jobs.values_mut() {
            if j.running() && now.saturating_sub(j.last_ms) > j.quiet_ms {
                if j.done > 0 {
                    j.summary = Some(format!(
                        "at {}",
                        count_words(j.done as u32, j.total as u32, j.unit)
                    ));
                }
                let at = j.last_ms;
                j.end(WorkState::Cancelled, at);
                touch(dirty, &j.id.clone());
            } else if j.ended_ms.is_some_and(|e| now.saturating_sub(e) > KEEP_MS) {
                gone.push(j.id.clone());
            }
        }
        for id in gone {
            self.jobs.remove(&id);
            self.open.retain(|_, v| *v != id);
            touch(dirty, &id);
        }
    }

    /// The view of `id`: the root while any job is held, else a job.
    pub(super) fn view(&self, id: &str) -> Option<WorkView> {
        if id == ROOT {
            return self.root();
        }
        self.jobs.get(id).map(job_view)
    }

    fn root(&self) -> Option<WorkView> {
        let first = self.jobs.values().map(|j| j.started_ms).min()?;
        let last = self.jobs.values().map(|j| j.last_ms).max()?;
        let mut v = blank(ROOT, WorkKind::System, last);
        v.title = ROOT_TITLE.to_string();
        v.state = WorkState::Ready;
        v.started_at_ms = first;
        v.state_since_ms = first;
        Some(v)
    }

    /// Forget a job the board pruned.
    pub(super) fn forget(&mut self, id: &str) {
        self.jobs.remove(id);
        self.open.retain(|_, v| v != id);
    }
}

fn import_summary(j: &Job) -> String {
    let mut s = plural(j.imported, "session", "sessions");
    if j.appended > 0 {
        s.push_str(&format!(", {} grown", j.appended));
    }
    s
}

fn job_view(j: &Job) -> WorkView {
    let mut v = blank(&j.id, WorkKind::System, j.last_ms);
    v.parent = Some(ROOT.to_string());
    v.root = ROOT.to_string();
    v.title = j.title.clone();
    v.state = j.state;
    v.started_at_ms = j.started_ms;
    v.state_since_ms = j.since_ms;
    v.ended_at_ms = j.ended_ms;
    if j.done > 0 || j.total > 0 {
        v.progress = Some(Progress {
            steps_done: j.done as u32,
            steps_total: j.total as u32,
            current: Some(j.unit.to_string()),
            said: None,
            at_ms: j.last_ms,
        });
    }
    if j.running() {
        let label = if j.done > 0 || j.total > 0 {
            count_words(j.done as u32, j.total as u32, j.unit)
        } else {
            String::new()
        };
        v.doing = Some(Doing {
            what: DoingKind::Job,
            label,
            since_ms: j.started_ms,
            turn: 0,
            loop_index: 0,
            last_line: None,
        });
    } else {
        v.report = Some(WorkReport {
            outcome: match j.state {
                WorkState::Done => "done",
                WorkState::Failed => "failed",
                _ => "cancelled",
            }
            .to_string(),
            first_line: j.summary.clone(),
            reason: j.error.clone(),
        });
    }
    v
}
