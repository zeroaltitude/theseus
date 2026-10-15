//! How full the context is (theseus-wy6v.2): the last loop's request in
//! tokens, and the window it compiled against. Kept as the loop compiles, no
//! read of its own, and carried in the turn's result so a status line is a
//! field read.

use super::TurnRunner;
use crate::compiler::{Compiled, RequestSpec};

/// The last compile's size and window; none before the turn's first compile.
#[derive(Default)]
pub(super) struct Fill {
    pub tokens: Option<u64>,
    pub window: Option<u64>,
}

impl Fill {
    /// What the compile of this loop made, against the window it was made for.
    pub(super) fn of(compiled: &Compiled, window: Option<u64>) -> Self {
        Fill {
            tokens: Some(compiled.est_tokens),
            window,
        }
    }
}

impl TurnRunner {
    /// The window `spec`'s request compiles against: the working window if
    /// one is set, else the catalog's.
    pub(super) fn fill_window(&self, spec: &RequestSpec) -> Option<u64> {
        self.working_window(spec)
            .or_else(|| self.catalog.get(&spec.model).map(|e| e.context_window))
    }
}
