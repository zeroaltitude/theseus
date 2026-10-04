//! The Language Server Protocol, written by hand (lane L1, theseus-n88g.7).
//!
//! Theseus gives its models language-server tools: diagnostics in the results
//! of its file edits, then navigation and a gated rename. This crate is the
//! client alone, over serde and tokio, with no core, as theseus-mcp is for
//! MCP:
//!
//! - [`framing`]: `Content-Length` messages over any async stream.
//! - [`client::Client`]: one connection to a server the caller started
//!   ([`client::Server`]): `initialize`, requests routed by id with a timeout
//!   and `$/cancelRequest` for a dropped one, the server's own requests
//!   answered, readiness from `$/progress` and rust-analyzer's
//!   `experimental/serverStatus`, and the stop (`shutdown`, `exit`, the kill).
//! - Document sync (`docs.rs`): full text, a version per document, a resync
//!   of any open file changed on disk before each request.
//! - Diagnostics ([`diagnostics`]): pushed and pulled, and the bounded wait
//!   for a file's list after a change.
//! - Navigation (`nav.rs`): definition, references, hover, symbols, and
//!   rename's [`types::WorkspaceEdit`], never applied.
//! - [`position::locate`]: the tools' addressing, a 1-based line and a
//!   symbol's text, made a UTF-16 LSP position.
//! - [`servers`]: the servers the probe found working, and their quirks.
//! - [`fake`]: a scripted fake server, in this process or as
//!   `theseus-lsp-fake`.
//!
//! What a core needs on top (the board, the tools, the gate, the hooks into
//! `fs.write`, `fs.edit`, and `fs.patch`) is lanes L2's and L3's.

pub mod client;
pub mod diagnostics;
mod docs;
pub mod fake;
pub mod framing;
pub mod jsonrpc;
mod nav;
pub mod position;
pub mod servers;
pub mod spawn;
pub mod types;
pub mod uri;

pub use client::{Client, Error, Event, Events, Options, Server, Stopped};
pub use diagnostics::{FileDiagnostics, Freshness};
pub use docs::language_id;
pub use position::locate;
