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
/// the daemon's own value of a name wins over the shell's.
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
        ("PATH", "/usr/bin"),
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
