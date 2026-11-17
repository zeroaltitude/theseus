//! The machine's own helper and unit: `sudo -n`, the staging directory, `~/.config/systemd/user`, `systemd-run`.

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use super::{rooted, Helper, Presence, Unit, Units, HELPER, STAGE_DIR};
use crate::rsi::join::install::{BINARIES, SERVICE};

/// How long a look at `sudo -n -l`, or the helper's run, may take.
const SUDO_WITHIN: Duration = Duration::from_secs(120);
const LOOK_WITHIN: Duration = Duration::from_secs(5);
/// How long a presence read is kept.
const KEEP: Duration = Duration::from_secs(300);
/// The environment every program here is run with: a fixed `PATH`, nothing else.
const PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

fn clean(program: &str, args: &[&str]) -> Command {
    let mut c = Command::new(program);
    c.args(args)
        .env_clear()
        .env("PATH", PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

/// What a pipe holds, up to 64 KiB.
fn drain(pipe: Option<impl Read>) -> String {
    let mut s = String::new();
    if let Some(p) = pipe {
        let _ = p.take(64 * 1024).read_to_string(&mut s);
    }
    s
}

/// Run `c`, wait up to `within`, and return its status and both outputs; a program that runs over is killed.
fn run_bounded(mut c: Command, within: Duration) -> Result<(std::process::ExitStatus, String)> {
    // A registered child (`children::spawn`), as every child of the daemon is: its exit is the sweep's to account for.
    let mut child = theseus_kernel::children::spawn(
        theseus_kernel::children::Kind::Owned,
        || c.spawn(),
        |c| Some(c.id()),
    )
    .context("the program could not start")?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let t0 = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if t0.elapsed() > within {
            let _ = child.kill();
            let _ = child.wait();
            bail!("it did not finish within {} s", within.as_secs());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut text = drain(out.take());
    text.push_str(&drain(err.take()));
    Ok((status, text.trim().to_string()))
}

/// The helper through `sudo -n`, and the staging directory it reads.
#[derive(Default)]
pub struct Sudo {
    seen: Mutex<Option<(Instant, Presence)>>,
}

impl Sudo {
    /// The helper is there, and `sudo -n -l` says the rule lets this user run it with a commit.
    fn look() -> Presence {
        let meta = match std::fs::symlink_metadata(HELPER) {
            Ok(m) => m,
            Err(_) => return Presence::NotInstalled(format!("there is no {HELPER}")),
        };
        if !meta.is_file() {
            return Presence::NotInstalled(format!("{HELPER} is not a plain file"));
        }
        let probe = "0000000000000000000000000000000000000000";
        match run_bounded(
            clean("sudo", &["-n", "-l", "--", HELPER, probe]),
            LOOK_WITHIN,
        ) {
            Ok((s, _)) if s.success() => Presence::Installed,
            Ok((_, said)) => Presence::NotInstalled(format!(
                "sudo does not let this user run it without a password ({})",
                said.lines().next().unwrap_or("no reason given")
            )),
            Err(e) => Presence::NotInstalled(format!("sudo could not be asked: {e:#}")),
        }
    }

    /// What was read in the last five minutes, or a fresh look.
    fn cached(&self) -> Presence {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, p)) = seen.as_ref() {
            if at.elapsed() < KEEP {
                return p.clone();
            }
        }
        let p = Self::look();
        *seen = Some((Instant::now(), p.clone()));
        p
    }
}

impl Helper for Sudo {
    fn presence(&self) -> Presence {
        self.cached()
    }

    fn stage(&self, release: &Path) -> Result<()> {
        let stage = Path::new(STAGE_DIR);
        let meta = std::fs::symlink_metadata(stage).with_context(|| {
            format!("the staging directory {STAGE_DIR} is missing: the administrator's step makes it (docs/root-install.md)")
        })?;
        if !meta.is_dir() {
            bail!("{STAGE_DIR} is not a directory");
        }
        // Whatever an earlier install left there goes first: the helper refuses a directory with anything else in it.
        for entry in std::fs::read_dir(stage)? {
            let path = entry?.path();
            if std::fs::symlink_metadata(&path)?.is_dir() {
                bail!(
                    "{} is a directory: the staging directory holds only programs",
                    path.display()
                );
            }
            std::fs::remove_file(&path)?;
        }
        for name in BINARIES {
            let from = release.join(name);
            let meta = std::fs::symlink_metadata(&from)
                .with_context(|| format!("{} is missing from the build", from.display()))?;
            if !meta.is_file() {
                bail!("{} is not a plain file", from.display());
            }
            let tmp = stage.join(format!(".{name}.new"));
            std::fs::copy(&from, &tmp)
                .with_context(|| format!("copying {name} into the staging directory"))?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
            std::fs::rename(&tmp, stage.join(name))?;
        }
        Ok(())
    }

    fn install(&self, commit: &str) -> Result<String> {
        let (status, said) =
            run_bounded(clean("sudo", &["-n", "--", HELPER, commit]), SUDO_WITHIN)?;
        if status.success() {
            Ok(said)
        } else {
            Err(anyhow!(
                "{}",
                if said.is_empty() {
                    "the helper refused, with no reason".into()
                } else {
                    said
                }
            ))
        }
    }
}

static WORD: Mutex<Option<(Instant, String)>> = Mutex::new(None);
static LOOKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What `health` says of the helper: nothing where the service does not run a root-owned build, else whether the
/// helper is installed. It never waits: the last answer is read, and a stale one is renewed on a thread of its own.
pub(crate) fn helper_word(running: &Path) -> String {
    if rooted(running).is_none() {
        return String::new();
    }
    let fresh = {
        let word = WORD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        word.as_ref()
            .filter(|(at, _)| at.elapsed() < KEEP)
            .map(|(_, w)| w.clone())
    };
    if let Some(w) = fresh {
        return w;
    }
    if !LOOKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        std::thread::spawn(|| {
            let w = match Sudo::look() {
                Presence::Installed => "installed".to_string(),
                Presence::NotInstalled(why) => format!("not installed ({why})"),
            };
            *WORD
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((Instant::now(), w));
            LOOKING.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }
    "being looked at".into()
}

/// The user's own unit for the service, and the transient unit that restarts it.
pub struct UserUnit;

impl UserUnit {
    fn path() -> Result<PathBuf> {
        let base = match std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            Some(p) => p,
            None => {
                PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config")
            }
        };
        Ok(base.join("systemd/user").join(SERVICE))
    }
}

impl Units for UserUnit {
    fn read(&self) -> Result<Unit> {
        let path = Self::path()?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok(Unit { path, text })
    }

    fn write(&self, at: &Path, text: &str) -> Result<()> {
        crate::config_change::write::atomic(at, text, Some(0o644))?;
        Ok(())
    }

    fn restart(&self, script: &str, short: &str) -> Result<String> {
        let unit = format!("theseus-root-install-{short}");
        // The script is the argument, not a file: nothing is written for the restart, so nothing can be replaced.
        let mut c = Command::new("systemd-run");
        c.args([
            "--user",
            "--collect",
            "--quiet",
            &format!("--unit={unit}"),
            "--property=Nice=10",
            "/bin/bash",
            "-c",
            script,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        let o = theseus_kernel::children::spawn(
            theseus_kernel::children::Kind::Owned,
            || c.spawn(),
            |c| Some(c.id()),
        )
        .and_then(std::process::Child::wait_with_output)
        .context("systemd-run could not start")?;
        if !o.status.success() {
            bail!(
                "systemd-run refused: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(unit)
    }
}
