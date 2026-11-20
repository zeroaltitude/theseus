//! The self loop's repair in place (issue 36; `crate::rsi::join::fix`): `self.fix.started`,
//! `self.fix.made`, `self.fix.refused`, `self.fix.none` and `self.fix.verdict`. Each carries its
//! `SELF_ROWS` fields (`item`, `branch`, `head`, `point` and its own) beside every `self.*` row's
//! `what`, `why`, `numbers` and `undo`, so `self.log` shows it as written, in the pipeline's
//! [`Moved`] shape.

use serde_json::Value;
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

pub use super::rsi_pipeline::Moved;
use super::{Fact, Say};

macro_rules! fixes {
    ($($(#[$doc:meta])* $name:ident => $kind:path),* $(,)?) => {
        $(
            $(#[$doc])*
            #[derive(Debug, Clone, PartialEq)]
            pub struct $name(pub Moved);

            impl Fact for $name {
                const KIND: Option<LedgerKind> = Some($kind);

                fn row(&self) -> Value {
                    self.0.row()
                }

                fn narrate(&self, say: &mut Say<'_>) {
                    say.line(Approval, sentence(&self.0));
                }
            }
        )*
    };
}

fixes! {
    /// A fault was given to a fixer.
    Started => LedgerKind::SelfFixStarted,
    /// The fixer's change, committed by Theseus.
    Made => LedgerKind::SelfFixMade,
    /// A bound refused the fix.
    Refused => LedgerKind::SelfFixRefused,
    /// No fix: the fixer would not, or the gate was red again.
    NoFix => LedgerKind::SelfFixNone,
    /// The builder's model accepted or rejected the fix.
    Verdict => LedgerKind::SelfFixVerdict,
}

fn sentence(m: &Moved) -> String {
    let mut c = m.what.chars();
    let what: String = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default();
    match &m.why {
        Some(w) => format!("Self fix: {what} ({w})."),
        None => format!("Self fix: {what}."),
    }
}
