//! A test that fakes the machine's pressure: it runs itself again in a user
//! and mount namespace of its own, with fake pressure files bound over
//! `/proc/pressure` (`crate::pace`'s waits read [`INNER`] to know a test made
//! the pressure an input). Shared by `pace`'s own tests and the onto sweep's.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// Set in the run inside the namespaces: the fake pressure files' directory.
pub(crate) const INNER: &str = "THESEUS_TEST_FAKE_PSI";

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
