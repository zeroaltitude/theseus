//! The owner asks Theseus to change one of its prompt files, and the daemon
//! writes it (issue 23; `docs/design/prompt-file-change.md` has the design).
//!
//! A second kind of change through issue 22's door, not a second door: the
//! same wall (`wall.rs`), the same question (`config.ack`), the same second
//! proof (`proof_seam.rs`), the same kept copies, history and undo. What is
//! different is the text the daemon changes. It is one of the files the loaded
//! settings name as prompt text (a `[context] files` entry, or a file of a
//! persona), never the settings file, and it is written whole by the daemon,
//! after the owner has seen exactly which lines change and said yes.
//!
//! - **Which file** is decided here ([`named`], [`resolve`]), by the daemon's
//!   own reading of the settings: the model gives a path, which is accepted
//!   only if it is, after the settings' own expansion, one the settings name,
//!   a regular file with no link at its end, and nothing the floor holds.
//!   There is no way to name a new path.
//! - **What changes** is built by the daemon from the file as it is now: the
//!   model gives either the whole new text or exact replacements, each of
//!   which must find its old text exactly once.
//! - **The card** (`card.rs`) is built from the two texts alone, by
//!   `config_change::shown`; the model's only words on it are its reason.
//! - **The yes** (`write.rs`) re-reads the file: one that is not byte for byte
//!   the file the card was built from is not written.
//!
//! A prompt file is read again at every request (`context_files.rs`), so a
//! change takes effect with the next message and nothing restarts; a context
//! file's yes is also its accept (`write.rs`), so the version carried is the
//! text written.

pub mod card;
pub mod history;
pub mod put;
pub mod tools;
pub mod undo;
pub mod write;

use std::path::{Path, PathBuf};

use crate::context_files::Level;
use crate::rpc::Core;

pub use tools::{Propose, Read};

pub const PROPOSE: &str = "config.propose_file";
pub const READ: &str = "config.read_file";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 2] = [PROPOSE, READ];

/// The largest a prompt file may be, before or after a change: the files are
/// prose, and only the first 64 KiB of one reaches a prompt
/// (`context_files::MAX_BYTES`).
pub const MAX_FILE_BYTES: usize = 256 * 1024;

/// The kind a question's arguments carry (`args["kind"]`).
pub const KIND: &str = "prompt_file";

/// A file the settings name as prompt text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The path as the settings spell it, expanded: what a request may name.
    pub expanded: PathBuf,
    /// Its canonical form: what is compared, and what is written.
    pub path: PathBuf,
    /// Every level that names it.
    pub levels: Vec<Level>,
    /// A shared place carries it too (`readers = "public"`).
    pub public: bool,
}

impl Named {
    /// `the system level`, `persona warm`.
    pub fn words(&self) -> String {
        let mut who: Vec<String> = Vec::new();
        for l in &self.levels {
            who.push(match l {
                Level::System => "system context file".into(),
                Level::Persona(p) => format!("file of persona {p}"),
                Level::User => "user instruction file".into(),
                Level::Project => "project file".into(),
            });
        }
        who.dedup();
        who.join(" and ")
    }

    /// A persona names the file, so a persona's size rule holds for it.
    pub fn is_persona(&self) -> bool {
        self.levels.iter().any(|l| matches!(l, Level::Persona(_)))
    }
}

/// What the settings name as prompt text: `[context] files`, and the files of
/// every live persona, each by its canonical path. A path that is not
/// absolute once expanded, or that does not resolve to a file, is not listed.
pub fn named(core: &Core) -> Vec<Named> {
    let paths = core.cfg.context_paths(&core.personas(), None, true);
    let mut out: Vec<Named> = Vec::new();
    for p in paths {
        let expanded = crate::config::expand(&p.path);
        if !expanded.is_absolute() {
            continue;
        }
        let Ok(path) = std::fs::canonicalize(&expanded) else {
            continue;
        };
        match out.iter_mut().find(|n| n.path == path) {
            Some(n) => {
                if !n.levels.contains(&p.level) {
                    n.levels.push(p.level);
                }
                n.public |= p.public;
            }
            None => out.push(Named {
                expanded,
                path,
                levels: vec![p.level],
                public: p.public,
            }),
        }
    }
    out
}

/// The paths a prompt file may never be, whatever the settings name: what the
/// floor holds (the settings file and its copies, the bindings file, the
/// authenticator's file, the stores, the token file, the sealed sets) and every
/// secret kept in a file. The prompt files are on the floor too, and are what
/// this accepts, so the floor is read without them.
fn forbidden(core: &Core, path: &Path) -> bool {
    let floor = crate::toolrun::floor_without_prompt_files(&core.cfg);
    let secret = core
        .cfg
        .secrets
        .values()
        .filter_map(|r| r.strip_prefix("file:"))
        .map(|p| theseus_tools::paths::canonical_best_effort(&crate::config::expand(p)));
    floor
        .into_iter()
        .chain(secret)
        .any(|f| theseus_tools::paths::within(path, &f))
}

/// Why a prompt file cannot be changed or read, or the one the request names.
/// `given` is what the model wrote: it is accepted only when, written out as
/// the settings write a path (`~` and variables expanded, nothing relative,
/// no `..`), it is the path one of the named files is spelled as or is its
/// canonical path, and the file is a regular file with no link at its end.
pub fn resolve(core: &Core, given: &str) -> Result<Named, String> {
    let said = given.trim();
    if said.is_empty() || said.contains('\0') {
        return Err("name the file by its path, as `config.read_file` lists it".into());
    }
    let asked = crate::config::expand(said);
    let plain = theseus_tools::paths::normalize(&asked);
    if !asked.is_absolute() || plain != asked || asked.components().count() < 2 {
        return Err(format!(
            "{said:?} is not a path as the settings write one: give an absolute path, with no `..` \
             and no `.`, as `config.read_file` lists it"
        ));
    }
    let named = named(core);
    let found = named
        .into_iter()
        .find(|n| n.expanded == asked || n.path == asked)
        .ok_or_else(|| {
            format!(
                "{said} is not one of the files the settings name as prompt text, so it cannot be \
                 changed here (this never makes a new file or names a new path; `config.read_file` \
                 lists the files that can be)"
            )
        })?;
    let end =
        std::fs::symlink_metadata(&asked).map_err(|e| format!("{said} cannot be read: {e}"))?;
    if end.file_type().is_symlink() {
        return Err(format!(
            "{said} is a link, and a prompt file is never reached through one"
        ));
    }
    if !end.is_file() {
        return Err(format!("{said} is not a regular file"));
    }
    if forbidden(core, &found.path) {
        return Err(format!(
            "{said} is a file Theseus keeps its own secrets or settings in, which is never a \
             prompt file"
        ));
    }
    Ok(found)
}

/// A prompt file's text, read whole: a regular file that is text and no larger
/// than [`MAX_FILE_BYTES`].
pub fn read_text(path: &Path) -> Result<String, String> {
    use std::io::Read as _;
    let end = std::fs::symlink_metadata(path).map_err(|e| format!("it cannot be read: {e}"))?;
    if end.file_type().is_symlink() || !end.is_file() {
        return Err("it is not a regular file".into());
    }
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut buf))
        .map_err(|e| format!("it cannot be read: {e}"))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(format!(
            "it is larger than {} KiB, which is more than a prompt file is: the owner edits it by hand",
            MAX_FILE_BYTES / 1024
        ));
    }
    String::from_utf8(buf).map_err(|_| "it is not text (it is not valid UTF-8)".into())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_answer;
#[cfg(test)]
mod tests_floor;
#[cfg(test)]
mod tests_held;
