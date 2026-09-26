//! theseus-sim: crash-and-recover harness and benchmark for the store.
//!
//! `worker`      appends random records forever, printing `C <pos> <crc> <kind>`
//!               to stdout only after the append returned (durable). Killed by
//!               the driver with SIGKILL at a random moment.
//! `crash-test`  the M1 exit test: spawn worker, kill it, optionally tear the
//!               WAL tail, reopen, verify every reported record exists with the
//!               same bytes, positions are contiguous, appends continue; repeat.
//! `bench`       redb vs fjall on our write shape (small records, frames of
//!               1–4, fsync per frame), plus reads by position, key, tail.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use theseus_store::{kinds, Engine, NewRecord, Store, WalConfig, WalStore};

#[derive(Parser)]
#[command(
    name = "theseus-sim",
    version,
    about = "Store crash-and-recover harness and benchmark"
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
        #[arg(long, default_value = "redb")]
        engine: Engine,
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
        #[arg(long, default_value = "redb")]
        engine: Engine,
        /// Restart the same store this many times per iteration.
        #[arg(long, default_value_t = 3)]
        restarts: u32,
        /// Also tear the WAL tail after each kill (truncate or flip bytes).
        #[arg(long, default_value_t = true)]
        tear: bool,
        /// Path to this binary (defaults to current_exe).
        #[arg(long)]
        worker_bin: Option<PathBuf>,
    },
    /// Append/read throughput for one engine.
    Bench {
        #[arg(long, default_value = "redb")]
        engine: Engine,
        #[arg(long, default_value_t = 20_000)]
        records: u64,
        /// Skip fdatasync per frame, to measure the index cost without the disk.
        #[arg(long)]
        no_fsync: bool,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Worker {
            dir,
            engine,
            seed,
            checkpoint_every,
        } => worker(&dir, engine, seed, checkpoint_every),
        Cmd::CrashTest {
            iterations,
            seed,
            engine,
            restarts,
            tear,
            worker_bin,
        } => crash_test(iterations, seed, engine, restarts, tear, worker_bin),
        Cmd::Bench {
            engine,
            records,
            no_fsync,
            dir,
        } => bench(engine, records, !no_fsync, dir),
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

fn worker(dir: &Path, engine: Engine, seed: u64, checkpoint_every: u64) -> Result<()> {
    let store =
        WalStore::open(dir, engine, WalConfig::default())?.with_checkpoint_every(checkpoint_every);
    let mut rng = StdRng::seed_from_u64(seed ^ store.last_position());
    let out = std::io::stdout();
    let mut out = out.lock();
    writeln!(
        out,
        "R {} {}",
        store.last_position(),
        store.recovery().truncated_bytes
    )?;
    out.flush()?;
    loop {
        let batch = random_batch(&mut rng);
        let crcs: Vec<u32> = batch.iter().map(|r| crc32fast::hash(&r.payload)).collect();
        let kinds_: Vec<u16> = batch.iter().map(|r| r.kind).collect();
        let positions = store.append(&batch)?; // durable when this returns
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
}

fn run_and_kill(
    bin: &Path,
    dir: &Path,
    engine: Engine,
    seed: u64,
    live_ms: u64,
    committed: &mut Committed,
) -> Result<()> {
    let mut child = Command::new(bin)
        .args([
            "worker",
            "--dir",
            &dir.to_string_lossy(),
            "--engine",
            engine.as_str(),
            "--seed",
            &seed.to_string(),
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
            ["R", last, _trunc] => {
                let last: u64 = last.parse()?;
                committed.reported_start = Some(last);
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
/// or append garbage, or flip a byte in the last frame.
fn tear_tail(dir: &Path, rng: &mut StdRng) -> Result<String> {
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
    let how = rng.random_range(0..3);
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(last)?;
    match how {
        0 => {
            let cut = rng.random_range(1..48);
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
            let at = len - rng.random_range(1..40);
            f.seek(SeekFrom::Start(at))?;
            let mut b = [0u8; 1];
            std::io::Read::read_exact(&mut f, &mut b)?;
            f.seek(SeekFrom::Start(at))?;
            f.write_all(&[b[0] ^ 0x5A])?;
            Ok(format!("flipped byte at {at}"))
        }
    }
}

fn verify(dir: &Path, engine: Engine, committed: &Committed) -> Result<(u64, u64)> {
    let store = WalStore::open(dir, engine, WalConfig::default())?.with_checkpoint_every(0);
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
    engine: Engine,
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
                engine,
                seed + it as u64 * 1000 + r as u64,
                live_ms,
                &mut committed,
            )?;
            if let Some(start) = committed.reported_start {
                if start < last_seen {
                    bail!("iteration {it} restart {r}: worker saw last={start} but a previous verify saw {last_seen}");
                }
            }
            let tore = if tear && rng.random_bool(0.7) {
                tear_tail(&dir, &mut rng)?
            } else {
                "no tear".into()
            };
            let (last, truncated) = verify(&dir, engine, &committed).with_context(|| {
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
        "CRASH TEST OK: {iterations} iterations × {restarts} restarts, engine {}, seed {seed}, {:.1}s; torn bytes removed {total_torn}; durable-but-unreported records {unreported} (allowed); zero committed records lost",
        engine.as_str(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

// ---------------------------------------------------------------- bench

fn bench(engine: Engine, records: u64, fsync: bool, dir: Option<PathBuf>) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dir = dir.unwrap_or_else(|| tmp.path().join("store"));
    let store = WalStore::open(
        &dir,
        engine,
        WalConfig {
            fsync,
            ..Default::default()
        },
    )?
    .with_checkpoint_every(1000);
    let mut rng = StdRng::seed_from_u64(42);
    let mut appended = 0u64;
    let mut frames = 0u64;
    let t = Instant::now();
    while appended < records {
        let b = random_batch(&mut rng);
        appended += b.len() as u64;
        frames += 1;
        store.append(&b)?;
    }
    let append_s = t.elapsed().as_secs_f64();
    let last = store.last_position();

    let t = Instant::now();
    let n_get = 5000u64.min(last);
    for _ in 0..n_get {
        let p = rng.random_range(1..=last);
        store.get(p)?.expect("exists");
    }
    let get_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    for _ in 0..2000 {
        let k = format!("ses_{}", rng.random_range(0..50));
        let _ = store.latest_by_key(kinds::SESSION, &k)?;
    }
    let key_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    for _ in 0..200 {
        let _ = store.tail_of_kind(kinds::LEDGER, 50)?;
    }
    let tail_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let _ = store.latest_of_kind(kinds::SESSION)?;
    let latest_s = t.elapsed().as_secs_f64();

    let st = store.stats()?;
    drop(store);
    let t = Instant::now();
    let reopened = WalStore::open(&dir, engine, WalConfig::default())?;
    let reopen_s = t.elapsed().as_secs_f64();
    let replayed = reopened.stats()?.replayed_into_index;

    println!(
        "engine {} · fsync {} · {} records in {} frames",
        engine.as_str(),
        fsync,
        appended,
        frames
    );
    println!(
        "  append : {:>9.0} rec/s  {:>9.0} frames/s  ({:.1} ms/frame)",
        appended as f64 / append_s,
        frames as f64 / append_s,
        append_s * 1000.0 / frames as f64
    );
    println!(
        "  get    : {:>9.0} /s  (random by position)",
        n_get as f64 / get_s
    );
    println!(
        "  bykey  : {:>9.0} /s  (latest session by key)",
        2000.0 / key_s
    );
    println!(
        "  tail50 : {:>9.0} /s  (newest 50 ledger rows)",
        200.0 / tail_s
    );
    println!("  latest-of-kind(session): {:.2} ms", latest_s * 1000.0);
    println!(
        "  reopen : {:.1} ms (replayed {} into index)",
        reopen_s * 1000.0,
        replayed
    );
    println!(
        "  wal    : {} B in {} segment(s); index engine {}",
        st.wal_bytes,
        st.wal_segments,
        st.engine.as_str()
    );
    Ok(())
}
