//! Recognition for the context design (issue 44, step 5's first half; the
//! design note is `docs/design/names.md`): what a person's words and the
//! judge's filing name, as things a later selector can call in. Nothing here
//! is sent to a model, and nothing here decides who may act.
//!
//! - **Topics per exchange** ([`topics`]): when `onto.v1`'s answer for a
//!   person's message lands, each category it chose at the act band touches
//!   that message's exchange, so a topic has a timeline like any other thing.
//!   The judge's own filing stays per session, as it was.
//! - **Numbered items** ([`items`]): the index's extractor names "issue 33",
//!   "#33" and "PR 12" (`index.items`); a bare number is scoped by the
//!   project of the place it was said in when one is known, and is otherwise
//!   kept as an unscoped candidate. They touch the exchange as the other
//!   things do.

pub mod items;
pub mod topics;
