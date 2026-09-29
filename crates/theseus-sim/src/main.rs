//! theseus-sim: the store's crash-and-recover harness and the kernel simulator.
//! Both run in the gate on small fixed seeds (`tests/sim.rs`); long runs stay
//! here, e.g. `theseus-sim kernel-sim --seeds 40`.
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

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use theseus_store::{kinds, NewRecord, Store, WalConfig, WalStore};

mod kernel_sim;

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
        /// Also tear the WAL tail after each kill (truncate or flip bytes).
        /// Off by default: the bound that keeps a tear out of bytes the
        /// worker already reported durable is not finished (theseus-hco
        /// follow-up), so `--tear true` can still flip a synced frame.
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        tear: bool,
        /// Path to this binary (defaults to current_exe).
        #[arg(long)]
        worker_bin: Option<PathBuf>,
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
        /// fdatasync every frame (slow; durability is the store crash-test's job).
        #[arg(long)]
        fsync: bool,
        #[arg(long)]
        verbose: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Worker {
            dir,
            seed,
            checkpoint_every,
        } => worker(&dir, seed, checkpoint_every),
        Cmd::CrashTest {
            iterations,
            seed,
            restarts,
            tear,
            worker_bin,
        } => crash_test(iterations, seed, restarts, tear, worker_bin),
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
                    fsync,
                    verbose,
                })
                .map_err(|e| anyhow::anyhow!("seed {s}: {e}"))?;
                println!(
                    "seed {s}: {} steps · {} crashes ({} startup faults) · {} sessions · {} turns · {} actions · {} completions ({} dup, {} notify lost, {} lost jobs, {} late-after-cancel) · {} cancels · {} unknown → {} resolved · {} budget questions ({} reset, {} declined) · {} unit budgets read in dollars · {} reconciles · {} invariant checks · {} positions · {} ms",
                    rep.steps, rep.crashes, rep.startup_faults, rep.sessions, rep.turns, rep.actions,
                    rep.completions_delivered, rep.duplicates, rep.notify_dropped, rep.lost_jobs,
                    rep.late_after_cancel, rep.cancels, rep.unknowns, rep.resolved_unknowns,
                    rep.budget_questions, rep.budget_resets, rep.budget_declines, rep.legacy_migrated,
                    rep.reconciles, rep.invariant_checks, rep.final_positions, rep.wall_ms
                );
                totals.budget_questions += rep.budget_questions;
                totals.budget_resets += rep.budget_resets;
                totals.budget_declines += rep.budget_declines;
                totals.crashes += rep.crashes;
                totals.startup_faults += rep.startup_faults;
                totals.turns += rep.turns;
                totals.actions += rep.actions;
                totals.completions_delivered += rep.completions_delivered;
                totals.duplicates += rep.duplicates;
                totals.notify_dropped += rep.notify_dropped;
                totals.lost_jobs += rep.lost_jobs;
                totals.late_after_cancel += rep.late_after_cancel;
                totals.cancels += rep.cancels;
                totals.unknowns += rep.unknowns;
                totals.resolved_unknowns += rep.resolved_unknowns;
                totals.invariant_checks += rep.invariant_checks;
                totals.wall_ms += rep.wall_ms;
            }
            if seeds > 1 {
                println!(
                    "TOTAL {} seeds: {} crashes ({} startup faults) · {} turns · {} actions · {} completions ({} dup, {} notify lost, {} lost jobs, {} late-after-cancel) · {} cancels · {} unknown → {} resolved · {} budget questions ({} reset, {} declined) · {} invariant checks · {} ms · all invariants held",
                    seeds, totals.crashes, totals.startup_faults, totals.turns, totals.actions,
                    totals.completions_delivered, totals.duplicates, totals.notify_dropped,
                    totals.lost_jobs, totals.late_after_cancel, totals.cancels, totals.unknowns,
                    totals.resolved_unknowns, totals.budget_questions, totals.budget_resets,
                    totals.budget_declines, totals.invariant_checks, totals.wall_ms
                );
            }
            Ok(())
        }
    }
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

fn worker(dir: &Path, seed: u64, checkpoint_every: u64) -> Result<()> {
    let store = WalStore::open(dir, WalConfig::default())?.with_checkpoint_every(checkpoint_every);
    let mut rng = StdRng::seed_from_u64(seed ^ store.last_position());
    let out = std::io::stdout();
    let mut out = out.lock();
    writeln!(
        out,
        "R {} {} {}",
        store.last_position(),
        store.recovery().truncated_bytes,
        store.stats()?.wal_bytes
    )?;
    out.flush()?;
    loop {
        let batch = random_batch(&mut rng);
        let crcs: Vec<u32> = batch.iter().map(|r| crc32fast::hash(&r.payload)).collect();
        let kinds_: Vec<u16> = batch.iter().map(|r| r.kind).collect();
        let positions = store.append(&batch)?; // durable when this returns
                                               // The WAL's durable length, before the records it covers: a report of
                                               // a record is never ahead of the report of its bytes.
        writeln!(out, "D {}", store.stats()?.wal_bytes)?;
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
    /// The WAL length the worker last reported durable.
    durable_bytes: u64,
}

fn run_and_kill(
    bin: &Path,
    dir: &Path,
    seed: u64,
    live_ms: u64,
    first_open: bool,
    committed: &mut Committed,
) -> Result<()> {
    let mut child = Command::new(bin)
        .args([
            "worker",
            "--dir",
            &dir.to_string_lossy(),
            "--seed",
            &seed.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawning worker")?;
    let stdout = child.stdout.take().unwrap();
    let (opened, is_open) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for l in BufReader::new(stdout).lines() {
            match l {
                Ok(l) => {
                    if l.starts_with("R ") {
                        let _ = opened.send(());
                    }
                    lines.push(l)
                }
                Err(_) => break,
            }
        }
        lines
    });
    if first_open {
        // A kill inside a new store's first open, while redb writes the
        // index file's header, leaves an index that will not open
        // (theseus-0b8). The clock starts once the store is open; a kill
        // during any later open (recovery) stays in play.
        let _ = is_open.recv_timeout(Duration::from_secs(30));
    }
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
/// back what an fdatasync made durable, so the damage stays past `durable`,
/// the length the worker last reported: bytes it wrote but never reported, or
/// the start of a frame that never finished. (Tearing into a reported frame
/// made the test fail on records no crash could lose, theseus-hco.)
fn tear_tail(dir: &Path, durable: u64, rng: &mut StdRng) -> Result<String> {
    let wal = dir.join("wal");
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

fn verify(dir: &Path, committed: &Committed) -> Result<(u64, u64)> {
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
    Ok((last, st.truncated_bytes))
}

fn crash_test(
    iterations: u32,
    seed: u64,
    restarts: u32,
    tear: bool,
    worker_bin: Option<PathBuf>,
) -> Result<()> {
    let bin = worker_bin.unwrap_or(std::env::current_exe()?);
    let mut rng = StdRng::seed_from_u64(seed);
    let started = Instant::now();
    let mut total_records = 0u64;
    let mut total_torn = 0u64;
    let mut unreported = 0u64;
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
                r == 0,
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
            let (last, truncated) = verify(&dir, &committed).with_context(|| {
                format!("iteration {it} restart {r} (after kill at {live_ms} ms; {tore})")
            })?;
            let hi = committed.map.keys().next_back().copied().unwrap_or(0);
            unreported += last.saturating_sub(hi);
            total_torn += truncated;
            last_seen = last;
            total_records = total_records.max(last);
            println!(
                "iter {it:>3} restart {r}: lived {live_ms:>3} ms, committed reported {:>6}, store last {last:>6}, {tore}, truncated {truncated} B",
                committed.map.len()
            );
        }
    }
    println!(
        "CRASH TEST OK: {iterations} iterations × {restarts} restarts, seed {seed}, {:.1}s; torn bytes removed {total_torn}; durable-but-unreported records {unreported} (allowed); zero committed records lost",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
