//! `bench turn --judge` (theseus-0j2.8): the judge's cost on a turn's path,
//! beside the judge-off turns the gate measures.
//!
//! - **Three arms**, each a scratch daemon of its own on the stand-in model:
//!   `off` (the gate's turn bench, `quiet_config`), `loop` (the judge on at
//!   theseus-judge's fake Jev, started in this process as `scratch` starts
//!   the stand-in model, with the inbound point's packs off, as theseusd's
//!   judge test runs it), and `packs` (every pack as this build wires it:
//!   `route.v1` live, so each person's message waits beside its first
//!   compile for the fake's verdict, scripted to a confident `chat`).
//! - **Each frame by what it holds**: the judge's (every record a
//!   `judge.*` or `pack.*` row, or a `judge.*` META record: the sink's
//!   frame, a budget block, the ladder's rows), or the turn's, which are
//!   checked against its trace as `bench turn` checks them. A judge frame
//!   lands before a turn's answer, after it (inside the 50 ms the turn's
//!   frames are counted to), or between turns.
//! - **The first turn** of each judged arm is submitted the moment its fresh
//!   daemon answers, and the ladder's `pack.mode` frames around it counted:
//!   the warm read writes its adoptions between turns (theseus-289c).
//! - **The disk's share**: the judge's frames and the blobs the store gained
//!   (each judged state's file and its directory, two syncs the WAL never
//!   sees), at the disk probe's `fdatasync`.
//!
//! Nothing here is gated: `--check` judges the `off` arm's frames against
//! today's budgets, and the judge-on arms go to the history under columns
//! of their own.

use std::collections::BTreeMap;

use theseus_judge::fake::{FakeJev, Scripted};

use super::*;

/// The arms, in the order they run.
pub const ARMS: [Arm; 3] = [Arm::Off, Arm::Loop, Arm::Packs];

/// How long the WAL must be still before a judged daemon is idle: the
/// sink's window (`theseus_core::judge::FLUSH_EVERY`), and a second more.
fn drained() -> Duration {
    theseus_core::judge::FLUSH_EVERY + Duration::from_secs(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Off,
    Loop,
    Packs,
}

impl Arm {
    pub fn name(self) -> &'static str {
        match self {
            Arm::Off => "off",
            Arm::Loop => "loop",
            Arm::Packs => "packs",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Arm::Off => "the judge off",
            Arm::Loop => "the judge on, classify, role and route off",
            Arm::Packs => "the judge on, every pack as wired (route.v1 live)",
        }
    }

    /// The history's columns of a judged arm: its turns' wall times, and
    /// the judge's frames a turn's answer waited behind.
    fn columns(self) -> Option<[&'static str; 3]> {
        match self {
            Arm::Off => None,
            Arm::Loop => Some(["turn_plain_jloop", "turn_tool_jloop", "jframes_jloop"]),
            Arm::Packs => Some(["turn_plain_jpacks", "turn_tool_jpacks", "jframes_jpacks"]),
        }
    }
}

/// Whether one record is the judge's own: a `judge.*` or `pack.*` row, or a
/// `judge.*` META record (the shadow budget's, a mark).
pub fn judges(record: &str) -> bool {
    record.starts_with("ledger:judge.")
        || record.starts_with("ledger:pack.")
        || record.starts_with("meta:judge.")
}

/// Whether a frame is the judge's alone. A frame that mixes the judge's
/// records with others' is not: it is counted as the turn's, and its trace
/// says whether it is.
pub fn is_judges(f: &Frame) -> bool {
    !f.records.is_empty() && f.records.iter().all(|r| judges(r))
}

/// Where the judge's frames landed, against the turns measured.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Placed {
    /// Inside a turn, before its answer: the turn's answer waited behind
    /// it whenever the store's one writer was syncing it.
    pub before_answer: u64,
    /// After an answer, inside the 50 ms its frames are counted to.
    pub after_answer: u64,
    /// Between two turns, with neither running.
    pub between: u64,
}

impl Placed {
    fn total(self) -> u64 {
        self.before_answer + self.after_answer + self.between
    }

    fn add(&mut self, o: Placed) {
        self.before_answer += o.before_answer;
        self.after_answer += o.after_answer;
        self.between += o.between;
    }
}

/// The first turn on a fresh store, submitted at once after serving, and the
/// ladder's `pack.mode` frames (the warm read's adoptions) around it.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct FirstTurn {
    /// Written before the turn was submitted.
    pub before_submit: u64,
    /// Written from its submit to its answer.
    pub before_answer: u64,
    /// Written after its answer, inside the 50 ms its frames are counted to.
    pub after_answer: u64,
    /// The turn's frames by its trace's count: a first judgment that read
    /// the ladder on the turn's path wrote its adoptions there.
    pub trace_frames: u64,
}

fn pack_modes(frames: &[Frame]) -> u64 {
    frames
        .iter()
        .filter(|f| f.records.iter().any(|r| r == "ledger:pack.mode"))
        .count() as u64
}

/// The first turn, submitted the moment the daemon answers: where the
/// ladder's `pack.mode` frames land around it.
fn first_turn(rig: &Rig, tail: &mut Tail) -> Result<FirstTurn> {
    let before = tail.read()?;
    let r = rig.call(
        "turn.submit",
        json!({"input": "a first turn, at once", "author": "bench", "attachments": []}),
    )?;
    let answered = tail.read()?;
    let after = until_quiet(tail)?;
    Ok(FirstTurn {
        before_submit: pack_modes(&before),
        before_answer: pack_modes(&answered),
        after_answer: pack_modes(&after),
        trace_frames: r["trace"]["attrs"]["frames"].as_u64().unwrap_or(0),
    })
}

/// One kind of turn in one arm.
#[derive(Debug, Serialize)]
pub struct JudgedKind {
    pub name: String,
    pub wall_ms: Summary,
    pub daemon_ms: Summary,
    /// The turn's own frames, each run's checked against its trace.
    pub frames: Summary,
    pub frames_each: Vec<u64>,
    /// The judge's frames before each run's answer.
    pub judge_before_each: Vec<u64>,
    pub placed: Placed,
    /// The last run's frames, each `turn [...]` or `judge [...]`.
    pub last_frames: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ArmReport {
    pub arm: Arm,
    /// The first turn, submitted as soon as the daemon answers, on a fresh
    /// store: the ladder's rows around it (theseus-289c). None with the
    /// judge off.
    pub first: Option<FirstTurn>,
    pub plain: JudgedKind,
    pub tool: JudgedKind,
    /// The judge's frames after the last measured turn, once the sink's
    /// window has passed.
    pub trailing: u64,
    /// Each shape of the judge's frames over the measured turns and after
    /// them, with its count.
    pub judge_shapes: BTreeMap<String, u64>,
    /// Blobs the store gained over the measured turns and after them: two
    /// syncs each.
    pub blobs: u64,
    /// The fake Jev's calls and warm-ups over the measured turns and after.
    pub jev_calls: u64,
    pub jev_warmups: u64,
    /// Measured turns, of both kinds.
    pub turns: usize,
}

impl ArmReport {
    /// The judge's frames over the measured turns and after them.
    pub fn judge_frames(&self) -> u64 {
        let mut p = self.plain.placed;
        p.add(self.tool.placed);
        p.total() + self.trailing
    }

    /// The `fdatasync`s the judge added, frames and blob syncs, at `fsync`
    /// ms each: in all, and a turn.
    pub fn fsync_added_ms(&self, fsync: f64) -> (f64, f64) {
        let syncs = self.judge_frames() + 2 * self.blobs;
        let all = syncs as f64 * fsync;
        (all, all / self.turns.max(1) as f64)
    }
}

#[derive(Debug, Serialize)]
pub struct JudgeReport {
    pub theseusd: String,
    pub runs: usize,
    pub arms: Vec<ArmReport>,
    pub fsync_ms: Summary,
    pub fsync_probes_ms: [f64; 2],
    /// The `off` arm's frames against today's budgets.
    pub verdicts: Vec<Verdict>,
    pub wall_ms: f64,
}

impl JudgeReport {
    pub fn ok(&self) -> bool {
        self.verdicts.iter().all(|v| v.ok)
    }

    fn arm(&self, arm: Arm) -> Option<&ArmReport> {
        self.arms.iter().find(|a| a.arm == arm)
    }

    /// The `off` arm's columns, as `bench turn` records them, then each
    /// judged arm's own.
    pub fn columns(&self) -> Vec<(String, Summary)> {
        let mut out = Vec::new();
        if let Some(off) = self.arm(Arm::Off) {
            out.extend([
                ("turn_plain".to_string(), off.plain.wall_ms),
                ("frames_plain".to_string(), off.plain.frames),
                ("turn_tool".to_string(), off.tool.wall_ms),
                ("frames_tool".to_string(), off.tool.frames),
            ]);
        }
        for a in &self.arms {
            let Some([plain, tool, before]) = a.arm.columns() else {
                continue;
            };
            let each: Vec<f64> = a
                .plain
                .judge_before_each
                .iter()
                .chain(&a.tool.judge_before_each)
                .map(|n| *n as f64)
                .collect();
            out.push((plain.to_string(), a.plain.wall_ms));
            out.push((tool.to_string(), a.tool.wall_ms));
            if let Some(s) = Summary::of(&each) {
                out.push((before.to_string(), s));
            }
        }
        out
    }
}

pub struct JudgeOpts {
    pub theseusd: PathBuf,
    pub runs: usize,
    pub dir: Option<PathBuf>,
}

/// `[judge]` on at the fake Jev, for `arm`.
pub(super) fn judged(t: &mut toml::Table, base: &str, arm: Arm) -> Result<()> {
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> Result<&'a mut toml::Table> {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .with_context(|| format!("[{key}] is a table"))
    }
    let j = table(t, "judge")?;
    j.insert("enabled".into(), true.into());
    j.insert("api_base".into(), base.into());
    j.insert("connect_secs".into(), 1.into());
    j.insert("total_secs".into(), 5.into());
    if arm == Arm::Loop {
        let packs = table(j, "packs")?;
        for p in ["classify.v1", "role.v1", "route.v1"] {
            let mut off = toml::Table::new();
            off.insert("mode".into(), "off".into());
            packs.insert(p.into(), off.into());
        }
    }
    Ok(())
}

/// The fake Jev's answers: a confident `chat` for `route.v1` (a switch to
/// chat's profile, which the bench points at the stand-in too), and the
/// bench's `true` sure to be harmless, so no notice is posted.
pub(super) fn scripted(jev: &FakeJev) {
    jev.script(
        "route.v1/mode",
        Scripted::Choice {
            option: "chat".into(),
            confidence: 0.95,
        },
    );
    jev.script("security.v3/risky", Scripted::Noul(0.02));
    jev.script("security.v1/risky", Scripted::Noul(0.02));
}

/// The WAL read until it has been still for `still`: an error when it never
/// settles within `deadline`.
fn until_still(tail: &mut Tail, still: Duration, deadline: Duration) -> Result<Vec<Frame>> {
    let mut seen = Vec::new();
    let mut still_since = Instant::now();
    let end = Instant::now() + deadline;
    loop {
        let more = tail.read()?;
        if more.is_empty() {
            if still_since.elapsed() >= still {
                return Ok(seen);
            }
        } else {
            seen.extend(more);
            still_since = Instant::now();
        }
        if Instant::now() > end {
            bail!(
                "the WAL was still growing after {} s: the daemon is not idle",
                deadline.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The files under the store's blobs, counted.
fn blobs(state: &Path) -> u64 {
    fn count(dir: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        rd.flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => count(&e.path()),
                Ok(t) if t.is_file() => 1,
                _ => 0,
            })
            .sum()
    }
    count(&state.join("store").join("blobs"))
}

/// A frame in one short line: a run of one record collapsed, as
/// `[ledger:judge.call ×27, meta:judge.budget]`.
pub fn short(f: &Frame) -> String {
    let mut runs: Vec<(&str, usize)> = Vec::new();
    for r in &f.records {
        match runs.last_mut() {
            Some((last, n)) if *last == r.as_str() => *n += 1,
            _ => runs.push((r, 1)),
        }
    }
    let parts: Vec<String> = runs
        .into_iter()
        .map(|(r, n)| match n {
            1 => r.to_string(),
            n => format!("{r} ×{n}"),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn tagged(f: &Frame) -> String {
    match is_judges(f) {
        true => format!("judge {}", short(f)),
        false => format!("turn {}", f.label()),
    }
}

/// `runs` measured turns of one kind, each frame told the judge's or the
/// turn's.
fn kind(
    d: &mut Driver<'_>,
    shapes: &mut BTreeMap<String, u64>,
    name: &str,
    session: &str,
    input: &str,
    runs: usize,
    shape: (u64, u64),
) -> Result<JudgedKind> {
    let (mut wall, mut daemon, mut frames_each, mut before_each) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut placed = Placed::default();
    let mut last = Vec::new();
    let seen = |frames: &[Frame], shapes: &mut BTreeMap<String, u64>| -> u64 {
        let judge: Vec<&Frame> = frames.iter().filter(|f| is_judges(f)).collect();
        for f in &judge {
            *shapes.entry(short(f)).or_default() += 1;
        }
        judge.len() as u64
    };
    for i in 0..runs {
        // What came since the last turn's frames were counted: the judge's
        // between turns, and a straggler of the turn's, as `bench turn`
        // leaves it, not this turn's.
        let between = until_quiet(&mut d.tail)?;
        placed.between += seen(&between, shapes);
        let t = Instant::now();
        let r = d.submit(session, &format!("{input} {i}"))?;
        let w = ms_since(t);
        let answered = d.tail.read()?;
        let after = until_quiet(&mut d.tail)?;
        let before = seen(&answered, shapes);
        placed.before_answer += before;
        placed.after_answer += seen(&after, shapes);
        let got = (
            r["loops"].as_u64().unwrap_or(0),
            r["tool_calls"].as_u64().unwrap_or(0),
        );
        if got != shape {
            bail!(
                "a {name} turn ran {} loop(s) and {} tool call(s), not {} and {}: it answered {:?}",
                got.0,
                got.1,
                shape.0,
                shape.1,
                r["output"]
            );
        }
        let all: Vec<Frame> = answered.into_iter().chain(after).collect();
        let turns: Vec<&Frame> = all.iter().filter(|f| !is_judges(f)).collect();
        let counted = r["trace"]["attrs"]["frames"].as_u64();
        if counted != Some(turns.len() as u64) {
            bail!(
                "a {name} turn's trace counts {counted:?} frames, and the WAL holds {} of the turn's: {}",
                turns.len(),
                all.iter().map(tagged).collect::<Vec<_>>().join(" ")
            );
        }
        wall.push(w);
        daemon.push(r["elapsed_ms"].as_f64().unwrap_or(0.0));
        frames_each.push(turns.len() as u64);
        before_each.push(before);
        last = all.iter().map(tagged).collect();
    }
    let frames: Vec<f64> = frames_each.iter().map(|n| *n as f64).collect();
    Ok(JudgedKind {
        name: name.to_string(),
        wall_ms: Summary::of(&wall).context("no runs")?,
        daemon_ms: Summary::of(&daemon).context("no runs")?,
        frames: Summary::of(&frames).context("no runs")?,
        frames_each,
        judge_before_each: before_each,
        placed,
        last_frames: last,
    })
}

/// One arm: its own scratch daemon, the warm-ups, the judge's frames of
/// them written, then the measured turns.
fn run_arm(o: &JudgeOpts, arm: Arm) -> Result<ArmReport> {
    let dir = o.dir.as_ref().map(|d| d.join(arm.name()));
    let jev = match arm {
        Arm::Off => None,
        _ => {
            let j = FakeJev::start()?;
            scripted(&j);
            Some(j)
        }
    };
    let s = match &jev {
        None => scratch(&o.theseusd, dir.as_deref())?,
        Some(j) => scratch_with(&o.theseusd, dir.as_deref(), |t| judged(t, &j.base(), arm))?,
    };
    let (mut daemon, _) = s.rig.start()?;
    let mut tail = Tail::at_start(&s.wal());
    let first = match arm {
        Arm::Off => None,
        _ => Some(first_turn(&s.rig, &mut tail)?),
    };
    after_serving(&mut tail)?;
    let mut d = Driver { rig: &s.rig, tail };
    let session = d.open_session(&format!("bench judge {}", arm.name()))?;
    for input in ["warm up", "warm up again", "warm up with bench-tool"] {
        d.submit(&session, input)?;
    }
    // The warm-ups' judgments written: the measured turns start idle.
    until_still(&mut d.tail, drained(), Duration::from_secs(60))?;
    let (blobs0, calls0, warm0) = (
        blobs(&s.rig.state),
        jev.as_ref().map_or(0, FakeJev::connections),
        jev.as_ref().map_or(0, FakeJev::warmups),
    );
    let mut shapes = BTreeMap::new();
    let plain = kind(
        &mut d,
        &mut shapes,
        "plain",
        &session,
        "a plain turn",
        o.runs,
        (1, 0),
    )?;
    let tool = kind(
        &mut d,
        &mut shapes,
        "tool-call",
        &session,
        &format!("a turn with a call, {TOOL_MARK}"),
        o.runs,
        (2, 1),
    )?;
    let after = until_still(&mut d.tail, drained(), Duration::from_secs(60))?;
    let trailing: Vec<&Frame> = after.iter().filter(|f| is_judges(f)).collect();
    for f in &trailing {
        *shapes.entry(short(f)).or_default() += 1;
    }
    let report = ArmReport {
        arm,
        first,
        plain,
        tool,
        trailing: trailing.len() as u64,
        judge_shapes: shapes,
        blobs: blobs(&s.rig.state).saturating_sub(blobs0),
        jev_calls: jev
            .as_ref()
            .map_or(0, |j| j.connections().saturating_sub(calls0) as u64),
        jev_warmups: jev
            .as_ref()
            .map_or(0, |j| j.warmups().saturating_sub(warm0) as u64),
        turns: 2 * o.runs,
    };
    s.rig.stop(&mut daemon)?;
    Ok(report)
}

pub fn run_judge(o: &JudgeOpts) -> Result<JudgeReport> {
    let wall = Instant::now();
    let probe_dir = match &o.dir {
        Some(d) => {
            std::fs::create_dir_all(d)?;
            d.clone()
        }
        None => std::env::temp_dir(),
    };
    let fsync_before = fsync_probe(&probe_dir, 20)?;
    let arms = ARMS
        .iter()
        .map(|a| run_arm(o, *a).with_context(|| format!("the {} arm", a.name())))
        .collect::<Result<Vec<_>>>()?;
    let fsync_after = fsync_probe(&probe_dir, 20)?;
    let fsync_probes_ms = [fsync_before.p50, fsync_after.p50];
    let fsync_ms = if fsync_after.p50 < fsync_before.p50 {
        fsync_after
    } else {
        fsync_before
    };
    let off = arms
        .iter()
        .find(|a| a.arm == Arm::Off)
        .context("no off arm")?;
    let verdicts = turn_verdicts(&off.plain.frames, &off.tool.frames);
    Ok(JudgeReport {
        theseusd: o.theseusd.display().to_string(),
        runs: o.runs,
        arms,
        fsync_ms,
        fsync_probes_ms,
        verdicts,
        wall_ms: ms_since(wall),
    })
}

pub fn print_judge(r: &JudgeReport) {
    println!(
        "bench turn --judge · {} · {} runs of each kind in each arm, on the stand-in model, \
         Discord and the web UI off, Jev the fake",
        r.theseusd, r.runs
    );
    println!(
        "  {:<6} {:<10} {:>7} {:>9} {:>9} {:>9}   judge frames: before the answer · after · between",
        "arm", "turn", "frames", "wall p50", "wall p95", "max"
    );
    for a in &r.arms {
        for k in [&a.plain, &a.tool] {
            println!(
                "  {:<6} {:<10} {:>7} {:>6.1} ms {:>6.1} ms {:>6.1} ms   {} · {} · {}",
                a.arm.name(),
                k.name,
                frames_text(&k.frames_each),
                k.wall_ms.p50,
                k.wall_ms.p95,
                k.wall_ms.max,
                k.placed.before_answer,
                k.placed.after_answer,
                k.placed.between
            );
        }
    }
    let fsync = r.fsync_ms.p50;
    println!(
        "  this disk's fdatasync: p50 {fsync:.2} ms, the quieter of two probes ({:.2} before the arms, {:.2} after)",
        r.fsync_probes_ms[0], r.fsync_probes_ms[1]
    );
    for a in &r.arms {
        if a.arm == Arm::Off {
            continue;
        }
        let (all, per) = a.fsync_added_ms(fsync);
        println!(
            "  {} ({}): {} judge frame(s) over {} turns ({} after the last), {} blob(s), {} Jev call(s), \
             {} warm-up(s): about {all:.1} ms of fdatasync, {per:.2} ms a turn",
            a.arm.name(),
            a.arm.about(),
            a.judge_frames(),
            a.turns,
            a.trailing,
            a.blobs,
            a.jev_calls,
            a.jev_warmups
        );
        if let Some(f) = a.first {
            println!(
                "    the first turn, submitted at once after serving: the ladder's pack.mode frames {} before \
                 its submit, {} before its answer, {} after it; its trace counts {} frames",
                f.before_submit, f.before_answer, f.after_answer, f.trace_frames
            );
        }
        for (shape, n) in &a.judge_shapes {
            println!("    {n} × {shape}");
        }
        println!(
            "    a tool-call turn's frames: {}",
            a.tool.last_frames.join("  ")
        );
    }
    for v in &r.verdicts {
        println!(
            "  {} (the judge off): {} frame(s) at the p95, budget {}: {}",
            v.phase,
            v.p95,
            v.budget,
            if v.ok { "ok" } else { "MISSED" }
        );
    }
    println!("  the bench took {:.1} s", r.wall_ms / 1000.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(records: &[&str]) -> Frame {
        Frame {
            first: 1,
            records: records.iter().map(|r| (*r).to_string()).collect(),
        }
    }

    /// The sink's frame, a budget block, and the ladder's rows are the
    /// judge's; a turn's frame is not, nor one that mixes the two.
    #[test]
    fn a_frame_is_the_judges_when_every_record_is() {
        assert!(is_judges(&frame(&[
            "ledger:judge.call",
            "ledger:judge.call",
            "meta:judge.budget"
        ])));
        assert!(is_judges(&frame(&["ledger:pack.mode"])));
        assert!(is_judges(&frame(&["meta:judge.budget"])));
        assert!(!is_judges(&frame(&["ledger:turn.started", "node"])));
        assert!(!is_judges(&frame(&[
            "ledger:judge.call",
            "ledger:turn.ended"
        ])));
        assert!(!is_judges(&frame(&["meta:session.x"])));
        assert!(!is_judges(&frame(&[])));
        assert_eq!(
            short(&frame(&[
                "ledger:judge.call",
                "ledger:judge.call",
                "meta:judge.budget"
            ])),
            "[ledger:judge.call ×2, meta:judge.budget]"
        );
    }

    /// The judge's columns are the history's, and the `off` arm's are
    /// `bench turn`'s own.
    #[test]
    fn every_column_the_judge_bench_records_has_a_history_column() {
        let columns: Vec<&str> = crate::history::columns().collect();
        let kind = |name: &str| JudgedKind {
            name: name.to_string(),
            wall_ms: single(1.0),
            daemon_ms: single(1.0),
            frames: single(5.0),
            frames_each: vec![5],
            judge_before_each: vec![0],
            placed: Placed::default(),
            last_frames: Vec::new(),
        };
        let arm = |arm: Arm| ArmReport {
            arm,
            first: None,
            plain: kind("plain"),
            tool: kind("tool-call"),
            trailing: 1,
            judge_shapes: BTreeMap::new(),
            blobs: 2,
            jev_calls: 0,
            jev_warmups: 0,
            turns: 2,
        };
        let r = JudgeReport {
            theseusd: String::new(),
            runs: 1,
            arms: ARMS.iter().map(|a| arm(*a)).collect(),
            fsync_ms: single(1.0),
            fsync_probes_ms: [1.0, 1.0],
            verdicts: turn_verdicts(&single(5.0), &single(9.0)),
            wall_ms: 0.0,
        };
        let cols = r.columns();
        assert_eq!(cols.len(), 4 + 3 * 2);
        for (name, _) in cols {
            assert!(columns.contains(&name.as_str()), "{name} has no column");
        }
        // One trailing frame and two blobs, at 2 ms: 5 syncs, 10 ms.
        assert_eq!(r.arms[1].fsync_added_ms(2.0), (10.0, 5.0));
    }
}
