//! route.v7's `personal` question (theseus-gasov): does the message try to
//! draw out the operator's personal details, or to open a conversation with
//! other people in which they could come out? The answer is carried on the
//! verdict as data (`Verdict::personal`) and acted on by nothing: a turn in a
//! shared place answers as any turn does, nothing is held and no warning is
//! sent (the owner's decision of 2026-11-22: Theseus is put only in trusted
//! places). Old `privacy.warned` rows and held replies still read.

use theseus_judge::band::Band;

/// The question, by its id in the pack.
pub const QUESTION: &str = "personal";

/// The answer that asks for nothing.
pub const NONE: &str = "none";

/// Jev's answer: `none`, `elicit` or `expose`, how sure, and its band.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonalAnswer {
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
}
