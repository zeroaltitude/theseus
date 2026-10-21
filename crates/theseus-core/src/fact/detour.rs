//! A detour's request (theseus-r5ydi): the compile of a turn the router sent
//! to a cheaper profile is never stored, so no `context.compiled` row names
//! what its model was shown. This row does, in names and counts, so
//! `context.explain` (and `theseus why`) can say it.

use serde_json::Value;
use theseus_protocol::context::ContextDetoured;
use theseus_protocol::LedgerKind;

use super::Fact;

/// `context.detoured`: the parts of one detoured loop's request. It rides the
/// frame of the loop's `loop.started` row.
pub struct Detoured<'a> {
    pub request: &'a ContextDetoured,
}

impl Fact for Detoured<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ContextDetoured);

    fn row(&self) -> Value {
        serde_json::to_value(self.request).unwrap_or(Value::Null)
    }
}
