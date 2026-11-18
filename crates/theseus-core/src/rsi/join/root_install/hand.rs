//! The one command that installs a joined change by hand, and the sentence that offers it (issue 20: "the first
//! install by the helper did not take"). The words of a failed install in the owner's notice and in `theseus self
//! pipeline` both end with it.

use std::path::Path;

use crate::rpc::Core;
use crate::rsi::join::{install, layout};

/// The sentence that offers the step, or says where the steps are when there is none to give.
pub(crate) fn hand_words(step: Option<&str>) -> String {
    match step {
        Some(s) => format!("To install it by hand, run: {s}"),
        None => "To install it by hand: docs/setup.md, \"A root-owned install\".".into(),
    }
}

impl Core {
    /// The command that installs `merge` by hand where the service ran `old_program`: what `layout::step` gives for the
    /// build this install made. None where this daemon has no repository to run it from.
    pub(crate) fn root_hand_step(&self, merge: &str, old_program: &str) -> Option<String> {
        let repo = self.cfg.self_improve.repo.clone()?;
        let place = self.tools.self_pr.place(&repo);
        Some(layout::step(
            Path::new(old_program),
            &install::target_of(&place, merge),
            &place.repo,
            merge,
        ))
    }
}
