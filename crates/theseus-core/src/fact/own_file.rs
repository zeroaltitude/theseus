//! The owner's own instruction file (`project_context::own_file`), and a
//! context file the settings name on the same records
//! (`project_context::held`): the file on disk differs from the version the
//! owner accepted, and an accept. A context file's row says so with one more
//! key, `"file": "context"`; the owner's own file's rows are as they were.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::{Approval, Context};

use super::{Fact, Say};
use crate::project_context::own_file::Whose;

/// The row's additive key for a context file's row; none for the owner's
/// own file.
fn mark(row: &mut Value, whose: Whose) {
    if whose == Whose::Context {
        row["file"] = json!("context");
    }
}

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
    pub whose: Whose,
}

impl Fact for OwnFileChanged<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnFileChanged);

    fn row(&self) -> Value {
        let mut row = json!({"path": self.path, "accepted": self.accepted, "disk": self.disk,
            "carried": "accepted"});
        mark(&mut row, self.whose);
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = if self.disk.is_empty() {
            "is gone or cannot be read"
        } else {
            "differs from the version you accepted"
        };
        let path = std::path::Path::new(self.path);
        let yours = match self.whose {
            Whose::Own => format!("your own instruction file {}", self.path),
            Whose::Context => format!("your context file {}", self.path),
        };
        say.line(
            Context,
            format!(
                "Context: {yours} {what}; the accepted version is carried until `{}`.",
                self.whose.accept(path)
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
    pub whose: Whose,
}

impl Fact for OwnFileAccepted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::OwnFileAccepted);

    fn row(&self) -> Value {
        let mut row = json!({"path": self.path, "digest": self.digest, "seq": self.seq,
            "bytes": self.bytes, "how": self.how, "by": self.by, "via": self.via});
        mark(&mut row, self.whose);
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = if self.digest.is_empty() {
            "its absence is accepted: nothing of it is carried".to_string()
        } else {
            format!("the {} B on disk are accepted and carried", self.bytes)
        };
        let how = match (self.how, self.whose) {
            (crate::project_context::own_file::HOW_FIRST_START, Whose::Own) => {
                ", once, at the first start with no version"
            }
            (crate::project_context::own_file::HOW_FIRST_START, Whose::Context) => {
                ", once, at the first start that names it"
            }
            _ => "",
        };
        let yours = match self.whose {
            Whose::Own => "Your own instruction file",
            Whose::Context => "Your context file",
        };
        say.line(Approval, format!("{yours} {}: {what}{how}.", self.path));
    }
}
