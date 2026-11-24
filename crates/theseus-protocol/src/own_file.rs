//! The owner's own instruction file, `<state_dir>/AGENTS.md`: the daemon
//! carries the version the owner last accepted, and a different file on disk
//! is named, not read, until the owner accepts it (`theseus own-file accept`).

/// The CLI's verb, the one place its name is written: `theseus own-file`
/// shows what differs, `theseus own-file accept` shows it and asks.
pub const COMMAND: &str = "own-file";

/// The words that name the accept, as every line that points to it says
/// them.
pub fn accept_command() -> String {
    format!("theseus {COMMAND} accept")
}
