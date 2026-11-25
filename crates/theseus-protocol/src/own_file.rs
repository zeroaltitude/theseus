//! The owner's own instruction file, `<state_dir>/AGENTS.md`, and the files
//! his settings name under `[context] files`: the daemon carries the version
//! the owner last accepted of each, and a different file on disk is named,
//! not read, until the owner accepts it (`theseus own-file accept`, with
//! `--file PATH` for a context file).

/// The CLI's verb, the one place its name is written: `theseus own-file`
/// shows what differs, `theseus own-file accept` shows it and asks.
pub const COMMAND: &str = "own-file";

/// The words that name the accept, as every line that points to it says
/// them.
pub fn accept_command() -> String {
    format!("theseus {COMMAND} accept")
}

/// The words that name the accept of `file`, a context file the settings
/// name; none: the owner's own instruction file. The path is one shell word
/// ([`shell_word`]), so the line can be pasted at a terminal.
pub fn accept_command_for(file: Option<&str>) -> String {
    match file {
        Some(f) => format!("{} --file {}", accept_command(), shell_word(f)),
        None => accept_command(),
    }
}

/// The words that name the show of `file`, as [`accept_command_for`] does.
pub fn show_command_for(file: Option<&str>) -> String {
    match file {
        Some(f) => format!("theseus {COMMAND} --file {}", shell_word(f)),
        None => format!("theseus {COMMAND}"),
    }
}

/// `s` as one word a shell reads back as `s`: as it is when every character
/// is plain, else in single quotes, a quote in it written `'\''`.
pub fn shell_word(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-:@+=,%".contains(c));
    if plain {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

use serde::{Deserialize, Serialize};

/// `own_file.show`: the owner's own instruction file, or `file`, a context file the settings name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileShowParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub file: Option<String>,
}

/// Where the file on disk stands against the accepted version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OwnFileState {
    /// No version is accepted yet: nothing of the file is carried.
    NotAccepted,
    /// The pointer to the accepted version cannot be read from the store: nothing is carried, and nothing
    /// can be accepted.
    Unreadable,
    /// The accepted version (`accepted` names it) cannot be read from the store: nothing is carried. An
    /// accept keeps what is on disk under the next number and leaves the old record as it is.
    Damaged,
    /// An accept may already be written that the daemon cannot read back until it is restarted:
    /// nothing is carried, and nothing can be accepted until then.
    Unconfirmed,
    /// The disk holds the accepted version (or no file, accepted as absent).
    Same,
    /// The disk holds another text than the accepted one.
    Differs,
    /// The accepted text is carried, and the file is gone from disk.
    Gone,
    /// No file was accepted, and one is on disk now.
    New,
}

/// The accepted version, as the show names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAccepted {
    pub seq: u64,
    /// Its digest; none: the file was accepted as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    pub at_ms: u64,
    /// Its day on the daemon's clock, `2026-11-20`.
    pub on: String,
}

/// What `own_file.show` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileShowResult {
    pub path: String,
    pub state: OwnFileState,
    /// The accepted version; none when none is, or it cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub accepted: Option<OwnFileAccepted>,
    /// The file on disk's size; none: no file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub bytes: Option<u64>,
    /// The digest an accept of what is on disk names: the disk's, empty for no file.
    pub digest: String,
    /// The changed lines with a little context, whole, or a summary that says it is one when they do not fit.
    /// None when nothing differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub change: Option<String>,
}

/// `own_file.accept`: accept what is on disk, which must still be what the owner was shown (`digest`, from
/// `own_file.show`; empty for no file).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAcceptParams {
    pub digest: String,
    /// A context file the settings name, as `own_file.show` was given it; none: the owner's own
    /// instruction file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// What `own_file.accept` returns: the version that stands now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnFileAcceptResult {
    pub path: String,
    pub seq: u64,
    /// Whether this accept kept a new version; false when the disk already held the accepted one.
    pub kept: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pasteable command keeps a plain path as it is, and quotes one with
    /// a space, a quote or a character a shell would read.
    #[test]
    fn the_accept_command_quotes_a_path_a_shell_would_split_or_read() {
        assert_eq!(
            accept_command_for(Some("/srv/notes/RULES.md")),
            "theseus own-file accept --file /srv/notes/RULES.md"
        );
        assert_eq!(
            accept_command_for(Some("/srv/my notes/RULES.md")),
            "theseus own-file accept --file '/srv/my notes/RULES.md'"
        );
        assert_eq!(
            accept_command_for(Some("/srv/it's/R.md")),
            "theseus own-file accept --file '/srv/it'\\''s/R.md'"
        );
        assert_eq!(
            show_command_for(Some("/srv/$(touch x);`y`|z&.md")),
            "theseus own-file --file '/srv/$(touch x);`y`|z&.md'"
        );
        assert_eq!(accept_command_for(None), "theseus own-file accept");
    }
}
