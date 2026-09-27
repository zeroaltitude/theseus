//! The Discord binding (spec P5, M3): one guild, direct messages and one text
//! channel. It is a protocol client of the core, exactly like the web UI and
//! the CLI: every turn it starts goes through `turn.submit`, a button press
//! through `action.confirm`, `/stop` through `execution.cancel`. It watches the
//! sessions behind its places, so continuation turns (a job that finished after
//! a restart, an answer given in the web UI) reach the channel with nobody
//! asking. What is Discord's alone stays here: the gateway, a streamed reply as
//! edited messages, the confirm button, and the slash commands.

pub mod bindings;
pub mod render;
mod rpc_client;
mod runtime;

pub use bindings::{Bindings, EXAMPLE_BINDINGS};
pub use runtime::run;
