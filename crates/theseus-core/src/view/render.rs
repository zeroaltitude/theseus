//! The renderer's seam: a view to the parts of a request, in the order on the wire (design §5.1). What is
//! fixed goes first and what moves goes last; parts 1 to 4 are the same bytes from one turn to the next and
//! change only at a fold, a clearing, a day's turn, or a changed fact about him.
//!
//! 1. [`RequestParts::system`]: slot A, with the memory page.
//! 2. [`RequestParts::fixed`]: B, then E older than two days. Marked blocks.
//! 3. [`RequestParts::ladder`]: G's ladder, oldest node first. Marked blocks.
//! 4. [`RequestParts::window`]: G's window as real turns, in order, tool calls paired with their results.
//! 5. [`RequestParts::beside`]: one marked block never stored (C, D, F, H), then [`RequestParts::message`].
//!
//! Implemented by the renderer work, with the strategy that `[context] assembly = "view"` selects.

use serde::{Deserialize, Serialize};

use super::budget::View;
use super::{Handle, Label, Snapshot, SourceRange};

/// One marked block: every piece in it is headed with its label (place, date and reason where it is not
/// from here); outside text is wrapped wherever it is rendered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkedBlock {
    pub label: Label,
    pub heading: String,
    pub text: String,
    pub sources: Vec<SourceRange>,
    pub handle: Option<Handle>,
}

/// Who a turn of the window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Person,
    Theseus,
    ToolCall,
    ToolResult,
}

/// One real turn of the window. A tool call and its result are adjacent and never split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowTurn {
    pub role: Role,
    /// `Here`, or `OutsideText` for a result from outside (wrapped).
    pub label: Label,
    pub source: SourceRange,
    pub text: String,
}

/// A request's parts, in the order on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RequestParts {
    pub system: String,
    /// The memory page's version (fixed text, held by a test).
    pub memory_page: u32,
    pub fixed: Vec<MarkedBlock>,
    pub ladder: Vec<MarkedBlock>,
    pub window: Vec<WindowTurn>,
    pub beside: Option<MarkedBlock>,
    pub message: String,
}

/// `view -> request parts`.
pub trait Renderer {
    /// The memory page: fixed, versioned text in the system part.
    fn memory_page(&self) -> (u32, &'static str);
    /// Read the pieces' ranges from the snapshot and lay them out in wire order.
    fn render(&self, view: &View, snap: &dyn Snapshot) -> anyhow::Result<RequestParts>;
    /// The nodes `view`'s shown exchanges lay with their words (`sources_recall::carries`): a recalled
    /// passage among them is not laid again (`budget::set_aside`). None, for a renderer that reads none.
    fn carried(&self, _view: &View) -> anyhow::Result<std::collections::BTreeSet<String>> {
        Ok(std::collections::BTreeSet::new())
    }
}
