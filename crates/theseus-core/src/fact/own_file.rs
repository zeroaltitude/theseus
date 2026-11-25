//! The owner's own instruction file (`project_context::own_file`): the file
//! on disk differs from the version the owner accepted, and an accept.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::{Approval, Context};

use super::{Fact, Say};

/// `own_file.changed`: the file on disk is not the version the owner
/// accepted, and the accepted one is what is carried. One row per disk
/// digest, riding the turn's next frame with the record that it was said.
/// Digests, never text.
pub struct OwnFileChanged<'a> {
    pub path: &'a str,
    /// The accepted version's digest; empty: it was accepted as absent.
    pub accepted: &'a str,
    /// The disk's digest; empty: no file, or one the reader cannot read.
    pub disk: &'a str,
}

impl Fact for OwnFileChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnFileChanged);

    fn row(&self) -> Value {
        json!({"path": self.path, "accepted": self.accepted, "disk": self.disk,
            "carried": "accepted"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = if self.disk.is_empty() {
            "is gone or cannot be read"
        } else {
            "differs from the version you accepted"
        };
        say.line(
            Context,
            format!(
                "Context: your own instruction file {} {what}; the accepted version is carried \
                 until `{}`.",
                self.path,
                theseus_protocol::own_file::accept_command()
            ),
        );
    }
}

/// `own_file.accepted`: a version of the file is accepted, by the owner's
/// act or by the daemon's first start with none.
pub struct OwnFileAccepted<'a> {
    pub path: &'a str,
    /// Its digest; empty: the file was accepted as absent.
    pub digest: &'a str,
    pub seq: u64,
    pub bytes: u64,
    /// `accept` or `first_start`.
    pub how: &'a str,
    pub by: &'a str,
    pub via: &'a str,
}

impl Fact for OwnFileAccepted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnFileAccepted);

    fn row(&self) -> Value {
        json!({"path": self.path, "digest": self.digest, "seq": self.seq,
            "bytes": self.bytes, "how": self.how, "by": self.by, "via": self.via})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = if self.digest.is_empty() {
            "its absence is accepted: nothing of it is carried".to_string()
        } else {
            format!("the {} B on disk are accepted and carried", self.bytes)
        };
        let how = if self.how == crate::project_context::own_file::HOW_FIRST_START {
            ", once, at the first start with no version"
        } else {
            ""
        };
        say.line(
            Approval,
            format!("Your own instruction file {}: {what}{how}.", self.path),
        );
    }
}
