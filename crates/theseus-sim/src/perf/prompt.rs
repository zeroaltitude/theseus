//! `bench prompt` (theseus-7m7w.1): what a fresh session's first request is, in tokens, for each kind of
//! place (`cli`, `dm`, `task` on the default config, `harness` on the benchmark's profile), by `context.census`
//! on a scratch daemon of each. It compares each with a committed baseline
//! (`baselines/prompt.json`) and warns at more than 5 % over it. A warning and never a failure: other rows add
//! tools every week, and the baseline is moved on purpose (`--update`), with the census beside it to say why.
//! Nothing is sent to a model: a census compiles and counts.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::census::ContextCensusResult;

use super::theseusd_or_beside;

/// A first request this far over its baseline, in percent, is warned of.
pub const WARN_OVER_PERCENT: f64 = 5.0;
/// What the benchmark's profile is called in the bench's table: the one place kind it counts.
const BENCH_PLACE: &str = "harness";
/// The key a scratch daemon's secret resolves from; a stand-in, since nothing calls a model.
const KEY_ENV: &str = "THESEUS_PROMPT_BENCH_KEY";

#[derive(Args)]
pub struct PromptArgs {
    /// The daemon to count with (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// The benchmark's profile, for the `harness` row.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/theseus-bench.toml"))]
    bench_config: PathBuf,
    /// The committed baseline the rows are compared with.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/baselines/prompt.json"))]
    baseline: PathBuf,
    /// Write this run's rows as the baseline (a change you mean: say why in the commit).
    #[arg(long)]
    update: bool,
    /// Also write the rows as JSON here.
    #[arg(long)]
    json: Option<PathBuf>,
}

/// The baseline: each place kind's first request, in tokens, and the model it was counted for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub model: String,
    pub places: BTreeMap<String, u64>,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub place: String,
    pub total: u64,
    pub system: u64,
    pub tools: u64,
    pub tool_count: usize,
    pub baseline: Option<u64>,
}

/// The rows that are over their baseline by more than `WARN_OVER_PERCENT`, as the words a warning says.
pub fn warnings(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| {
            let base = r.baseline.filter(|b| *b > 0)?;
            let over = (r.total as f64 - base as f64) * 100.0 / base as f64;
            (over > WARN_OVER_PERCENT).then(|| {
                format!(
                    "warning: a {} session's first request is {} tokens, {over:+.1}% over the baseline's {base} \
                     (the line is +{WARN_OVER_PERCENT}%): `theseus census --place {}` says where it grew; move the \
                     baseline with `bench prompt --update` when the growth is meant",
                    r.place, r.total, r.place
                )
            })
        })
        .collect()
}

/// A scratch daemon: stopped when it drops.
struct Scratch {
    child: Child,
    sock: PathBuf,
    _dir: tempfile::TempDir,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let stop = request(&self.sock, "shutdown", Value::Null);
        let until = Instant::now() + Duration::from_secs(10);
        while stop.is_ok() && Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A scratch daemon on `config` with its socket, state and projects in a directory of its own.
fn start(theseusd: &Path, mut config: toml::Table) -> Result<Scratch> {
    let dir = tempfile::tempdir()?;
    let (state, sock, projects) = (
        dir.path().join("state"),
        dir.path().join("sock"),
        dir.path().join("projects"),
    );
    std::fs::create_dir_all(&projects)?;
    let mut set = |section: &str, pairs: &[(&str, toml::Value)]| {
        let t = config
            .entry(section.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if let Some(t) = t.as_table_mut() {
            for (k, v) in pairs {
                t.insert((*k).to_string(), v.clone());
            }
        }
    };
    let path = |p: &Path| toml::Value::String(p.display().to_string());
    set(
        "server",
        &[("socket", path(&sock)), ("state_dir", path(&state))],
    );
    set("tools", &[("projects_dir", path(&projects))]);
    for off in ["web", "discord", "index"] {
        set(off, &[("enabled", false.into())]);
    }
    set(
        "secrets",
        &[("anthropic_api_key", format!("env:{KEY_ENV}").into())],
    );
    let file = dir.path().join("config.toml");
    std::fs::write(&file, toml::to_string(&config)?)?;
    let child = Command::new(theseusd)
        .arg("--config")
        .arg(&file)
        .env(KEY_ENV, "a-stand-in-key")
        .env("ANTHROPIC_API_KEY", "a-stand-in-key")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.path().join("theseusd.log"))?)
        .spawn()
        .context("starting a scratch daemon")?;
    let s = Scratch {
        child,
        sock,
        _dir: dir,
    };
    let until = Instant::now() + Duration::from_secs(30);
    while UnixStream::connect(&s.sock).is_err() {
        if Instant::now() > until {
            bail!("a scratch daemon did not answer in 30 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(s)
}

/// One request on a connection of its own.
fn request(sock: &Path, method: &str, params: Value) -> Result<Value> {
    let s = UnixStream::connect(sock).context("connecting to the scratch daemon")?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req)?;
    line.push('\n');
    (&s).write_all(line.as_bytes())?;
    let mut r = BufReader::new(&s);
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            bail!("the daemon closed the connection before answering {method}");
        }
        let v: Value = serde_json::from_str(&line)?;
        if v.get("id") == Some(&json!(1)) {
            if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                bail!("{method}: {e}");
            }
            return Ok(v["result"].clone());
        }
    }
}

fn census(sock: &Path, place: &str) -> Result<ContextCensusResult> {
    Ok(serde_json::from_value(request(
        sock,
        theseus_protocol::method::CONTEXT_CENSUS,
        json!({"place": place}),
    )?)?)
}

fn row(place: &str, c: &ContextCensusResult, baseline: &Baseline) -> Row {
    Row {
        place: place.to_string(),
        total: c.total_tokens,
        system: c.system_tokens,
        tools: c.tools_tokens,
        tool_count: c.tools.len(),
        baseline: baseline.places.get(place).copied(),
    }
}

pub fn prompt_cmd(a: PromptArgs) -> Result<()> {
    let theseusd = theseusd_or_beside(a.theseusd)?;
    let baseline: Baseline = match std::fs::read_to_string(&a.baseline) {
        Ok(s) => {
            serde_json::from_str(&s).with_context(|| format!("reading {}", a.baseline.display()))?
        }
        Err(_) => Baseline::default(),
    };
    let mut rows = Vec::new();
    let mut model = String::new();
    {
        let d = start(&theseusd, toml::Table::new())?;
        for place in ["cli", "dm", "task"] {
            let c = census(&d.sock, place)?;
            model = c.model.clone();
            rows.push(row(place, &c, &baseline));
        }
    }
    {
        let text = std::fs::read_to_string(&a.bench_config)
            .with_context(|| format!("reading {}", a.bench_config.display()))?;
        let d = start(&theseusd, text.parse::<toml::Table>()?)?;
        let c = census(&d.sock, "cli")?;
        rows.push(row(BENCH_PLACE, &c, &baseline));
    }
    println!("a fresh session's first request, in tokens at {model}'s figures (nothing is sent):");
    println!(
        "  {:<8} {:>8} {:>8} {:>8} {:>6}  {:>9}",
        "place", "total", "system", "tools", "n", "baseline"
    );
    for r in &rows {
        println!(
            "  {:<8} {:>8} {:>8} {:>8} {:>6}  {:>9}",
            r.place,
            r.total,
            r.system,
            r.tools,
            r.tool_count,
            r.baseline.map_or("-".to_string(), |b| b.to_string()),
        );
    }
    for w in warnings(&rows) {
        println!("{w}");
        eprintln!("{w}");
    }
    if let Some(p) = &a.json {
        std::fs::write(p, serde_json::to_vec_pretty(&rows)?)?;
    }
    if a.update {
        let next = Baseline {
            model,
            places: rows.iter().map(|r| (r.place.clone(), r.total)).collect(),
        };
        std::fs::write(&a.baseline, serde_json::to_string_pretty(&next)? + "\n")?;
        println!("baseline written: {}", a.baseline.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(place: &str, total: u64, baseline: Option<u64>) -> Row {
        Row {
            place: place.into(),
            total,
            system: 0,
            tools: 0,
            tool_count: 0,
            baseline,
        }
    }

    #[test]
    fn a_first_request_over_five_percent_warns_and_one_at_it_does_not() {
        let rows = [
            row("cli", 10_500, Some(10_000)),
            row("dm", 10_501, Some(10_000)),
            row("task", 9_000, Some(10_000)),
            row("harness", 50_000, None),
        ];
        let w = warnings(&rows);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("dm session's first request is 10501 tokens, +5.0% over"),
            "{}",
            w[0]
        );
        assert!(w[0].contains("theseus census --place dm"));
    }

    #[test]
    fn a_missing_or_zero_baseline_warns_of_nothing() {
        assert!(warnings(&[row("cli", 99_999, None), row("dm", 99_999, Some(0))]).is_empty());
    }
}
