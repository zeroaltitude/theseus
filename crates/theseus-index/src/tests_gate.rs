//! A recall query's load gate (theseus-d7fy), on fake pressure: each test
//! that needs the machine busy or calm runs itself again in a mount
//! namespace of its own (`unshare -rm`) with a directory of fake files bound
//! over `/proc/pressure`, as theseus-store's `tests/pressure.rs` does, and
//! rewrites them to drive the gate with no load at all. Where namespaces
//! can't be made, it says so and passes.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::gate::{Gate, Lane, Missed};
use crate::tests::user;
use crate::vtests::{settle_all, VRig, TEXTS};

/// Set in the run inside the namespaces: the fake files' directory.
const INNER: &str = "THESEUS_TEST_FAKE_PSI";

/// The fake `/proc/pressure`.
struct Fake(PathBuf);

impl Fake {
    fn set(&self, cpu: f64, io: f64) {
        let line = |v: f64| format!("some avg10={v:.2} avg60=0.00 avg300=0.00 total=1\n");
        std::fs::write(self.0.join("cpu"), line(cpu)).unwrap();
        std::fs::write(self.0.join("io"), line(io)).unwrap();
    }
    fn busy(&self) {
        self.set(64.0, 0.0);
    }
    fn calm(&self) {
        self.set(1.0, 1.0);
    }
    /// No PSI at all: a kernel without it.
    fn none(&self) {
        let _ = std::fs::remove_file(self.0.join("cpu"));
        let _ = std::fs::remove_file(self.0.join("io"));
    }
}

/// Run `name` (this test's path) again inside the namespaces and call `body`
/// there; outside, check that it said it was done.
fn in_fake_psi(name: &str, body: impl FnOnce(&Fake)) {
    if let Some(dir) = std::env::var_os(INNER) {
        body(&Fake(PathBuf::from(dir)));
        println!("PSI-INNER ok");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    Fake(d.path().to_path_buf()).calm();
    let exe = std::env::current_exe().unwrap();
    let out = Command::new("unshare")
        .args(["-rm", "sh", "-c"])
        .arg(r#"mount --bind "$1" /proc/pressure && exec "$2" --exact "$3" --nocapture --test-threads=1"#)
        .args([
            OsStr::new("sh"),
            d.path().as_os_str(),
            exe.as_os_str(),
            OsStr::new(name),
        ])
        .env(INNER, d.path())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            eprintln!("skipped: no unshare here ({e})");
            return;
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && !text.contains("PSI-INNER") && text.contains("unshare") {
        eprintln!("skipped: no namespaces to fake /proc/pressure in: {text}");
        return;
    }
    assert!(out.status.success(), "{text}");
    assert!(text.contains("PSI-INNER ok"), "{text}");
}

fn never() -> bool {
    false
}

/// A gate that looks again every 20 ms, so a test's rewrite is seen at once.
fn gate() -> Gate {
    Gate::with_look(Duration::from_millis(20))
}

/// A machine that reads calm: a query takes the full pool, and any number run
/// at once.
#[test]
fn a_calm_machine_runs_queries_on_the_full_pool_all_at_once() {
    in_fake_psi(
        "tests_gate::a_calm_machine_runs_queries_on_the_full_pool_all_at_once",
        |f| {
            f.calm();
            let g = gate();
            let a = g.admit(&never).unwrap();
            let b = g.admit(&never).unwrap();
            let c = g.admit(&never).unwrap();
            assert_eq!(
                [a.lane(), b.lane(), c.lane()],
                [Lane::Full, Lane::Full, Lane::Full]
            );
            let counts = g.counts();
            assert!(!counts.shut);
            assert_eq!(
                (counts.waited, counts.expired, counts.one_thread),
                (0, 0, 0)
            );
        },
    );
}

/// A busy machine (CPU, then IO): a query runs on one thread and holds the
/// line; the next waits for it, and the ones behind go in the order they came.
#[test]
fn a_busy_machine_admits_one_query_at_a_time_in_order() {
    in_fake_psi(
        "tests_gate::a_busy_machine_admits_one_query_at_a_time_in_order",
        |f| {
            let g = Arc::new(gate());
            for (cpu, io) in [(64.0, 0.0), (0.0, 12.5)] {
                f.set(cpu, io);
                std::thread::sleep(Duration::from_millis(30));
                let first = g.admit(&never).unwrap();
                assert_eq!(first.lane(), Lane::One, "cpu {cpu} io {io}");
                let (tx, rx) = mpsc::channel();
                let mut waiters = Vec::new();
                for n in 0..3 {
                    let (gate, tx) = (g.clone(), tx.clone());
                    waiters.push(std::thread::spawn(move || {
                        let p = gate.admit(&never).unwrap();
                        assert_eq!(p.lane(), Lane::One);
                        tx.send(n).unwrap();
                        // Holding the turn a moment: nobody else is admitted.
                        std::thread::sleep(Duration::from_millis(40));
                    }));
                    // In order of arrival: each is in the line before the next.
                    let t0 = Instant::now();
                    while g.counts().waited < (n + 1) as u64 + before(cpu) {
                        assert!(t0.elapsed() < Duration::from_secs(5));
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
                assert!(rx.try_recv().is_err(), "they wait while the first runs");
                drop(first);
                let order: Vec<i32> = (0..3)
                    .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
                    .collect();
                assert_eq!(order, [0, 1, 2], "in the order they came");
                for w in waiters {
                    w.join().unwrap();
                }
            }
            let c = g.counts();
            assert_eq!(c.waited, 6);
            assert_eq!(c.expired, 0);
            assert_eq!(c.one_thread, 8);
        },
    );
}

/// The waits the first round left in the count.
fn before(cpu: f64) -> u64 {
    if cpu > 0.0 {
        0
    } else {
        3
    }
}

/// A query in line whose caller goes leaves it, never failing on the gate's
/// account: its turn is not taken, and the one behind it runs. One that waits
/// out the longest wait is `Waited`. Both count as expired.
#[test]
fn a_queued_query_that_runs_out_of_time_leaves_the_line() {
    in_fake_psi(
        "tests_gate::a_queued_query_that_runs_out_of_time_leaves_the_line",
        |f| {
            f.busy();
            let g = Arc::new(gate());
            std::thread::sleep(Duration::from_millis(30));
            let first = g.admit(&never).unwrap();
            let gone = Arc::new(AtomicBool::new(false));
            let (g2, gone2) = (g.clone(), gone.clone());
            let leaver = std::thread::spawn(move || {
                g2.admit(&|| gone2.load(Ordering::SeqCst)).map(|p| p.lane())
            });
            let g3 = g.clone();
            let patient = std::thread::spawn(move || g3.admit(&never).map(|p| p.lane()));
            std::thread::sleep(Duration::from_millis(100));
            gone.store(true, Ordering::SeqCst);
            assert_eq!(leaver.join().unwrap(), Err(Missed::Gone));
            assert!(!patient.is_finished());
            drop(first);
            assert_eq!(
                patient.join().unwrap(),
                Ok(Lane::One),
                "the one behind runs"
            );
            // The longest wait, for a caller that never goes.
            let held = g.admit(&never).unwrap();
            let t0 = Instant::now();
            let late = g
                .admit_within(&never, Duration::from_millis(150))
                .map(|p| p.lane());
            assert_eq!(late, Err(Missed::Waited));
            assert!(t0.elapsed() >= Duration::from_millis(150));
            drop(held);
            let c = g.counts();
            assert_eq!((c.waited, c.expired), (3, 2));
        },
    );
}

/// A recall's whole query, through the tender: held in line past the longest
/// wait, it answers with its words and says why in `skipped`: no error. Its
/// caller gone, it stops (the contract of a closed connection).
#[test]
fn a_queued_recall_answers_with_the_words_alone() {
    in_fake_psi(
        "tests_gate::a_queued_recall_answers_with_the_words_alone",
        |f| {
            f.busy();
            let v = VRig::new();
            let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
            v.rig.put(&nodes);
            let mut t = v.open();
            settle_all(&mut t);
            let shared = t.shared();
            let turn = shared.vectors.ahead.gate.admit(&never).unwrap();
            assert_eq!(turn.lane(), Lane::One);
            let mut p = crate::proto::QueryParams::new("tides follow the moon");
            p.k = 3;
            p.wait_ms = 10_000;
            p.sources = vec!["bm25".into(), "vector".into()];
            let t0 = Instant::now();
            let r = shared.query(&p).expect("the gate never fails a query");
            assert!(t0.elapsed() >= crate::gate::MAX_WAIT);
            assert!(!r.hits.is_empty(), "the words answered");
            let why = r.skipped.get("vector").expect("the vector source says why");
            assert!(why.contains("busy"), "{why}");
            // A caller that has gone: the query stops.
            let gone = shared.query_while(&p, &|| true);
            assert!(gone.is_err());
            drop(turn);
            // The line free again, the same query has its vectors.
            let r = shared.query(&p).unwrap();
            assert!(r.skipped.is_empty(), "{:?}", r.skipped);
            let c = shared.vectors.ahead.gate.counts();
            assert!(c.shut);
            assert!(c.waited >= 2 && c.expired >= 2, "{c:?}");
            // `index.status` says it: the pool, the gate shut now, the counts.
            let g = shared
                .status()
                .vectors
                .unwrap()
                .gate
                .expect("the gate in the status");
            assert_eq!(g.threads as usize, crate::ahead::threads());
            assert!(g.shut);
            assert_eq!(
                (g.waited, g.expired, g.one_thread),
                (c.waited, c.expired, c.one_thread)
            );
            f.calm();
            std::thread::sleep(Duration::from_millis(1100));
            assert!(
                !shared.status().vectors.unwrap().gate.unwrap().shut,
                "open again"
            );
        },
    );
}

/// The gate opens when the files go calm: a query in line goes at once on the
/// full pool, though the one ahead of it still runs, and the next one is not
/// held at all.
#[test]
fn the_gate_opens_when_the_machine_goes_calm() {
    in_fake_psi(
        "tests_gate::the_gate_opens_when_the_machine_goes_calm",
        |f| {
            f.busy();
            let g = Arc::new(gate());
            std::thread::sleep(Duration::from_millis(30));
            let first = g.admit(&never).unwrap();
            assert_eq!(first.lane(), Lane::One);
            assert!(g.counts().shut);
            let g2 = g.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || tx.send(g2.admit(&never).map(|p| p.lane())));
            std::thread::sleep(Duration::from_millis(100));
            assert!(rx.try_recv().is_err(), "held while busy");
            f.calm();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(5)),
                Ok(Ok(Lane::Full)),
                "calm: the full pool, with the first still running"
            );
            assert!(!g.counts().shut);
            assert_eq!(g.admit(&never).unwrap().lane(), Lane::Full);
            drop(first);
        },
    );
}

/// A kernel without PSI: the gate is open.
#[test]
fn without_pressure_information_the_gate_is_open() {
    in_fake_psi(
        "tests_gate::without_pressure_information_the_gate_is_open",
        |f| {
            f.none();
            let g = gate();
            let a = g.admit(&never).unwrap();
            let b = g.admit(&never).unwrap();
            assert_eq!([a.lane(), b.lane()], [Lane::Full, Lane::Full]);
            assert!(!g.counts().shut);
        },
    );
}

/// A pressure read is kept for a `look`: a file rewritten inside it is not
/// read, and one past it is. Never a file a query.
#[test]
fn pressure_is_read_once_a_look() {
    in_fake_psi("tests_gate::pressure_is_read_once_a_look", |f| {
        f.calm();
        let g = Gate::with_look(Duration::from_millis(400));
        assert!(!g.shut());
        f.busy();
        for _ in 0..50 {
            assert!(!g.shut(), "kept: the busy file is not read inside the look");
        }
        std::thread::sleep(Duration::from_millis(450));
        assert!(g.shut(), "read again past the look");
    });
}

/// Each thread of this process, and the CPU ticks it has used (`utime` and
/// `stime` of `/proc/self/task/<tid>/stat`).
fn thread_ticks() -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    for t in std::fs::read_dir("/proc/self/task").unwrap().flatten() {
        let Ok(stat) = std::fs::read_to_string(t.path().join("stat")) else {
            continue;
        };
        // After the command's closing parenthesis: state is field 3, utime
        // field 14, stime 15.
        let rest = stat.rsplit_once(')').unwrap().1;
        let f: Vec<&str> = rest.split_whitespace().collect();
        let ticks = f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap();
        out.insert(t.file_name().to_string_lossy().into_owned(), ticks);
    }
    out
}

/// The threads that did the work `f` ran, and how long it took: those whose
/// CPU time grew by a tenth of the total or more.
fn threads_used(f: impl FnOnce()) -> (usize, Duration) {
    let before = thread_ticks();
    let t0 = Instant::now();
    f();
    let wall = t0.elapsed();
    let after = thread_ticks();
    let grew: Vec<u64> = after
        .iter()
        .map(|(tid, t)| t - before.get(tid).copied().unwrap_or(0))
        .collect();
    let total: u64 = grew.iter().sum();
    assert!(
        total >= 20,
        "the work used {total} ticks: too little to tell"
    );
    (grew.iter().filter(|g| **g * 10 >= total).count(), wall)
}

/// candle's matmul, as an embedding runs it: `RAYON_NUM_THREADS` jobs a
/// matmul, in the pool its caller is in.
fn matmuls() {
    use candle_core::{DType, Device, Tensor};
    let d = Device::Cpu;
    // The shapes of a query's layer (37 tokens through nomic-bert's 768-d
    // hidden size and 3072-d feed-forward): the three projections' and the
    // two feed-forward matmuls'.
    let w = |k, n| Tensor::ones((k, n), DType::F32, &d).unwrap();
    let x = Tensor::ones((37, 768), DType::F32, &d).unwrap();
    let h = Tensor::ones((37, 3072), DType::F32, &d).unwrap();
    let (qkv, out, up, down) = (w(768, 2304), w(768, 768), w(768, 3072), w(3072, 768));
    for _ in 0..60 {
        std::hint::black_box(x.matmul(&qkv).unwrap());
        std::hint::black_box(x.matmul(&out).unwrap());
        std::hint::black_box(x.matmul(&up).unwrap());
        std::hint::black_box(h.matmul(&down).unwrap());
    }
}

/// The finding behind the gate's one-thread path: `RAYON_NUM_THREADS` is
/// process-wide (the backfill's and the queries' larger one), so candle splits
/// every matmul four ways wherever it runs; run in a pool of one thread, the
/// four jobs run one after another on that thread, and nothing else
/// computes. Counted by the CPU each thread of the process used, so it is a
/// test for a process of its own, as nextest runs it (`cargo test` with
/// `--test-threads=1`): another test's threads would be counted too.
#[test]
fn a_busy_lane_embeds_on_one_thread_and_a_calm_one_on_the_pool() {
    std::env::set_var("RAYON_NUM_THREADS", "4");
    let cores = crate::ahead::cores();
    let (one, one_wall) = threads_used(|| crate::ahead::install_in(Lane::One, matmuls));
    println!("TWO-LANES one thread: {one} thread(s) computed, {one_wall:?}");
    assert_eq!(one, 1, "the busy lane's work ran on {one} threads");
    if cores >= 2 && crate::ahead::threads() >= 2 {
        let (full, full_wall) = threads_used(|| crate::ahead::install_in(Lane::Full, matmuls));
        println!("TWO-LANES full pool: {full} thread(s) computed, {full_wall:?}");
        assert!(full >= 2, "the calm lane's work ran on {full} thread");
    }
    // For the record: the same pool with matmuls not split at all.
    std::env::set_var("RAYON_NUM_THREADS", "1");
    let (_, unsplit) = threads_used(|| crate::ahead::install_in(Lane::One, matmuls));
    println!("TWO-LANES one thread, unsplit matmuls: {unsplit:?} (split four ways: {one_wall:?})");
}

/// A measure, not a check (`--ignored --nocapture`; theseus-d7fy): recalls'
/// vector queries against the real model, each given a recall's 250 ms
/// deadline, `THESEUS_MEASURE_AT_ONCE` of them at a time. `THESEUS_TEST_WEIGHTS`
/// is the directory holding `nomic-embed-text-v1.5/`; `THESEUS_MEASURE_GATE` is
/// `auto` (the kernel's pressure), `calm` (the full pool, no line), `one` (one
/// thread, one at a time) or `four-wide` (the full pool even while busy: the
/// gate absent). Start busy loops beside it for a busy machine.
#[test]
#[ignore = "a measure: run with --ignored --nocapture, with real weights"]
fn measure_recalls_vectors_inside_their_deadline() {
    use crate::vectors::VectorConfig;
    let Some(dir) = std::env::var_os("THESEUS_TEST_WEIGHTS") else {
        eprintln!("THESEUS_TEST_WEIGHTS is not set: nothing to measure");
        return;
    };
    let mode = std::env::var("THESEUS_MEASURE_GATE").unwrap_or_else(|_| "auto".into());
    let at_once: usize = std::env::var("THESEUS_MEASURE_AT_ONCE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let rounds: usize = std::env::var("THESEUS_MEASURE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    std::env::set_var("RAYON_NUM_THREADS", crate::ahead::threads().to_string());
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut cfg = v.rig.cfg(&v.rig.index);
    cfg.vectors = VectorConfig::new(Some(PathBuf::from(dir)));
    let mut t = crate::Tender::open(cfg).unwrap();
    settle_all(&mut t);
    let shared = t.shared();
    match mode.as_str() {
        "calm" | "four-wide" => shared.vectors.ahead.gate.force(Some(false)),
        "one" => shared.vectors.ahead.gate.force(Some(true)),
        _ => {}
    }
    let words = [
        "what did we decide about the harbour tide tables last week",
        "which ocelot slept by the river and who fed it afterwards",
        "remind me how the kumquat orchard irrigation schedule was set",
        "the lighthouse keeper asked for a new lamp before the winter storms",
    ];
    let mut in_time = Vec::new();
    let (mut expired, mut skipped) = (0, 0);
    for round in 0..rounds {
        let hs: Vec<_> = (0..at_once)
            .map(|i| {
                let shared = shared.clone();
                let text = format!("{} {round} {i}", words[(round + i) % words.len()]);
                std::thread::spawn(move || {
                    let mut p = crate::proto::QueryParams::new(&text);
                    p.sources = vec!["vector".into()];
                    p.vector_tokens = 32;
                    p.wait_ms = 10_000;
                    let t0 = Instant::now();
                    let r = shared.query_while(&p, &|| t0.elapsed() > Duration::from_millis(250));
                    (
                        t0.elapsed(),
                        r.map(|r| (r.timings.embed_ms, !r.skipped.is_empty())),
                    )
                })
            })
            .collect();
        for h in hs {
            let (wall, r) = h.join().unwrap();
            match r {
                Ok((ms, false)) => in_time.push((ms, wall)),
                Ok((_, true)) => skipped += 1,
                Err(_) => expired += 1,
            }
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    let pct = |mut v: Vec<f64>, p: f64| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.get(((v.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(f64::NAN)
    };
    let embed: Vec<f64> = in_time.iter().map(|(e, _)| *e).collect();
    let wall: Vec<f64> = in_time.iter().map(|(_, w)| w.as_secs_f64() * 1e3).collect();
    let c = shared.vectors.ahead.gate.counts();
    println!(
        "MEASURE gate={mode} at_once={at_once} queries={} vectors_in_time={} expired={expired} words_only={skipped} \
         embed_ms p50={:.0} p90={:.0} · wall_ms p50={:.0} p90={:.0} · gate waited={} expired={} one_thread={}",
        rounds * at_once,
        in_time.len(),
        pct(embed.clone(), 0.5),
        pct(embed, 0.9),
        pct(wall.clone(), 0.5),
        pct(wall, 0.9),
        c.waited,
        c.expired,
        c.one_thread
    );
}
