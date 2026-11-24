//! The owner's own instruction file at the daemon's level
//! (`project_context::own_file`): the first start's accept, and the line
//! `theseus health` shows while the file on disk is not the accepted one.

use anyhow::Result;

use super::Core;
use crate::fact::own_file::OwnFileAccepted;
use crate::project_context::own_file::{self, Look, Version};

impl Core {
    /// The first start's accept, once the socket answers: what is on disk,
    /// or its absence, when this store has never accepted a version
    /// (`own_file::first_start`). One frame: the version, its pointer and
    /// its `own_file.accepted` row; and one line in the daemon's log. Never
    /// again by itself: a later start finds the pointer and does nothing,
    /// and a pointer it cannot read is not "none".
    pub fn own_file_first_start(&self) -> Option<Version> {
        let look = Look::of(&self.cfg, &self.store, &self.runner.context_files);
        let v = own_file::first_start(&look, &self.store, theseus_protocol::now_unix_ms())?;
        match self.own_file_keep(&look, &v) {
            Ok(seq) => {
                tracing::info!(path = %look.path.display(), bytes = v.bytes, seq,
                    present = v.digest.is_some(),
                    "your own instruction file: what is on disk is accepted, once, at this first start; from now on a change to it waits for `theseus own-file accept`");
                Some(v)
            }
            Err(e) => {
                tracing::warn!(path = %look.path.display(), error = %e,
                    "your own instruction file: the first start's accept was not written, and nothing of it is carried");
                None
            }
        }
    }

    /// [`Core::own_file_first_start`] off the runtime's workers: it reads
    /// the file and writes one frame.
    pub fn own_file_after_serving(&self) {
        theseus_store::blocking(|| self.own_file_first_start());
    }

    /// `v` accepted for the file `look` read: its records and its row, in
    /// one frame. The version's number.
    pub(super) fn own_file_keep(&self, look: &Look, v: &Version) -> Result<u64> {
        let seq = look.next_seq()?;
        let mut records = look.accept_records(v)?;
        let path = look.path.display().to_string();
        let fact = OwnFileAccepted {
            path: &path,
            digest: v.digest.as_deref().unwrap_or_default(),
            seq,
            bytes: v.bytes,
            how: &v.how,
            by: &v.by,
            via: &v.via,
        };
        let rec = self.rec(None);
        records.push(rec.row(&fact)?);
        self.store.append(&records)?;
        rec.announce(&fact);
        Ok(seq)
    }

    /// `theseus health`'s line about the file (`Look::health`).
    pub(super) fn own_file_health(&self) -> Option<String> {
        Look::of(&self.cfg, &self.store, &self.runner.context_files)
            .health(&self.store, theseus_protocol::now_unix_ms())
    }
}
