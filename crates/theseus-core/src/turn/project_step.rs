//! What a compile saw of a granted repository's instruction files (§8,
//! "After a grant"). See `crate::project_context::change`, which decides it;
//! this is the turn's half, which writes the row and the sighting.

use super::{Turn, TurnRunner};
use crate::ceiling::PlaceView;
use crate::fact::project::ProjectChanged;
use crate::places::PlaceClass;

impl TurnRunner {
    /// Compare what this compile reads of a granted repository's instruction
    /// files with what the last one saw, and write a `project.changed` row
    /// for each difference, plus the sighting that now stands, in the turn's
    /// next frame. A first sighting stores the state and says nothing, as a
    /// persona's does (`observe_persona`, whose shape this is).
    ///
    /// The block's half is `project_context::block`, which named the same
    /// changes in the text the model reads. Both come from one
    /// `project_context::change::observe` call on the same inputs, so they
    /// cannot disagree -- `flagged` included, which is why the turn reads
    /// the session's label once and hands the same answer to both rather
    /// than each reading the record for itself. While a change is held what
    /// the owner read does not move forward -- that is what makes the
    /// withheld file keep waiting instead of arriving by itself on the next
    /// turn -- and the row says the text was not carried.
    ///
    /// A change is held at `ask` posture, **or** when the session carries
    /// `injection` (`flagged`, S31-1), which holds it whatever the posture
    /// says, **or** when the writer key stored a hold for the repository
    /// (`change::Held`, S31-1) -- which holds it for this session whether or
    /// not this session carries anything, because the session it is about is
    /// the one whose call moved the file. The row names that session, so the
    /// ledger says whose call a withheld file waits on and not merely that
    /// it waits.
    ///
    /// Nothing at all in a shared place or without tools, where block 3 is
    /// empty and no repository's file was read in the first place.
    pub(super) fn observe_project(
        &self,
        t: &mut Turn<'_>,
        dir: Option<&str>,
        place: PlaceView,
        flagged: bool,
    ) {
        if place.class == PlaceClass::Shared || !self.tools.enabled() {
            return;
        }
        let cwd = self.tools.cwd_for(dir);
        let found = crate::project_context::discover(&self.cfg.state_dir(), &cwd);
        let Some(root) = found.root.as_deref() else {
            return;
        };
        if found.project.is_empty() || !crate::project_context::trust::granted(&self.store, root) {
            return;
        }
        let seen = crate::project_context::change::observe(
            &self.cfg,
            &self.store,
            &self.context_files,
            root,
            &found.project,
            flagged,
        );
        for c in &seen.fresh {
            t.record(&ProjectChanged {
                id: &seen.id,
                root: &seen.root,
                path: &c.path,
                old: &c.old,
                new: &c.new,
                carried: !seen.withholds(&c.path),
                wrote: seen.held.as_ref().map(|h| h.by.as_str()),
            });
        }
        let Some(write) = &seen.write else { return };
        match write.record(&seen.id) {
            Ok(r) => {
                if let Err(e) = t.tc.store.defer(r) {
                    tracing::warn!(repo = %seen.id, error = %e,
                        "a granted repository's sighting was not stored");
                }
            }
            Err(e) => tracing::warn!(repo = %seen.id, error = %e,
                "a granted repository's sighting cannot be encoded"),
        }
    }
}
