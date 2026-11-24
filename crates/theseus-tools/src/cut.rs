//! Where a plan may cut text it puts in its summary. The runtime sets a
//! call's cut ([`ToolCtx::cut`](crate::ToolCtx)) so that it never falls
//! inside a value the runtime knows. It can only place a cut in text the plan
//! already holds: it returns a piece of that text and nothing else.

use std::sync::Arc;

/// Places a cut in text: a head, or the first line.
pub trait Cut: Send + Sync {
    /// `text`'s first `max` characters, ending earlier where a cut there
    /// would fall inside a known value, and whether anything was cut.
    fn head<'t>(&self, text: &'t str, max: usize) -> (&'t str, bool);

    /// The text's first line that is not blank, trimmed: whole when it has
    /// at most `max` characters, else a head of at most `keep` characters
    /// that ends earlier as [`Cut::head`] does; and whether it was cut.
    fn first_line<'t>(&self, text: &'t str, max: usize, keep: usize) -> (&'t str, bool);
}

/// The cut where it is asked: it knows no value.
pub struct Plain;

impl Cut for Plain {
    fn head<'t>(&self, text: &'t str, max: usize) -> (&'t str, bool) {
        match text.char_indices().nth(max) {
            None => (text, false),
            Some((at, _)) => (&text[..at], true),
        }
    }

    fn first_line<'t>(&self, text: &'t str, max: usize, keep: usize) -> (&'t str, bool) {
        let Some(line) = text.lines().find(|l| !l.trim().is_empty()) else {
            return ("", false);
        };
        let line = line.trim();
        if line.chars().count() <= max {
            return (line, false);
        }
        match line.char_indices().nth(keep) {
            None => (line, false),
            Some((at, _)) => (&line[..at], true),
        }
    }
}

/// A call's cut: the one the runtime set, or [`Plain`].
#[derive(Clone, Default)]
pub struct Cutter(Option<Arc<dyn Cut>>);

impl Cutter {
    pub fn new(cut: Arc<dyn Cut>) -> Self {
        Self(Some(cut))
    }

    pub fn get(&self) -> &dyn Cut {
        self.0.as_deref().unwrap_or(&Plain)
    }
}

impl std::fmt::Debug for Cutter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Some(_) => "Cutter(set)",
            None => "Cutter(plain)",
        })
    }
}
