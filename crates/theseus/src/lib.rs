//! The `theseus` CLI's library (theseus-7yx, step 10a): what a client of
//! `theseusd` needs to talk to it and to show what it says. The CLI's binary
//! is built on it, and so is the TUI (design `stage2` §2.9). It is the
//! `theseus` package's library, named `theseus_client`: a library named like
//! the binary collides with it in `cargo doc --bins`.
//!
//! - [`client`]: a connection to a daemon, over its socket or a spawned
//!   `theseusd --stdio`. It prints nothing and knows no command line.
//! - [`render`]: what a terminal shows of the daemon's answers and events, as
//!   [`render::Line`]s, each a text with a style tag. The CLI prints the
//!   text, and the TUI styles it by its tag.

pub mod client;
pub mod render;

pub use client::{CallError, Conn};
