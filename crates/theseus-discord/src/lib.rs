//! The Discord binding (spec P5, M3): one guild, direct messages and one text
//! channel. It is a protocol client of the core, exactly like the web UI and
//! the CLI: every turn it starts goes through `turn.submit`, a button press
//! through `action.confirm`, `/stop` through `execution.cancel`. What must
//! reach Discord (a turn's reply, a confirm card and how it closed, a notice)
//! the core writes to the outbox, and the binding only delivers it, in order
//! per place and once (theseus-q4v). It watches the sessions behind its places
//! for live progress: a streamed reply as edited messages, the tool calls as
//! they run. What is Discord's alone stays here: the gateway, the rendering,
//! the confirm button, and the slash commands.

pub mod bindings;
mod courier;
mod files;
pub mod render;
mod rpc_client;
mod runtime;
#[cfg(test)]
mod tests_outbox;
pub mod viewers;

pub use bindings::{Bindings, EXAMPLE_BINDINGS};
pub use courier::{nonce, NONCE_WINDOW_MS};
pub use runtime::run;
