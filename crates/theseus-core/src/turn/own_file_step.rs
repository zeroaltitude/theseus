//! The owner's own instruction file, and the context files the settings
//! name, the turn's half (`project_context::own_file`,
//! `project_context::held`): the block carries the accepted version and
//! names a difference; this writes the difference's row, once per disk
//! digest of each file.

use super::{Turn, TurnRunner};
use crate::fact::own_file::OwnFileChanged;
use crate::project_context::held;
use crate::project_context::own_file::{Accepted, Look, Noticed, Whose};

impl TurnRunner {
    /// One `own_file.changed` row when a file on disk differs from its
    /// accepted version and this digest has not been ledgered yet, and the
    /// record that it has, in the turn's next frame: the owner's own file,
    /// then each context file. The block (`project_context::block`) reads
    /// and writes nothing of this: every caller of it sees the same answer,
    /// and the turn alone writes.
    ///
    /// Nothing without tools, where block 3 is empty.
    pub(super) fn observe_own_file(&self, t: &mut Turn<'_>) {
        if !self.tools.enabled() {
            return;
        }
        let own = Look::of(&self.cfg, &self.store, &self.context_files);
        self.observe_look(t, &own, Whose::Own);
        for path in held::paths(&self.cfg) {
            let look = held::look(&self.store, &self.context_files, &path);
            self.observe_look(t, &look, Whose::Context);
        }
    }

    /// [`Self::observe_own_file`] for the one file `look` read.
    fn observe_look(&self, t: &mut Turn<'_>, look: &Look, whose: Whose) {
        let Accepted::Some(n) = &look.accepted else {
            return;
        };
        if !look.differs()
            || Noticed::read(&self.store, &look.pid).is_some_and(|s| s.digest == look.disk.digest)
        {
            return;
        }
        let path = look.path.display().to_string();
        t.record(&OwnFileChanged {
            path: &path,
            accepted: n.digest.as_deref().unwrap_or_default(),
            disk: look.disk.digest.as_deref().unwrap_or_default(),
            whose,
        });
        let noticed = Noticed {
            digest: look.disk.digest.clone(),
            at_ms: theseus_protocol::now_unix_ms(),
        };
        match noticed.record(&look.pid) {
            Ok(r) => {
                if let Err(e) = t.tc.store.defer(r) {
                    tracing::warn!(error = %e, "the own file's notice was not stored");
                }
            }
            Err(e) => tracing::warn!(error = %e, "the own file's notice cannot be encoded"),
        }
    }
}
