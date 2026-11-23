//! Theseus installs what its own loop joined, on a machine whose service runs root-owned programs (issue 22;
//! `docs/design/self-install.md` is the whole design, and says what the root-owned directory protects and what this
//! gives up).
//!
//! The service is a user unit whose `ExecStart` names `/usr/local/lib/theseus/<commit8>/theseusd`, a directory only
//! the administrator can write. The administrator installs, once, a helper (`theseus-root-install`, behind one
//! sudoers rule: `scripts/root-install.sh`) that copies the four programs of a commit from one staging directory
//! into such a directory. This module is the daemon's side:
//!
//! 1. **After a join** ([`offer`]): on a rooted layout, with `[self] install` not `"off"` and the helper there, the
//!    change the loop just joined is built (in L1, as every install builds) and, for `"ask"`, put to the owner on a
//!    card: "Install the change Theseus joined: <title>, <commit8>?". It is sensitive, so it goes through the same
//!    mark as `config.propose` for the second proof (`config_change::proof_seam`): with no authenticator app paired, no
//!    card is made and the owner is told how to pair one. `"auto"` makes no card (turning it on needed a fresh code).
//! 2. **On the yes** ([`answer`]): the commit must be one the loop's own `self.joined` row names and main's head on
//!    the remote at that moment. The programs are staged, the helper runs through `sudo -n`, the program of the service's start command
//!    (in the unit, or in the drop-in whose `ExecStart=` wins: [`start`]) is pointed at the new directory and systemd is
//!    asked that it will run it, a marker is kept, and a transient unit restarts the service and **waits for the
//!    new daemon's own confirmation**; without it the unit points back at the directory before and restarts onto it.
//!    One install restarts at most twice (onto the build, and back) and never loops.
//! 3. **At the next start** ([`verify`]): the daemon that finds the marker and is the build it names waits until it
//!    is healthy, confirms, and only then says "joined, installed and running"; the daemon that finds the marker and
//!    is not that build was put back, and says so.
//!
//! Every step is a `self.root_install` ledger row. The restart is not `config_change`'s exec of the same image (that
//! cannot change the program the daemon runs): it is the install's own transient `systemd-run --user` unit, with
//! `config_change`'s stages in the marker.

mod answer;
mod hand;
mod machine;
mod offer;
mod start;
mod unit;
mod verify;

pub use offer::confirm_request;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_busy;
#[cfg(test)]
mod tests_dropin;
#[cfg(test)]
mod tests_stage;
#[cfg(test)]
mod tests_start;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub(crate) use hand::hand_words;
pub(crate) use machine::helper_word;
pub use machine::{Sudo, UserUnit};
pub use unit::{exec_program, repoint, rooted, Rooted};

/// The question the owner's yes answers: a planned action of this name.
pub const ACK: &str = "self.install.ack";
/// What an agent's process is refused with when it answers the card.
pub const AGENT_REFUSAL: &str =
    "an agent's process is not the owner, and this installs a change into what Theseus \
     runs, so only the owner answers it: press it in the DM, or answer from your own terminal";
/// The helper, as the administrator's step installs it.
pub const HELPER: &str = "/usr/local/sbin/theseus-root-install";
/// The one directory the helper reads from, owned by the service's user.
pub const STAGE_DIR: &str = "/var/lib/theseus-root-install/stage";
/// Where the builds are, one directory per commit.
pub const LIB_DIR: &str = "/usr/local/lib/theseus";
/// How long the restart waits for the new daemon to confirm itself before it goes back, in seconds. Said on the card.
pub const CONFIRM_WITHIN_SECS: u64 = 240;
/// How long the new daemon waits to be healthy before it confirms, in seconds.
pub const HEALTHY_WITHIN_SECS: u64 = 120;
/// How often health is looked at while waiting.
pub const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
/// The directory under the state directory that holds the confirmation, and its name there.
pub const DIR: &str = "self-install";
pub const SIGNAL: &str = "confirmed";

/// Whether the administrator's helper is there and the owner's account may run it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Installed,
    NotInstalled(String),
}

/// The helper and what feeds it: a seam, so a test's stand-in answers for a machine it is not.
pub trait Helper: Send + Sync {
    fn presence(&self) -> Presence;
    /// Put the four programs of a build (`release`'s files) in the staging directory, replacing what it held.
    fn stage(&self, release: &Path) -> Result<()>;
    /// Empty the staging directory: the helper has copied what was staged (or refused it), and the programs, a few
    /// hundred megabytes of the owner's, are not left there.
    fn unstage(&self) -> Result<()>;
    /// Run the helper for `commit` (through `sudo -n`): its one-line answer, or why it refused.
    fn install(&self, commit: &str) -> Result<String>;
    /// `program` is what the helper installs: a plain file, root's, that no one else can write. Checked before the
    /// service is pointed at it.
    fn check_installed(&self, program: &Path) -> Result<()>;
}

/// The user unit, and the restart: a seam as [`Helper`] is.
pub trait Units: Send + Sync {
    /// The file that holds the service's start command in force (`start.rs`): the unit file, or the drop-in whose
    /// `ExecStart=` wins, as it is, with its path.
    fn read(&self) -> Result<Unit>;
    /// Write that file's text in one step, keeping its permissions.
    fn write(&self, at: &Path, text: &str) -> Result<()>;
    /// What systemd will run to start the service, after reloading its units: one program per start command. The
    /// install restarts only when this is the new build.
    fn will_run(&self) -> Result<Vec<String>>;
    /// Start the transient unit that runs `script` (its text, not a file: nothing on disk is read back), named for
    /// `short`: the restart that waits and goes back.
    fn restart(&self, script: &str, short: &str) -> Result<String>;
}

/// The file that holds the start command in force, as read: the unit file itself, or a drop-in that sets `ExecStart=`.
#[derive(Debug, Clone)]
pub struct Unit {
    pub path: PathBuf,
    pub text: String,
    /// Why Theseus will not write that file (it is the administrator's, or gone at the next boot, or not this
    /// user's to write), in words for the owner. The install is refused, before anything changes, when this is set.
    pub refusal: Option<String>,
}

/// Where an install is: the stages of its marker, as `config_change::restart`'s are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Restarting,
    Started,
    Confirmed,
    RolledBack,
}

/// The install a restart is onto, kept in the store (META [`verify::KEY`]) because the daemon that wrote it is not the
/// one that reads it, and nothing the daemon trusts about an install may live where a tool can write it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// The question that approved it (or `auto`).
    pub question: String,
    /// The commit installed.
    pub merge: String,
    pub branch: String,
    pub what: String,
    /// The program the unit named before, and the one it names now.
    pub old_program: String,
    pub new_program: String,
    /// The file that held the start command (the unit, or the drop-in that won), and its text as it was before it
    /// was changed: where the way back goes, in the script and in the daemon's own rollback.
    pub unit: PathBuf,
    pub unit_before: String,
    pub approved_by: String,
    pub via: String,
    pub requested_ms: u64,
    pub stage: Stage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// The seams a test replaces: the machine's own helper, unit and build otherwise.
#[derive(Clone)]
pub struct Seams {
    pub helper: Arc<dyn Helper>,
    pub units: Arc<dyn Units>,
    /// The build of a joined merge in L1: the machine's own, a stand-in in a test's.
    pub build: Arc<super::install::Build>,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            helper: Arc::new(Sudo::default()),
            units: Arc::new(UserUnit::default()),
            build: Arc::new(super::install::build),
        }
    }
}

/// What the daemon has of this: the state directory, the seams, and a handle on itself once built.
#[derive(Clone)]
pub struct Board {
    pub state: PathBuf,
    seams: Arc<std::sync::Mutex<Seams>>,
    core: Arc<OnceLock<Weak<crate::rpc::Core>>>,
}

impl Default for Board {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

impl Board {
    pub fn new(state: PathBuf) -> Self {
        Self {
            state,
            seams: Arc::default(),
            core: Arc::default(),
        }
    }

    fn seams(&self) -> std::sync::MutexGuard<'_, Seams> {
        self.seams
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn helper(&self) -> Arc<dyn Helper> {
        self.seams().helper.clone()
    }

    pub fn units(&self) -> Arc<dyn Units> {
        self.seams().units.clone()
    }

    pub fn build(&self) -> Arc<super::install::Build> {
        self.seams().build.clone()
    }

    /// A test's stand-ins, in place of the machine's.
    #[cfg(test)]
    pub fn stand_in(&self, s: Seams) {
        *self.seams() = s;
    }

    /// The core, once built: held weakly, as every board holds it.
    pub fn attach(&self, core: &Arc<crate::rpc::Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    pub(crate) fn core(&self) -> Option<Arc<crate::rpc::Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The directory of the one file the restart's unit looks for: the new daemon's confirmation. On the floor
    /// (inside the store directory): no tool writes it. Nothing else of an install is kept in a file.
    pub fn dir(&self) -> PathBuf {
        dir_in(&self.state)
    }

    /// The confirmation: written by the new daemon once it is healthy, looked for by the restart's unit.
    pub fn signal(&self) -> PathBuf {
        self.dir().join(SIGNAL)
    }
}

/// The directory of the confirmation, in a state directory: inside the store directory, which the floor holds, so no
/// tool reads or writes it (the WAL reads only its `.seg` files).
pub fn dir_in(state: &Path) -> PathBuf {
    crate::rpc::store_dir(state, false).join(DIR)
}

/// The first eight digits of a commit: what its directory is named.
pub fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

/// Forty lowercase hex digits.
pub fn is_commit(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Where the program of `commit`'s build is.
pub fn program_of(commit: &str) -> PathBuf {
    Path::new(LIB_DIR).join(short(commit)).join("theseusd")
}
