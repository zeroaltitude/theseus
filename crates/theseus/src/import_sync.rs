//! `theseus import sync` (theseus-pjmj7): the OpenClaw import made incremental.
//!
//! It asks the daemon for its plan (`import.sync.plan`: `[import.openclaw]`'s
//! state directory, the owner's sender ids, the cursor the last run left, the
//! first run's start), runs the exporter (`scripts/openclaw-export.py`, built into
//! this binary: Python's standard library only) on OpenClaw's own stores, sends the
//! episodes it prints to `import.episodes` a batch at a time (a session that grew
//! is appended to by the daemon, which scrubs every message as it writes it), and
//! ends the run with `import.sync.record`: the new cursor, the counts, and the
//! daemon's people steps. A systemd user timer runs it nightly
//! (`scripts/user-service.sh sync-timer install`).
//!
//! - The exporter reads the stores read only, and prints episodes to a pipe, never
//!   a file: the raw text is in this process and the daemon's alone.
//! - A run that fails (no Python, an unreadable store, a refused batch) is
//!   recorded as failed with its reason, which is health's warning, and leaves the
//!   cursor where it was; what it wrote is kept, and a rerun skips it.
//! - `--dry-run` runs the exporter with no output and prints what it would read,
//!   recording nothing.
//! - The owner's act, refused inside a job: the plan is an operator's method.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, bail, Context as _, Result};
use clap::Args;
use serde::Deserialize;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEpisodesResult, ImportLine, ImportRejected,
};
use theseus_protocol::import_sync::{
    ImportSyncPlan, ImportSyncRecord, ImportSyncResult, ImportTouched, SyncPeople,
};
use theseus_protocol::method;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

use crate::cmd::output;
use crate::import::{BATCH_BYTES, BATCH_LINES, PAUSE};

/// The exporter, built in: `scripts/openclaw-export.py`.
pub const SCRIPT: &str = include_str!("../../../scripts/openclaw-export.py");

#[derive(Args, Debug, Clone, Default)]
pub struct SyncArgs {
    /// OpenClaw's state directory (the one holding agents/); default `[import.openclaw] state_dir`.
    #[arg(long = "from")]
    pub from: Option<PathBuf>,
    /// The first run's start (an age, a time of day or an RFC 3339 time); with no cursor, the
    /// default is the newest end of the 2026-10-06 import's sessions. Not with a cursor.
    #[arg(long)]
    pub since: Option<String>,
    /// The agents to read, comma separated; default `[import.openclaw] agents`, else every agent.
    #[arg(long)]
    pub agents: Option<String>,
    /// The import tag of a new session; default openclaw-sync-<year>-<month> of the run.
    #[arg(long)]
    pub tag: Option<String>,
    /// Read the stores and print what a run would take; write and record nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Import only: skip the people steps.
    #[arg(long)]
    pub no_people: bool,
}

/// The exporter's report (its `--report` JSON), the parts this reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Report {
    #[serde(default)]
    pub episodes: u64,
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub cursor: Value,
    #[serde(default)]
    pub agents: std::collections::BTreeMap<String, AgentReport>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentReport {
    #[serde(default)]
    pub sessions: u64,
    #[serde(default)]
    pub messages: u64,
    #[serde(default)]
    pub archives: u64,
    #[serde(default)]
    pub skipped: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// `~/` made the home directory.
fn expand(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(h)) => PathBuf::from(h).join(rest),
        _ => PathBuf::from(p),
    }
}

/// The exporter's arguments for a run: pure, so a test reads them.
pub fn script_args(
    plan: &ImportSyncPlan,
    a: &SyncArgs,
    state_dir: &Path,
    since_ms: Option<u64>,
    cursor_in: Option<&Path>,
    report: &Path,
    now_ms: u64,
) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "--from".into(),
        state_dir.display().to_string(),
        "--report".into(),
        report.display().to_string(),
        "--now-ms".into(),
        now_ms.to_string(),
    ];
    if let Some(c) = cursor_in {
        v.extend(["--cursor-in".into(), c.display().to_string()]);
    }
    if let Some(s) = since_ms {
        v.extend(["--since-ms".into(), s.to_string()]);
    }
    for id in &plan.owner_ids {
        v.extend(["--owner-id".into(), id.clone()]);
    }
    let agents = a.agents.clone().unwrap_or_else(|| plan.agents.join(","));
    if !agents.is_empty() {
        v.extend(["--agents".into(), agents]);
    }
    if let Some(t) = &a.tag {
        v.extend(["--tag".into(), t.clone()]);
    }
    if a.dry_run {
        v.push("--dry-run".into());
    }
    v
}

/// A private scratch directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let ms = theseus_protocol::now_unix_ms();
        let d = std::env::temp_dir().join(format!("theseus-sync-{}-{ms}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&d)
            .with_context(|| format!("making {}", d.display()))?;
        Ok(Self(d))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the episodes sent came to.
#[derive(Debug, Default)]
struct Sent {
    imported: u64,
    appended: u64,
    skipped: u64,
    rejected: Vec<ImportRejected>,
    touched: Vec<ImportTouched>,
    /// Batches sent so far: each says its number, never a total (the
    /// exporter streams), so the board shows `batch 41` until the run's
    /// record ends it (theseus-jn17m).
    batches: u32,
}

async fn send(conn: &mut Conn, name: &str, lines: Vec<ImportLine>, total: &mut Sent) -> Result<()> {
    total.batches += 1;
    let p = ImportEpisodesParams {
        file: name.to_string(),
        lines,
        batch: Some(total.batches),
        batches: None,
    };
    let v = conn
        .request(method::IMPORT_EPISODES, serde_json::to_value(p)?)
        .await?;
    let r: ImportEpisodesResult = serde_json::from_value(v)?;
    total.imported += r.imported;
    total.appended += r.appended;
    total.skipped += r.skipped;
    total.rejected.extend(r.rejected);
    total.touched.extend(r.touched);
    Ok(())
}

/// Run the exporter and send what it prints: the counts, or why it failed.
async fn export(
    conn: &mut Conn,
    name: &str,
    args: &[String],
    report: &Path,
    dry: bool,
) -> Result<(Sent, Option<Report>), String> {
    let mut child = tokio::process::Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(SCRIPT)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("python3 did not start ({e}): the exporter needs it"))?;
    let stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let err = tokio::spawn(async move {
        let mut s = String::new();
        let _ = (&mut stderr).take(64 << 10).read_to_string(&mut s).await;
        s
    });
    let mut sent = Sent::default();
    let mut reader = BufReader::new(stdout);
    let (mut batch, mut bytes, mut number) = (Vec::new(), 0usize, 0u64);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .await
            .map_err(|e| format!("reading the exporter: {e}"))?;
        if n == 0 {
            break;
        }
        number += 1;
        let Ok(text) = String::from_utf8(std::mem::take(&mut buf)) else {
            return Err("the exporter printed a line that is not UTF-8".into());
        };
        bytes += text.len();
        batch.push(ImportLine { line: number, text });
        if batch.len() >= BATCH_LINES || bytes >= BATCH_BYTES {
            send(conn, name, std::mem::take(&mut batch), &mut sent)
                .await
                .map_err(|e| format!("{e:#}"))?;
            bytes = 0;
            tokio::time::sleep(PAUSE).await;
        }
    }
    if !batch.is_empty() {
        send(conn, name, batch, &mut sent)
            .await
            .map_err(|e| format!("{e:#}"))?;
    }
    let status = child.wait().await.map_err(|e| format!("{e}"))?;
    let stderr = err.await.unwrap_or_default();
    if !status.success() {
        let tail: String = stderr
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!("the exporter failed ({status}): {tail}"));
    }
    let _ = dry;
    let rep = std::fs::read_to_string(report)
        .ok()
        .and_then(|t| serde_json::from_str::<Report>(&t).ok());
    Ok((sent, rep))
}

pub async fn run(conn: &mut Conn, json: bool, a: SyncArgs) -> Result<()> {
    let started_ms = theseus_protocol::now_unix_ms();
    let v = conn.request(method::IMPORT_SYNC_PLAN, Value::Null).await?;
    let plan: ImportSyncPlan = serde_json::from_value(v)?;
    let state_dir = match (&a.from, &plan.state_dir) {
        (Some(f), _) => f.clone(),
        (None, Some(d)) => expand(d),
        (None, None) => bail!(
            "the sync is off: set [import.openclaw] state_dir in the config (OpenClaw's state \
             directory), or name it with --from"
        ),
    };
    let since_ms = match (&plan.cursor, &a.since) {
        (Some(_), Some(_)) => bail!(
            "a cursor exists: --since only sets the first run's start (the cursor holds where \
             the last run stopped)"
        ),
        (Some(_), None) => None,
        (None, Some(s)) => Some(crate::postmortem::when(s, started_ms)?),
        (None, None) => Some(plan.since_ms.ok_or_else(|| {
            anyhow!(
                "no cursor, and no imported openclaw-store session to start after: pass --since \
                 (an age, a time of day or an RFC 3339 time)"
            )
        })?),
    };
    let scratch = Scratch::new()?;
    let report = scratch.0.join("report.json");
    let cursor_in = match &plan.cursor {
        Some(c) => {
            let p = scratch.0.join("cursor.json");
            std::fs::write(&p, serde_json::to_vec(c)?)?;
            Some(p)
        }
        None => None,
    };
    let args = script_args(
        &plan,
        &a,
        &state_dir,
        since_ms,
        cursor_in.as_deref(),
        &report,
        started_ms,
    );
    let name = format!("openclaw-sync:{}", state_dir.display());
    let got = export(conn, &name, &args, &report, a.dry_run).await;
    if a.dry_run {
        let (sent, rep) = got.map_err(|e| anyhow!(e))?;
        let _ = sent;
        let rep = rep.unwrap_or_default();
        let v = serde_json::json!({"dry_run": true, "episodes": rep.episodes, "messages": rep.messages,
            "agents": rep.agents.iter().map(|(k, v)| (k.clone(), serde_json::json!({
                "sessions": v.sessions, "messages": v.messages, "archives": v.archives,
                "skipped": v.skipped, "errors": v.errors}))).collect::<serde_json::Map<_, _>>()});
        return output(json, v, |v: Value| {
            print!("{}", dry_lines(&v));
            Ok(())
        });
    }
    let mut rec = ImportSyncRecord {
        started_ms,
        no_people: a.no_people,
        ..ImportSyncRecord::default()
    };
    let failure = match got {
        Ok((sent, rep)) => {
            let rep = rep.unwrap_or_default();
            rec.cursor = Some(rep.cursor.clone()).filter(|c| !c.is_null());
            rec.episodes = rep.episodes;
            rec.messages = rep.messages;
            rec.imported = sent.imported;
            rec.appended = sent.appended;
            rec.skipped = sent.skipped;
            rec.rejected = sent.rejected;
            rec.touched = sent.touched;
            let errors: Vec<String> = rep
                .agents
                .iter()
                .flat_map(|(k, v)| v.errors.iter().map(move |e| format!("{k}: {e}")))
                .collect();
            (!errors.is_empty()).then(|| errors.join("; "))
        }
        Err(why) => Some(why),
    };
    rec.error = failure.clone();
    let v = conn
        .request(method::IMPORT_SYNC_RECORD, serde_json::to_value(&rec)?)
        .await?;
    let samples = rec.rejected.clone();
    output(json, v, |r: ImportSyncResult| {
        print!("{}", lines(&r, &rec, &samples));
        Ok(())
    })?;
    match failure {
        Some(e) => bail!("the sync failed: {e}"),
        None => Ok(()),
    }
}

fn dry_lines(v: &Value) -> String {
    let mut out = format!(
        "dry run: {} sessions with {} messages would be taken; nothing was written or recorded\n",
        v["episodes"], v["messages"]
    );
    if let Some(a) = v["agents"].as_object() {
        for (k, x) in a {
            out.push_str(&format!(
                "  {k}: {} sessions, {} messages, {} from the archive\n",
                x["sessions"], x["messages"], x["archives"]
            ));
        }
    }
    out
}

/// A run's end in words: what it read and wrote, and its people step.
pub fn lines(r: &ImportSyncResult, rec: &ImportSyncRecord, rejected: &[ImportRejected]) -> String {
    let s = &r.sync;
    let mut out = format!(
        "sync: {} sessions read ({} messages): {} new, {} appended to, {} already held, {} refused; \
         next run {}\n",
        rec.episodes,
        rec.messages,
        s.sessions_new,
        s.sessions_appended,
        rec.skipped,
        s.rejected,
        theseus_client::render::time::fmt_date(s.next_ms),
    );
    for x in rejected.iter().take(5) {
        match &x.episode_id {
            Some(id) => out.push_str(&format!("  refused {id}: {}\n", x.why)),
            None => out.push_str(&format!("  refused: {}\n", x.why)),
        }
    }
    if let Some(p) = &r.people {
        out.push_str(&people_line(p));
    }
    if let Some(w) = &s.warn {
        out.push_str(&format!("⚠ {w}\n"));
    }
    out
}

fn people_line(p: &SyncPeople) -> String {
    format!(
        "people: {} tags' handles ({} declared, {} sessions joined); {} sessions read for people, {} \
         candidates, {} judged, ${:.4} spent{}\n",
        p.tags.len(),
        p.declared,
        p.joined,
        p.swept,
        p.candidates,
        p.judged,
        p.spent_usd,
        match (&p.stopped, p.left) {
            (Some(why), left) => format!("; stopped: {why} ({left} left for the next run)"),
            (None, 0) => String::new(),
            (None, left) => format!("; {left} left for the next run"),
        }
    )
}

#[cfg(test)]
#[path = "import_sync_tests.rs"]
mod tests;
