//! The Discord binding (spec P5, M3): one guild, direct messages and one text
//! channel. It runs in the daemon's process, beside the core, and reaches the
//! core two ways (theseus-0g4).
//!
//! - **What a person does goes through the protocol**, over an in-process
//!   connection (`rpc_client.rs`) on the Discord surface, so the core judges it
//!   as it judges the web UI's and the CLI's: a message is a `turn.submit`, a
//!   button press an `action.confirm`, `/stop` an `execution.stop`, "should
//!   have asked" a `policy.tighten`, `/trust` a `policy.trust`, and the
//!   listings and cancels of `/tasks`, `/wakes`, and `/cancel` theirs too. It
//!   watches the sessions behind its places the same way (`session.watch`),
//!   for live progress: a streamed reply as edited messages, the tool calls as
//!   they run.
//! - **What it delivers and reports, it reads and writes in the core
//!   directly**: the outbox, whose posts it delivers in order per place and
//!   once (the courier, theseus-q4v), the question behind each card, the
//!   `[approval]` channels, its state on the bindings board and its ledger
//!   rows, and at its start the config gate, the secrets, and the startup log.
//!
//! What is Discord's alone stays here: the gateway, the rendering, the confirm
//! button, and the slash commands.

pub mod bindings;
mod courier;
mod files;
pub mod render;
mod render_cred;
mod rpc_client;
mod runtime;
#[cfg(test)]
mod tests_gateway;
#[cfg(test)]
mod tests_outbox;
pub mod viewers;

pub use bindings::{Bindings, EXAMPLE_BINDINGS};
pub use courier::{nonce, NONCE_WINDOW_MS};
pub use runtime::run;
