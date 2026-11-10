//! `bench prompt` (theseus-7m7w.1): what a fresh session's first request is, in tokens, for each kind of
//! place (`cli`, `dm`, `channel`, `task`, `headless` on the default config, `harness` on the benchmark's profile), by `context.census`
//! on a scratch daemon of each. It compares each with a committed baseline
//! (`baselines/prompt.json`) and warns at more than 5 % over it. A warning and never a failure: other rows add
//! tools every week, and the baseline is moved on purpose (`--update`), with the census beside it to say why.
//! Nothing is sent to a model: a census compiles and counts.
//!
//! It also carries the routing cliff's watch (theseus-4vsrz, B7). `[routing] cold_switch_tokens` bounds **the
//! session's own context, not the request's size** (`config/routing.rs`'s doc says so in those words), and the
//! assertion that used to watch an ordinary turn against that bound was deleted on the owner's word on
//! 2026-11-08 -- rightly, because it was a size assertion that the row it sat in existed to remove, and it
//! measured the tool catalog's growth rather than what a session carries. Nothing then watched the figure at
//! all. So each row here also carries [`Row::own`], the session's own context as the census counts it -- the
//! total less the tool definitions and the first system block, the header, which every place class carries
//! identically -- with its share of that place's `cold_switch_tokens`.
//!
//! **What that column is and is not, measured rather than assumed.** On 2026-11-10 it read **0 to 29 tokens
//! across all six place classes**, 0.1 % of the 30,000 bound: a *fresh* request carries no message and almost
//! nothing of its own, so the prefix is essentially the whole request. It is therefore **not** the 14,977 that
//! `tests_route_cliff` measured -- that figure came from a rig with an input and a catalog of its own -- and it
//! is the floor of what an ordinary one-message turn weighs, not the whole of it. What it is good for is the
//! regression: the 2026-11-07 cliff was the universal prefix being **counted into** the bound's quantity, and if
//! that ever comes back this column goes from 29 to about 7,600 in one step. Its line is therefore
//! [`WARN_OWN_TOKENS`] of **absolute** movement rather than a percentage: tens-of-tokens jitter says nothing,
//! thousands says everything, and a place whose own context is genuinely zero (`channel` is) still has a line,
//! where a ratio would have none.
//!
//! All of it is **a number recorded in the tree and a warning a reader sees, never a failure.** A hard gate on a
//! share nobody has calibrated would redden a departure for the ordinary drift of a catalog that grows every
//! week, which is the mistake the deleted assertion was. `scripts/prompt-step.sh` is the gate phase that runs
//! this, and it exits 0 on every path.

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

/// The baseline: each place kind's first request, in tokens, the session's own context within it, and the
/// model it was counted for. `own` is absent from a baseline written before the cliff watch (theseus-4vsrz):
/// a place with no `own` entry is warned of on its total alone, as before.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub model: String,
    pub places: BTreeMap<String, u64>,
    #[serde(default)]
    pub own: BTreeMap<String, u64>,
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
    /// The session's own context: the total less the universal prefix -- the tool definitions and the header
    /// block, which every place class carries identically. This is the quantity `[routing]
    /// cold_switch_tokens` bounds, and the one the deleted cliff assertion was about (theseus-4vsrz).
    pub own: u64,
    /// That place's `[routing] cold_switch_tokens`, the bound `own` is read against.
    pub cold: u64,
    pub own_baseline: Option<u64>,
}

impl Row {
    /// `own` as a share of `cold`, in percent; none where the place has no bound.
    pub fn own_share(&self) -> Option<f64> {
        (self.cold > 0).then(|| self.own as f64 * 100.0 / self.cold as f64)
    }
}

/// The movement in `own` that is worth a word, in tokens. It is an absolute figure and not a percentage, and
/// it is the whole line for that column. **Measured**
/// 2026-11-10 (theseus-4vsrz): a fresh request's own context is **0 to 29 tokens** across all six place
/// classes, because a fresh session carries no message and almost no block of its own -- the prefix is the
/// request. So +5 % of it is a token and a half, and a percentage alone would warn on nothing but jitter. A
/// move of this many tokens is more than three times the whole present range and cannot be noise; it is what a
/// regression of the 2026-11-07 cliff would look like, the prefix counted back into the bound's quantity, which
/// would take this figure from 29 to about 7,600. A noise floor, not a bound.
pub const WARN_OWN_TOKENS: u64 = 100;

/// How far `now` is over `base`, in percent, where `base` is a figure worth comparing.
fn over(now: u64, base: Option<u64>) -> Option<(f64, u64)> {
    let base = base.filter(|b| *b > 0)?;
    Some(((now as f64 - base as f64) * 100.0 / base as f64, base))
}

/// The rows that are over their baseline by more than `WARN_OVER_PERCENT`, as the words a warning says: the
/// first request's total, and the session's own context against the routing cliff's bound (theseus-4vsrz).
pub fn warnings(rows: &[Row]) -> Vec<String> {
    let mut out = Vec::new();
    for r in rows {
        if let Some((over, base)) =
            over(r.total, r.baseline).filter(|(o, _)| *o > WARN_OVER_PERCENT)
        {
            out.push(format!(
                "warning: a {} session's first request is {} tokens, {over:+.1}% over the baseline's {base} \
                 (the line is +{WARN_OVER_PERCENT}%): `theseus census --place {}` says where it grew; move the \
                 baseline with `bench prompt --update` when the growth is meant",
                r.place, r.total, r.place
            ));
        }
        // The own context's line is ABSOLUTE movement, not a percentage (theseus-4vsrz): the figure is tens
        // of tokens, where a percentage warns on jitter and is undefined at a baseline of zero, which
        // `channel` actually has. [`WARN_OWN_TOKENS`] comes from the measured range.
        if let Some(base) = r
            .own_baseline
            .filter(|b| r.own.abs_diff(*b) >= WARN_OWN_TOKENS)
        {
            let share = r.own_share().map_or(String::new(), |s| {
                format!(", {s:.1}% of its cold-switch bound of {}", r.cold)
            });
            out.push(format!(
                "warning: a {} session's own context is {} tokens{share}, {} tokens off the baseline's \
                 {base} (the line is {WARN_OWN_TOKENS}): this is the figure `[routing] cold_switch_tokens` \
                 bounds, and the routing cliff of 2026-11-07 was the universal prefix being counted into it \
                 (theseus-4vsrz). A jump of thousands here is that counting coming back. Move the baseline \
                 with `bench prompt --update` when the growth is meant",
                r.place,
                r.own,
                r.own as i64 - base as i64
            ));
        }
    }
    out
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

/// The universal prefix as the router counts it: every tool definition and the first system block, the header.
/// Both come from the one census, so the prefix is subtracted and not estimated twice -- which is where the
/// deleted cliff assertion's residual went wrong (theseus-4vsrz).
fn prefix(c: &ContextCensusResult) -> u64 {
    c.tools_tokens + c.system.first().map_or(0, |b| b.tokens)
}

fn row(place: &str, c: &ContextCensusResult, cold: u64, baseline: &Baseline) -> Row {
    Row {
        place: place.to_string(),
        total: c.total_tokens,
        system: c.system_tokens,
        tools: c.tools_tokens,
        tool_count: c.tools.len(),
        baseline: baseline.places.get(place).copied(),
        own: c.total_tokens.saturating_sub(prefix(c)),
        cold,
        own_baseline: baseline.own.get(place).copied(),
    }
}

/// Just the `[routing]` section of a config document, so the bound can be read from a partial one: `start`
/// hands the daemon a table with a socket and a state directory added, and `Config::parse` validates the
/// whole document, which an empty table does not pass.
#[derive(Deserialize)]
struct JustRouting {
    #[serde(default)]
    routing: theseus_core::config::RoutingConfig,
}

/// `[routing] cold_switch_tokens` as a daemon started on `config` reads it: the serde default where the config
/// says nothing, which is what a scratch daemon on an empty table gets. Zero when the section will not parse,
/// which leaves the row's share blank rather than failing the run.
fn cold_switch_of(config: &str) -> u64 {
    toml::from_str::<JustRouting>(config).map_or(0, |r| r.routing.cold_switch_tokens)
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
        let cold = cold_switch_of("");
        let d = start(&theseusd, toml::Table::new())?;
        for place in ["cli", "dm", "channel", "task", "headless"] {
            let c = census(&d.sock, place)?;
            model = c.model.clone();
            rows.push(row(place, &c, cold, &baseline));
        }
    }
    {
        let text = std::fs::read_to_string(&a.bench_config)
            .with_context(|| format!("reading {}", a.bench_config.display()))?;
        let cold = cold_switch_of(&text);
        let d = start(&theseusd, text.parse::<toml::Table>()?)?;
        let c = census(&d.sock, "cli")?;
        rows.push(row(BENCH_PLACE, &c, cold, &baseline));
    }
    println!("a fresh session's first request, in tokens at {model}'s figures (nothing is sent):");
    println!(
        "  {:<8} {:>8} {:>8} {:>8} {:>6}  {:>9}  {:>8} {:>7}  {:>9}",
        "place", "total", "system", "tools", "n", "baseline", "own", "of cold", "own base"
    );
    for r in &rows {
        println!(
            "  {:<8} {:>8} {:>8} {:>8} {:>6}  {:>9}  {:>8} {:>7}  {:>9}",
            r.place,
            r.total,
            r.system,
            r.tools,
            r.tool_count,
            r.baseline.map_or("-".to_string(), |b| b.to_string()),
            r.own,
            r.own_share()
                .map_or("-".to_string(), |s| format!("{s:.1}%")),
            r.own_baseline.map_or("-".to_string(), |b| b.to_string()),
        );
    }
    // The cliff's watch, recorded in the open (theseus-4vsrz, B7): `own` is the session's own context, the
    // total less the tool definitions and the header block, and it is the quantity `[routing]
    // cold_switch_tokens` bounds. A fresh request carries no message of its own, so `own` here is the floor of
    // what an ordinary one-message turn weighs against that bound, not the whole of it. It is read, moved on
    // purpose and warned of -- never asserted: the assertion that stood in `tests_route_cliff` went on the
    // owner's word on 2026-11-08 because a size assertion on a catalog that grows every week reddens
    // departures for ordinary drift.
    println!(
        "  own: the session's own context (total less the tools and the header block), against each place's \
         [routing] cold_switch_tokens; a fresh request carries no message, so it is that turn's floor. \
         Warned of at {WARN_OWN_TOKENS} tokens of movement either way, never failed (theseus-4vsrz)."
    );
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
            own: rows.iter().map(|r| (r.place.clone(), r.own)).collect(),
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
            own: 0,
            cold: 0,
            own_baseline: None,
        }
    }

    /// A row whose own context is `own` of a 30,000 bound, against `baseline`.
    fn own_row(place: &str, own: u64, baseline: Option<u64>) -> Row {
        Row {
            own,
            cold: 30_000,
            own_baseline: baseline,
            ..row(place, 1_000, Some(1_000))
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

    /// The cliff's watch (theseus-4vsrz): the session's own context [`WARN_OWN_TOKENS`] off its baseline warns
    /// and names its share of the cold-switch bound, **in either direction** -- the card asks to be told when
    /// the figure moves, and a figure that fell by thousands is as much news as one that rose. A baseline
    /// written before the watch (no `own` entry) warns of nothing, and the row's total is judged as it always
    /// was.
    #[test]
    fn an_own_context_that_moved_past_the_floor_warns_with_its_share_of_the_cold_switch_bound() {
        let rows = [
            own_row("cli", 10_050, Some(10_000)), // fifty tokens: under the floor
            own_row("dm", 15_001, Some(14_000)),  // up by 1,001
            own_row("task", 9_000, Some(10_000)), // down by 1,000: still news
            own_row("channel", 29_000, None),     // no baseline, no line
        ];
        let w = warnings(&rows);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(
            w[0].contains(
                "dm session's own context is 15001 tokens, 50.0% of its cold-switch bound of 30000, 1001 \
                 tokens off the baseline's 14000"
            ),
            "{}",
            w[0]
        );
        assert!(w[0].contains("cold_switch_tokens"), "{}", w[0]);
        assert!(
            w[1].contains("task") && w[1].contains("-1000 tokens off"),
            "{}",
            w[1]
        );
        // A place with no bound keeps the warning and leaves the share out.
        let no_bound = Row {
            cold: 0,
            ..own_row("harness", 15_001, Some(14_000))
        };
        let w = warnings(&[no_bound]);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(!w[0].contains("cold-switch bound of"), "{}", w[0]);
    }

    /// A zero baseline is a real one for the own context, where it is not for a total: `channel`'s own context
    /// measured 0, and a ratio would leave that place with no line at all (theseus-4vsrz).
    #[test]
    fn a_zero_own_baseline_still_has_a_line_where_a_zero_total_baseline_has_none() {
        assert!(warnings(&[own_row("channel", 50, Some(0))]).is_empty());
        let w = warnings(&[own_row("channel", 7_600, Some(0))]);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("7600 tokens off the baseline's 0"),
            "{}",
            w[0]
        );
    }

    /// The own context's noise floor (theseus-4vsrz): the real figure is tens of tokens, so a move that would
    /// be a large percentage but is under [`WARN_OWN_TOKENS`] is jitter and warns of nothing -- while the
    /// regression this watches for, the prefix counted back into the bound's quantity, is thousands and warns.
    #[test]
    fn a_small_own_contexts_jitter_is_under_the_token_floor_and_a_prefix_regression_is_not() {
        // 29 -> 40 would be +37.9 % on a ratio, and is eleven tokens.
        assert!(warnings(&[own_row("cli", 40, Some(29))]).is_empty());
        // 29 -> 128 is the floor exactly, and warns.
        let w = warnings(&[own_row("cli", 29 + WARN_OWN_TOKENS, Some(29))]);
        assert_eq!(w.len(), 1, "{w:?}");
        // What the 2026-11-07 cliff looked like: the prefix counted in.
        let w = warnings(&[own_row("cli", 7_600, Some(29))]);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("counting coming back"), "{}", w[0]);
    }

    /// `own` is the total less the tools and the header block, and only the FIRST system block is the header:
    /// the blocks after it are what the session carries (theseus-4vsrz).
    #[test]
    fn the_prefix_is_the_tools_and_the_first_system_block_only() {
        let block = |tokens| theseus_protocol::census::CensusBlock {
            block: "b".into(),
            bytes: 0,
            tokens,
            framing: 0,
            parts: vec![],
        };
        let mut c = ContextCensusResult {
            place: "cli".into(),
            ..Default::default()
        };
        c.tools_tokens = 7_000;
        c.system = vec![block(8_000), block(500), block(100)];
        c.system_tokens = 8_600;
        c.total_tokens = 16_000;
        assert_eq!(prefix(&c), 15_000);
        let r = super::row("cli", &c, 30_000, &Baseline::default());
        assert_eq!((r.own, r.own_share()), (1_000, Some(1_000.0 / 300.0)));
    }

    /// An empty config reads the serde default of `[routing] cold_switch_tokens`, which is what a scratch
    /// daemon on an empty table runs with; a config that will not parse leaves the bound at zero rather than
    /// failing the run.
    #[test]
    fn the_cold_switch_bound_comes_from_the_config_the_daemon_got() {
        assert_eq!(cold_switch_of(""), 30_000);
        assert_eq!(
            cold_switch_of("[routing]\ncold_switch_tokens = 12345\n"),
            12_345
        );
        assert_eq!(cold_switch_of("[routing"), 0);
    }
}
