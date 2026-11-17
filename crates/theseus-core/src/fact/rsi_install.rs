//! What a self-join's install found (issue 20): `self.install.not_installed` (the service runs a
//! build the loop cannot replace, so nothing was copied and nothing restarted),
//! `self.install.mismatch` (the service came back on another build than the one installed) and
//! `self.install.verified` (it came back on the build installed). Each carries its `SELF_ROWS`
//! fields beside `what`, `why`, `numbers` and `undo`, so `self.log` shows it as written.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A joined build that was not installed: the loop does not write where the service runs.
pub struct SelfInstallNotInstalled {
    pub merge: String,
    pub branch: String,
    /// Why, in the owner's words: the program, and whose directory it is in.
    pub why: String,
    /// Where the built programs wait.
    pub target: String,
    /// The one step that installs them.
    pub step: String,
}

impl Fact for SelfInstallNotInstalled {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfInstallNotInstalled);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "branch": self.branch, "target": self.target,
               "step": self.step,
               "what": format!("self branch {} joined main as {} and is not installed",
                   self.branch, short(&self.merge)),
               "why": self.why, "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Self-join of {} joined main as {} but is not installed: {}.",
                self.branch,
                short(&self.merge),
                self.why
            ),
        );
    }
}

/// An install whose service came back on another build than the one installed.
pub struct SelfInstallMismatch {
    pub merge: String,
    pub branch: String,
    /// The commit the service is running; none when the running build names none.
    pub running: Option<String>,
}

impl Fact for SelfInstallMismatch {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfInstallMismatch);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "branch": self.branch, "running": self.running,
               "what": format!("the install of {} came back running {}", short(&self.merge),
                   self.running.as_deref().map_or("a build with no commit", short)),
               "why": "the service restarted, and its build is not the one that was installed",
               "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "The install of {} did not take: the service is running {}.",
                short(&self.merge),
                self.running
                    .as_deref()
                    .map_or("a build with no commit", short)
            ),
        );
    }
}

/// An install whose service came back on the build installed.
pub struct SelfInstallVerified {
    pub merge: String,
    pub branch: String,
}

impl Fact for SelfInstallVerified {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfInstallVerified);

    fn row(&self) -> Value {
        json!({"merge": self.merge, "branch": self.branch,
               "what": format!("the service came back running {}, the build installed",
                   short(&self.merge)),
               "why": null, "numbers": null, "undo": null})
    }
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}
