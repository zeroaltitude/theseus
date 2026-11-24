//! The owner's own instruction file, the turn's half
//! (`project_context::own_file`): the block carries the accepted version and
//! names a difference; this writes the difference's row, once per disk
//! digest.

use super::{Turn, TurnRunner};
use crate::fact::own_file::OwnFileChanged;
use crate::project_context::own_file::{Accepted, Look, Noticed};

impl TurnRunner {
    /// One `own_file.changed` row when the file on disk differs from the
    /// accepted version and this digest has not been ledgered yet, and the
    /// record that it has, in the turn's next frame. The block
    /// (`project_context::block`) reads and writes nothing of this: every
    /// caller of it sees the same answer, and the turn alone writes.
    ///
    /// Nothing without tools, where block 3 is empty.
    pub(super) fn observe_own_file(&self, t: &mut Turn<'_>) {
        if !self.tools.enabled() {
            return;
        }
        let look = Look::of(&self.cfg, &self.store, &self.context_files);
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
