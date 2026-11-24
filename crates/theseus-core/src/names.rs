//! Recognition for the context design (issue 44, step 5's first half; the
//! design note is `docs/design/names.md`): what a person's words and the
//! judge's filing name, as things a later selector can call in. Nothing here
//! is sent to a model by this module (the view's sources lay a thing's card,
//! `crate::view::sources_names`), and nothing here decides who may act.
//!
//! - **Topics per exchange** ([`topics`]): when `onto.v1`'s answer for a
//!   person's message lands, each category it chose at the act band touches
//!   that message's exchange, so a topic has a timeline like any other thing.
//!   The judge's own filing stays per session, as it was.
//! - **Picture labels per exchange** ([`pictures`]): the same pack's answers
//!   about the pictures a person's message carried, at the act band, touch
//!   that message's exchange under `picture:<subject>` and, for a yes,
//!   `picture:shows_screen_or_document`.
//! - **Numbered items** ([`items`]): the index's extractor names "issue 33",
//!   "#33" and "PR 12" (`index.items`); a bare number is scoped by the
//!   project of the place it was said in when one is known, and is otherwise
//!   kept as an unscoped candidate. They touch the exchange as the other
//!   things do.
//! - **Scoped names** ([`table`]): the names and aliases of lines of work,
//!   tasks and topics, from the stores that hold them, and a matcher of a
//!   person's words against them: a qualified name wins, an unqualified alias
//!   only when one live thing holds it, and otherwise every match is a
//!   candidate.
//! - **A thing's card** ([`card`]): its title, and under it at most 12 lines
//!   of what it is, its current facts as the caller's gate admits them, its
//!   open tasks, the owner's guidance for it and its newest exchanges, as one
//!   read.

pub mod card;
pub mod items;
pub mod pictures;
pub mod table;
pub mod topics;
