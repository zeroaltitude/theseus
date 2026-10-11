//! theseus-sim: the store's crash-and-recover harness and the kernel
//! simulator. Each runs in the gate on small fixed seeds
//! (`tests/sim.rs`); long runs stay here, e.g. `theseus-sim kernel-sim --seeds
//! 40`.
//!
//! `worker`      appends random records forever, printing `C <pos> <crc> <kind>`
//!               to stdout only after the append returned (durable). Killed by
//!               the driver with SIGKILL at a random moment.
//! `crash-test`  the M1 exit test: spawn worker, kill it, optionally tear the
//!               WAL tail, reopen, verify every reported record exists with the
//!               same bytes, positions are contiguous, appends continue; repeat.
//! `kernel-sim`  the M2 exit test: the kernel under a virtual clock with
//!               seeded fault injection (crash between any two frames, crash
//!               inside startup steps, lost/duplicate/late completions,
//!               dropped notifies, cancels), invariants checked every step.
//! `bench lifecycle`  the §9 lifecycle budgets on a real `theseusd` (M3.5,
//!               theseus-qa0): cold start, clean shutdown with a job running,
//!               SIGKILL and restart, each p50/p95; `--check` fails a miss.
//!               The gate runs it (`scripts/gate.sh`), and records each run
//!               (`--record`); `bench history` reads them back (theseus-1hk).
//! `bench turn`  what a turn costs (theseus-goa8): a plain turn and a tool-call
//!               turn on the stand-in model, their wall time and their frames
//!               (counted from the WAL; the plain turn's are a gated budget),
//!               and the daemon's memory. `bench idle` is an idle daemon's CPU
//!               and wakeups over a window, and `bench size` the binaries'.
//! `synth-store` a store of parked sessions, many to a frame.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use theseus_store::{kinds, NewRecord, Store, WalConfig, WalStore};

mod discord_cli;
mod fake_mcp;
mod fake_model;
mod history;
mod jobs;
mod kernel_sim;
mod lifecycle;
mod perf;
mod procfs;
mod synth;
mod walcount;

#[derive(Parser)]
#[command(
    name = "theseus-sim",
    version,
    about = "Store crash-and-recover harness and kernel simulator"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

// The command line, parsed once at startup: a variant's size is never paid.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Cmd {
    /// Append random records until killed; report each committed record on stdout.
    Worker {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Checkpoint every N records (0 = never), to exercise index rebuild.
        #[arg(long, default_value_t = 200)]
        checkpoint_every: u64,
        /// Threads appending at once: more than one makes the store's writer
        /// commit their frames together, with one sync (theseus-vni9).
        #[arg(long, default_value_t = 1)]
        writers: usize,
    },
    /// Kill -9 at random points; verify recovery. The M1 exit test.
    CrashTest {
        #[arg(long, default_value_t = 20)]
        iterations: u32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Restart the same store this many times per iteration.
        #[arg(long, default_value_t = 3)]
        restarts: u32,
        /// Also tear the WAL tail after most kills (truncate, append junk, or
        /// flip a byte), only past what the worker reported durable or an
        /// open read back (theseus-4x6).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        tear: bool,
        /// Path to this binary (defaults to current_exe).
        #[arg(long)]
        worker_bin: Option<PathBuf>,
        /// Threads appending at once in each worker (`worker --writers`).
        #[arg(long, default_value_t = 1)]
        writers: usize,
    },
    /// The deterministic kernel simulator (M2 exit test). Reproducible from --seed.
    KernelSim {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Run this many seeds starting at --seed.
        #[arg(long, default_value_t = 1)]
        seeds: u32,
        #[arg(long, default_value_t = 400)]
        steps: u32,
        #[arg(long, default_value_t = 12)]
        sessions: u32,
        #[arg(long, default_value_t = 3)]
        ceiling: u32,
        /// Probability of a crash between any two kernel frames.
        #[arg(long, default_value_t = 0.02)]
        p_crash: f64,
        /// Probability a finished job's notify is lost (spool only).
        #[arg(long, default_value_t = 0.2)]
        p_drop_notify: f64,
        /// Probability a delivered completion is delivered twice.
        #[arg(long, default_value_t = 0.15)]
        p_dup: f64,
        /// Probability a job never finishes.
        #[arg(long, default_value_t = 0.08)]
        p_lost_job: f64,
        /// Share of steps that are a /cancel (0..0.3).
        #[arg(long, default_value_t = 0.06)]
        p_cancel: f64,
        /// Share of turns raced by a second OS thread on the same execution.
        #[arg(long, default_value_t = 0.3)]
        p_race: f64,
        /// fdatasync every frame (slow; durability is the store crash-test's job).
        #[arg(long)]
        fsync: bool,
        #[arg(long)]
        verbose: bool,
    },
    /// Benchmarks against a real daemon.
    Bench {
        #[command(subcommand)]
        bench: BenchCmd,
    },
    /// Write a store of parked sessions, many to a frame (the lifecycle
    /// bench's synthetic store).
    SynthStore {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        sessions: u64,
        /// This many more ledger rows across the sessions, their history
        /// (with a settled action for every tenth).
        #[arg(long, default_value_t = 0)]
        ledger_rows: u64,
    },
    /// A stand-in for Discord's REST API (theseus-q4v), for a scratch
    /// daemon's `[discord] rest_proxy`: it keeps messages, honors nonces, and
    /// never records a header. Serves until killed.
    FakeDiscord {
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:9447")]
        addr: String,
        /// A file whose first line is the mode, read at every request: `up`,
        /// `down`, `hang-creates`, or `fail`.
        #[arg(long)]
        control: Option<PathBuf>,
        /// Every request as a JSON line (method, path, outcome; no headers),
        /// and the messages beside it in `<log>.messages.json`.
        #[arg(long)]
        log: Option<PathBuf>,
        /// Also serve a gateway here (theseus-6g62), for `[discord]
        /// gateway_proxy`; `theseus-sim discord say` and `press` act through it.
        #[arg(long)]
        gateway: Option<String>,
        /// A guild for the viewer check (theseus-ck0k), as JSON: `id`, `name`,
        /// `owner`, `everyone` (permission bits), `members`, `channels` with
        /// `overwrites`, and `roles`.
        #[arg(long)]
        guild: Option<PathBuf>,
    },
    /// A scripted stand-in for the Messages API (37b), for a scratch daemon's
    /// `[model] api_base` and its providers': `--rules` is a JSON array of
    /// `{when, calls: [{name, input}], text, hold_ms}`, and each turn's last
    /// user text takes the first rule it holds, answered after `hold_ms`; a
    /// call that answers a tool call gets `Done.`. Serves until killed.
    FakeModel {
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:9448")]
        addr: String,
        #[arg(long)]
        rules: PathBuf,
    },
    /// The fake MCP server (M7 36b), for a scratch daemon's `[mcp.servers]`:
    /// stdio by default, as a daemon starts it, or `--http`.
    FakeMcp(fake_mcp::FakeMcpArgs),
    /// Discord without a person (theseus-9kjv): the kl8m proof against a real
    /// daemon, and a typed message, a press, and a read for a live check
    /// against a running fake-discord.
    Discord {
        #[command(subcommand)]
        cmd: discord_cli::Cmd,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum BenchCmd {
    /// The §9 lifecycle budgets: cold start to the first health answer, the
    /// same from the copy of an `op://` config note (vault), clean shutdown
    /// with executions waiting and a job running, SIGKILL then restart, a
    /// binary swap under the same load, and restore from a local WAL, each
    /// run N times with p50 and p95.
    Lifecycle {
        /// The daemon to measure (default: the `theseusd` beside this binary).
        #[arg(long)]
        theseusd: Option<PathBuf>,
        /// The build the swap phase alternates with --theseusd (default: a
        /// copy of it). Both must read the store: F4a or later.
        #[arg(long)]
        swap_to: Option<PathBuf>,
        #[arg(long, default_value_t = 10)]
        runs: usize,
        /// A synthetic store of this many parked sessions (0: empty).
        #[arg(long, default_value_t = 0)]
        sessions: u64,
        /// Or a copy of this store directory (e.g. a copy of ~/.theseus/store).
        #[arg(long, conflicts_with = "sessions")]
        store: Option<PathBuf>,
        /// The fake op's answer time: a start that waited for it would show.
        #[arg(long, default_value_t = 1000)]
        resolver_ms: u64,
        /// Which phases, comma-separated.
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "cold,vault,shutdown,inflight,kill,swap,restore,seed,health,cancel"
        )]
        phases: Vec<String>,
        /// Compare each p95 with §9 plus the margin, and exit 1 on a miss.
        #[arg(long)]
        check: bool,
        /// The noise allowed over every budget, in ms. Default: each phase's
        /// own, measured on this machine (`lifecycle::margin_ms`).
        #[arg(long)]
        margin_ms: Option<f64>,
        /// The busy allowance, a percentage of each limit (theseus-lew7): a
        /// phase over its limit by no more than this passes --check, and says
        /// so. The gate gives it only when its settle step found no quiet
        /// window. The history records the strict verdict, a miss, with the
        /// allowance the run passed on.
        #[arg(long, default_value_t = 0)]
        allowance: u32,
        /// Also write the report as JSON here.
        #[arg(long)]
        json: Option<PathBuf>,
        /// Work in this directory and keep it (default: a temporary one).
        #[arg(long)]
        dir: Option<PathBuf>,
        /// A real config instead of the bench's, with the real op: the live
        /// check (cold starts only). Turn Discord and the web UI off in it.
        #[arg(long, requires = "op_token_file")]
        config: Option<PathBuf>,
        /// The service-account token file for the real op (with --config).
        #[arg(long)]
        op_token_file: Option<PathBuf>,
        /// Wait for the vault to confirm each vault start's copy, and time it.
        #[arg(long)]
        confirm: bool,
        /// Append the run's p50s, p95s, and limits to this history, a CSV
        /// that `bench history` reads. The gate passes $THESEUS_BENCH_HISTORY,
        /// or ~/.cache/theseus/bench-history.csv. A failure to write it is
        /// reported, and doesn't change the exit status.
        #[arg(long)]
        record: Option<PathBuf>,
        /// The run's label in the history (the gate's: the branch, and `git
        /// describe --always --dirty`).
        #[arg(long, requires = "record")]
        label: Option<String>,
    },
    /// What a turn costs, on the stand-in model: a plain turn's and a
    /// tool-call turn's wall time and frames (a frame is one fdatasync; each
    /// kind's count is held to its budget by --check), the daemon's
    /// memory after the start and after a burst of turns, and this disk's
    /// fdatasync (theseus-goa8).
    Turn(perf::TurnArgs),
    /// An idle daemon over a window (30 s): its CPU time, its wakeups, the
    /// frames it writes, and its memory, on an empty store or a synthetic
    /// one (--sessions 10000). Measured, with no budget yet (theseus-goa8).
    Idle(perf::IdleArgs),
    /// What a prompt's `theseus status --short` costs: the CLI's whole run against a daemon
    /// serving a synthetic store (--sessions 1000), p50 and p90 over many runs; --check holds
    /// p90 to 5 ms (theseus-lweh).
    Status(perf::StatusArgs),
    /// The release binaries' sizes, against §9's 60 MB (theseus-goa8).
    Size(perf::SizeArgs),
    /// What a job's start costs, by class (M4 17b): `/bin/true` through the
    /// real job wrapper, at L0 and in L1; an L1 start's p95 against 25 ms.
    Jobs(jobs::JobsArgs),
    /// The lifecycle bench's history: each phase's last runs, with the
    /// headroom left under its limit.
    History {
        /// How many runs of each phase.
        #[arg(long, default_value_t = 10)]
        last: usize,
        /// The history (default: ~/.cache/theseus/bench-history.csv).
        #[arg(long, env = "THESEUS_BENCH_HISTORY")]
        file: Option<PathBuf>,
    },
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::FakeDiscord {
            addr,
            control,
            log,
            gateway,
            guild,
        } => {
            let fake = theseus_sim::fake_discord::FakeDiscord::start_on(&addr, control, log)
                .with_context(|| format!("listening on {addr}"))?;
            println!("fake discord REST on {}", fake.addr);
            if let Some(g) = guild {
                // Read again at every request, so a live check can change
                // who can view a channel while a turn runs (M4 19c).
                fake.watch_guild_file(&g)
                    .with_context(|| format!("reading the guild from {}", g.display()))?;
            }
            if let Some(gw) = gateway {
                let url = fake
                    .serve_gateway(&gw)
                    .with_context(|| format!("listening on {gw}"))?;
                println!("fake discord gateway on {url}");
            }
            loop {
                std::thread::park();
            }
        }
        Cmd::FakeModel { addr, rules } => {
            let text = std::fs::read_to_string(&rules)
                .with_context(|| format!("reading {}", rules.display()))?;
            let rules: Vec<fake_model::Rule> = serde_json::from_str(&text)
                .with_context(|| format!("parsing the rules in {}", rules.display()))?;
            let fake = fake_model::FakeModel::start_rules_on(&addr, rules)?;
            println!("fake model on {}", fake.base());
            loop {
                std::thread::park();
            }
        }
        Cmd::FakeMcp(args) => fake_mcp::run(args),
        Cmd::Discord { cmd } => discord_cli::run(cmd),
        Cmd::SynthStore {
            dir,
            sessions,
            ledger_rows,
        } => {
            let g = synth::generate_with(&dir, sessions, ledger_rows)?;
            println!(
                "synthetic store at {}: {} sessions, {} records in {} frames, {:.1} MB of WAL, in {:.0} ms",
                dir.display(),
                g.sessions,
                g.records,
                g.frames,
                g.wal_bytes as f64 / 1e6,
                g.ms
            );
            Ok(())
        }
        Cmd::Bench {
            bench:
                BenchCmd::Lifecycle {
                    theseusd,
                    swap_to,
                    runs,
                    sessions,
                    store,
                    resolver_ms,
                    phases,
                    check,
                    margin_ms,
                    allowance,
                    json,
                    dir,
                    config,
                    op_token_file,
                    confirm,
                    record,
                    label,
                },
        } => {
            let theseusd = match theseusd {
                Some(p) => p,
                None => std::env::current_exe()?
                    .parent()
                    .context("locating this binary's directory")?
                    .join("theseusd"),
            };
            if let Some(p) = phases
                .iter()
                .find(|p| !lifecycle::PHASES.contains(&p.as_str()))
            {
                bail!("no phase {p:?}; the phases are {:?}", lifecycle::PHASES);
            }
            let report = lifecycle::run(&lifecycle::Opts {
                theseusd,
                swap_to,
                runs: runs.max(1),
                sessions,
                store,
                resolver_ms,
                phases,
                margin_ms,
                dir,
                config,
                op_token_file,
                confirm,
            })?;
            lifecycle::print(&report);
            if let Some(path) = json {
                std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            if !lifecycle_verdict(&report, record, label, allowance) && check {
                std::process::exit(1);
            }
            Ok(())
        }
        Cmd::Bench {
            bench: BenchCmd::Turn(args),
        } => perf::turn_cmd(args),
        Cmd::Bench {
            bench: BenchCmd::Idle(args),
        } => perf::idle_cmd(args),
        Cmd::Bench {
            bench: BenchCmd::Status(args),
        } => perf::status_cmd(args),
        Cmd::Bench {
            bench: BenchCmd::Size(args),
        } => perf::size_cmd(args),
        Cmd::Bench {
            bench: BenchCmd::Jobs(args),
        } => jobs::jobs_cmd(args),
        Cmd::Bench {
            bench: BenchCmd::History { last, file },
        } => {
            let path = match file {
                Some(p) => p,
                None => history::default_path()?,
            };
            print!(
                "{}",
                history::render(&path, history::read(&path)?.as_ref(), last.max(1))
            );
            Ok(())
        }
        Cmd::Worker {
            dir,
            seed,
            checkpoint_every,
            writers,
        } => worker(&dir, seed, checkpoint_every, writers),
        Cmd::CrashTest {
            iterations,
            seed,
            restarts,
            tear,
            worker_bin,
            writers,
        } => crash_test(iterations, seed, restarts, tear, worker_bin, writers),
        Cmd::KernelSim {
            seed,
            seeds,
            steps,
            sessions,
            ceiling,
            p_crash,
            p_drop_notify,
            p_dup,
            p_lost_job,
            p_cancel,
            p_race,
            fsync,
            verbose,
        } => {
            let mut totals = kernel_sim::SimReport::default();
            for s in seed..seed + seeds as u64 {
                let rep = kernel_sim::run(kernel_sim::SimParams {
                    seed: s,
                    steps,
                    sessions,
                    ceiling,
                    p_crash,
                    p_drop_notify,
                    p_dup,
                    p_lost_job,
                    p_cancel,
                    p_race,
                    fsync,
                    verbose,
                })
                .map_err(|e| anyhow::anyhow!("seed {s}: {e}"))?;
                println!(
                    "seed {s}: {} steps · {} crashes ({} startup faults) · {} sessions · {} turns · {} actions ({} in one frame; {} batches of {}, {} crashes inside one) · theseus-l6y: {} authorized and dispatched in one frame, {} input turns woken and admitted in one frame, {} results their turn read itself, {} faults after them ({} woken) · {} raced turns ({} ops on a second thread, {} of them transactions, {} crashes inside one) · {} completions ({} dup, {} notify lost, {} lost jobs, {} late-after-cancel) · {} cancels · {} unknown → {} resolved · {} budget questions ({} reset, {} declined) · theseus-w98: {} calls asked the operator ({} declined; {} answers in one frame), {} unsent actions a cancel ended · {} limit changes ({} raised; {} limits followed, {} budget waits proceeded) · {} unit budgets read in dollars · 37a: {} wakes set, {} repeating; {} taken, {} series put back, {} occurrences passed over ({} crashes down for minutes), {} ended by until; {} wakes cancelled · {} · {} reconciles · {} invariant checks · {} positions · {} ms",
                    rep.steps, rep.crashes, rep.startup_faults, rep.sessions, rep.turns, rep.actions,
                    rep.one_frame_dispatches, rep.batches, rep.batch_actions, rep.batch_crashes,
                    rep.authorized_and_dispatched, rep.input_admits, rep.own_results, rep.faults,
                    rep.fault_wakes,
                    rep.races, rep.race_ops, rep.race_frames, rep.race_crashes,
                    rep.completions_delivered, rep.duplicates, rep.notify_dropped, rep.lost_jobs,
                    rep.late_after_cancel, rep.cancels, rep.unknowns, rep.resolved_unknowns,
                    rep.budget_questions, rep.budget_resets, rep.budget_declines,
                    rep.asked, rep.asked_declined, rep.one_frame_answers, rep.ended_unsent,
                    rep.limit_changes, rep.limit_raises, rep.limits_followed, rep.limit_proceeds,
                    rep.legacy_migrated,
                    rep.wakes_set, rep.repeats_set, rep.wakes_fired, rep.repeats_rearmed,
                    rep.wakes_missed, rep.long_downs, rep.wakes_ended, rep.wakes_cancelled,
                    rep.sim2, rep.reconciles, rep.invariant_checks, rep.final_positions, rep.wall_ms
                );
                totals.budget_questions += rep.budget_questions;
                totals.budget_resets += rep.budget_resets;
                totals.budget_declines += rep.budget_declines;
                totals.asked += rep.asked;
                totals.asked_declined += rep.asked_declined;
                totals.one_frame_answers += rep.one_frame_answers;
                totals.ended_unsent += rep.ended_unsent;
                totals.limit_changes += rep.limit_changes;
                totals.limit_raises += rep.limit_raises;
                totals.limits_followed += rep.limits_followed;
                totals.limit_proceeds += rep.limit_proceeds;
                totals.wakes_set += rep.wakes_set;
                totals.repeats_set += rep.repeats_set;
                totals.wakes_fired += rep.wakes_fired;
                totals.repeats_rearmed += rep.repeats_rearmed;
                totals.wakes_missed += rep.wakes_missed;
                totals.long_downs += rep.long_downs;
                totals.wakes_ended += rep.wakes_ended;
                totals.wakes_cancelled += rep.wakes_cancelled;
                totals.crashes += rep.crashes;
                totals.startup_faults += rep.startup_faults;
                totals.turns += rep.turns;
                totals.actions += rep.actions;
                totals.one_frame_dispatches += rep.one_frame_dispatches;
                totals.batches += rep.batches;
                totals.batch_actions += rep.batch_actions;
                totals.batch_crashes += rep.batch_crashes;
                totals.authorized_and_dispatched += rep.authorized_and_dispatched;
                totals.input_admits += rep.input_admits;
                totals.own_results += rep.own_results;
                totals.faults += rep.faults;
                totals.fault_wakes += rep.fault_wakes;
                totals.races += rep.races;
                totals.race_ops += rep.race_ops;
                totals.race_frames += rep.race_frames;
                totals.race_crashes += rep.race_crashes;
                totals.completions_delivered += rep.completions_delivered;
                totals.duplicates += rep.duplicates;
                totals.notify_dropped += rep.notify_dropped;
                totals.lost_jobs += rep.lost_jobs;
                totals.late_after_cancel += rep.late_after_cancel;
                totals.cancels += rep.cancels;
                totals.unknowns += rep.unknowns;
                totals.resolved_unknowns += rep.resolved_unknowns;
                totals.invariant_checks += rep.invariant_checks;
                totals.sim2.add(&rep.sim2);
                totals.wall_ms += rep.wall_ms;
            }
            if seeds > 1 {
                println!(
                    "TOTAL {} seeds: {} crashes ({} startup faults) · {} turns · {} actions ({} in one frame; {} batches of {}, {} crashes inside one) · theseus-l6y: {} authorized and dispatched in one frame, {} input turns woken and admitted in one frame, {} results their turn read itself, {} faults after them ({} woken) · {} raced turns ({} ops on a second thread, {} of them transactions, {} crashes inside one) · {} completions ({} dup, {} notify lost, {} lost jobs, {} late-after-cancel) · {} cancels · {} unknown → {} resolved · {} budget questions ({} reset, {} declined) · theseus-w98: {} calls asked the operator ({} declined; {} answers in one frame), {} unsent actions a cancel ended · {} limit changes ({} raised; {} limits followed, {} budget waits proceeded) · 37a: {} wakes set, {} repeating; {} taken, {} series put back, {} occurrences passed over ({} crashes down for minutes), {} ended by until; {} wakes cancelled · {} · {} invariant checks · {} ms · all invariants held",
                    seeds, totals.crashes, totals.startup_faults, totals.turns, totals.actions,
                    totals.one_frame_dispatches, totals.batches, totals.batch_actions,
                    totals.batch_crashes, totals.authorized_and_dispatched, totals.input_admits,
                    totals.own_results, totals.faults, totals.fault_wakes, totals.races, totals.race_ops, totals.race_frames, totals.race_crashes,
                    totals.completions_delivered, totals.duplicates, totals.notify_dropped,
                    totals.lost_jobs, totals.late_after_cancel, totals.cancels, totals.unknowns,
                    totals.resolved_unknowns, totals.budget_questions, totals.budget_resets,
                    totals.budget_declines, totals.asked, totals.asked_declined,
                    totals.one_frame_answers, totals.ended_unsent, totals.limit_changes, totals.limit_raises,
                    totals.limits_followed, totals.limit_proceeds,
                    totals.wakes_set, totals.repeats_set, totals.wakes_fired, totals.repeats_rearmed,
                    totals.wakes_missed, totals.long_downs, totals.wakes_ended, totals.wakes_cancelled,
                    totals.sim2, totals.invariant_checks, totals.wall_ms
                );
            }
            Ok(())
        }
    }
}

/// The lifecycle bench's verdict after its report, and its row in the history
/// when `record` names one: whether the run passed, strictly or on the busy
/// allowance (theseus-lew7). A run the allowance carried says so phase by
/// phase, and its row records the strict verdict, a miss, with the allowance.
fn lifecycle_verdict(
    report: &lifecycle::Report,
    record: Option<PathBuf>,
    label: Option<String>,
    allowance: u32,
) -> bool {
    let passed = report.ok();
    let carried = !passed && report.ok_with(allowance);
    if let Some(path) = record {
        let label = label.unwrap_or_default();
        let mut row = history::Row::of(
            &report.phases,
            &report.verdicts,
            passed,
            &label,
            history::load1(),
            history::now(),
        );
        row.allowance = carried.then_some(allowance);
        match history::append(&path, &row) {
            Ok(()) => println!("lifecycle: recorded in {} as {label:?}", path.display()),
            Err(e) => eprintln!("lifecycle: the run was NOT recorded: {e:#}"),
        }
    }
    if passed {
        // Drift shows before it fails: a warning, never a failure.
        for line in history::near_limits(&report.verdicts) {
            println!("{line}");
        }
    } else if carried {
        for line in history::allowance_applied("lifecycle", &report.verdicts, allowance) {
            println!("{line}");
        }
        println!(
            "lifecycle: passed on the busy allowance (+{allowance}%); the history records the strict verdict, a miss"
        );
    }
    passed || carried
}

// ---------------------------------------------------------------- workload

fn random_batch(rng: &mut StdRng) -> Vec<NewRecord> {
    let n = rng.random_range(1..=4);
    (0..n)
        .map(|_| {
            let kind = match rng.random_range(0..10) {
                0..=4 => kinds::LEDGER,
                5..=6 => kinds::SESSION,
                7 => kinds::META,
                8 => kinds::COMPLETION,
                _ => kinds::EXECUTION,
            };
            let key = match kind {
                kinds::SESSION => Some(format!("ses_{}", rng.random_range(0..50))),
                kinds::META => Some(format!("meta_{}", rng.random_range(0..5))),
                kinds::COMPLETION | kinds::EXECUTION => {
                    Some(format!("x_{}", rng.random_range(0..500)))
                }
                _ => None,
            };
            let len = match rng.random_range(0..10) {
                0..=6 => rng.random_range(16..256),
                7..=8 => rng.random_range(256..4096),
                _ => rng.random_range(4096..32768),
            };
            let mut payload = vec![0u8; len];
            rng.fill_bytes(&mut payload);
            NewRecord::bytes(kind, key.as_deref(), payload)
        })
        .collect()
}

fn worker(dir: &Path, seed: u64, checkpoint_every: u64, writers: usize) -> Result<()> {
    let store = WalStore::open(dir, WalConfig::default())?.with_checkpoint_every(checkpoint_every);
    let base = seed ^ store.last_position();
    {
        let mut out = std::io::stdout().lock();
        writeln!(
            out,
            "R {} {} {}",
            store.last_position(),
            store.recovery().truncated_bytes,
            store.stats()?.wal_bytes
        )?;
        out.flush()?;
    }
    // Each writer appends on its own thread, so their frames queue together
    // at the store's writer; the first is this thread.
    let store = std::sync::Arc::new(store);
    for w in 1..writers.max(1) {
        let store = store.clone();
        std::thread::spawn(move || {
            if let Err(e) = append_until_killed(&store, base ^ ((w as u64) << 32)) {
                eprintln!("worker writer {w}: {e:#}");
                std::process::exit(1);
            }
        });
    }
    append_until_killed(&store, base)
}

/// Append random frames, and report each one's records once its append has
/// returned, which is when they are durable.
fn append_until_killed(store: &WalStore, seed: u64) -> Result<()> {
    let mut rng = StdRng::seed_from_u64(seed);
    loop {
        let batch = random_batch(&mut rng);
        let crcs: Vec<u32> = batch.iter().map(|r| crc32fast::hash(&r.payload)).collect();
        let kinds_: Vec<u16> = batch.iter().map(|r| r.kind).collect();
        let positions = store.append(&batch)?; // durable when this returns
                                               // The WAL's length, before the records it covers: a report of a
                                               // record is never ahead of the report of its bytes. With several
                                               // writers it may count another's frame not yet synced, which only
                                               // keeps a tear further back.
        let wal_bytes = store.stats()?.wal_bytes;
        let mut out = std::io::stdout().lock();
        writeln!(out, "D {wal_bytes}")?;
        for ((p, crc), k) in positions.iter().zip(crcs).zip(kinds_) {
            writeln!(out, "C {p} {crc} {k}")?;
        }
        out.flush()?;
    }
}

// ---------------------------------------------------------------- crash test

#[derive(Default, Debug)]
struct Committed {
    // position -> (crc, kind)
    map: std::collections::BTreeMap<u64, (u32, u16)>,
    reported_start: Option<u64>,
    /// The WAL length no tear may reach into: the longest the worker
    /// reported durable, or an open (the worker's, or a verify's) read back
    /// after a kill (theseus-4x6).
    durable_bytes: u64,
}

/// Run a worker on `dir` and SIGKILL it `live_ms` after it starts. The kill
/// may land anywhere, the store's very first open included: an index that
/// open left half written is moved aside and built again from the WAL by the
/// next (theseus-0b8).
fn run_and_kill(
    bin: &Path,
    dir: &Path,
    seed: u64,
    live_ms: u64,
    writers: usize,
    committed: &mut Committed,
) -> Result<()> {
    // A worker killed inside its own open (a recovery after the last kill)
    // reports no start, and the last worker's must not stand in for it: on
    // a loaded machine, that once failed the test on records no crash lost
    // (theseus-qa0 F4b).
    committed.reported_start = None;
    let mut child = Command::new(bin)
        .args([
            "worker",
            "--dir",
            &dir.to_string_lossy(),
            "--seed",
            &seed.to_string(),
            "--writers",
            &writers.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawning worker")?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for l in BufReader::new(stdout).lines() {
            match l {
                Ok(l) => lines.push(l),
                Err(_) => break,
            }
        }
        lines
    });
    std::thread::sleep(Duration::from_millis(live_ms));
    child.kill()?; // SIGKILL on unix
    let _ = child.wait();
    let lines = reader.join().unwrap();
    for l in lines {
        let parts: Vec<&str> = l.split_whitespace().collect();
        match parts.as_slice() {
            ["R", last, _trunc, bytes] => {
                let last: u64 = last.parse()?;
                committed.reported_start = Some(last);
                committed.durable_bytes = committed.durable_bytes.max(bytes.parse()?);
            }
            ["D", bytes] => {
                committed.durable_bytes = committed.durable_bytes.max(bytes.parse()?);
            }
            ["C", p, crc, k] => {
                committed.map.insert(p.parse()?, (crc.parse()?, k.parse()?));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Damage the WAL tail the way a crash mid-write would: truncate a few bytes,
/// or append garbage, or flip a byte in the last frame. A crash cannot take
/// back what an fdatasync made durable, nor what an open has read back since,
/// so the damage stays past `durable` (`Committed::durable_bytes`): bytes the
/// worker wrote but never reported, or the start of a frame that never
/// finished. Tearing into a reported frame made the test fail on records no
/// crash could lose (theseus-hco, theseus-4x6).
fn tear_tail(dir: &Path, durable: u64, rng: &mut StdRng) -> Result<String> {
    let wal = dir.join("wal");
    if !wal.is_dir() {
        // Killed inside the store's first open, before its WAL existed.
        return Ok("no WAL yet".into());
    }
    let mut segs: Vec<PathBuf> = std::fs::read_dir(&wal)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "seg"))
        .collect();
    segs.sort();
    let Some(last) = segs.last() else {
        return Ok("no segment".into());
    };
    let len = std::fs::metadata(last)?.len();
    if len < 64 {
        return Ok("segment too small to tear".into());
    }
    let mut total = 0u64;
    for seg in &segs {
        total += std::fs::metadata(seg)?.len();
    }
    if total < durable {
        bail!("the WAL holds {total} bytes but {durable} were reported durable");
    }
    // Bytes of the last segment that no report covers.
    let open = (total - durable).min(len);
    let how = if open == 0 { 1 } else { rng.random_range(0..3) };
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(last)?;
    match how {
        0 => {
            let cut = rng.random_range(1..=open.min(47));
            f.set_len(len - cut)?;
            Ok(format!("truncated {cut} bytes"))
        }
        1 => {
            use std::io::Seek;
            let mut f = f;
            f.seek(std::io::SeekFrom::End(0))?;
            let n = rng.random_range(1..200);
            let mut junk = vec![0u8; n];
            rng.fill_bytes(&mut junk);
            f.write_all(&junk)?;
            Ok(format!("appended {n} junk bytes"))
        }
        _ => {
            use std::io::{Seek, SeekFrom};
            let mut f = f;
            let at = len - rng.random_range(1..=open.min(39));
            f.seek(SeekFrom::Start(at))?;
            let mut b = [0u8; 1];
            std::io::Read::read_exact(&mut f, &mut b)?;
            f.seek(SeekFrom::Start(at))?;
            f.write_all(&[b[0] ^ 0x5A])?;
            Ok(format!("flipped byte at {at}"))
        }
    }
}

/// What a verify found: the store's last position, the torn bytes its open
/// cut, the WAL's length after that cut, and whether the open found an index
/// that was not a database (a kill inside the first open) and moved it aside.
struct Verified {
    last: u64,
    truncated: u64,
    wal_bytes: u64,
    moved_aside: bool,
}

fn verify(dir: &Path, committed: &Committed) -> Result<Verified> {
    let store = WalStore::open(dir, WalConfig::default())?.with_checkpoint_every(0);
    let last = store.last_position();
    // Every reported-committed record must exist with identical payload.
    for (&p, &(crc, kind)) in &committed.map {
        let r = store.get(p)?.ok_or_else(|| {
            anyhow::anyhow!("committed position {p} missing after recovery (last={last})")
        })?;
        if r.position != p || r.kind != kind || r.payload_crc() != crc {
            bail!("committed position {p} differs after recovery: pos {} kind {} crc {:08x} vs {:08x}", r.position, r.kind, r.payload_crc(), crc);
        }
    }
    // Positions are contiguous 1..=last with no gaps.
    let mut p = 1u64;
    while p <= last {
        if store.get(p)?.is_none() {
            bail!("gap: position {p} missing but last is {last}");
        }
        p += 1;
    }
    // The highest committed position is <= last (records may be durable but unreported, never the reverse).
    if let Some((&hi, _)) = committed.map.iter().next_back() {
        if hi > last {
            bail!("reported committed {hi} but store last is {last}");
        }
    }
    let st = store.stats()?;
    Ok(Verified {
        last,
        truncated: st.truncated_bytes,
        wal_bytes: st.wal_bytes,
        moved_aside: st.index_moved_aside.is_some(),
    })
}

fn crash_test(
    iterations: u32,
    seed: u64,
    restarts: u32,
    tear: bool,
    worker_bin: Option<PathBuf>,
    writers: usize,
) -> Result<()> {
    let bin = worker_bin.unwrap_or(std::env::current_exe()?);
    let mut rng = StdRng::seed_from_u64(seed);
    let started = Instant::now();
    let mut total_records = 0u64;
    let mut total_torn = 0u64;
    let mut unreported = 0u64;
    let mut moved_aside = 0u32;
    for it in 0..iterations {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path().join("store");
        let mut committed = Committed::default();
        let mut last_seen = 0u64;
        for r in 0..restarts {
            let live_ms = rng.random_range(15..250);
            run_and_kill(
                &bin,
                &dir,
                seed + it as u64 * 1000 + r as u64,
                live_ms,
                writers,
                &mut committed,
            )?;
            if let Some(start) = committed.reported_start {
                if start < last_seen {
                    bail!("iteration {it} restart {r}: worker saw last={start} but a previous verify saw {last_seen}");
                }
            }
            let tore = if tear && rng.random_bool(0.7) {
                tear_tail(&dir, committed.durable_bytes, &mut rng)?
            } else {
                "no tear".into()
            };
            let v = verify(&dir, &committed).with_context(|| {
                format!("iteration {it} restart {r} (after kill at {live_ms} ms; {tore})")
            })?;
            // What this open read back is on disk now: no later crash takes
            // it back, so no later tear reaches into it, though no worker
            // reported its last frames (the next one may die inside its own
            // open before it reports anything).
            committed.durable_bytes = committed.durable_bytes.max(v.wal_bytes);
            let hi = committed.map.keys().next_back().copied().unwrap_or(0);
            unreported += v.last.saturating_sub(hi);
            total_torn += v.truncated;
            last_seen = v.last;
            total_records = total_records.max(v.last);
            let aside = if v.moved_aside {
                moved_aside += 1;
                ", index moved aside"
            } else {
                ""
            };
            println!(
                "iter {it:>3} restart {r}: lived {live_ms:>3} ms, committed reported {:>6}, store last {:>6}, {tore}, truncated {} B{aside}",
                committed.map.len(),
                v.last,
                v.truncated,
            );
        }
    }
    println!(
        "CRASH TEST OK: {iterations} iterations × {restarts} restarts, {writers} writer(s), seed {seed}, tear {tear}, {:.1}s; torn bytes removed {total_torn}; durable-but-unreported records {unreported} (allowed); indexes moved aside after a kill in the first open {moved_aside}; zero committed records lost",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAL of two segments, 5000 and 3000 bytes, of seeded bytes.
    fn wal_of(dir: &Path) -> Vec<u8> {
        let wal = dir.join("wal");
        std::fs::create_dir_all(&wal).unwrap();
        let mut rng = StdRng::seed_from_u64(42);
        let mut all = Vec::new();
        for (n, len) in [(1u32, 5000usize), (2, 3000)] {
            let mut b = vec![0u8; len];
            rng.fill_bytes(&mut b);
            std::fs::write(wal.join(format!("{n:09}.seg")), &b).unwrap();
            all.extend_from_slice(&b);
        }
        all
    }

    fn read_wal(dir: &Path) -> Vec<u8> {
        let mut all = std::fs::read(dir.join("wal/000000001.seg")).unwrap();
        all.extend(std::fs::read(dir.join("wal/000000002.seg")).unwrap());
        all
    }

    /// A tear never lands inside the length reported durable (theseus-4x6):
    /// over many seeds and every bound, from nothing reported to all of it,
    /// the WAL's first `durable` bytes are left exactly as they were, and
    /// every way of tearing (a cut, junk appended, a flipped byte) happens.
    #[test]
    fn a_tear_never_lands_inside_the_reported_durable_length() {
        let mut ways = std::collections::BTreeSet::new();
        for durable in [0u64, 1, 4999, 5000, 5001, 7000, 7952, 7999, 8000] {
            for seed in 0..200u64 {
                let dir = tempfile::tempdir().unwrap();
                let before = wal_of(dir.path());
                let mut rng = StdRng::seed_from_u64(seed);
                let how = tear_tail(dir.path(), durable, &mut rng).unwrap();
                ways.insert(how.split(' ').next().unwrap().to_string());
                let after = read_wal(dir.path());
                let d = durable as usize;
                assert!(after.len() >= d, "{how}: {} < {durable}", after.len());
                assert_eq!(&after[..d], &before[..d], "{how}, durable {durable}");
                assert_ne!(after, before, "{how}: the tear changed nothing");
            }
        }
        let ways: Vec<_> = ways.into_iter().collect();
        assert_eq!(ways, ["appended", "flipped", "truncated"]);
        // A WAL shorter than its reported length is a lost frame, never a tear.
        let dir = tempfile::tempdir().unwrap();
        wal_of(dir.path());
        let e = tear_tail(dir.path(), 8001, &mut StdRng::seed_from_u64(1)).unwrap_err();
        assert!(e.to_string().contains("reported durable"), "{e}");
        // Killed inside the first open: no WAL yet, and nothing to tear.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            tear_tail(dir.path(), 0, &mut StdRng::seed_from_u64(1)).unwrap(),
            "no WAL yet"
        );
    }
}
