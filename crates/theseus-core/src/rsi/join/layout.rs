//! Where the service's program is, and whether the loop can replace it (issue 20: "a joined
//! change is not what runs afterwards").
//!
//! The install copies the new build over the programs the service starts and restarts it. That
//! only changes what runs when the copy lands where the service looks. On a machine set up as the
//! user guide says, the service runs the user's own copy and the copy lands there. On a machine
//! whose service runs a build kept in a directory only an administrator can write, the copy landed
//! in the user's own directory, the restart came back on the old build, and nothing said so.
//!
//! So the install first reads the layout: [`Layout::User`] when the directory of the program the
//! service starts is one the loop can write (it installs there, and the new daemon then checks
//! that it is the build the install was for: `verify`); [`Layout::Rooted`] when it is not (it
//! copies nothing and restarts nothing, and tells the owner the one step that installs the build
//! itself). It never writes the administrator's directory: that is a decision of its own.

use std::path::{Path, PathBuf};

use super::install::BINARIES;

/// How the machine's service runs its program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// The program's directory is one the loop can write: the service runs the user's own copy.
    User { dir: PathBuf },
    /// The program is in a directory the loop cannot write: the service runs a root-owned build.
    Rooted { program: PathBuf },
    /// Neither the unit nor this process said where the program is.
    Unknown { why: String },
}

/// What the layout is read from: a test's stand-in answers for a machine it is not.
pub trait Host: Send + Sync {
    /// The program the service starts, from the unit's `ExecStart`; none when it cannot be read.
    fn unit_program(&self) -> Option<PathBuf>;
    /// The running daemon's own program (`current_exe`).
    fn running_program(&self) -> Option<PathBuf>;
    /// Whether the loop can put a file in `dir` (and so replace the programs there).
    fn writable(&self, dir: &Path) -> bool;
}

/// The machine's own: the user manager's unit, this process, and the permissions on disk.
pub struct Machine;

impl Host for Machine {
    fn unit_program(&self) -> Option<PathBuf> {
        let out = std::process::Command::new("systemctl")
            .args([
                "--user",
                "show",
                "-p",
                "ExecStart",
                "--value",
                super::install::SERVICE,
            ])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| program_of(&String::from_utf8_lossy(&out.stdout)))
            .flatten()
    }

    fn running_program(&self) -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        // A program replaced since this process started reads as `… (deleted)`.
        let shown = exe.to_string_lossy();
        Some(PathBuf::from(
            shown
                .strip_suffix(" (deleted)")
                .unwrap_or(&shown)
                .to_string(),
        ))
    }

    fn writable(&self, dir: &Path) -> bool {
        crate::binary::writable(dir)
    }
}

/// The program in `systemctl show -p ExecStart --value`'s text: `{ path=/a/b ; argv[]=… ; … }`.
pub(super) fn program_of(shown: &str) -> Option<PathBuf> {
    let path = shown.split("path=").nth(1)?.split(" ;").next()?.trim();
    (path.starts_with('/')).then(|| PathBuf::from(path))
}

/// The layout, as `host` reports it. The unit's program is the one a restart runs, so it decides
/// where there is one; the running daemon's is the answer when the unit cannot be read.
pub fn read(host: &dyn Host) -> Layout {
    let Some(program) = host.unit_program().or_else(|| host.running_program()) else {
        return Layout::Unknown {
            why: "neither the service's unit nor this daemon says where its program is".into(),
        };
    };
    let Some(dir) = program.parent().map(Path::to_path_buf) else {
        return Layout::Unknown {
            why: format!("{} has no directory", program.display()),
        };
    };
    if host.writable(&dir) {
        Layout::User { dir }
    } else {
        Layout::Rooted { program }
    }
}

/// How many characters of a commit a build's directory is named by.
const SHORT: usize = 8;

/// The one step that installs the build in `target` where the service runs a root-owned program:
/// what `docs/setup.md` ("A root-owned install") gives, filled in for this machine. A service
/// whose program sits in a directory named for a commit gets a directory for the new one and its
/// unit pointed at it; any other is overwritten in place. `repo` is the checkout, which holds the
/// script that writes the unit.
pub fn step(program: &Path, target: &Path, repo: &Path, merge: &str) -> String {
    let dir = program.parent().unwrap_or(Path::new("/"));
    let per_commit = dir.parent().filter(|_| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.len() >= 7 && n.bytes().all(|b| b.is_ascii_hexdigit()))
    });
    let built = format!("{}/release/{{{}}}", target.display(), BINARIES.join(","));
    let Some(root) = per_commit else {
        return format!(
            "sudo install -m 0755 {built} {dir}/ && {repo}/scripts/user-service.sh restart",
            dir = dir.display(),
            repo = repo.display()
        );
    };
    let new = root.join(&merge[..merge.len().min(SHORT)]);
    format!(
        "sudo install -d -m 0755 {new} && sudo install -m 0755 {built} {new}/ && \
         PATH={new}:$PATH {repo}/scripts/user-service.sh install --yes && \
         {repo}/scripts/user-service.sh restart",
        new = new.display(),
        repo = repo.display()
    )
}

/// The step when the service's program could not be found: where to copy to is the owner's to say.
pub fn unknown_step(target: &Path) -> String {
    format!(
        "copy {}/release/{{{}}} to the directory the service's program is in, then restart the \
         service (docs/setup.md, \"A root-owned install\")",
        target.display(),
        BINARIES.join(",")
    )
}
