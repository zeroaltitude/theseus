//! The helper that does one thing (issue 22; `docs/design/self-install.md` is the whole design): copy the four
//! programs of a named commit from one fixed staging directory into `/usr/local/lib/theseus/<commit8>/`, as root-owned
//! files in a root-owned directory, and change nothing else on the machine.
//!
//! - **One argument**, forty lowercase hex digits. No path, no option; the environment is cleared by `main` and never
//!   read.
//! - **The real paths are compiled in** ([`Config::real`]). A [`Config`] with other paths exists only for the tests,
//!   which run this logic against directories they own; `main.rs` never builds one (a test holds it to that).
//! - **Nothing the owner supplied is run.** The programs are copied from open descriptors into a directory made for
//!   the purpose, and every check reads the copy, which the owner can no longer change.
//! - **One directory is written**, by a temporary directory beside it and a rename that never replaces.

mod install;
mod stage;
pub mod stamp;
mod sys;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::path::PathBuf;

/// The programs an install holds, and the only names the staging directory may hold: what `scripts/build.sh
/// --installed` lists (a test in `theseus-core` holds the lists together).
pub const PROGRAMS: [&str; 4] = ["theseusd", "theseus", "theseus-tui", "theseus-index"];
/// The most build directories `/usr/local/lib/theseus` may hold before the helper refuses another: the owner's account
/// can ask it, so the disk it can fill is bounded here, and the administrator prunes old ones.
pub const MAX_BUILDS: usize = 20;
/// The largest a program may be, in bytes.
pub const MAX_PROGRAM: u64 = 1 << 30;

/// Where the helper reads and writes, and what it requires of who owns them.
#[derive(Debug, Clone)]
pub struct Config {
    /// The one staging directory.
    pub stage: PathBuf,
    /// The directory the builds go in.
    pub lib_dir: PathBuf,
    /// Where a walk of a path starts: its components below this are each checked.
    pub anchor: PathBuf,
    /// The uid that must own every directory on a walk and every result.
    pub trusted_uid: u32,
    /// The real program: it must run as root, hands its results to root, and requires the staging directory to belong
    /// to someone else.
    pub real: bool,
}

impl Config {
    /// The paths of the real program, and the only configuration `main` uses.
    pub fn real() -> Self {
        Self {
            stage: "/var/lib/theseus-root-install/stage".into(),
            lib_dir: "/usr/local/lib/theseus".into(),
            anchor: "/".into(),
            trusted_uid: 0,
            real: true,
        }
    }
}

/// Why the helper did nothing, in words for the administrator's log.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn refuse<T>(why: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(why.into()))
}

/// What a successful run did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A new directory, `/usr/local/lib/theseus/<commit8>`.
    Installed(PathBuf),
    /// The directory was there with exactly this content; nothing changed.
    Already(PathBuf),
}

/// The commit in the one argument, or why it is refused.
pub fn parse_args(args: &[OsString]) -> Result<String, Refusal> {
    let [one] = args else {
        return refuse(format!(
            "it takes exactly one argument, a commit of forty hex digits; it was given {}",
            args.len()
        ));
    };
    let Some(s) = one.to_str() else {
        return refuse("the argument is not text");
    };
    if s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(s.to_string())
    } else {
        refuse("the argument is not a commit: forty lowercase hex digits")
    }
}

/// One run: the exit code, and one line to `log` for the install or the refusal.
pub fn run(cfg: &Config, args: &[OsString], log: &mut dyn FnMut(&str)) -> i32 {
    let asked = match parse_args(args) {
        Ok(c) => c,
        Err(Refusal(why)) => {
            log(&format!("refused: {why}"));
            return 1;
        }
    };
    match install::install(cfg, &asked) {
        Ok(Outcome::Installed(dir)) => {
            log(&format!("installed {asked} in {}", dir.display()));
            0
        }
        Ok(Outcome::Already(dir)) => {
            log(&format!(
                "already installed: {asked} is in {} with the same content, nothing changed",
                dir.display()
            ));
            0
        }
        Err(Refusal(why)) => {
            log(&format!("refused {asked}: {why}"));
            1
        }
    }
}

/// The real program's line: the system log, and its own standard error.
pub fn log_line(line: &str) {
    sys::syslog(line);
    eprintln!("theseus-root-install: {line}");
}
