//! The login-shell capture (theseus-v4cj). Every shell here is a stub script
//! the test writes, never the machine's own; the deadline is a duration given
//! to tokio's timer beside a real child, so no test reads the wall clock.

use super::login_env::{parse_env, pick_shell, LoginEnv};
use super::ToolRuntime;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("theseus-login-env-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A stub login shell: a script that ignores `-l -c env` and runs `body`.
fn stub(d: &std::path::Path, body: &str) -> String {
    let p = d.join("stubsh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.to_string_lossy().into_owned()
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn runtime(listed: &[&str], own: &[(&str, &str)]) -> ToolRuntime {
    let mut t = ToolRuntime::disabled();
    t.login_env = LoginEnv::new(names(listed));
    t.proc_env = own
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    t
}

const TOOLCHAIN: &str = "echo 'greeting from an rc file'\n\
    echo VIRTUAL_ENV=/opt/example/venv\necho JAVA_HOME=/opt/example/jdk\n\
    echo NVM_DIR=/opt/example/nvm\necho PATH=/opt/example/shell/bin";

/// (a) A captured, allowlisted name reaches the environment a job starts with;
/// the daemon's own value of a name wins over the shell's — **except `PATH`,
/// which is MERGED** (theseus-v4cj, parity with OpenClaw).
///
/// This test is the rewrite of the one that asserted the shell's `PATH` was
/// discarded: it asserted `("PATH", "/usr/bin")`, the daemon's own value, and
/// that exactly one `PATH` reached a job. The old behaviour was deliberate,
/// which is why the test is rewritten rather than deleted. It now asserts the
/// merge, and it is **stronger** than it was, not weaker: it still pins
/// exactly one `PATH`, it still pins the daemon's own directory and still
/// pins its POSITION (first, so nothing a job could resolve before resolves
/// elsewhere now), and it pins in addition that the shell's directory is
/// reachable and that it follows.
#[tokio::test]
async fn a_captured_allowlisted_name_reaches_the_jobs_environment() {
    let d = dir("a");
    let sh = stub(&d, TOOLCHAIN);
    let t = runtime(
        &["VIRTUAL_ENV", "JAVA_HOME", "NVM_DIR", "PATH"],
        &[("PATH", "/usr/bin")],
    );
    assert!(
        !t.job_base_env().iter().any(|(k, _)| k == "JAVA_HOME"),
        "before the capture"
    );
    assert_eq!(
        t.job_base_env()
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str()),
        Some("/usr/bin"),
        "before the capture a job has the daemon's PATH alone"
    );
    let got = t
        .login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    assert_eq!(got.len(), 4, "{got:?}");
    let env = t.job_base_env();
    for (k, v) in [
        ("VIRTUAL_ENV", "/opt/example/venv"),
        ("JAVA_HOME", "/opt/example/jdk"),
        ("NVM_DIR", "/opt/example/nvm"),
        // Part 1 of the ask: the shell's directory is reachable by a job, and
        // the daemon's keeps its place in front of it.
        ("PATH", "/usr/bin:/opt/example/shell/bin"),
    ] {
        assert!(
            env.contains(&(k.to_string(), v.to_string())),
            "{k}: {env:?}"
        );
    }
    assert_eq!(
        env.iter().filter(|(k, _)| k == "PATH").count(),
        1,
        "{env:?}"
    );
    // Part 2: the daemon's own environment is untouched. `proc_env` is what
    // the daemon resolved for itself at startup, and the merge is per job.
    assert_eq!(
        t.proc_env,
        vec![("PATH".to_string(), "/usr/bin".to_string())]
    );
}

/// Part 4 of theseus-v4cj: `[tools] path_prepend` puts the operator's own
/// directories first, before the daemon's and before the shell's, and it is
/// empty by default (the test above runs with it empty and sees the daemon's
/// directory first).
#[tokio::test]
async fn the_operators_path_prepend_is_searched_before_everything_else() {
    let d = dir("f");
    let sh = stub(&d, TOOLCHAIN);
    let mut t = runtime(&["PATH"], &[("PATH", "/usr/bin:/bin")]);
    t.path_prepend = vec!["/opt/example/mine".to_string()];
    let got = t
        .login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    assert_eq!(got, names(&["PATH"]), "{got:?}");
    let env = t.job_base_env();
    assert_eq!(
        env.iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.as_str()),
        Some("/opt/example/mine:/usr/bin:/bin:/opt/example/shell/bin"),
        "{env:?}"
    );
}

/// (b) and (c): a captured secret-shaped name never reaches a job, even
/// when the operator listed it; a name on neither list is not passed on.
#[tokio::test]
async fn captured_secrets_and_unlisted_names_do_not_reach_a_job() {
    let d = dir("b");
    let sh = stub(
        &d,
        "echo JAVA_HOME=/opt/example/jdk\necho AWS_SECRET_ACCESS_KEY=example-secret-a\n\
         echo GITHUB_TOKEN=example-secret-b\necho UNLISTED_THING=example-c",
    );
    // The operator listed a secret by mistake; the rule still refuses it.
    let t = runtime(&["JAVA_HOME", "GITHUB_TOKEN"], &[]);
    let got = t
        .login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    assert_eq!(got, names(&["JAVA_HOME"]));
    let env = t.job_base_env();
    assert_eq!(env.len(), 1, "{env:?}");
    for (k, v) in &env {
        assert!(!k.contains("SECRET") && !k.contains("TOKEN") && k != "UNLISTED_THING");
        assert!(!v.contains("example-"), "{k}");
    }
}

/// The warning names a refused variable and never its value.
#[tokio::test]
async fn the_warning_names_a_refused_variable_and_not_its_value() {
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sink = Sink::default();
    let w = sink.clone();
    let sub = tracing_subscriber::fmt()
        .with_writer(move || w.clone())
        .with_ansi(false)
        .finish();
    let _g = tracing::subscriber::set_default(sub);
    let d = dir("w");
    let sh = stub(
        &d,
        "echo JAVA_HOME=/opt/example/jdk\necho AWS_SECRET_ACCESS_KEY=example-secret-zzz",
    );
    let t = runtime(&["JAVA_HOME"], &[]);
    t.login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    let log = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("AWS_SECRET_ACCESS_KEY"), "{log}");
    assert!(!log.contains("example-secret-zzz"), "{log}");
    assert!(!log.contains("/opt/example/jdk"), "{log}");
}

/// (d) A shell that never answers is killed at the deadline; the job
/// environment is what it was, and the capture does not run again.
#[tokio::test]
async fn a_shell_that_hangs_is_cut_at_the_deadline() {
    let d = dir("d");
    let sh = stub(&d, "sleep 60");
    let t = runtime(&["JAVA_HOME"], &[("HOME", "/var/example")]);
    let got = t
        .login_env
        .capture(Some(&sh), Duration::from_millis(300))
        .await;
    assert!(got.is_empty(), "{got:?}");
    assert!(t.login_env.done());
    assert_eq!(
        t.job_base_env(),
        vec![("HOME".to_string(), "/var/example".to_string())]
    );
}

/// (e) Building the runtime runs no shell: the stub leaves a marker only
/// when it runs, and it has not run until the capture is asked for.
#[tokio::test]
async fn nothing_runs_a_shell_until_the_capture_is_asked_for() {
    let d = dir("e");
    let marker = d.join("ran");
    let sh = stub(
        &d,
        &format!(
            "touch {}\necho JAVA_HOME=/opt/example/jdk",
            marker.display()
        ),
    );
    let t = runtime(&["JAVA_HOME"], &[]);
    // `std::env::var("SHELL")` is read by the after-serving task alone.
    assert!(!t.login_env.done());
    assert!(
        !marker.exists(),
        "a shell ran before the capture was asked for"
    );
    t.login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    assert!(marker.exists());
    // Once: a second capture neither runs the shell nor changes the result.
    std::fs::remove_file(&marker).unwrap();
    t.login_env
        .capture(Some(&sh), Duration::from_secs(10))
        .await;
    assert!(!marker.exists(), "the shell ran twice");
    assert_eq!(t.login_env.kept().len(), 1);
}

/// (f) `$SHELL` unset, missing, or not executable degrades to the fallback
/// or to nothing, without a panic.
#[tokio::test]
async fn an_unset_or_unusable_shell_degrades() {
    let d = dir("f");
    let plain = d.join("notexec");
    std::fs::write(&plain, "x").unwrap();
    let is_exec = |p: &str| p == "/bin/sh";
    assert_eq!(pick_shell(None, is_exec).as_deref(), Some("/bin/sh"));
    assert_eq!(pick_shell(Some(""), is_exec).as_deref(), Some("/bin/sh"));
    assert_eq!(
        pick_shell(Some(plain.to_str().unwrap()), is_exec).as_deref(),
        Some("/bin/sh")
    );
    assert_eq!(pick_shell(Some("/nope"), |_| false), None);
    let t = runtime(&["JAVA_HOME"], &[]);
    for sh in [
        None,
        Some("/nonexistent/shell"),
        Some(plain.to_str().unwrap()),
    ] {
        let t2 = runtime(&["JAVA_HOME"], &[]);
        t2.login_env.capture(sh, Duration::from_secs(10)).await;
        assert!(t2.login_env.done());
    }
    drop(t);
}

#[test]
fn parse_skips_noise_and_keeps_the_last_of_a_name() {
    let p = parse_env("hello world\nA=1\n1B=2\nC D=3\nA=4\nE=\nF=x=y\n");
    assert_eq!(
        p,
        vec![
            ("A".to_string(), "4".to_string()),
            ("E".to_string(), String::new()),
            ("F".to_string(), "x=y".to_string())
        ]
    );
}

/// The capture's child is registered with the daemon's children registry
/// before anything waits for it (theseus-v4cj, the rework of the capture).
///
/// `theseus_kernel::children`'s rule is that every child the daemon starts
/// goes through `spawn`: one that did not, and whose spawner waits for it,
/// can be reaped by the serve path's SIGCHLD sweep as an orphan, and the
/// spawner's own `wait()` then fails `ECHILD`. `children::census` tells the
/// two apart without racing anything: a live child of this process that the
/// registry owns counts in `owned`, and one it does not counts in `orphans`.
/// So the test holds the shell open at a gate file, reads the census while
/// the child certainly runs, and asserts both sides — one more owned child,
/// and no more orphans — so an empty census cannot pass the negative.
///
/// Nextest gives each test its own process, so these counts are this test's
/// own; the deltas are taken regardless. No wall clock is read: the gate is a
/// file, and the waits are durations given to tokio's timer.
#[tokio::test]
async fn the_captures_child_is_registered_and_is_no_orphan() {
    use theseus_kernel::children;
    let d = dir("registered");
    let gate = d.join("let-it-exit");
    let sh = stub(
        &d,
        &format!(
            "echo JAVA_HOME=/opt/example/jdk\nwhile [ ! -f {} ]; do sleep 0.01; done",
            gate.display()
        ),
    );
    let before = children::census();
    let t = runtime(&["JAVA_HOME"], &[]);
    let capture = t.login_env.capture(Some(&sh), Duration::from_secs(30));
    let watch = async {
        let mut seen = None;
        for _ in 0..1_000 {
            let c = children::census();
            if c.owned > before.owned || c.orphans > before.orphans {
                seen = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        std::fs::write(&gate, "go").expect("open the gate");
        seen
    };
    let (got, seen) = tokio::join!(capture, watch);
    let c = seen.expect("the capture's child appeared among this process's children");
    assert_eq!(
        c.owned,
        before.owned + 1,
        "the capture's child is registered as owned: {c:?} after {before:?}"
    );
    assert_eq!(
        c.orphans, before.orphans,
        "and the registry does not read it as an orphan: {c:?} after {before:?}"
    );
    // The registration did not cost the capture its result, and the sweep
    // reaped nothing of it.
    assert_eq!(got, names(&["JAVA_HOME"]));
    assert_eq!(
        children::census().reaped_orphans,
        before.reaped_orphans,
        "no sweep took the capture's child"
    );
}
