//! The owner's agents come over as personas, unchanged (theseus-a8n7, T10b).
//!
//! `persona.import` reads an agent's workspace and carries it over:
//!
//! - **The files are copied byte for byte** (`run::import`) into
//!   `<state>/personas/<name>/`, never edited. A later import whose bytes
//!   differ is a new version: the files it replaces move to
//!   `.versions/<n>/`, and the compile that next sees the persona's digests
//!   change writes the usual `persona.changed` row. A file over the context
//!   file limit is cut at its headings (`split`), every part verbatim, so the
//!   parts joined are the file.
//! - **The card** (`card`) is drafted without a model: the identity file's
//!   role line and the agents file's headings. It is a *draft* until the
//!   owner reads it and keeps it; Jev is offered only confirmed cards.
//! - **Feedback** (`feedback`): items of the agent's memory notes that are
//!   feedback (a note whose frontmatter says `type: feedback`, or the bullets
//!   under a `## Feedback` heading) are *proposed* standing records scoped to
//!   the persona. Nothing is live until the owner keeps it.
//!
//! Pure parts (`split`, `card`, `feedback`) take text and return text; `run`
//! touches the disk. Tests: `tests.rs` over the invented workspaces in
//! `tests/fixtures/agents/`.

pub mod card;
pub mod feedback;
pub mod manifest;
pub mod run;
pub mod split;

#[cfg(test)]
mod tests;

pub use manifest::Manifest;
pub use run::import;
