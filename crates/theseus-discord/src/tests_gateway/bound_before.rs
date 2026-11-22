//! A channel a thread was bound to before threads were retired (issue 44),
//! which the bindings file does not name: after a restart on that store it
//! runs as an ordinary place, with the binding it had then.

use std::sync::Arc;

use serde_json::json;
use theseus_core::places::PlaceClass;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_protocol::threads::ThreadInfo;
use theseus_sim::fake_discord::FakeDiscord;

use super::{bindings, guild, Rig, ANA, BEN, CY};

/// A Discord thread the binding made for the thread `tides`, in no bindings
/// file.
const REEL: u64 = 900_000_000_000_000_412;

/// An old store's thread record, written as an older build wrote it, naming
/// `#reel` as its surface; and a post for `#reel` the last run left pending.
fn old_store(core: &Arc<theseus_core::Core>) {
    let rec: ThreadInfo = serde_json::from_value(json!({
        "slug": "tides", "title": "Tides", "state": "active",
        "surfaces": [format!("discord:channel:{REEL}")],
        "created_at_ms": 1, "last_active_ms": 1,
    }))
    .unwrap();
    core.store
        .put_meta(&theseus_core::threads::key("tides"), &rec)
        .unwrap();
    core.outbox
        .post(
            "sess_old",
            "",
            &format!("discord:channel:{REEL}"),
            json!({"kind": "notice", "text": "left over from the last run", "quiet": true}),
        )
        .unwrap();
}

/// After the restart: the pending post goes out in `#reel`; the owner's
/// message there is a turn in the channel's own session, answered there;
/// the place is private as it was, with no ceiling; a person who is no owner
/// starts nothing.
#[tokio::test]
async fn an_unlisted_channel_a_thread_was_bound_to_runs_as_itself_after_a_restart() {
    let fake = FakeDiscord::start_with_gateway();
    fake.set_guild(guild(false).private_channel(REEL, "reel", &[ANA, BEN, CY]));
    let r = Rig::start_seeded(
        fake,
        |_, _| -> Arc<dyn Provider> {
            Arc::new(FakeProvider::scripted(vec![Scripted::text("ready")]))
        },
        &bindings(true),
        &[],
        |_| {},
        old_store,
    )
    .await;
    r.until("the pending post in #reel", || {
        r.posted(REEL)
            .iter()
            .any(|m| m.content.contains("left over from the last run"))
    })
    .await;
    let target = format!("discord:channel:{REEL}");
    assert_eq!(
        r.core.runner.place_rule.class(&r.core.cfg, Some(&target)),
        PlaceClass::Private,
        "the place is private, as it was"
    );
    r.say(
        (ANA, "ana"),
        Some(REEL),
        "Reply with exactly one word: ready",
    );
    r.until("the reply in #reel", || {
        r.posted(REEL).iter().any(|m| m.content.contains("ready"))
    })
    .await;
    let session = r
        .core
        .outbox
        .place_session(&format!("channel:{REEL}"))
        .unwrap()
        .expect("the channel's own session");
    let rec: theseus_core::session::SessionRecord =
        r.core.store.get_session(&session).unwrap().unwrap();
    assert_eq!(
        rec.kind,
        theseus_protocol::SessionKind::Conversation,
        "an ordinary conversation"
    );
    // The place's label is the one the binding gave it then.
    let row = r
        .ledger("discord.message.in")
        .into_iter()
        .find(|row| row["place"] == "##Tides")
        .expect("ana's message, as the place's");
    assert_eq!(row["author"], "ana");
    // cy is no owner, so not one of the place's users.
    let turns = r.ledger("discord.message.in").len();
    r.say((CY, "cy"), Some(REEL), "Reply with exactly one word: ready");
    let cy = CY.to_string();
    r.until("cy's message ignored", || {
        r.ledger("discord.ignored")
            .iter()
            .any(|d| d["author_id"] == cy)
    })
    .await;
    assert_eq!(
        r.ledger("discord.message.in").len(),
        turns,
        "cy started nothing"
    );
    r.stop();
}
