//! Background work yields to the machine (theseus-tood), on the real
//! `/proc/pressure`: this test binary runs itself again in a user and mount
//! namespace of its own (`unshare -rm`), with a directory of fake pressure
//! files bound over `/proc/pressure`, and drives it busy and quiet by
//! rewriting them. A chunk waits while it is busy, goes once it is quiet,
//! and goes at its bound when it stays busy. Where namespaces can't be made
//! (no `unshare`, or unprivileged user namespaces off), it says so and
//! passes.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use theseus_store::pressure::{self, Busy, LOOK_EVERY};

/// Set in the run inside the namespaces: the fake files' directory.
const INNER: &str = "THESEUS_TEST_FAKE_PSI";

fn fake(dir: &Path, cpu: f64, io: f64) {
    let line = |v: f64| format!("some avg10={v:.2} avg60=0.00 avg300=0.00 total=1\n");
    std::fs::write(dir.join("cpu"), line(cpu)).unwrap();
    std::fs::write(dir.join("io"), line(io)).unwrap();
}

#[test]
fn a_chunk_waits_while_the_machine_is_busy_and_goes_when_quiet() {
    if let Some(dir) = std::env::var_os(INNER) {
        inside(&PathBuf::from(dir));
        return;
    }
    let d = tempfile::tempdir().unwrap();
    fake(d.path(), 0.0, 0.0);
    let exe = std::env::current_exe().unwrap();
    let out = Command::new("unshare")
        .args(["-rm", "sh", "-c"])
        .arg(r#"mount --bind "$1" /proc/pressure && exec "$2" --exact a_chunk_waits_while_the_machine_is_busy_and_goes_when_quiet --nocapture"#)
        .args([OsStr::new("sh"), d.path().as_os_str(), exe.as_os_str()])
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
    if !out.status.success() && !text.contains("PSI-INNER") {
        eprintln!("skipped: no namespaces to fake /proc/pressure in: {text}");
        return;
    }
    assert!(out.status.success(), "{text}");
    assert!(text.contains("PSI-INNER ok"), "{text}");
    for line in text.lines().filter(|l| l.starts_with("PSI-INNER")) {
        println!("{line}");
    }
}

/// In the namespaces: `/proc/pressure` is the fake directory.
fn inside(dir: &Path) {
    println!("PSI-INNER started");
    fake(dir, 1.0, 1.0);
    assert_eq!(pressure::busy(), None, "quiet");
    fake(dir, 1.0, 12.5);
    assert_eq!(pressure::busy(), Some(Busy::Io(12.5)));

    // Busy, then quiet after 1.5 s: the chunk waited, and went within a look.
    fake(dir, 64.0, 0.0);
    assert_eq!(pressure::busy(), Some(Busy::Cpu(64.0)));
    let t0 = Instant::now();
    let chunk = std::thread::spawn(|| pressure::quiet_blocking(Duration::from_secs(20)));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!chunk.is_finished(), "it waits while busy");
    fake(dir, 2.0, 0.0);
    let waited = chunk.join().unwrap();
    println!("PSI-INNER waited {waited:?} busy, then went");
    assert!(waited >= Duration::from_millis(1500), "{waited:?}");
    assert!(
        waited < Duration::from_millis(1500) + LOOK_EVERY * 3,
        "{waited:?}"
    );
    assert!(t0.elapsed() < Duration::from_secs(20));

    // Busy for good: it goes at its bound.
    fake(dir, 0.0, 80.0);
    let bound = Duration::from_millis(2500);
    let waited = pressure::quiet_blocking(bound);
    println!("PSI-INNER waited {waited:?} at the bound {bound:?}");
    assert!(waited >= bound, "{waited:?}");
    assert!(waited < bound + LOOK_EVERY * 2, "{waited:?}");

    // Quiet: no wait at all.
    fake(dir, 0.0, 0.0);
    assert!(pressure::quiet_blocking(bound) < Duration::from_millis(100));
    println!("PSI-INNER ok");
}
