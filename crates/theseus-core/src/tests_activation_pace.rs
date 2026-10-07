//! The adjacency projection's warm build is paced by the machine's pressure
//! between its pages of the walk (theseus-3edq); a refresh, which a turn
//! runs inside recall's deadline, never is.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_store::pressure::LOOK_EVERY;

use crate::recall::activation::Adjacent;
use crate::recall::adjacency::{Projection, PACES, PAGE};
use crate::store::Store;
use crate::tests_activation::{put, user};

/// `n` nodes of a session, in one frame.
pub(crate) fn write(store: &Store, session: &str, n: usize) {
    let nodes: Vec<_> = (0..n)
        .map(|i| user(session, &format!("note {i}")))
        .collect();
    put(store, &nodes.iter().collect::<Vec<_>>());
}

fn paces() -> u64 {
    PACES.with(|c| c.get())
}

/// A store of more than a page of nodes: the warm build paces once between
/// its two pages, a search's own build never does, and a refresh over more
/// than a page of new writes paces zero times.
#[test]
fn the_warm_build_paces_between_its_pages_and_a_refresh_never_does() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    write(&store, "ses_heron", PAGE + 1);

    let before = paces();
    Adjacent::default().build(&store, false).unwrap();
    assert_eq!(paces() - before, 0, "a search's build waits for no one");

    let before = paces();
    let warm = Adjacent::default();
    warm.build(&store, true).unwrap();
    assert_eq!(
        paces() - before,
        1,
        "the warm build paces once, between pages"
    );
    assert_eq!(warm.stats().unwrap().nodes, PAGE as u64 + 1);

    // More than a page again, written after: the refresh folds both pages
    // and waits for neither.
    let mut p = Projection::build(&store).unwrap();
    write(&store, "ses_osprey", PAGE + 1);
    let before = paces();
    p.refresh(&store).unwrap();
    assert_eq!(paces() - before, 0, "a refresh inside a turn never waits");
    assert_eq!(p.stats().nodes, 2 * PAGE as u64 + 2);
}

/// Set in the run inside the namespaces: the fake pressure files' directory.
pub(crate) const INNER: &str = "THESEUS_TEST_FAKE_PSI";

/// A clean stop ends the warm build's waits (the join fix to theseus-3edq): a
/// stop drops the runtime, whose end waits for the blocking pool, so a build
/// still waiting out a busy machine held the stop up to `BOUND` a page. This
/// test binary runs itself again in a user and mount namespace of its own
/// (`unshare -rm`) with a directory of fake pressure files bound over
/// `/proc/pressure`, saying IO is busy, as theseus-store's pressure test does:
/// a warm build of three pages waits at its first pace, and goes within a
/// look once the stop begins. What it holds is the build's waits, summed on its
/// own thread (`adjacency::WAITED`), from the stop's beginning at its first
/// wait: they end within a few looks of the stop, whatever the machine's speed, where a wait the stop did not end is a whole
/// `BOUND` a pace. It does not time the build, whose work a loaded debug build
/// makes slow. Where namespaces can't be made (no `unshare`, or unprivileged
/// user namespaces off), it says so and passes.
#[test]
fn a_clean_stop_ends_the_warm_builds_waits() {
    if let Some(dir) = std::env::var_os(INNER) {
        stopped_inside(Path::new(&dir));
        return;
    }
    let Some(text) = namespaced(
        "tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits",
        "STOP-INNER",
    ) else {
        return;
    };
    assert!(text.contains("STOP-INNER ok"), "{text}");
}

/// Run `test` again in a user and mount namespace of its own, with a
/// directory of fake pressure files saying busy bound over `/proc/pressure`
/// and [`INNER`] naming it, and give its output; its lines that start with
/// `marker` are printed. `None` (and the test passes) where namespaces
/// can't be made; a run that fails inside them fails the test.
pub(crate) fn namespaced(test: &str, marker: &str) -> Option<String> {
    let d = tempfile::tempdir().unwrap();
    busy(d.path());
    let exe = std::env::current_exe().unwrap();
    let out = Command::new("unshare")
        .args(["-rm", "sh", "-c"])
        .arg(r#"mount --bind "$1" /proc/pressure && exec "$2" --exact "$3" --nocapture"#)
        .args([
            OsStr::new("sh"),
            d.path().as_os_str(),
            exe.as_os_str(),
            OsStr::new(test),
        ])
        .env(INNER, d.path())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            eprintln!("skipped: no unshare here ({e})");
            return None;
        }
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && !text.contains(marker) {
        eprintln!("skipped: no namespaces to fake /proc/pressure in: {text}");
        return None;
    }
    assert!(out.status.success(), "{text}");
    for line in text.lines().filter(|l| l.starts_with(marker)) {
        println!("{line}");
    }
    Some(text)
}

/// IO pressure over the bar: every pace waits its whole bound.
pub(crate) fn busy(dir: &Path) {
    let line = |v: f64| format!("some avg10={v:.2} avg60=0.00 avg300=0.00 total=1\n");
    std::fs::write(dir.join("cpu"), line(1.0)).unwrap();
    std::fs::write(dir.join("io"), line(55.0)).unwrap();
}

/// In the namespaces: `/proc/pressure` says busy.
fn stopped_inside(dir: &Path) {
    println!("STOP-INNER started");
    busy(dir);
    assert!(theseus_store::pressure::busy().is_some(), "busy");
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(&d.path().join("store")).unwrap();
    write(&store, "ses_heron", 2 * PAGE + 1);
    let warm = Arc::new(Adjacent::default());
    let w = warm.clone();
    let t0 = Instant::now();
    // The build's own thread reports how long its paces waited in all.
    let build = std::thread::spawn(move || {
        w.build(&store, true)?;
        Ok::<_, anyhow::Error>(crate::recall::adjacency::WAITED.with(|c| c.get()))
    });
    // Its first pace counts before it waits: the stop begins only once the
    // build is at a wait, however long a loaded debug build takes to fold its
    // first page.
    while warm.paces() == 0 {
        assert!(t0.elapsed() < Duration::from_secs(120), "no first pace");
        std::thread::sleep(Duration::from_millis(5));
    }
    let paced_at = Instant::now();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !build.is_finished(),
        "the warm build waits while the machine is busy"
    );
    let held = paced_at.elapsed();
    crate::startup::stop_began();
    let waited = build.join().unwrap().unwrap();
    println!(
        "STOP-INNER the build ended {:?} after it began and waited {waited:?} in all; the stop \
         began {held:?} into its first wait",
        t0.elapsed()
    );
    // It waited until the stop, and at most a look past it. A wait the stop
    // did not end is a whole `BOUND` (10 s) a pace. The build's own work is no
    // part of this: a debug build's fold is slow under load, and a stop does
    // not shorten it.
    assert!(
        waited >= held * 9 / 10,
        "the build did not wait while busy: {waited:?}, held {held:?}"
    );
    assert!(
        waited < held + LOOK_EVERY * 3,
        "a clean stop still waited for the warm build: {waited:?} of waits, held {held:?}"
    );
    assert!(warm.built());
    assert_eq!(warm.stats().unwrap().nodes, 2 * PAGE as u64 + 1);
    println!("STOP-INNER ok");
}
