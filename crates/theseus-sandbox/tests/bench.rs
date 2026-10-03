//! The spawn micro-bench (design §2.2, "Cost and the probe"): 100 L1
//! starts of `/bin/true`, each as 17b will start a job (a workspace root
//! under an overlay, HOME, the system binds, `/proc`, `/sys`, `/dev`), timed
//! from the dispatch (`spawn`) to the command's exec, which is when `spawn`
//! returns. The target is a p95 under 25 ms.
//!
//! It prints p50, p95, and the max, with the init's own part split out, and
//! the same for an L0 start (a plain fork and exec) beside it. The case
//! fails only past ten times the target, so a loaded machine never fails the
//! gate; `THESEUS_SANDBOX_BENCH_STRICT=1` holds it to the target itself.
//!
//! And the egress proxy's cost (design §2.10, 18c): a `CONNECT`'s first byte
//! back through the proxy, against a direct connection to the same server,
//! 200 of each; the target is under 2 ms added, held as the spawn's is.

mod common;

use std::time::{Duration, Instant};

use common::{check, job, Case, Output};

const RUNS: usize = 100;
const TARGET: Duration = Duration::from_millis(25);

fn main() {
    common::main(
        &[
            Case {
                name: "spawn_100",
                run: bench,
            },
            Case {
                name: "connect_first_byte_200",
                run: first_byte,
            },
        ],
        &[],
        |_| {},
    );
}

fn pct(sorted: &[Duration], p: f64) -> Duration {
    let i = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
    sorted[i]
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn line(what: &str, mut v: Vec<Duration>) -> Duration {
    v.sort();
    let p95 = pct(&v, 0.95);
    println!(
        "{what:<26} p50 {:>9}  p95 {:>9}  max {:>9}",
        ms(pct(&v, 0.50)),
        ms(p95),
        ms(*v.last().unwrap_or(&Duration::ZERO))
    );
    p95
}

fn bench() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spec = job(vec!["/bin/true".into()], ws.path());
    let out = Output::new();
    // One start to warm the page cache, untimed.
    common::spawn(&spec, &out)?
        .wait()
        .map_err(|e| e.to_string())?;
    let (mut start, mut init, mut whole) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut child = common::spawn(&spec, &out)?;
        start.push(t.elapsed());
        init.push(Duration::from_micros(child.started().setup_us));
        let exit = child.wait().map_err(|e| e.to_string())?;
        whole.push(t.elapsed());
        check(exit.success(), format!("/bin/true in L1: {exit:?}"))?;
    }
    let mut l0 = Vec::new();
    for _ in 0..RUNS {
        let t = Instant::now();
        let mut c = std::process::Command::new("/bin/true")
            .spawn()
            .map_err(|e| e.to_string())?;
        l0.push(t.elapsed());
        c.wait().map_err(|e| e.to_string())?;
    }
    println!(
        "{RUNS} starts of /bin/true (target: an L1 start's p95 under {})",
        ms(TARGET)
    );
    let p95 = line("L1 start (dispatch→exec)", start);
    line("  of which the init", init);
    line("L1 start to exit", whole);
    line("L0 start (fork+exec)", l0);
    let strict = std::env::var_os("THESEUS_SANDBOX_BENCH_STRICT").is_some();
    let bound = if strict { TARGET } else { TARGET * 10 };
    println!(
        "L1 p95 {} the target{}",
        if p95 <= TARGET { "meets" } else { "misses" },
        if strict { " (strict)" } else { "" }
    );
    check(
        p95 <= bound,
        format!("an L1 start's p95 is {}, past {}", ms(p95), ms(bound)),
    )
}

/// 18c's bench: from a client's connect to its first byte back, directly to
/// an echo server and through the proxy (its `CONNECT`, the list, the
/// resolver, the connect, the 200, and the tunnel), each with one byte sent
/// as a TLS hello would be.
fn first_byte() -> Result<(), String> {
    use std::io::{Read, Write};
    use std::net::{IpAddr, TcpListener, TcpStream};
    use theseus_sandbox::egress::{Proxy, Resolver};
    const N: usize = 200;
    const ADDED: Duration = Duration::from_millis(2);
    let e = |e: std::io::Error| e.to_string();
    let echo = TcpListener::bind("127.0.0.1:0").map_err(e)?;
    let target = echo.local_addr().map_err(e)?;
    std::thread::spawn(move || {
        for s in echo.incoming().flatten() {
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let _ = std::io::copy(&mut &s, &mut w);
            });
        }
    });
    let l = TcpListener::bind("127.0.0.1:0").map_err(e)?;
    let proxy = l.local_addr().map_err(e)?;
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    let mut resolver = Resolver::default();
    resolver.hosts.insert("echo.test".into(), vec![lo]);
    resolver.public.push(lo);
    let allow = vec![format!("echo.test:{}", target.port())
        .parse()
        .map_err(|e: String| e)?];
    let running = Proxy::new(l.into(), allow, resolver).start().map_err(e)?;
    let request = format!("CONNECT echo.test:{} HTTP/1.1\r\n\r\nx", target.port());
    let direct = |_: usize| -> Result<Duration, String> {
        let t = Instant::now();
        let mut s = TcpStream::connect(target).map_err(e)?;
        s.write_all(b"x").map_err(e)?;
        s.read_exact(&mut [0u8; 1]).map_err(e)?;
        Ok(t.elapsed())
    };
    let tunnelled = |_: usize| -> Result<Duration, String> {
        let t = Instant::now();
        let mut s = TcpStream::connect(proxy).map_err(e)?;
        s.write_all(request.as_bytes()).map_err(e)?;
        // The 200's head, then the echoed byte.
        let mut got = Vec::new();
        let mut b = [0u8; 256];
        while !got.ends_with(b"\r\n\r\nx") {
            let n = s.read(&mut b).map_err(e)?;
            check(n > 0, "the proxy closed the tunnel")?;
            got.extend_from_slice(&b[..n]);
        }
        Ok(t.elapsed())
    };
    // Warm both paths, untimed.
    direct(0)?;
    tunnelled(0)?;
    let d: Vec<Duration> = (0..N).map(direct).collect::<Result<_, _>>()?;
    let p: Vec<Duration> = (0..N).map(tunnelled).collect::<Result<_, _>>()?;
    drop(running);
    println!(
        "{N} first bytes (target: under {} added through the proxy)",
        ms(ADDED)
    );
    let (mut ds, mut ps) = (d.clone(), p.clone());
    ds.sort();
    ps.sort();
    let dp95 = line("direct (connect→1st byte)", d);
    let pp95 = line("through the proxy", p);
    let added50 = pct(&ps, 0.50).saturating_sub(pct(&ds, 0.50));
    let added95 = pp95.saturating_sub(dp95);
    println!("added: p50 {}, p95 {}", ms(added50), ms(added95));
    let strict = std::env::var_os("THESEUS_SANDBOX_BENCH_STRICT").is_some();
    let bound = if strict { ADDED } else { ADDED * 10 };
    check(
        added50 <= bound,
        format!("the proxy adds {} at p50, past {}", ms(added50), ms(bound)),
    )
}
