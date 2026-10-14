//! The sealed holdout sets' fact (theseus-pw1q.5): `self.exam.counted`, an
//! exam item counted after its independent review. A sealed item's row
//! names its opaque id and family and never any text of it.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// An exam item counted. `sealed` rows carry the item id alone.
pub struct SelfExamCounted<'a> {
    pub item: &'a str,
    pub family: &'a str,
    pub sealed: bool,
    pub reviewer: &'a str,
}

impl Fact for SelfExamCounted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfExamCounted);

    fn row(&self) -> Value {
        json!({"item": self.item, "family": self.family, "sealed": self.sealed,
               "reviewer": self.reviewer,
               "what": format!("exam item {} counted ({}{})", self.item,
                   if self.sealed { "sealed, " } else { "" }, self.family),
               "why": null, "numbers": null, "undo": null})
    }
}
