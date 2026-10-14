//! A compile's phases, timed for the debug log (theseus-0l3g): where a
//! compile's time goes, by phase, with the nodes it read and decoded, so a
//! long session's first turn says which part grows. Nothing reads them but
//! the log: `THESEUS_LOG=info,theseus_core::compiler::phases=debug`.

use std::time::Instant;

/// The phases of one compile, each with its microseconds.
pub(crate) struct Phases {
    what: &'static str,
    last: Instant,
    parts: Vec<(&'static str, u64)>,
    on: bool,
}

impl Phases {
    /// Phases of `what` (`compile`, `compile_step`), timed only while the
    /// debug log would show them.
    pub(crate) fn new(what: &'static str) -> Self {
        Phases {
            what,
            last: Instant::now(),
            parts: Vec::new(),
            on: tracing::enabled!(tracing::Level::DEBUG),
        }
    }

    /// The phase that ends now.
    pub(crate) fn mark(&mut self, name: &'static str) {
        if !self.on {
            return;
        }
        let now = Instant::now();
        self.parts
            .push((name, now.duration_since(self.last).as_micros() as u64));
        self.last = now;
    }

    /// Log the phases, with the nodes read and decoded.
    pub(crate) fn log(&self, nodes: &[(u64, crate::stub::Stub)], detail: &str) {
        if !self.on {
            return;
        }
        let parts: Vec<String> = self
            .parts
            .iter()
            .map(|(n, us)| format!("{n} {:.1} ms", *us as f64 / 1000.0))
            .collect();
        tracing::debug!(
            nodes = nodes.len(),
            decoded = crate::stub::read(nodes),
            "{}: {}{}",
            self.what,
            parts.join(", "),
            detail
        );
    }
}
