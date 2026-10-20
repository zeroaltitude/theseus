//! `scripts/setup.sh` in a scratch directory (theseus-00me): the real script and the real `theseusd`, with
//! `systemctl`, `loginctl`, and `op` replaced by a stand-in that logs each call, and a scratch prefix, config, and
//! target dir. Nothing here reaches the machine's systemd, its accounts, its `/etc`, or any daemon. Its service step
//! (`user-service.sh install`, a real unit) is proved live on a scratch unit, never here: `--no-build` runs here stop
//! before it, for want of a token file.
//!
//! The script refuses root, and a machine without cgroup v2: these tests are skipped there.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Every stand-in: it logs its call, says the user manager runs and linger is on, and changes nothing.
const STANDIN: &str = r#"#!/bin/bash
{ printf '%s' "${0##*/}"; printf ' %s' "$@"; echo; } >>"${FAKE:?}/calls.log"
case ${0##*/} in
systemctl) [ "$*" = "--user is-system-running" ] && echo running ;;
loginctl) echo yes ;;
esac
exit 0
"#;

/// python3 as this test finds it on its own PATH: the driver runs in a cleared
/// environment whose PATH holds only the stand-ins and the system's dirs.
fn python3() -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("python3"))
                .find(|f| f.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("python3"))
}

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/setup.sh")
}

/// Whether this process runs as root: the `Uid:` line's second field.
fn root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1).map(|e| e == "0"))
        .unwrap()
}

/// One entry of a scratch tree: its path, mode, mtime (seconds, nanoseconds), and a file's bytes.
type Entry = (String, u32, i64, i64, Option<Vec<u8>>);

fn write_exe(at: &Path, text: &str) {
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, text).unwrap();
    std::fs::set_permissions(at, PermissionsExt::from_mode(0o755)).unwrap();
}

/// A scratch dir: `bin/` the stand-ins, `fake/` their log, `home/`, and `target/release/` a build of the five
/// binaries, whose `theseusd` runs the real one and whose others are stubs.
struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        for tool in ["systemctl", "loginctl", "op"] {
            write_exe(&d.join("bin").join(tool), STANDIN);
        }
        std::fs::create_dir_all(d.join("fake")).unwrap();
        std::fs::create_dir_all(d.join("home")).unwrap();
        let built = d.join("target/release");
        write_exe(
            &built.join("theseusd"),
            &format!(
                "#!/bin/sh\nexec '{}' \"$@\"\n",
                env!("CARGO_BIN_EXE_theseusd")
            ),
        );
        for b in ["theseus", "theseus-tui", "theseus-sim", "theseus-index"] {
            write_exe(
                &built.join(b),
                &format!("#!/bin/sh\necho 'a stand-in {b}'\n"),
            );
        }
        Self { dir }
    }

    fn at(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn config(&self) -> PathBuf {
        self.at("etc/theseus/theseus.toml")
    }

    /// The script with a scratch prefix and config, and no inherited Theseus settings.
    fn run(&self, more: &[&str]) -> (i32, String) {
        let out = Command::new("bash")
            .arg(script())
            .arg("--no-build")
            .arg("--prefix")
            .arg(self.at("prefix"))
            .arg("--config")
            .arg(self.config())
            .args(more)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.at("bin").display()),
            )
            .env("HOME", self.at("home"))
            .env("FAKE", self.at("fake"))
            .env("CARGO_TARGET_DIR", self.at("target"))
            .env("LANG", "C.UTF-8")
            .current_dir(self.dir.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.code().unwrap_or(-1), text)
    }

    /// `run`, with the script's stdin a terminal (a pty) that types `answer` and a newline at the Jev key's
    /// question (theseus-is8t), and nothing at any other prompt. Returns the exit code and the whole output.
    fn run_on_a_terminal(&self, more: &[&str], answer: &str) -> (i32, String) {
        const DRIVER: &str = r#"
import os, pty, sys, select
answer, script, *args = sys.argv[1:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp("bash", ["bash", script] + args)
out, sent = b"", 0
while True:
    r, _, _ = select.select([fd], [], [], 60)
    if not r:
        break
    try:
        d = os.read(fd, 4096)
    except OSError:
        break
    if not d:
        break
    out += d
    asked = out.count(b"empty to skip): ")
    if asked > sent:
        os.write(fd, (answer + "\n").encode())
        sent = asked
_, status = os.waitpid(pid, 0)
sys.stdout.write(out.decode(errors="replace"))
sys.exit(os.waitstatus_to_exitcode(status))
"#;
        let out = Command::new(python3())
            .args(["-I", "-c", DRIVER, answer])
            .arg(script())
            .arg("--no-build")
            .arg("--prefix")
            .arg(self.at("prefix"))
            .arg("--config")
            .arg(self.config())
            .args(more)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.at("bin").display()),
            )
            .env("HOME", self.at("home"))
            .env("FAKE", self.at("fake"))
            .env("CARGO_TARGET_DIR", self.at("target"))
            .env("LANG", "C.UTF-8")
            .current_dir(self.dir.path())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        (out.status.code().unwrap_or(-1), text)
    }

    /// Every file and dir under the scratch dir but the stand-ins' log.
    fn tree(&self) -> Vec<Entry> {
        let top = self.dir.path();
        let mut all = vec![];
        let mut stack = vec![top.to_path_buf()];
        while let Some(p) = stack.pop() {
            for e in std::fs::read_dir(&p).unwrap() {
                let e = e.unwrap();
                let rel = e.path().strip_prefix(top).unwrap().display().to_string();
                if rel.starts_with("fake") {
                    continue;
                }
                let m = std::fs::symlink_metadata(e.path()).unwrap();
                let bytes = m.is_file().then(|| std::fs::read(e.path()).unwrap());
                if m.is_dir() {
                    stack.push(e.path());
                }
                all.push((rel, m.mode() & 0o7777, m.mtime(), m.mtime_nsec(), bytes));
            }
        }
        all.sort();
        all
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.at("fake/calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

/// A rig, unless this machine can't run the script: as root, or without cgroup v2 (its preflight's FAIL).
macro_rules! rig {
    () => {{
        if root() {
            eprintln!("skipped: the script refuses root");
            return;
        }
        let r = Rig::new();
        let (_, out) = r.run(&["--dry-run"]);
        if out.contains("FAIL  cgroup v2") {
            eprintln!("skipped: this machine has no cgroup v2");
            return;
        }
        r
    }};
}

#[test]
fn a_second_daemon_needs_its_own_state_dir_and_socket_and_a_bad_option_is_a_usage_error() {
    let r = rig!();
    let (code, out) = r.run(&["--unit", "theseus-scratch"]);
    assert_eq!(code, 2, "{out}");
    assert!(
        out.contains("give it its own --state-dir and --socket"),
        "{out}"
    );
    let (code, out) = r.run(&["--unit", "a/b", "--state-dir", "s", "--socket", "k"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("a unit's name is letters"), "{out}");
    let (code, out) = r.run(&["--no-such-option"]);
    assert_eq!(code, 2, "{out}");
    assert!(!r.at("prefix").exists() && !r.config().exists());
}

/// A dry run prints every step and changes nothing: no file is written, and the stand-ins were only asked to read.
#[test]
fn a_dry_run_prints_every_step_and_changes_nothing() {
    let r = rig!();
    std::fs::remove_file(r.at("fake/calls.log")).ok();
    let before = r.tree();
    let token = r.at("home/op-token");
    std::fs::write(&token, "SENTINEL-not-a-token\n").unwrap();
    std::fs::set_permissions(&token, PermissionsExt::from_mode(0o600)).unwrap();
    let before_token = r.tree();
    let (code, out) = r.run(&["--dry-run", "--op-token-file", token.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "dry run: every command that would change something is printed with a leading +",
        "== 1. preflight",
        "ok    token file: ",
        "== 3. install",
        "+ install -m 0755 ",
        "== 4. config",
        "+ install -d -m 0750 ",
        "== 5. check",
        "== 6. service: theseusd.service",
        "+ systemctl --user enable --now theseusd.service",
        "== 7. health",
        "(dry run) Nothing was changed.",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    assert!(!out.contains("SENTINEL"), "{out}");
    assert_eq!(r.tree(), before_token, "a dry run changed a file");
    assert_ne!(before, before_token);
    for c in r.calls() {
        assert!(
            c == "systemctl --user is-system-running" || c.starts_with("loginctl show-user "),
            "a dry run asked a stand-in to {c:?}"
        );
    }
}

/// A first run installs the binaries and writes the config from the template: its directory 0750, the file 0640,
/// every secret an op:// placeholder, the projects dir the operator's, and a header; then it stops for the operator
/// (exit 3). A second run changes nothing and says so. Once the operator's secrets are in, the check runs and passes,
/// and the run says which keys differ from the template, never a value.
#[test]
fn a_first_run_writes_the_config_a_second_changes_nothing_and_the_check_follows_the_edit() {
    let r = rig!();
    let (code, out) = r.run(&[]);
    assert_eq!(code, 3, "{out}");
    for b in ["theseusd", "theseus", "theseus-tui", "theseus-index"] {
        assert_eq!(
            std::fs::read(r.at("prefix").join(b)).unwrap(),
            std::fs::read(r.at("target/release").join(b)).unwrap(),
            "{b}"
        );
    }
    // theseus-sim is built for the size check and the benches, never installed (theseus-rwpbg.5).
    assert!(!r.at("prefix/theseus-sim").exists());
    let cfg = r.config();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().mode() & 0o777;
    assert_eq!(mode(&cfg), 0o640);
    assert_eq!(mode(cfg.parent().unwrap()), 0o750);
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        text.starts_with("# Theseus's config on this machine, written by scripts/setup.sh"),
        "{text}"
    );
    assert!(text.contains("\nprojects_dir = \"~/projects\"\n"), "{text}");
    let secrets: Vec<&str> = text
        .split("\n[secrets]\n")
        .nth(1)
        .unwrap()
        .lines()
        .take_while(|l| !l.starts_with('['))
        .filter(|l| l.contains('='))
        .collect();
    assert!(secrets.len() >= 8, "{text}");
    for l in &secrets {
        let v = l.split_once('=').unwrap().1.trim();
        assert!(
            v.starts_with("\"op://<"),
            "a secret that is not a placeholder: {l}"
        );
    }
    for want in [
        "installed 4 (0 already matched)",
        "wrote ",
        "each an op:// placeholder",
        "Stopped before the check and the service: your turn.",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    // It loads, as the daemon will read it.
    let loads = Command::new(env!("CARGO_BIN_EXE_theseusd"))
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .arg("--config")
        .arg(&cfg)
        .args(["config", "--sparse"])
        .output()
        .unwrap();
    assert!(
        loads.status.success(),
        "{}",
        String::from_utf8_lossy(&loads.stderr)
    );

    // The second run: nothing changes, and each step says so.
    let after_first = r.tree();
    let (code, again) = r.run(&[]);
    assert_eq!(code, 3, "{again}");
    assert_eq!(r.tree(), after_first, "a second run changed a file");
    for want in [
        "all 4 match the build: nothing to install",
        "it exists, and says what the template says: nothing to do",
    ] {
        assert!(again.contains(want), "{want:?} missing from:\n{again}");
    }

    let edited = operators_edit(&r, &text);
    let (code, out) = r.run(&[]);
    assert_eq!(code, 3, "{out}");
    for want in [
        "it exists: left as it is. Where it differs from the template (keys only; values are never shown):",
        "another value:         [secrets] anthropic_api_key",
        "only yours:            [discord] enabled",
        "only the template's:  [secrets] github_token",
        "ok    the config loads, and every secret resolves",
        "Stopped before the service: the unit reads the vault with a token file it names",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    assert!(
        !out.contains(&r.at("home/key").display().to_string()),
        "a value was shown:\n{out}"
    );
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), edited);
}

/// The operator's edit of the config the script wrote (`text`): a stand-in key from a private file in every
/// secret's place, no vault reference left, no GitHub token (`check` would ask GitHub about one), and Discord, the
/// web UI, and the index tender off. Written, and returned.
fn operators_edit(r: &Rig, text: &str) -> String {
    let key = r.at("home/key");
    std::fs::write(&key, "stand-in-not-a-key").unwrap();
    std::fs::set_permissions(&key, PermissionsExt::from_mode(0o600)).unwrap();
    let edited: String = text
        .lines()
        .filter(|l| !l.starts_with("github_token = "))
        .map(|l| match l.split_once(" = \"op://") {
            Some((name, _)) => format!("{name} = \"file:{}\"\n", key.display()),
            None => format!("{l}\n"),
        })
        .collect::<String>()
        .replace(
            "\n[tools]\n",
            "\n[discord]\nenabled = false\n\n[web]\nenabled = false\n\n[index]\nenabled = false\n\n[tools]\n",
        );
    std::fs::write(r.config(), &edited).unwrap();
    edited
}

/// The `[secrets]` entry `name` of the config the script wrote, as written.
fn secret_line(cfg: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(cfg).unwrap();
    let secrets = text.split("\n[secrets]\n").nth(1)?;
    secrets
        .lines()
        .take_while(|l| !l.starts_with('['))
        .find(|l| l.starts_with(&format!("{name} = ")))
        .map(String::from)
}

/// With no terminal, or under `--yes`, the Jev key's question is never asked (theseus-is8t): the config keeps the
/// template's placeholder, the judge stays off until it is filled in, and the run says so.
#[test]
fn a_run_with_no_terminal_or_under_yes_never_asks_for_the_jev_key() {
    for more in [&[][..], &["--yes"][..]] {
        let r = rig!();
        let (code, out) = r.run(more);
        assert_eq!(code, 3, "{out}");
        assert!(out.contains("Jev: not asked"), "{out}");
        assert!(!out.contains("Jev key reference"), "asked:\n{out}");
        let line = secret_line(&r.config(), "jev_api_key").unwrap();
        assert!(line.contains("op://<"), "the placeholder stays: {line}");
    }
}

/// `--jev-key` answers the question in advance: an `op://` reference or `env:NAME` is written as given, `skip`
/// removes the entry, and a value (anything else) is refused before anything is written.
#[test]
fn jev_key_names_a_reference_or_skips_and_never_takes_a_value() {
    for (given, want) in [
        (
            "op://Work/Jev/key",
            Some("jev_api_key = \"op://Work/Jev/key\""),
        ),
        ("env:JEV_KEY", Some("jev_api_key = \"env:JEV_KEY\"")),
        ("skip", None),
    ] {
        let r = rig!();
        let (code, out) = r.run(&["--jev-key", given]);
        assert_eq!(code, 3, "{given}: {out}");
        assert_eq!(
            secret_line(&r.config(), "jev_api_key").as_deref(),
            want,
            "{given}"
        );
        assert!(
            out.contains(if want.is_some() {
                "Jev: the key is"
            } else {
                "Jev: skipped"
            }),
            "{given}: {out}"
        );
        // The rest of the config is as before: the other placeholders stay.
        assert!(secret_line(&r.config(), "anthropic_api_key")
            .unwrap()
            .contains("op://<"));
        // It loads, as the daemon will read it.
        let loads = Command::new(env!("CARGO_BIN_EXE_theseusd"))
            .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .arg("--config")
            .arg(r.config())
            .args(["config", "--sparse"])
            .output()
            .unwrap();
        assert!(
            loads.status.success(),
            "{given}: {}",
            String::from_utf8_lossy(&loads.stderr)
        );
    }
    let r = rig!();
    let key = "sk-ant-api03-0123456789abcdefghijklmnopqrstuvwxyz";
    let (code, out) = r.run(&["--jev-key", key]);
    assert_eq!(code, 2, "{out}");
    assert!(
        !out.contains(key) || out.contains("never the key itself"),
        "{out}"
    );
    assert!(!r.config().exists(), "a refused answer wrote nothing");
}

/// On a terminal the question is asked once: a reference is written, an empty answer skips it (the entry goes,
/// so the run is not stopped for it, and the judge stays off), and a typed key is refused, asked for again up to three times, and then taken as a skip.
#[test]
fn on_a_terminal_the_jev_key_is_asked_once_and_skipping_leaves_the_judge_off() {
    let have_python = Command::new(python3()).arg("-V").output().is_ok();
    if !have_python {
        eprintln!("skipped: no python3 to drive a terminal");
        return;
    }
    let r = rig!();
    let (code, out) = r.run_on_a_terminal(&[], "env:JEV_KEY");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("Jev key reference (op://"), "{out}");
    assert_eq!(out.matches("Jev key reference (op://").count(), 1, "{out}");
    assert_eq!(
        secret_line(&r.config(), "jev_api_key").as_deref(),
        Some("jev_api_key = \"env:JEV_KEY\"")
    );
    let r = rig!();
    let (code, out) = r.run_on_a_terminal(&[], "");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("Jev: skipped. The judge stays off"), "{out}");
    assert_eq!(secret_line(&r.config(), "jev_api_key"), None);
    // Under --yes, even on a terminal, it is never asked.
    let r = rig!();
    let (code, out) = r.run_on_a_terminal(&["--yes"], "env:JEV_KEY");
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("Jev: not asked") && !out.contains("Jev key reference"),
        "{out}"
    );
    assert!(secret_line(&r.config(), "jev_api_key")
        .unwrap()
        .contains("op://<"));
    // A typed key is refused and never reaches the config.
    let r = rig!();
    let key = "sk-ant-api03-0123456789abcdefghijklmnopqrstuvwxyz";
    let (_, out) = r.run_on_a_terminal(&[], key);
    assert!(out.contains("not a reference"), "{out}");
    assert_eq!(out.matches("Jev key reference (op://").count(), 3, "{out}");
    let cfg = std::fs::read_to_string(r.config()).unwrap();
    assert!(!cfg.contains(key), "the typed key reached the config");
    assert_eq!(secret_line(&r.config(), "jev_api_key"), None, "{out}");
}
