//! `scripts/user-service.sh` against stand-in tools (theseus-w1nf): the real script and the real
//! `theseusd install --user`, in a scratch `HOME`, with `systemctl`, `loginctl`, `journalctl`, `theseus`, and
//! `op` replaced by one stand-in script that logs each call and keeps its state in files. `grep`, `cat`, and
//! `stat` are stand-ins that read the test's files where the script names the machine's (the kernel's
//! release, `/etc/wsl.conf`), and are the real tools otherwise. Nothing here reaches the
//! machine's systemd, its journal, its accounts, or any daemon.
//!
//! The config tests swap `theseusd` for a stand-in whose plan names its config the way a build with a file
//! as its built-in default does (theseus-8d1b), because the real one's default is a vault reference until that
//! lane joins: `check` reads the config from the plan's `config:` line, so it has to hold on either build.
//!
//! The script refuses root, so these tests are skipped as root.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// What every stand-in does. State lives in files under `$FAKE`: `system-running`, `active`, `enabled`,
/// `linger`, `mainpid`, `daemon-up` (a daemon answers), `osrelease`, `wsl.conf`.
const STANDIN: &str = r#"#!/bin/bash
F=${FAKE:?}
name=${0##*/}
{ printf '%s' "$name"; printf ' %s' "$@"; echo; } >>"$F/calls.log"
state() { if [ -f "$F/$1" ]; then printf '%s' "$(<"$F/$1")"; else printf '%s' "$2"; fi; }
real() { for d in /usr/bin /bin; do [ -x "$d/$name" ] && exec "$d/$name" "$@"; done; exit 127; }
remap() {
  local a
  for a in "$@"; do
    case $a in
    /proc/sys/kernel/osrelease) printf '%s\0' "$F/osrelease" ;;
    /etc/wsl.conf) printf '%s\0' "$F/wsl.conf" ;;
    *) printf '%s\0' "$a" ;;
    esac
  done
}
case $name in
grep | cat)
  mapfile -d '' -t args < <(remap "$@")
  real "${args[@]}"
  ;;
stat)
  real "$@"
  ;;
systemctl)
  [ "$1" = --user ] && shift
  verb=$1
  shift
  case $verb in
  is-system-running) s=$(state system-running running); echo "$s"; [ "$s" = running ] || [ "$s" = degraded ] ;;
  is-active) s=$(state active inactive); echo "$s"; [ "$s" = active ] ;;
  is-enabled)
    if [ -f "$F/enabled" ]; then state enabled enabled; echo; else echo "Failed to get unit file state for theseusd.service: No such file or directory" >&2; exit 1; fi
    ;;
  show)
    case "$*" in
    *MainPID*) state mainpid 0; echo ;;
    *ExecStart*)
      line=$(/usr/bin/grep '^ExecStart=' "$HOME/.config/systemd/user/theseusd.service" 2>/dev/null | /usr/bin/head -n 1)
      [ -n "$line" ] && echo "{ path=x ; argv[]=${line#ExecStart=} ; ignore_errors=no ; start_time=[n/a] }"
      ;;
    esac
    ;;
  daemon-reload) ;;
  enable) : >"$F/daemon-up"; echo active >"$F/active"; echo enabled >"$F/enabled" ;;
  disable) rm -f "$F/daemon-up" "$F/enabled"; echo inactive >"$F/active" ;;
  start | restart)
    if [ -f "$F/start-fails" ]; then echo activating >"$F/active"; else : >"$F/daemon-up"; echo active >"$F/active"; fi
    ;;
  stop) rm -f "$F/daemon-up"; echo inactive >"$F/active" ;;
  status) echo "theseusd.service - Theseus daemon (a stand-in's status)" ;;
  esac
  ;;
loginctl)
  case $1 in
  show-user) state linger yes; echo ;;
  enable-linger) echo yes >"$F/linger" ;;
  esac
  ;;
journalctl) echo "(the journal)" ;;
theseus)
  [ "$1" = --socket ] && shift 2
  case $1 in
  health)
    if [ -e "$F/daemon-up" ]; then
      echo "theseusd 0.0.1 · protocol 1 · up 3s"
      echo "telemetry: ok"
      echo "kernel: accepting"
    else
      echo "cannot connect" >&2
      exit 1
    fi
    ;;
  shutdown) rm -f "$F/daemon-up" ;;
  esac
  ;;
op) ;;
esac
"#;

/// A `theseusd` whose plan names its config the way a build with a file as its default does: `THESEUS_CONFIG`,
/// else `$FAKE/default-config` when that file exists (a build whose default is a vault reference), else
/// `~/.theseus/theseus.toml`. `$FAKE/plan-says` replaces the whole plan, with exit status 2. It logs each call,
/// answers `--version`, and refuses what a read-only check never asks of it.
const PLAN_STANDIN: &str = r#"#!/bin/bash
F=${FAKE:?}
{ printf 'theseusd'; printf ' %s' "$@"; echo; } >>"$F/calls.log"
case " $* " in
*" --version "*)
  echo "theseusd 0.0.1"
  exit 0
  ;;
*" --apply "* | *" --check "* | *" --remove "*) exit 1 ;;
*" install --user "*)
  if [ -f "$F/plan-says" ]; then
    printf '%s\n' "$(<"$F/plan-says")"
    exit 2
  fi
  if [ -f "$F/default-config" ]; then def=$(<"$F/default-config"); else def=$HOME/.theseus/theseus.toml; fi
  echo "theseusd install --user: the plan. Nothing is changed: --apply performs it."
  echo "  operator:  ada (uid 1000)"
  echo "  binary:    $0"
  echo "  config:    ${THESEUS_CONFIG:-$def}"
  exit 0
  ;;
esac
exit 1
"#;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/user-service.sh")
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

/// This process's uid.
fn my_uid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|l| l.split_whitespace().next().map(|u| u.parse().unwrap()))
        .unwrap()
}

/// A scratch `HOME` holding the stand-ins (`bin/`), their state (`fake/`), a token file, and a machine that
/// is ready: systemd running, linger on, not WSL, memory and pids delegated, nothing answering.
struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(home.join("fake")).unwrap();
        std::fs::write(home.join("standin"), STANDIN).unwrap();
        std::fs::set_permissions(home.join("standin"), PermissionsExt::from_mode(0o755)).unwrap();
        for tool in [
            "systemctl",
            "loginctl",
            "journalctl",
            "theseus",
            "op",
            "grep",
            "cat",
            "stat",
        ] {
            std::os::unix::fs::symlink(home.join("standin"), home.join("bin").join(tool)).unwrap();
        }
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_theseusd"), home.join("bin/theseusd"))
            .unwrap();
        let rig = Self { dir };
        rig.set("osrelease", "6.1.0-generic\n");
        rig.token(0o600);
        rig
    }

    fn home(&self) -> &Path {
        self.dir.path()
    }

    fn set(&self, state: &str, text: &str) {
        std::fs::write(self.home().join("fake").join(state), text).unwrap();
    }

    fn token_path(&self) -> PathBuf {
        self.home().join("op-token")
    }

    /// The token file, with this mode and a sentinel text that no output may carry.
    fn token(&self, mode: u32) {
        std::fs::write(self.token_path(), "SENTINEL-not-a-token-DO-NOT-PRINT\n").unwrap();
        std::fs::set_permissions(self.token_path(), PermissionsExt::from_mode(mode)).unwrap();
    }

    fn unit(&self) -> PathBuf {
        self.home().join(".config/systemd/user/theseusd.service")
    }

    fn socket(&self) -> PathBuf {
        self.home().join("s.sock")
    }

    /// A `stat` that answers `-c` with these fields (`type|uid|mode|size`) whatever the file is, and the real
    /// one for anything else: the script's view of a token file this process cannot make (another owner's).
    fn stat_says(&self, fields: &str) {
        let at = self.home().join("bin/stat");
        std::fs::remove_file(&at).unwrap(); // a link to the shared stand-in: never write through it
        std::fs::write(
            &at,
            format!("#!/bin/bash\nif [ \"$1\" = -c ]; then echo '{fields}'; exit 0; fi\nexec /usr/bin/stat \"$@\"\n"),
        )
        .unwrap();
        std::fs::set_permissions(&at, PermissionsExt::from_mode(0o755)).unwrap();
    }

    /// A `theseusd` that is the plan stand-in: a build whose built-in config is a file this machine does not
    /// have, until a test sets `default-config` or `plan-says`.
    fn plan_build(&self) {
        let at = self.home().join("bin/theseusd");
        std::fs::remove_file(&at).unwrap(); // a link to the real binary: never write through it
        std::fs::write(&at, PLAN_STANDIN).unwrap();
        std::fs::set_permissions(&at, PermissionsExt::from_mode(0o755)).unwrap();
    }

    /// `check` in a shell that has not exported `THESEUS_CONFIG`.
    fn check_without_the_variable(&self) -> (i32, String) {
        let mut c = self.command(&["check"]);
        c.env_remove("THESEUS_CONFIG");
        Self::said(&c.output().unwrap())
    }

    /// The script, with the variables a shell that starts the daemon has.
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new("bash");
        c.arg(script())
            .args(args)
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.home().join("bin").display()),
            )
            .env("HOME", self.home())
            .env("FAKE", self.home().join("fake"))
            .env("XDG_RUNTIME_DIR", "/run/user/1")
            .env("LANG", "C.UTF-8")
            .env("THESEUS_CONFIG", "op://Example/theseus-config/notesPlain")
            .env("THESEUS_SOCKET", self.socket())
            .env("THESEUS_OP_TOKEN_FILE", self.token_path())
            .current_dir(self.home())
            .stdin(Stdio::null());
        c
    }

    /// The exit status and everything the script printed.
    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = self.command(args).output().unwrap();
        Self::said(&out)
    }

    fn said(out: &Output) -> (i32, String) {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !text.contains("SENTINEL"),
            "the script printed the token file's text:\n{text}"
        );
        (out.status.code().unwrap_or(-1), text)
    }

    /// Every stand-in call so far, one line each.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.home().join("fake/calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// The position of the first call that starts with `prefix`.
    fn at(&self, prefix: &str) -> Option<usize> {
        self.calls().iter().position(|c| c.starts_with(prefix))
    }
}

macro_rules! rig {
    () => {{
        if root() {
            eprintln!("skipped: the script refuses root");
            return;
        }
        Rig::new()
    }};
}

#[test]
fn check_passes_on_a_machine_that_is_ready() {
    let r = rig!();
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "ok    systemd: your user manager is running",
        "ok    linger: on for",
        "ok    theseus: ",
        "ok    op: ",
        &format!(
            "ok    token file: {} (yours, mode 0600, not empty)",
            r.token_path().display()
        ),
        "ok    config: op://Example/theseus-config/notesPlain (a vault note",
        "info  unit: not installed",
        "info  daemon: none is running",
        "result: ready (0 warning(s))",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    // Read-only: nothing was started, stopped, enabled, or written.
    assert_nothing_changed(&r);
}

/// What neither `check` nor a stopped `install` may have run: nothing started, stopped, enabled, or written, and
/// no daemon asked to shut down.
fn assert_nothing_changed(r: &Rig) {
    for mutating in [
        "systemctl --user daemon-reload",
        "systemctl --user enable",
        "systemctl --user disable",
        "systemctl --user start",
        "systemctl --user stop",
        "systemctl --user restart",
        "loginctl enable-linger",
    ] {
        assert!(
            r.at(mutating).is_none(),
            "ran `{mutating}`:\n{:?}",
            r.calls()
        );
    }
    assert!(
        !r.calls().iter().any(|c| c.ends_with(" shutdown")),
        "{:?}",
        r.calls()
    );
    assert!(
        !r.calls().iter().any(|c| c.contains(" --apply")),
        "{:?}",
        r.calls()
    );
    assert!(!r.unit().exists());
}

/// A fault in a ready machine: what it is, how the test makes it, and what `check` must say.
type Fault<'a> = (&'a str, fn(&Rig), &'a [&'a str]);

fn faults<'a>() -> [Fault<'a>; 12] {
    [
        (
            "no user manager",
            |r| r.set("system-running", "offline\n"),
            &[
                "FAIL  systemd: no user manager answers",
                "on WSL: put systemd=true under [boot]",
            ],
        ),
        (
            "no token named",
            |_| {},
            &["FAIL  token file: none named", "then add: --op-token-file"],
        ),
        (
            "token mode 0644",
            |r| r.token(0o644),
            &[
                "FAIL  token file: ",
                "mode 0644 is looser than 0600: chmod 600 ",
            ],
        ),
        (
            "token mode 0200",
            |r| r.token(0o200),
            &["mode 0200 does not let you read it: chmod 600 "],
        ),
        (
            "token missing",
            |r| std::fs::remove_file(r.token_path()).unwrap(),
            &["does not exist", "make it with mode 0600"],
        ),
        (
            "token a directory",
            |r| {
                std::fs::remove_file(r.token_path()).unwrap();
                std::fs::create_dir(r.token_path()).unwrap();
            },
            &["is not a regular file (directory): name the file itself"],
        ),
        (
            "token empty",
            |r| std::fs::write(r.token_path(), "").unwrap(),
            &["it is empty: put the 1Password"],
        ),
        (
            "token another user's",
            |r| r.stat_says("regular file|0|600|40"),
            &["owned by uid 0, not by you (", "sudo chown "],
        ),
        (
            "no theseusd",
            |r| std::fs::remove_file(r.home().join("bin/theseusd")).unwrap(),
            &["FAIL  theseusd: not on PATH"],
        ),
        (
            "no op",
            |r| std::fs::remove_file(r.home().join("bin/op")).unwrap(),
            &["FAIL  op: the 1Password CLI is not on PATH"],
        ),
        (
            "config not a vault reference",
            |_| {},
            // The plan makes a relative path absolute, and the check reads it from the plan.
            &[
                "FAIL  config: ",
                "/not-a-ref is not a readable file (THESEUS_CONFIG names it)",
                "export THESEUS_CONFIG=op://<vault>/<item>/notesPlain",
            ],
        ),
        (
            "config reference malformed",
            |_| {},
            &["FAIL  config: op://only-a-vault is not op://<vault>/<item>/<field>"],
        ),
    ]
}

#[test]
fn check_fails_each_way_a_machine_can_be_unready_and_says_how_to_mend_it() {
    for (what, setup, says) in faults() {
        // A machine that has a real `op` in a system directory cannot show this fault.
        if what == "no op"
            && ["/usr/bin/op", "/bin/op"]
                .iter()
                .any(|p| Path::new(p).exists())
        {
            continue;
        }
        let r = rig!();
        setup(&r);
        let mut c = r.command(&["check"]);
        match what {
            "no token named" => {
                c.env_remove("THESEUS_OP_TOKEN_FILE");
            }
            "config not a vault reference" => {
                c.env("THESEUS_CONFIG", "not-a-ref");
            }
            "config reference malformed" => {
                c.env("THESEUS_CONFIG", "op://only-a-vault");
            }
            _ => {}
        }
        let (code, out) = Rig::said(&c.output().unwrap());
        assert_eq!(code, 1, "{what}: a FAIL is exit 1:\n{out}");
        assert!(out.contains("result: not ready"), "{what}:\n{out}");
        for want in says {
            assert!(out.contains(want), "{what}: {want:?} missing from:\n{out}");
        }
    }
}

/// What `check` says of a config that comes from the build's own default.
const BUILT_IN: &str = "THESEUS_CONFIG is not set here, so this is theseusd's built-in default";

#[test]
fn check_fails_when_the_plans_config_is_a_file_that_is_not_there_whatever_the_variable_says() {
    let r = rig!();
    r.plan_build();
    let file = r.home().join(".theseus/theseus.toml");
    // No variable, and the build's default is a file this machine does not have: the variable alone says nothing,
    // the plan's `config:` line does.
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 1, "{out}");
    for want in [
        format!(
            "FAIL  config: {} is not a readable file ({BUILT_IN})",
            file.display()
        ),
        "the unit would be written to read it, and the daemon would not start.".into(),
        "export THESEUS_CONFIG=op://<vault>/<item>/notesPlain".into(),
        "or write the file.".into(),
        "result: not ready".into(),
    ] {
        assert!(out.contains(&want), "{want:?} missing from:\n{out}");
    }
    // The plan was asked, with the token flag before the subcommand, as install asks it.
    let asked = format!(
        "theseusd --op-token-file {} install --user",
        r.token_path().display()
    );
    assert!(r.at(&asked).is_some(), "{:?}", r.calls());
    // A variable that names a file that is not there fails the same way, and says it named it.
    let other = r.home().join("elsewhere.toml");
    let mut c = r.command(&["check"]);
    c.env("THESEUS_CONFIG", &other);
    let (code, out) = Rig::said(&c.output().unwrap());
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(&format!(
            "FAIL  config: {} is not a readable file (THESEUS_CONFIG names it)",
            other.display()
        )),
        "{out}"
    );
    // A file that is there is a config.
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "# a config\n").unwrap();
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!(
            "ok    config: {} (a file; {BUILT_IN})",
            file.display()
        )),
        "{out}"
    );
    // A file that cannot be read is not (unless this process reads it anyway), and nor is a directory.
    std::fs::set_permissions(&file, PermissionsExt::from_mode(0o000)).unwrap();
    if std::fs::read(&file).is_err() {
        let (code, out) = r.check_without_the_variable();
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("is not a readable file"), "{out}");
    }
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("is not a readable file"), "{out}");
    assert_nothing_changed(&r);
}

#[test]
fn check_passes_when_the_plans_config_is_a_vault_reference_and_looks_for_no_file() {
    let r = rig!();
    r.plan_build();
    // Named by the variable, as the rig's shell has it: the plan's line is the reference as it is.
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(
            "ok    config: op://Example/theseus-config/notesPlain (a vault note: the daemon reads it with the token)"
        ),
        "{out}"
    );
    // The build's own default is a vault reference (every build before theseus-8d1b's), no variable names one:
    // it passes, and says whose it is.
    r.set("default-config", "op://Example/built-in/notesPlain\n");
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!(
            "ok    config: op://Example/built-in/notesPlain (a vault note: the daemon reads it with the token; {BUILT_IN})"
        )),
        "{out}"
    );
    assert!(!r.home().join(".theseus").exists(), "no file was needed");
    // The reference still has to have a vault, an item, and a field, wherever it came from.
    r.set("default-config", "op://only-a-vault\n");
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("FAIL  config: op://only-a-vault is not op://<vault>/<item>/<field>"),
        "{out}"
    );
}

#[test]
fn check_fails_when_the_plan_names_no_config_and_says_what_theseusd_said() {
    let r = rig!();
    r.plan_build();
    r.set("plan-says", "error: unrecognized subcommand 'install'\n");
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(
            "FAIL  config: the plan names none (theseusd install --user said: error: unrecognized subcommand 'install')"
        ),
        "{out}"
    );
}

#[test]
fn install_stops_before_its_first_question_when_the_config_is_a_file_that_is_not_there() {
    let r = rig!();
    r.plan_build();
    // A daemon started by hand answers: an install that went on would stop it. --yes answers every question it
    // is asked, so any question that was reached would have been answered.
    r.set("daemon-up", "");
    let mut c = r.command(&["install", "--yes"]);
    c.env_remove("THESEUS_CONFIG");
    let (code, out) = Rig::said(&c.output().unwrap());
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL  config: "), "{out}");
    assert!(
        out.contains("Not installing: fix the FAIL lines above, then run this again."),
        "{out}"
    );
    assert!(!out.contains("[y/N]"), "a question was asked:\n{out}");
    assert!(!out.contains("== the plan"), "{out}");
    // The daemon started by hand still answers, and nothing was written or run.
    assert!(r.home().join("fake/daemon-up").exists());
    assert_nothing_changed(&r);
}

#[test]
fn check_warns_without_failing_for_linger_off() {
    let r = rig!();
    r.set("linger", "no\n");
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("WARN  linger: off for"), "{out}");
    assert!(out.contains("turn it on: loginctl enable-linger"), "{out}");
    assert!(out.contains("result: ready (1 warning(s))"), "{out}");
    // An L1 job needs no cgroup (theseus-gyin): nothing checks them.
    assert!(!out.contains("cgroups"), "{out}");
}

#[test]
fn check_on_wsl_reads_wsl_conf_with_or_without_spaces() {
    let r = rig!();
    r.set("osrelease", "6.18.40.1-microsoft-standard-WSL2\n");
    for conf in [
        "[boot]\nsystemd=true\n",
        "[boot]\nsystemd = true\n",
        "[boot]\n  Systemd = True\n",
    ] {
        r.set("wsl.conf", conf);
        let (code, out) = r.run(&["check"]);
        assert_eq!(code, 0, "{conf:?}:\n{out}");
        assert!(
            out.contains("ok    WSL: /etc/wsl.conf has systemd=true"),
            "{conf:?}:\n{out}"
        );
    }
    // No setting, and no user manager: the fix is named.
    r.set("wsl.conf", "[network]\ngenerateHosts = false\n");
    r.set("system-running", "offline\n");
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("FAIL  WSL: /etc/wsl.conf has no systemd=true"),
        "{out}"
    );
    assert!(out.contains("wsl --shutdown"), "{out}");
    // Systemd runs by another setting: said, not failed.
    r.set("system-running", "running\n");
    let (code, out) = r.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("info  WSL: /etc/wsl.conf has no systemd=true, yet systemd runs"),
        "{out}"
    );
}

#[test]
fn a_dry_run_prints_every_command_and_runs_none_of_them() {
    let r = rig!();
    let cases: [(&[&str], &[&str]); 8] = [
        (
            &["--dry-run", "check"],
            &[
                "+ systemctl --user is-system-running",
                "+ loginctl show-user ",
                "+ grep -qi microsoft /proc/sys/kernel/osrelease",
                "+ command -v theseusd",
                "+ theseusd --version",
                "+ stat -c '%F|%u|%a|%s' -- ",
                "+ timeout 20 theseusd --op-token-file ",
                " install --user\n",
                "+ systemctl --user is-active theseusd.service",
                "+ timeout 5 theseus --socket ",
            ],
        ),
        (
            &["--dry-run", "install"],
            &[
                "+ theseusd --op-token-file ",
                " install --user",
                " install --user --check",
                " install --user --apply",
                "+ systemctl --user daemon-reload",
                "+ loginctl enable-linger ",
                " shutdown",
                "+ systemctl --user enable --now theseusd.service",
                "(dry run: assumed yes)",
            ],
        ),
        (
            &["--dry-run", "uninstall"],
            &[
                "+ theseusd install --user --remove",
                "+ systemctl --user disable --now theseusd.service",
                "+ theseusd install --user --remove --apply",
                "+ systemctl --user daemon-reload",
            ],
        ),
        (
            &["--dry-run", "logs"],
            &["+ journalctl --user -u theseusd.service -f"],
        ),
        (
            &["--dry-run", "logs", "-n", "50"],
            &["+ journalctl --user -u theseusd.service -f -n 50"],
        ),
        (
            &["--dry-run", "status"],
            &["+ systemctl --user status theseusd.service --no-pager"],
        ),
        (
            &["--dry-run", "restart"],
            &["+ systemctl --user restart theseusd.service"],
        ),
        (
            &["--dry-run", "stop"],
            &["+ systemctl --user stop theseusd.service"],
        ),
    ];
    for (args, says) in cases {
        let (code, out) = r.run(args);
        assert_eq!(code, 0, "{args:?}:\n{out}");
        assert!(
            out.starts_with("dry run: every command is printed with a leading +, and none is run"),
            "{args:?}:\n{out}"
        );
        // Nothing ran, so nothing was found wrong: the checks that need an answer say nothing in a dry run.
        assert!(
            !out.contains("FAIL  "),
            "{args:?}: a dry run found a fault:\n{out}"
        );
        for want in says {
            assert!(
                out.contains(want),
                "{args:?}: {want:?} missing from:\n{out}"
            );
        }
    }
    assert_eq!(r.calls(), Vec::<String>::new(), "a dry run ran a command");
    assert!(!r.unit().exists(), "a dry run wrote the unit");
}

/// The state a successful `install --yes` leaves, from a machine that is ready.
fn installed(r: &Rig) -> (i32, String) {
    r.run(&["install", "--yes"])
}

#[test]
fn install_walks_the_steps_in_order_and_ends_with_the_cheat_sheet() {
    let r = rig!();
    let (code, out) = installed(&r);
    assert_eq!(code, 0, "{out}");
    let unit = std::fs::read_to_string(r.unit()).unwrap();
    assert!(
        unit.contains(&format!(" --op-token-file {}\n", r.token_path().display())),
        "{unit}"
    );
    // No delegation and no stop hook (theseus-gyin); a stop signals the daemon
    // alone, so its jobs run on.
    assert!(
        unit.contains("\nKillSignal=SIGINT\n") && unit.contains("\nKillMode=process\n"),
        "{unit}"
    );
    assert!(
        !unit.contains("Delegate=") && !unit.contains("ExecStopPost="),
        "{unit}"
    );
    for want in [
        "== check: is this machine ready?",
        "== the plan: what theseusd would write (nothing is changed yet)",
        "ok      token  ",
        "? Write ",
        " as the plan shows? [y/N] y (--yes)",
        "+ systemctl --user daemon-reload",
        "+ systemctl --user enable --now theseusd.service",
        "It answers. What the daemon says of itself",
        "theseusd 0.0.1 · protocol 1 · up 3s",
        "theseusd now runs as a systemd user service",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    // The order of the commands that change something.
    let reload = r
        .at("systemctl --user daemon-reload")
        .expect("daemon-reload");
    let enable = r
        .at("systemctl --user enable --now theseusd.service")
        .expect("enable");
    assert!(reload < enable, "{:?}", r.calls());
    assert!(r.at("theseus --socket").is_some());
    assert!(
        !r.calls().iter().any(|c| c.ends_with(" shutdown")),
        "a stop, with no daemon by hand"
    );
    assert!(
        r.at("loginctl enable-linger").is_none(),
        "linger was on already"
    );
    // A second install finds the unit as the plan says, and writes nothing.
    let (code, again) = installed(&r);
    assert_eq!(code, 0, "{again}");
    assert!(
        again.contains("The unit already matches the plan: nothing to write."),
        "{again}"
    );
}

/// `--unit` installs a second daemon's unit beside the first (scripts/setup.sh's scratch runs): a unit file of
/// its own, named in every command, with the shell's socket and state dir in its `ExecStart`, and the first unit
/// left alone. Without a socket and a state dir of its own, it installs nothing: on the first daemon's socket it
/// would take that daemon for one started by hand, and stop it.
#[test]
fn install_with_a_unit_name_writes_a_second_daemons_unit_and_leaves_the_first_alone() {
    let r = rig!();
    let (code, out) = r.run(&["--unit", "theseus-scratch", "install", "--yes"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("a second daemon needs a socket and a state dir of its own"),
        "{out}"
    );
    assert_eq!(
        r.calls(),
        Vec::<String>::new(),
        "a refused install ran a command"
    );
    let (code, out) = r.run(&["--unit", "../x", "check"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("a unit's name is letters"), "{out}");

    let state = r.home().join("scratch-state");
    let with_state = |args: &[&str]| {
        let mut c = r.command(args);
        c.env("THESEUS_STATE_DIR", &state);
        Rig::said(&c.output().unwrap())
    };
    let (code, out) = with_state(&["--dry-run", "--unit", "theseus-scratch.service", "install"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        " install --user --unit theseus-scratch --check",
        " install --user --unit theseus-scratch --apply",
        "+ systemctl --user enable --now theseus-scratch.service",
    ] {
        assert!(out.contains(want), "{want:?} missing from:\n{out}");
    }
    assert_eq!(r.calls(), Vec::<String>::new(), "a dry run ran a command");

    let (code, out) = with_state(&["--unit", "theseus-scratch", "install", "--yes"]);
    assert_eq!(code, 0, "{out}");
    let text = std::fs::read_to_string(
        r.home()
            .join(".config/systemd/user/theseus-scratch.service"),
    )
    .unwrap();
    assert!(
        text.contains(&format!(
            " --state-dir {} --socket {} ",
            state.display(),
            r.socket().display()
        )),
        "{text}"
    );
    assert!(
        text.contains("\nDescription=Theseus daemon (theseus-scratch)\n"),
        "{text}"
    );
    assert!(!r.unit().exists(), "the first daemon's unit was written");
    assert!(
        r.at("systemctl --user enable --now theseus-scratch.service")
            .is_some(),
        "{:?}",
        r.calls()
    );
    assert!(
        !r.calls().iter().any(|c| c.contains("theseusd.service")),
        "{:?}",
        r.calls()
    );
    // The cheat sheet's commands act on this unit.
    assert!(out.contains("--unit theseus-scratch status"), "{out}");
    // A second install writes nothing.
    let (code, again) = with_state(&["--unit", "theseus-scratch", "install", "--yes"]);
    assert_eq!(code, 0, "{again}");
    assert!(
        again.contains("The unit already matches the plan: nothing to write."),
        "{again}"
    );
}

#[test]
fn install_names_the_token_file_it_was_given_by_option_and_not_by_variable() {
    let r = rig!();
    let mut c = r.command(&[
        "--yes",
        "--op-token-file",
        r.token_path().to_str().unwrap(),
        "install",
    ]);
    c.env_remove("THESEUS_OP_TOKEN_FILE");
    let (code, out) = Rig::said(&c.output().unwrap());
    assert_eq!(code, 0, "{out}");
    let unit = std::fs::read_to_string(r.unit()).unwrap();
    assert!(
        unit.contains(&format!(" --op-token-file {}\n", r.token_path().display())),
        "{unit}"
    );
    // The plan was asked with the flag before the subcommand, which every build of theseusd reads.
    assert!(
        out.contains(&format!(
            "+ theseusd --op-token-file {} install --user",
            r.token_path().display()
        )),
        "{out}"
    );
}

#[test]
fn install_turns_linger_on_when_asked_and_stops_a_daemon_started_by_hand_first() {
    let r = rig!();
    r.set("linger", "no\n");
    r.set("daemon-up", "");
    let (code, out) = installed(&r);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("info  daemon: a daemon you started by hand answers on "),
        "{out}"
    );
    let linger = r.at("loginctl enable-linger").expect("linger on");
    let shutdown = r
        .calls()
        .iter()
        .position(|c| c.starts_with("theseus --socket") && c.ends_with(" shutdown"))
        .expect("shutdown");
    let enable = r.at("systemctl --user enable --now").expect("enable");
    assert!(linger < enable && shutdown < enable, "{:?}", r.calls());
    assert!(out.contains("It answers."), "{out}");
}

#[test]
fn install_asks_before_it_writes_and_no_leaves_the_machine_as_it_was() {
    let r = rig!();
    // No terminal and no --yes: the answer is no.
    let (code, out) = r.run(&["install"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("no: there is no terminal to ask (--yes answers yes)"),
        "{out}"
    );
    assert!(out.contains("Nothing was changed."), "{out}");
    assert!(!r.unit().exists());
    assert!(
        r.at("systemctl --user enable").is_none()
            && r.at("systemctl --user daemon-reload").is_none()
    );
}

#[test]
fn install_stops_at_a_failed_check_and_at_a_plan_that_refuses() {
    let r = rig!();
    r.token(0o644);
    let (code, out) = installed(&r);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("Not installing: fix the FAIL lines above, then run this again."),
        "{out}"
    );
    assert!(!out.contains("== the plan"), "{out}");
    assert!(!r.unit().exists());
    // The plan refuses too: the same token, with the script's check blinded by a stand-in stat that
    // says the file is fine.
    let r = rig!();
    r.token(0o644);
    r.stat_says(&format!("regular file|{}|600|40", my_uid()));
    let (code, out) = installed(&r);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("REFUSE  token"), "{out}");
    assert!(out.contains("mode 0644 is looser than 0600"), "{out}");
    assert!(
        out.contains("The plan did not finish (the lines above say why)"),
        "{out}"
    );
    assert!(!r.unit().exists());
}

#[test]
fn check_follows_the_installed_unit_for_the_token_and_the_socket() {
    let r = rig!();
    let (code, out) = installed(&r);
    assert_eq!(code, 0, "{out}");
    // A shell with none of the daemon's variables: the unit still says which token and which socket.
    let mut c = r.command(&["check"]);
    c.env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET");
    let (code, out) = Rig::said(&c.output().unwrap());
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("token file: {} (yours", r.token_path().display())),
        "{out}"
    );
    assert!(out.contains("info  unit: installed and enabled"), "{out}");
    assert!(
        out.contains("ok    daemon: the service's daemon answers on "),
        "{out}"
    );
    // THESEUS_SOCKET was set when it was installed, so the unit names it.
    assert!(out.contains(&r.socket().display().to_string()), "{out}");
}

/// A day-to-day `check` in a shell without `THESEUS_CONFIG` reads the config the installed unit names, not the
/// build's default, which a plan would name and which may be a file this machine does not have (theseus-a7gx).
#[test]
fn check_follows_the_installed_unit_for_the_config_when_the_shell_has_none() {
    let r = rig!();
    let (code, out) = installed(&r);
    assert_eq!(code, 0, "{out}");
    // The build's plan would now name a default file that is not there.
    r.plan_build();
    let (code, out) = r.check_without_the_variable();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(
            "ok    config: op://Example/theseus-config/notesPlain (a vault note: the daemon reads it with the \
             token; THESEUS_CONFIG is not set here, so this is the installed unit's config)"
        ),
        "{out}"
    );
    // The variable, when the shell has it, still decides.
    let mut c = r.command(&["check"]);
    c.env("THESEUS_CONFIG", "not-a-ref");
    let (code, out) = Rig::said(&c.output().unwrap());
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("FAIL  config: not-a-ref is not a readable file"),
        "{out}"
    );
}

#[test]
fn uninstall_removes_the_unit_and_leaves_the_store_and_the_token_alone() {
    let r = rig!();
    installed(&r);
    let store = r.home().join(".theseus/store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("marker"), "kept").unwrap();
    let (code, out) = r.run(&["uninstall", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.unit().exists(), "{out}");
    assert!(store.join("marker").exists() && r.token_path().exists());
    assert!(
        out.contains("Removed. Your state dir and store were not touched."),
        "{out}"
    );
    let disable = r
        .at("systemctl --user disable --now theseusd.service")
        .expect("disable");
    let last_reload = r
        .calls()
        .iter()
        .rposition(|c| c == "systemctl --user daemon-reload")
        .expect("reload");
    assert!(disable < last_reload, "{:?}", r.calls());
    // With nothing installed it removes nothing, and stops nothing.
    let stops = r
        .calls()
        .iter()
        .filter(|c| c.contains("disable --now"))
        .count();
    let (code, out) = r.run(&["uninstall", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("Nothing to remove: no unit that theseusd wrote is installed."),
        "{out}"
    );
    assert_eq!(
        r.calls()
            .iter()
            .filter(|c| c.contains("disable --now"))
            .count(),
        stops,
        "{:?}",
        r.calls()
    );
    // A unit someone else wrote is kept, and its service is not stopped.
    std::fs::create_dir_all(r.unit().parent().unwrap()).unwrap();
    std::fs::write(r.unit(), "[Service]\nExecStart=/bin/true\n").unwrap();
    let (code, out) = r.run(&["uninstall", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Nothing to remove"), "{out}");
    assert!(
        r.unit().exists(),
        "a unit theseusd did not write was removed"
    );
    assert_eq!(
        r.calls()
            .iter()
            .filter(|c| c.contains("disable --now"))
            .count(),
        stops,
        "{:?}",
        r.calls()
    );
}

#[test]
fn the_wrappers_say_what_they_run_and_wait_for_the_daemon_to_answer() {
    let r = rig!();
    let (code, out) = r.run(&["status"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("The unit is not installed"), "{out}");
    installed(&r);
    for (verb, run) in [
        ("restart", "+ systemctl --user restart theseusd.service"),
        ("stop", "+ systemctl --user stop theseusd.service"),
        ("start", "+ systemctl --user start theseusd.service"),
        (
            "status",
            "+ systemctl --user status theseusd.service --no-pager",
        ),
    ] {
        let (code, out) = r.run(&[verb]);
        assert_eq!(code, 0, "{verb}:\n{out}");
        assert!(out.contains(run), "{verb}: {run:?} missing from:\n{out}");
    }
    let (_, out) = r.run(&["restart"]);
    assert!(out.contains("It answers on "), "{out}");
    let (_, out) = r.run(&["stop"]);
    assert!(out.contains("Stopped. It is still enabled"), "{out}");
    let (code, out) = r.run(&["logs", "-n", "5"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("+ journalctl --user -u theseusd.service -f -n 5"),
        "{out}"
    );
    assert!(
        r.calls()
            .iter()
            .any(|c| c == "journalctl --user -u theseusd.service -f -n 5"),
        "{:?}",
        r.calls()
    );
}

#[test]
fn start_says_so_when_what_answers_is_not_the_service() {
    let r = rig!();
    installed(&r);
    // A daemon started by hand holds the socket; the unit's daemon cannot take the store and does not come up.
    r.set("start-fails", "");
    r.set("daemon-up", "");
    r.set("active", "inactive\n");
    let (code, out) = r.run(&["start"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("but the unit is activating: a daemon you started by hand may hold the socket and the store."), "{out}");
    assert!(!out.contains("It answers on"), "{out}");
}

#[test]
fn status_says_when_the_running_binary_was_replaced_on_disk() {
    let r = rig!();
    installed(&r);
    // A process whose image is gone, as a daemon is after an install renames a new binary over it.
    let image = r.home().join("old-image");
    std::fs::copy("/usr/bin/sleep", &image).unwrap();
    let mut child = Command::new(&image).arg("30").spawn().unwrap();
    std::fs::remove_file(&image).unwrap();
    r.set("mainpid", &child.id().to_string());
    let (code, out) = r.run(&["status"]);
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("note: the running daemon's binary was replaced on disk since it started."),
        "{out}"
    );
    // A daemon on the file that is still there gets no note.
    r.set("mainpid", &std::process::id().to_string());
    let (_, out) = r.run(&["status"]);
    assert!(
        !out.contains("note: the running daemon's binary was replaced"),
        "{out}"
    );
}

#[test]
fn usage_errors_exit_2_and_help_exits_0() {
    let r = rig!();
    for args in [
        &["--bogus"][..],
        &["frob"],
        &[],
        &["check", "extra"],
        &["--op-token-file"],
    ] {
        let (code, out) = r.run(args);
        assert_eq!(code, 2, "{args:?}:\n{out}");
    }
    let (code, out) = r.run(&["--help"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("uninstall   stop the service and remove its unit"),
        "{out}"
    );
    // A token path with `~` after `=` is expanded: the shell does not.
    let (_, out) = r.run(&["--dry-run", "--op-token-file=~/op-token", "check"]);
    assert!(
        out.contains(&format!("-- {}/op-token", r.home().display())),
        "{out}"
    );
}
