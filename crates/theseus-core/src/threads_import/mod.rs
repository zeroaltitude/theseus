//! Candidate threads from the owner's earlier harness (theseus-71ib, T11).
//!
//! The owner's streams live on another harness today: a channel per concern,
//! plus the DMs where the rest lands. The soul import brought their episodes
//! in, labelled by agent, place, topic and sensitivity, and the nightly sync
//! keeps them current. This turns those streams into threads, per stream and
//! never all at once.
//!
//! - **Candidates** (`candidates.rs`, pure): each imported channel
//!   conversation with an owner message in the window is one candidate; the
//!   DMs are split by topic cluster, one candidate for each cluster with
//!   enough owner messages. The title is the channel's label, the persona
//!   the agent's only when that persona is configured, the topics the
//!   episodes' labels (at most three), the working set what the episodes
//!   name (`extract.rs`, by rules).
//! - **Partner-confidential episodes** count (their owner messages are in
//!   the table's numbers) and give no text: none reaches a title, a topic,
//!   the working set, or the model that writes the card.
//! - **The dry run** reads, and writes nothing. **The apply** makes the
//!   ticked threads one at a time, through `thread.new` and `thread.edit`.
//! - **The card** (`card.rs`, `run.rs`): one budgeted model call a thread
//!   (`[threads] import_profile`, under `import_day_usd`), off any turn's
//!   path, over the stream's summaries and owner lines, answered through one
//!   tool. Every line cites the episodes it came from as `[m:…]` handles,
//!   the ones recall uses, and a line citing none, or pasting a message, is
//!   dropped. A thread past the day's line is made without a card.
//! - **Testimony stays testimony**: the card is a derived summary on the
//!   thread's record. The episodes are read only through recall, with their
//!   provenance; nothing here writes a node into the thread's session.

pub mod candidates;
pub mod card;
pub mod extract;
pub mod source;

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

/// One owner message of an imported episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msg {
    pub node: String,
    pub at_ms: u64,
    pub text: String,
}

/// An imported episode's summary node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub node: String,
    pub text: String,
}

/// What the scan reads of one imported episode.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub session_id: String,
    pub agent: Option<String>,
    pub place_kind: String,
    pub place_name: Option<String>,
    pub place_id: Option<String>,
    pub topics: Vec<String>,
    pub sensitivity: String,
    /// Partner-confidential by label or by a partner codename.
    pub partner: bool,
    pub end_ms: u64,
    /// The owner's messages, oldest first.
    pub owner: Vec<Msg>,
    pub summary: Option<Summary>,
}
