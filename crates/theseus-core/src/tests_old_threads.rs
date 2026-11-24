//! Thread records as an older store holds them. Threads were retired on 2026-11-22 (issue 44) and nothing
//! writes one any more, so the tests of what still reads them (the board, the card, the place digest,
//! recall) write the old records here, literally: a `thread:<slug>` META record and the place
//! binding of the session the thread ran on.

use serde_json::{json, Value};
use theseus_protocol::SessionOpenParams;
use theseus_store::{kinds, NewRecord};

use crate::rpc::Core;
use crate::session::SessionRecord;

/// The record `thread:<slug>`, as a `ThreadInfo` was stored: `slug`, `title` and an active state, with
/// `more`'s fields over them (a `ThreadInfo`'s names: `goal`, `card`, `working_set`, `sealed`,
/// `seal_pending`, `state`, ...). The board is read again at once.
pub(crate) fn thread(core: &Core, slug: &str, title: &str, more: Value) {
    let now = theseus_protocol::now_unix_ms();
    let mut rec = json!({
        "slug": slug,
        "title": title,
        "state": "active",
        "last_active_ms": now,
        "created_at_ms": now,
    });
    if let (Some(r), Some(m)) = (rec.as_object_mut(), more.as_object()) {
        for (k, v) in m {
            r.insert(k.clone(), v.clone());
        }
    }
    let r = NewRecord::json(kinds::META, Some(&crate::threads::key(slug)), &rec).unwrap();
    core.store.append(&[r]).unwrap();
    reread(core);
}

/// The session the thread `slug`'s place runs on, as the old first message to a thread left it: the
/// current one while it is not retired; else a new one, opened with its place's binding in one frame the
/// first time and superseding the retired one after. The board is read again at once.
pub(crate) fn session(core: &Core, slug: &str) -> String {
    let place = crate::threads::place(slug);
    let current = core
        .threads()
        .get(slug)
        .and_then(|t| t.session_id.clone())
        .and_then(|s| core.store.get_session::<SessionRecord>(&s).ok().flatten())
        .filter(|r| r.retired.is_none());
    if let Some(rec) = current {
        return rec.session_id;
    }
    let params = SessionOpenParams {
        label: Some(place.clone()),
        ..SessionOpenParams::default()
    };
    let sid = match core.outbox.place_session(&place).unwrap() {
        None => {
            let operator = theseus_kernel::Authority {
                principal: crate::turn::OPERATOR.to_string(),
                ..Default::default()
            };
            let place_rec = |r: &SessionRecord| -> anyhow::Result<Vec<NewRecord>> {
                Ok(vec![core.outbox.place_record(&place, &r.session_id)?])
            };
            let rec = core
                .open_session_with(params, (operator, None), &place_rec)
                .unwrap();
            core.outbox.place_bound(&place, &rec.session_id).unwrap();
            rec.session_id
        }
        Some(_) => {
            let rec = core.open_session(params).unwrap();
            core.bind_place_to(&place, &rec.session_id).unwrap();
            rec.session_id
        }
    };
    reread(core);
    sid
}

/// The board read again from the store, as a writer once left it held.
fn reread(core: &Core) {
    core.runner.threads.forget();
    core.threads();
}
