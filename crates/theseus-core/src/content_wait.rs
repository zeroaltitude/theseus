//! A session's content labels as the store holds them (theseus-9ymz0): the
//! labels themselves, and the mark the wire shows of them. No turn waits for
//! a verdict: the floor that did was deleted on 2026-11-21.

use crate::content::ContentFlags;

/// The session's content labels as the store holds them, or none: a session
/// with no record, or with no labels, reads as none. One record read.
pub fn flags_of(store: &crate::store::Store, session: &str) -> Option<ContentFlags> {
    let rec = store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()??;
    rec.data.map(|b| *b)
}

/// What the wire shows of `session`'s data, read from its labels
/// (`ContentFlags::mark`): `open` for a session with none or none that reads.
pub fn mark_of(store: &crate::store::Store, session: &str) -> theseus_protocol::ops::DataMark {
    try_mark_of(store, session).unwrap_or(theseus_protocol::ops::DataMark::Sensitive)
}

/// [`mark_of`], failing where it would hold the session at the top: a session
/// record the store cannot read is an error. A session with no record is open.
pub fn try_mark_of(
    store: &crate::store::Store,
    session: &str,
) -> anyhow::Result<theseus_protocol::ops::DataMark> {
    let rec = store.get_session::<crate::session::SessionRecord>(session)?;
    Ok(rec
        .and_then(|r| r.data)
        .map_or(theseus_protocol::ops::DataMark::Open, |d| d.mark()))
}
