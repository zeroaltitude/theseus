//! `bench turn --session-nodes N` and `bench idle --active N` (step 33,
//! theseus-6fn.13; design M6 §2.10's bench rows).
//!
//! - **A long session**: one session of `N` nodes, exchanges in which the
//!   model reads a large file and answers (`synth::long_session`), written
//!   before the daemon starts, then turns measured in it on the stand-in
//!   model: each turn's wall time, its frames, and the nodes the daemon
//!   decoded for it (health's `store.node_cache.decodes`, read before and after:
//!   a build from before tiering has no such count, and the row says so),
//!   and resident memory, the index tender's included.
//! - **Active sessions**: after `bench idle`'s window, `N` sessions opened
//!   and a turn in each, then resident memory again, the tender's included:
//!   with `--sessions 10000`, §9's row of 10,000 parked and 50 active.

use super::*;

/// The index tender's resident memory, in kB: the `theseus-index` child of
/// `pid`, read from `/proc` (0 while none runs).
pub fn tender_rss_kb(pid: u32) -> u64 {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return 0;
    };
    let mut kb = 0;
    for e in dir.flatten() {
        let Some(child) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else {
            continue;
        };
        // `pid (comm) state ppid …`: the name may hold spaces, so from its `)`.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let comm = &stat[open + 1..close];
        let ppid = stat[close + 1..].split_whitespace().nth(1);
        if ppid == Some(pid.to_string().as_str()) && comm.starts_with("theseus-index") {
            kb += procfs::sample(child).map_or(0, |s| s.rss_kb);
        }
    }
    kb
}

/// The daemon's count of nodes it decoded (`node_cache.decodes` in health),
/// or none from a build before tiering.
fn decodes(rig: &Rig) -> Result<Option<u64>> {
    let h = rig.call("health", json!({}))?;
    Ok(h["store"]["node_cache"]["decodes"].as_u64())
}

pub struct LongOpts {
    pub theseusd: PathBuf,
    pub runs: usize,
    pub nodes: u64,
    pub result_bytes: usize,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct LongReport {
    pub theseusd: String,
    pub nodes: u64,
    pub result_bytes: usize,
    pub generated: crate::synth::Generated,
    pub runs: usize,
    pub start_ms: f64,
    /// Each measured turn's wall time, in ms, the first (which compiles the
    /// session for the first time) included.
    pub wall_each: Vec<f64>,
    pub wall_ms: Summary,
    pub daemon_ms: Summary,
    pub frames_each: Vec<u64>,
    /// Nodes decoded for each turn; empty from a build that does not count.
    pub decodes_each: Vec<u64>,
    /// The turns after the first, whose session the daemon has read before.
    pub decodes_after_first: Option<Summary>,
    pub rss_start: Sample,
    pub rss_after: Sample,
    pub tender_start_kb: u64,
    pub tender_after_kb: u64,
    /// Health's cache block after the turns, as the daemon said it.
    pub node_cache: Value,
    pub bench_ms: f64,
}

impl LongReport {
    pub fn columns(&self) -> Vec<(String, Summary)> {
        let mut out = vec![
            ("turn_long".to_string(), self.wall_ms),
            (
                "rss_long".to_string(),
                single((self.rss_after.rss_kb + self.tender_after_kb) as f64 / 1024.0),
            ),
        ];
        if let Some(d) = self.decodes_after_first {
            out.push(("decodes_long".to_string(), d));
        }
        out
    }
}

pub fn run_long(o: &LongOpts) -> Result<LongReport> {
    let t0 = Instant::now();
    let s = scratch(&o.theseusd, o.dir.as_deref())?;
    let (session, generated) =
        crate::synth::long_session(&s.rig.state.join("store"), o.nodes, o.result_bytes)?;
    let (mut daemon, start) = s.rig.start()?;
    let pid = daemon.0.id();
    let mut tail = Tail::at_start(&s.wal());
    after_serving(&mut tail)?;
    // The tender starts after serving and reads the WAL: its start is not a turn's.
    std::thread::sleep(START_AFTER + Duration::from_millis(500));
    until_quiet(&mut tail)?;
    let (rss_start, tender_start_kb) = (procfs::sample(pid)?, tender_rss_kb(pid));
    let mut d = Driver { rig: &s.rig, tail };
    let (mut wall, mut daemon_ms, mut frames_each, mut decodes_each) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for i in 0..o.runs {
        let before = decodes(&s.rig)?;
        let (r, w, frames) = d.measured(&session, &format!("a turn in a long session {i}"))?;
        if r["output"].is_null() && r["error"].is_object() {
            bail!("a turn in the long session failed: {r}");
        }
        let after = decodes(&s.rig)?;
        wall.push(w);
        daemon_ms.push(r["elapsed_ms"].as_f64().unwrap_or(0.0));
        frames_each.push(frames.len() as u64);
        if let (Some(a), Some(b)) = (after, before) {
            decodes_each.push(a.saturating_sub(b));
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    let (rss_after, tender_after_kb) = (procfs::sample(pid)?, tender_rss_kb(pid));
    let node_cache = s.rig.call("health", json!({}))?["store"]["node_cache"].clone();
    s.rig.stop(&mut daemon)?;
    let later: Vec<f64> = decodes_each.iter().skip(1).map(|n| *n as f64).collect();
    Ok(LongReport {
        theseusd: o.theseusd.display().to_string(),
        nodes: o.nodes,
        result_bytes: o.result_bytes,
        generated,
        runs: o.runs,
        start_ms: start.ms,
        wall_ms: Summary::of(&wall).context("no runs")?,
        daemon_ms: Summary::of(&daemon_ms).context("no runs")?,
        wall_each: wall,
        frames_each,
        decodes_after_first: Summary::of(&later),
        decodes_each,
        rss_start,
        rss_after,
        tender_start_kb,
        tender_after_kb,
        node_cache,
        bench_ms: ms_since(t0),
    })
}

pub fn print_long(r: &LongReport) {
    println!(
        "bench turn · {} · {} measured turns in one session of {} nodes ({} B tool results, {:.1} MB of WAL), \
         on the stand-in model, Discord and the web UI off",
        r.theseusd,
        r.runs,
        r.nodes,
        r.result_bytes,
        r.generated.wal_bytes as f64 / 1e6
    );
    println!(
        "  wall p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms; the daemon's p50 {:.1} ms; each: {}",
        r.wall_ms.p50,
        r.wall_ms.p95,
        r.wall_ms.max,
        r.daemon_ms.p50,
        r.wall_each
            .iter()
            .map(|w| format!("{w:.0}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!("  frames each: {}", frames_text(&r.frames_each));
    if r.decodes_each.is_empty() {
        println!("  nodes decoded: not counted by this build (before tiering, each turn decodes the session whole)");
    } else {
        println!(
            "  nodes decoded each turn: {}",
            r.decodes_each
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    println!(
        "  resident memory: {:.1} MB after the start, {:.1} MB (peak {:.1}) after the turns; the tender {:.1} MB, then {:.1} MB",
        r.rss_start.rss_mb(),
        r.rss_after.rss_mb(),
        r.rss_after.hwm_mb(),
        r.tender_start_kb as f64 / 1024.0,
        r.tender_after_kb as f64 / 1024.0
    );
    if !r.node_cache.is_null() {
        println!("  the node cache: {}", r.node_cache);
    }
    println!("  the bench took {:.1} s", r.bench_ms / 1000.0);
}

/// `bench idle --active N`'s half: `n` sessions opened and a turn in each,
/// then the daemon's memory and the tender's.
pub fn active(rig: &Rig, pid: u32, n: usize) -> Result<(Sample, u64)> {
    for i in 0..n {
        let r = rig.call(
            "session.open",
            json!({ "label": format!("bench active {i}") }),
        )?;
        let sid = r["session_id"]
            .as_str()
            .context("session.open gave no session id")?;
        rig.call(
            "turn.submit",
            json!({"session_id": sid, "input": format!("active turn {i}"), "author": "bench", "attachments": []}),
        )?;
    }
    std::thread::sleep(Duration::from_millis(300));
    Ok((procfs::sample(pid)?, tender_rss_kb(pid)))
}

/// A long report's columns, for the history's test.
#[cfg(test)]
pub fn sample_columns() -> Vec<(String, Summary)> {
    let one = Summary::of(&[1.0]).unwrap();
    LongReport {
        theseusd: String::new(),
        nodes: 2000,
        result_bytes: 8192,
        generated: crate::synth::Generated {
            sessions: 1,
            records: 0,
            frames: 0,
            wal_bytes: 0,
            ms: 0.0,
        },
        runs: 1,
        start_ms: 0.0,
        wall_each: vec![1.0],
        wall_ms: one,
        daemon_ms: one,
        frames_each: vec![5],
        decodes_each: vec![1],
        decodes_after_first: Some(one),
        rss_start: Sample::default(),
        rss_after: Sample::default(),
        tender_start_kb: 0,
        tender_after_kb: 0,
        node_cache: Value::Null,
        bench_ms: 0.0,
    }
    .columns()
}

#[cfg(test)]
mod tests {
    /// A long session reads back as the product reads it: its nodes in
    /// order, five to an exchange, every call answered.
    #[test]
    fn a_long_session_reads_back_whole() {
        let d = tempfile::tempdir().unwrap();
        let (sid, g) = crate::synth::long_session(d.path(), 23, 300).unwrap();
        assert_eq!(g.sessions, 1);
        let store = theseus_core::store::Store::open(d.path()).unwrap();
        let nodes = store.session_nodes(&sid).unwrap();
        assert_eq!(nodes.len(), 23);
        let kinds: Vec<&str> = nodes.iter().take(5).map(|(_, n)| n.kind_str()).collect();
        assert_eq!(
            kinds,
            [
                "user_message",
                "assistant_message",
                "tool_call",
                "tool_result",
                "assistant_message"
            ]
        );
        let theseus_core::node::Body::ToolResult { content, .. } = &nodes[3].1.body else {
            panic!("a result");
        };
        assert_eq!(content.len(), 300);
    }
}
