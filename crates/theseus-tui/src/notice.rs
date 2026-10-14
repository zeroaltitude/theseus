//! Notices (design `stage2` §2.9; theseus-753z): the TUI's half is its
//! viewer (the session on screen while the terminal has focus, the seen file,
//! `--notify off` as quiet), in `app.rs`. The rest, the policy's notices with
//! their second and the burst fold, and how a ping reaches the owner (the
//! bell, OSC 9, OSC 777, or a Windows toast), is `theseus_client::notify`,
//! shared with `theseus status --watch` (theseus-f0bi9).

pub use theseus_client::notify::{Delivery, Notices};
