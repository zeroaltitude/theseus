//! The thread records an older store holds, after the read-only `thread.list` and `thread.show` were
//! removed (issue 44; threads were retired 2026-11-22): every `thread.*` method is refused, the records
//! stay as they were, and the board the daemon still reads them into holds each one whole.

use std::sync::Arc;

use theseus_protocol::threads::ThreadState;

use super::tests::test_core;
use super::Core;
use crate::places::{BoundPlace, PlaceClass};

const LAB: u64 = 314_159_265_358_979_323;

fn core() -> Arc<Core> {
    test_core("the tide is out")
}

/// A stored thread record, as an old store holds it.
fn stored(core: &Core, slug: &str) {
    let title = format!("Title of {slug}");
    crate::tests_old_threads::thread(core, slug, &title, serde_json::json!({}));
}

/// `thread.list` and `thread.show` are gone (issue 44): over the wire each is refused, saying threads were
/// retired, as every other `thread.*` method is.
#[tokio::test]
async fn thread_list_and_show_are_refused_over_the_wire() {
    let core = core();
    stored(&core, "tides");
    for m in ["thread.list", "thread.show"] {
        let req = theseus_protocol::Request::new(
            theseus_protocol::Id::Num(1),
            m,
            serde_json::json!({"slug": "tides"}),
        );
        let got = super::tests::roundtrip(core.clone(), vec![req]).await;
        let answer = got
            .into_iter()
            .find_map(|msg| match msg {
                theseus_protocol::Message::Response(r) => Some(serde_json::to_value(r).unwrap()),
                _ => None,
            })
            .expect("an answer");
        assert_eq!(
            answer["error"]["code"],
            theseus_protocol::error_code::REFUSED,
            "{m}: {answer}"
        );
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|w| w.contains("threads were retired")),
            "{m}: {answer}"
        );
    }
}

#[test]
fn a_shared_place_can_neither_list_nor_show_a_thread() {
    use theseus_protocol::error_code::REFUSED;
    let c = core();
    c.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    stored(&c, "tides");
    let refused = |e: super::server::RpcFailure| assert_eq!(e.code, REFUSED, "{}", e.message);
    refused(Core::rpc_threads("thread.list").unwrap_err());
    refused(Core::rpc_threads("thread.show").unwrap_err());
    assert!(
        c.outbox.place_session("thread:tides").unwrap().is_none(),
        "no place was bound"
    );
    assert_eq!(c.threads().get("tides").unwrap().state, ThreadState::Active);
    // A thread's places are private always.
    let class = |t: &str| c.runner.place_rule.class(&c.cfg, Some(t));
    assert_eq!(class("thread:tides"), PlaceClass::Private);
    assert_eq!(class("discord:thread:tides"), PlaceClass::Private);
    assert_eq!(class(&format!("discord:channel:{LAB}")), PlaceClass::Shared);
    let board = c.threads();
    assert!(board.live(PlaceClass::Shared).is_empty());
    assert_eq!(board.live(PlaceClass::Private).len(), 1);
}

/// Every stored record under the threads' two META prefixes (the records and their places' bindings), as raw
/// bytes, by key: what a read must leave exactly as it found.
fn raw_threads(core: &Core) -> Vec<(Option<String>, u64, Vec<u8>)> {
    use theseus_store::Store as _;
    let mut out = Vec::new();
    for prefix in [
        crate::threads::PREFIX.to_string(),
        format!(
            "{}{}",
            crate::outbox::PLACE_META_PREFIX,
            crate::threads::PREFIX
        ),
    ] {
        for r in core
            .store
            .inner()
            .latest_with_prefix(theseus_store::kinds::META, &prefix)
            .unwrap()
        {
            out.push((r.key.clone(), r.position, r.payload.clone()));
        }
    }
    out.sort();
    out
}

/// A turn submitted by session id, as the CLI and the TUI send one: its answer.
async fn submit_to(core: &Arc<Core>, sid: &str, input: &str) -> serde_json::Value {
    use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams};
    let req = Request::new(
        Id::Num(1),
        method::TURN_SUBMIT,
        TurnSubmitParams {
            session_id: Some(sid.to_string()),
            input: input.into(),
            ..Default::default()
        },
    );
    let got = super::tests::roundtrip(core.clone(), vec![req]).await;
    let answer = got
        .into_iter()
        .find_map(|m| match m {
            Message::Response(r) => Some(r),
            _ => None,
        })
        .expect("an answer");
    serde_json::to_value(answer).unwrap()
}

/// The old store's raw thread records and one place binding (see the test below).
fn write_old_store(core: &Core, sid: &str) {
    use theseus_store::{kinds, NewRecord};
    let raw = |slug: &str, v: serde_json::Value| {
        NewRecord::json(kinds::META, Some(&crate::threads::key(slug)), &v).unwrap()
    };
    let place = |slug: &str, sid: &str| {
        NewRecord::json(
            kinds::META,
            Some(&format!(
                "{}{}",
                crate::outbox::PLACE_META_PREFIX,
                theseus_protocol::threads::place_of(slug)
            )),
            &sid.to_string(),
        )
        .unwrap()
    };
    core.store
        .append(&[
            raw(
                "tides",
                serde_json::json!({"slug": "tides", "title": "Tide gauges", "goal": "recalibrate gauge 4",
                    "topics": ["topic:tides"], "card": ["gauge 4 drifts"],
                    "working_set": {"repos": ["gauge-log"], "hosts": ["tides.example"]},
                    "surfaces": ["discord:channel:4242"], "links": ["moons"],
                    "session_id": sid, "last_active_ms": 1_790_000_000_000u64,
                    "created_at_ms": 1_780_000_000_000u64}),
            ),
            place("tides", sid),
            raw(
                "embargo",
                serde_json::json!({"slug": "embargo", "title": "Embargoed fix",
                    "sealed": {"by": "the CLI", "at_ms": 1u64, "reason": "embargoed"}}),
            ),
            raw(
                "maybe",
                serde_json::json!({"slug": "maybe", "title": "Maybe private",
                    "seal_pending": "it names a vault"}),
            ),
            raw("moons", serde_json::json!({"slug": "moons", "title": "Moons", "state": "paused",
                "links": ["tides"], "parent": "tides"})),
            raw("old", serde_json::json!({"slug": "old", "title": "Old work", "state": "done",
                "merged_into": "tides"})),
            raw("disk", serde_json::json!({"slug": "disk", "title": "Disk", "split_from": "tides"})),
        ])
        .unwrap();
}

/// What the daemon reads of the old store `write_old_store` planted: the board holds every record, with
/// every field it was written with.
fn assert_read(core: &Core, sid: &str) {
    let board = core.threads();
    assert_eq!(board.len(), 6, "every record reads");
    let show = |s: &str| board.get(s).cloned().unwrap();
    let t = show("tides");
    assert_eq!(
        (t.title.as_str(), t.goal.as_str()),
        ("Tide gauges", "recalibrate gauge 4")
    );
    assert_eq!(t.working_set.repos, ["gauge-log"]);
    assert_eq!(t.working_set.hosts, ["tides.example"]);
    assert_eq!(t.surfaces, ["discord:channel:4242"]);
    assert_eq!(t.card, ["gauge 4 drifts"]);
    assert_eq!(t.links, ["moons"]);
    assert_eq!(t.session_id.as_deref(), Some(sid));
    assert_eq!(t.topics, ["topic:tides"]);
    assert_eq!(t.created_at_ms, 1_780_000_000_000);
    assert_eq!(t.last_active_ms, 1_790_000_000_000);
    let seal = show("embargo").sealed.unwrap();
    assert_eq!(
        (seal.by.as_str(), seal.at_ms, seal.reason.as_str()),
        ("the CLI", 1, "embargoed")
    );
    assert_eq!(
        show("maybe").seal_pending.as_deref(),
        Some("it names a vault")
    );
    let moons = show("moons");
    assert_eq!(
        (moons.state, moons.parent.as_deref()),
        (ThreadState::Paused, Some("tides"))
    );
    assert_eq!(moons.links, ["tides"], "the reverse link");
    let old = show("old");
    assert_eq!(
        (old.state, old.merged_into.as_deref()),
        (ThreadState::Done, Some("tides"))
    );
    assert_eq!(show("disk").split_from.as_deref(), Some("tides"));
    assert_eq!(show("disk").session_id, None);
}

/// The owner's store as an older build left it (issue 44, threads retired 2026-11-22): thread records written
/// as RAW stored JSON (not by today's constructors), one of each shape an older build wrote (sealed, a pending
/// seal, paused, done, split from, merged into, a parent, linked, a working set, a Discord surface, with and
/// without a session), and one thread whose session holds an earlier exchange. After a restart, the board
/// holds every one with its fields and `thread.list` and `thread.show` are refused; no stored thread record or
/// binding changes by a byte; and the session that was a thread's runs an ordinary turn by its session id, with
/// its earlier exchange in the request.
#[tokio::test]
async fn an_old_stores_threads_read_unchanged_and_a_threads_session_runs_as_any_other() {
    use crate::provider::Scripted;
    use crate::tests_day_ceiling::build;
    let dir = tempfile::tempdir().unwrap();
    let (sid, before) = {
        let (core, _) = build(dir.path(), vec![Scripted::text("noted")], |_| {});
        let rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        core.store.put_session(&rec.session_id, &rec).unwrap();
        let first = submit_to(
            &core,
            &rec.session_id,
            "The gauge at pier four reads 3.2 metres.",
        )
        .await;
        assert!(first["result"].is_object(), "{first}");
        write_old_store(&core, &rec.session_id);
        // The baseline, taken before the restarted core opens the store.
        (rec.session_id, raw_threads(&core))
    };
    let (core, fake) = build(dir.path(), vec![Scripted::text("still 3.2")], |_| {});
    assert_eq!(before.len(), 7, "six records and one binding: {before:?}");
    assert_eq!(
        raw_threads(&core),
        before,
        "the restart wrote nothing to the thread records or their bindings"
    );

    assert_read(&core, &sid);
    assert_eq!(
        raw_threads(&core),
        before,
        "a read changed no stored thread record"
    );

    let next = submit_to(&core, &sid, "And what does it read now?").await;
    assert!(next["result"].is_object(), "{next}");
    assert_eq!(next["result"]["session_id"], sid.as_str(), "{next}");
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1, "one ordinary turn");
    let sent = serde_json::to_string(&reqs[0].messages).unwrap();
    assert!(
        sent.contains("pier four"),
        "the earlier exchange is in the request: {sent}"
    );
    assert!(sent.contains("what does it read now"), "{sent}");
    assert_eq!(
        raw_threads(&core),
        before,
        "the turn rewrote no stored thread record"
    );
}

/// The daily ceiling is the daemon's (the budget half of the retired thread test
/// `the_daily_ceiling_covers_every_thread`, on ordinary sessions): what one session spent today survives a
/// restart and stops another session's next provider call.
#[tokio::test]
async fn the_daily_ceiling_covers_every_session_across_a_restart() {
    use crate::tests_day_ceiling::{billed, build, turn_in};
    use crate::turn::TurnError;
    let dir = tempfile::tempdir().unwrap();
    let open = |c: &Core| {
        let rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        c.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let (spent, sb) = {
        let (core, _) = build(dir.path(), vec![billed("one")], |_| {});
        let (sa, sb) = (open(&core), open(&core));
        turn_in(&core, &sa, "hello").await.unwrap();
        let c = core.kernel.day_ceiling();
        let t = c.today(c.now());
        assert!(t.spent > 0);
        (t.spent, sb)
    };
    let ceiling = theseus_kernel::micros_to_usd(spent + 1);
    let (core, fake) = build(dir.path(), vec![billed("two")], |c| {
        c.kernel.daily_spend_ceiling_usd = ceiling;
    });
    let res = turn_in(&core, &sb, "hi").await;
    assert_eq!(
        res.unwrap_err().downcast_ref::<TurnError>().unwrap().class,
        "daily_ceiling",
        "another session's turn is stopped by the day's spend"
    );
    assert!(fake.requests().is_empty(), "no call was made");
}
