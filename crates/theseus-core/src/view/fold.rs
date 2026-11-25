//! The fold node and the ladder's read (design §5.2; trim items 6, 7, 8). A node is written once and never
//! changes. A fold is by count: no fold takes the newest [`WINDOW_FLOOR`] exchanges of the window, and when it holds
//! [`WINDOW_FLOOR`] + [`FOLD_STEP`] its oldest [`FOLD_STEP`] fold into one first-level node ([`fold_due`]);
//! when a level holds 2 [`FOLD_STEP`] nodes its oldest [`FOLD_STEP`] fold into one node of the level above.
//! A first-level node's account is a small model's, written off the turn's path from the original exchanges;
//! a node above the first is built by CODE from first-level nodes' typed fields, with no model.

use serde::{Deserialize, Serialize};

use super::timeline::ExchangeRef;
use super::{Handle, Marks, Pos};

/// The window's floor and the fold step, defined once by the ladder (`crate::ladder`).
pub use crate::ladder::{FOLD_STEP, WINDOW_FLOOR};
/// A first-level node is at most about this many tokens; a node above it about [`UPPER_NODE_TOKENS`].
pub const FIRST_NODE_TOKENS: u64 = 900;
pub const UPPER_NODE_TOKENS: u64 = 600;
/// The key of a fold node: `fold.<place>\u{1}<level>\u{1}<first exchange's ms, 20 digits>` (META; no format
/// step).
pub const FOLD_PREFIX: &str = "fold.";

/// Who wrote a node's account: a model and version for the first level, `code` above it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Writer {
    pub by: String,
    pub version: String,
}

/// One fold node: [`FOLD_STEP`] exchanges of a place (level 1), or [`FOLD_STEP`] nodes' worth (level 2 and
/// above).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldNode {
    /// 1 for a fold of exchanges.
    pub level: u8,
    pub place: String,
    /// The first and last exchange it covers.
    pub first: Pos,
    pub last: Pos,
    pub exchanges: u32,
    /// A title of at most 120 characters: a small model's at the first level, code's above it (added by the
    /// fold work; absent on a node read from before it).
    #[serde(default)]
    pub title: String,
    /// His words in them, by reference (node ids), word for word at level 1. Above it: his lines that are
    /// corrections, made a standing record or still wait.
    pub his_words: Vec<String>,
    /// His lines above level 1 that were counted, not kept.
    pub his_counted: u32,
    /// What Theseus did and decided there.
    pub account: String,
    /// The marks of everything it was made from (marks only grow).
    pub marks: Marks,
    /// Opens the exchanges themselves.
    pub handle: Handle,
    pub writer: Writer,
}

/// The ladder's read for a place: its nodes, oldest first, and the window after the newest node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderRead {
    pub nodes: Vec<FoldNode>,
    /// Oldest first, as the window is sent.
    pub window: Vec<ExchangeRef>,
}

/// The nodes and window for a place, given its fold step `n` ([`FOLD_STEP`]; another is refused); `since` is
/// the place's newest `/new` mark (nothing before it is read). Implemented by the fold work.
pub trait Ladder {
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead>;
}

/// The fold writer: given the window that is due, write the node. Implemented by the fold work; it runs off
/// the turn's path.
pub trait FoldWriter {
    /// Fold `exchanges` (the oldest [`FOLD_STEP`] of the window, oldest first) of `place` into a first-level
    /// node.
    fn fold(&self, place: &str, exchanges: &[ExchangeRef]) -> anyhow::Result<FoldNode>;
    /// Build a node of `level` (2 and above) from [`FOLD_STEP`] nodes of the level below, by code.
    fn lift(&self, level: u8, nodes: &[FoldNode]) -> anyhow::Result<FoldNode>;
}

/// Whether a window of `len` exchanges is due a fold: at [`WINDOW_FLOOR`] + [`FOLD_STEP`], by count, never by
/// a judge.
pub fn fold_due(len: usize) -> bool {
    len >= WINDOW_FLOOR + FOLD_STEP
}
