//! A view turn that cannot be assembled soundly fails plainly (issue 44): no partial view, no other way of
//! building the request, and nothing is sent. Its words are a person's: the place could not be read, the
//! context could not be read (with the error), or what the turn may see kept changing while its call was
//! being prepared.

use super::super::{Failure, Turn, TurnRunner};

/// The class of a view turn's plain failure, in its `turn.failed` row.
pub(in crate::turn) const VIEW_FAILED_CLASS: &str = "context_unreadable";

/// Why a view turn failed before any call was sent.
#[derive(Debug)]
pub(in crate::turn) enum ViewFailed {
    /// The turn's place (where its message arrived, or where its reply is delivered) could not be read.
    Place,
    /// A read during the view's assembly failed: its words.
    Assembly(String),
    /// The gate's reading moved under the view a third time within one call's preparation.
    Moved,
    /// A turn no message opened continues work asked in a place it may not answer in now.
    ResumeHere,
}

impl std::fmt::Display for ViewFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Place => f.write_str("this conversation's place could not be read"),
            Self::Assembly(why) => write!(f, "the conversation's context could not be read: {why}"),
            Self::Moved => f.write_str(
                "what this turn may see changed while it was being prepared; send it again",
            ),
            Self::ResumeHere => f.write_str(
                "this turn's work cannot be resumed here: it was asked where this conversation does not answer now",
            ),
        }
    }
}

impl std::error::Error for ViewFailed {}

impl From<super::super::exchange_step::Unresumable> for ViewFailed {
    fn from(why: super::super::exchange_step::Unresumable) -> Self {
        use super::super::exchange_step::Unresumable;
        match why {
            Unresumable::Elsewhere => Self::ResumeHere,
        }
    }
}

impl ViewFailed {
    /// Its `why` in the `turn.failed` row's reason.
    fn why(&self) -> &'static str {
        match self {
            Self::Place => "place",
            Self::Assembly(_) => "assembly",
            Self::Moved => "moved",
            Self::ResumeHere => "resume_place",
        }
    }
}

impl TurnRunner {
    /// The turn's failure for `e` from a view compile: a [`ViewFailed`] is the turn's plain failure; any
    /// other error is returned as it was.
    pub(in crate::turn) fn view_failure(
        t: &mut Turn<'_>,
        e: anyhow::Error,
    ) -> anyhow::Result<Failure> {
        let Some(v) = e.downcast_ref::<ViewFailed>() else {
            return Err(e);
        };
        Ok(Self::view_failed(t, v))
    }

    /// The turn fails with `v`'s words: not transient (the same request would fail again).
    pub(in crate::turn) fn view_failed(t: &mut Turn<'_>, v: &ViewFailed) -> Failure {
        let words = v.to_string();
        tracing::warn!(turn_id = %t.tc.turn_id, why = v.why(), "{words}");
        Failure {
            class: VIEW_FAILED_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{VIEW_FAILED_CLASS} ({}): {words}", v.why()),
            source: anyhow::anyhow!("{words}"),
        }
    }
}

/// A test's seam: the sessions whose next view assembly meets a read failure.
#[cfg(test)]
static READ_FAULT: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Make the next view assembly of `session` fail on a read (a test's).
#[cfg(test)]
pub(crate) fn fail_next_assembly(session: &str) {
    READ_FAULT.lock().unwrap().push(session.into());
}

/// The read failure a test asked for `session`, once.
#[cfg(test)]
pub(super) fn injected(session: &str) -> anyhow::Result<()> {
    let mut faults = READ_FAULT.lock().unwrap();
    if let Some(i) = faults.iter().position(|s| s == session) {
        faults.remove(i);
        anyhow::bail!("a record of the conversation did not decode");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_says_its_own_words() {
        assert_eq!(
            ViewFailed::Place.to_string(),
            "this conversation's place could not be read"
        );
        assert_eq!(
            ViewFailed::Assembly("the store is closed".into()).to_string(),
            "the conversation's context could not be read: the store is closed"
        );
        assert!(ViewFailed::Moved.to_string().ends_with("send it again"));
    }
}
