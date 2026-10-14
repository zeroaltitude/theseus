//! The seals' facts (theseus-f2g0): a thread sealed, unsealed, a seal
//! proposed by the core (Jev recommends, the owner seals), and a proposal the
//! owner answered "not needed", which a later Jev pack's learning report
//! reads as a label. The rows hold the slug, who, why and counts, never the
//! thread's words.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// What a seal's move says of itself.
pub struct SealChange<'a> {
    pub slug: &'a str,
    pub who: &'a str,
    pub via: &'a str,
    /// The reason sealed, or proposed, or dismissed.
    pub reason: &'a str,
    /// Sessions the thread's notes had already crossed into (sealing).
    pub crossed: usize,
    /// A digest of the thread's content, for a dismissal: the proposal is not
    /// made again until it changes.
    pub digest: &'a str,
}

impl SealChange<'_> {
    fn data(&self) -> Value {
        json!({"slug": self.slug, "who": self.who, "via": self.via,
               "reason": self.reason, "crossed": self.crossed, "digest": self.digest})
    }
}

macro_rules! seal_fact {
    ($(#[$doc:meta])* $name:ident, $kind:path, $line:literal) => {
        $(#[$doc])*
        pub struct $name<'a>(pub SealChange<'a>);

        impl Fact for $name<'_> {
            const KIND: Option<LedgerKind> = Some($kind);

            fn row(&self) -> Value {
                self.0.data()
            }

            fn narrate(&self, say: &mut Say<'_>) {
                say.line(
                    Session,
                    format!($line, who = self.0.who, slug = self.0.slug, reason = self.0.reason),
                );
            }
        }
    };
}

seal_fact!(
    /// A thread was sealed (`thread.sealed`).
    ThreadSealed,
    LedgerKind::ThreadSealed,
    "Thread: {who} sealed the thread {slug} ({reason}); its notes never come into another thread."
);
seal_fact!(
    /// A sealed thread was opened again (`thread.unsealed`).
    ThreadUnsealed,
    LedgerKind::ThreadUnsealed,
    "Thread: {who} unsealed the thread {slug} ({reason})."
);
seal_fact!(
    /// A seal was proposed (`thread.seal_proposed`); the thread is held out of
    /// other threads' recall until the owner answers.
    ThreadSealProposed,
    LedgerKind::ThreadSealProposed,
    "Thread: {who} proposed sealing the thread {slug} ({reason}); it is held out of other threads' recall until the owner answers."
);
seal_fact!(
    /// The owner answered a proposed seal "not needed" (`thread.seal_dismissed`):
    /// a label for the judge's learning report.
    ThreadSealDismissed,
    LedgerKind::ThreadSealDismissed,
    "Thread: {who} said sealing the thread {slug} is not needed ({reason})."
);
