//! What each background key named before Jev chose the model (route.v10,
//! theseus-ej6bj). An unset key asks Jev; while the acting route pack asks no
//! `model` question (every pack before route.v10), an unset key runs on the
//! profile it always did, so installing this changes no live decision. The
//! constants go with the pack that does not ask (the row that flips
//! `ROUTE_PACK`, then the one that deletes the old routing).

/// `[card] profile`, `[people] extract_profile`, `[threads] import_profile`,
/// `[ontology] name_profile`.
pub const CHEAP: &str = "haiku";
/// `[judge.learn] writer_profile`.
pub const LEARN_WRITER: &str = "opus";
/// `[self] reviewer_profile`.
pub const SELF_REVIEWER: &str = "fable";
/// `[memory] summary_profile` and `synth_profile`: the profile the turn, or
/// every source's session, last ran on (`config::memory::SUMMARY_SESSION`).
pub const SESSION: &str = super::memory::SUMMARY_SESSION;
