//! The Model Context Protocol, written by hand (M7, steps 36a and 41a).
//!
//! MCP is JSON-RPC 2.0 over two transports: newline-delimited JSON on a
//! server process's stdin and stdout, and streamable HTTP (one message per
//! POST, answered as JSON or as a stream of server-sent events). This crate
//! is the protocol alone, over serde, tokio, and reqwest, with no core:
//!
//! - [`client::Client`]: connect over stdio or streamable HTTP, negotiate the
//!   protocol revision, list tools and prompts (every page), call a tool, get
//!   a prompt, ping, cancel, and a stream of the server's notifications
//!   ([`client::Event`]). A dropped call sends `notifications/cancelled`.
//! - [`fake::Fake`]: a fake MCP server for tests and the simulator, over
//!   pipes, this process's stdio, or HTTP on 127.0.0.1, with `fake_discord`'s
//!   kind of modes (`ok`, `slow`, `crash-after N`, `change-tools`, `error`).
//! - [`server`] (feature `server`, step 41a): Theseus's own MCP server at
//!   `/mcp` on loopback, behind a static key, an `Origin` check, and a rate
//!   limit, over a small [`server::CoreClient`] trait, with a fake core.
//!
//! What a core needs on top (an MCP board, the `Tool` contract, the gate,
//! the store) is the wire-in steps' (36b, 41b), not this crate's.

pub mod client;
pub mod fake;
pub mod jsonrpc;
pub mod names;
#[cfg(feature = "server")]
pub mod server;
pub mod sse;
pub mod types;

pub use client::{Client, Error, Event, Events, Options, Transport};

/// The newest protocol revision this crate speaks, which a client offers and
/// a server answers when asked for one it does not know.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// Every revision this crate speaks, newest first. A server's answer from
/// this list is accepted; any other ends the connection. None of them has
/// JSON-RPC batches on the wire from this side; one from an older server is
/// read.
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Whether `version` is one this crate speaks.
pub fn supports(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}
