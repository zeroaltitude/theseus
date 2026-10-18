//! The guild calendar (theseus-4zc4d) through the stand-in's scheduled-event
//! routes: a reminder, a routine's run and a milestone's verb become events;
//! a moved item edits its event, a cancelled one deletes it, a passed one is
//! settled; the soonest ten stand and the eleventh waits; a restart finds its
//! events again and makes no duplicate; off, not one request is made; and a
//! private title never reaches a guild the bindings file does not trust.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_sim::fake_discord::{FakeEvent, DEFAULT_GUILD};

use super::{guild, Rig, ANA, LAB};
use crate::runtime::calendar::{Pace, PACES};

/// Quick, so a test need not wait ten minutes.
const FAST: Pace = Pace {
    debounce: Duration::from_millis(150),
    grace: Duration::from_millis(150),
    every: Duration::from_secs(3600),
    lead_ms: 0,
};

/// Ana's guild, which the bindings file trusts or not, with `#lab` bound.
fn bindings(trusted: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n{}\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n",
        if trusted { "private = true\n" } else { "" }
    )
}

async fn rig(on: bool, trusted: bool) -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(true), &bindings(trusted), &[], move |cfg| {
        PACES
            .lock()
            .unwrap()
            .push((cfg.server.state_dir.clone(), FAST));
        cfg.discord.calendar.enabled = on;
        cfg.discord.calendar.guild = DEFAULT_GUILD.to_string();
    })
    .await
}

impl Rig {
    /// A reminder in Home, `ahead_ms` from now; its wake id.
    fn remind(&self, ahead_ms: u64, note: &str) -> String {
        let home = self.core.home().unwrap();
        let due = self.core.kernel.now_ms() + ahead_ms;
        self.core
            .kernel
            .create_wake(
                &home.execution_id,
                None,
                theseus_kernel::wake_owner::reminder(due, note),
                "the test",
            )
            .unwrap()
            .wake
            .id
    }

    fn events(&self) -> Vec<FakeEvent> {
        self.fake.events()
    }

    fn open_events(&self) -> Vec<FakeEvent> {
        self.events().into_iter().filter(|e| e.status < 3).collect()
    }
}

const HOUR: u64 = 3_600_000;

#[tokio::test]
async fn a_reminder_becomes_an_event_that_moves_settles_and_goes() {
    let r = rig(true, true).await;
    let wake = r.remind(HOUR, "stretch");
    r.until("the event", || r.events().len() == 1).await;
    let e = &r.events()[0];
    assert_eq!(
        (e.name.as_str(), e.location.as_str(), e.status),
        ("stretch", "Theseus", 1)
    );
    assert!(
        e.description
            .ends_with(&format!("theseus-calendar:{wake}#0")),
        "{}",
        e.description
    );
    assert_eq!(e.history, ["create"]);

    // Moved: the wake is snoozed, and the same event is edited, not remade.
    let home = r.core.home().unwrap();
    let before = r.events()[0].starts.clone();
    r.core
        .kernel
        .snooze_wake(
            &home.execution_id,
            &wake,
            r.core.kernel.now_ms() + 2 * HOUR,
            false,
            "the test",
        )
        .unwrap();
    r.until("the edit", || r.events()[0].starts != before).await;
    assert_eq!(r.events().len(), 1, "an edit, not a second event");
    assert_eq!(r.events()[0].history, ["create", "edit"]);

    // Cancelled: the event goes.
    r.core
        .kernel
        .cancel_wake(&home.execution_id, &wake, "the test")
        .unwrap();
    r.until("the delete", || r.events().is_empty()).await;
    assert!(r
        .fake
        .event_requests()
        .iter()
        .any(|s| s.method == "DELETE" && s.outcome == "deleted"));

    // Passed: a reminder due in a moment is made, fires, and its event is
    // completed (started first: Discord moves a status only forward).
    r.remind(600, "stand up");
    r.until("settled", || {
        r.events()
            .iter()
            .any(|e| e.name == "stand up" && e.status == 3)
    })
    .await;
    let done = r
        .events()
        .into_iter()
        .find(|e| e.name == "stand up")
        .unwrap();
    assert_eq!(done.history, ["create", "status:2", "status:3"]);
    // And the store forgot it: the pass saves after the settle's PATCH, so
    // the event can read settled a moment before the store is written.
    r.until("the store forgot it", || {
        let saved: Option<serde_json::Value> = r.core.store.get_meta("discord.calendar").unwrap();
        saved.is_some_and(|s| s["entries"] == json!([]))
    })
    .await;
}

#[tokio::test]
async fn the_soonest_ten_stand_and_the_eleventh_waits_for_a_place() {
    let r = rig(true, true).await;
    let ids: Vec<String> = (0..11)
        .map(|n| r.remind(HOUR + n * 60_000, &format!("item {n}")))
        .collect();
    r.until("ten events", || r.events().len() == 10).await;
    // Give a pass the chance to make an eleventh, as a bug would.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let names: Vec<String> = r.events().iter().map(|e| e.name.clone()).collect();
    assert_eq!(names.len(), 10, "{names:?}");
    assert!(!names.contains(&"item 10".to_string()), "{names:?}");

    // One goes; the eleventh takes its place.
    let home = r.core.home().unwrap();
    r.core
        .kernel
        .cancel_wake(&home.execution_id, &ids[0], "the test")
        .unwrap();
    r.until("the eleventh", || {
        r.events().iter().any(|e| e.name == "item 10")
    })
    .await;
    assert_eq!(r.open_events().len(), 10);
}

#[tokio::test]
async fn a_restart_finds_its_events_by_the_store_and_by_their_marker() {
    let r = rig(true, true).await;
    r.remind(HOUR, "water the plants");
    r.remind(2 * HOUR, "call the dentist");
    r.until("two events", || r.events().len() == 2).await;
    let made = r.events();

    // A restart: a fresh context over the same store and guild.
    let c = &r.core.cfg.discord.calendar;
    let again = || {
        crate::runtime::calendar::Cx::open(&r.core, c, Default::default(), 0, Arc::new(|_| {}))
            .unwrap()
    };
    let mut cx = again();
    cx.pass().await;
    assert_eq!(r.events().len(), 2, "no duplicate after a restart");

    // The write that named the events lost: the guild's own listing does.
    r.core
        .store
        .put_meta(
            "discord.calendar",
            &json!({"guild": c.guild, "entries": []}),
        )
        .unwrap();
    let mut cx = again();
    cx.pass().await;
    let now = r.events();
    assert_eq!(now.len(), 2, "no duplicate when the store forgot: {now:?}");
    assert_eq!(
        now.iter().map(|e| e.id).collect::<Vec<_>>(),
        made.iter().map(|e| e.id).collect::<Vec<_>>()
    );
    assert!(
        now.iter().all(|e| e.history == ["create"]),
        "nothing was edited either: {now:?}"
    );
}

#[tokio::test]
async fn off_not_one_request_is_made() {
    let r = rig(false, true).await;
    r.remind(HOUR, "stretch");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        r.fake.event_requests().is_empty(),
        "{:?}",
        r.fake.event_requests()
    );
    assert!(r.events().is_empty());
    // The same wait with it on, so the wait above is proven long enough.
    let on = rig(true, true).await;
    on.remind(HOUR, "stretch");
    on.until("an event", || !on.events().is_empty()).await;
}

#[tokio::test]
async fn a_guild_the_bindings_file_does_not_trust_gets_nothing_and_health_says_why() {
    let r = rig(true, false).await;
    r.remind(HOUR, "call the plumber about the invoice");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        r.fake.event_requests().is_empty(),
        "{:?}",
        r.fake.event_requests()
    );
    assert!(r.events().is_empty());
    let err = r
        .core
        .bindings
        .all()
        .first()
        .and_then(|b| b.last_error.clone())
        .unwrap_or_default();
    assert!(
        err.contains("calendar") && err.contains("not one the bindings file trusts"),
        "{err}"
    );
}

#[tokio::test]
async fn a_title_with_money_is_named_neutrally() {
    let r = rig(true, true).await;
    r.remind(HOUR, "pay the $400 invoice");
    r.until("the event", || r.events().len() == 1).await;
    assert_eq!(r.events()[0].name, "Theseus · reminder");
    assert!(!r.events()[0].description.contains("invoice"));
}

#[tokio::test]
async fn a_full_guild_is_told_once_and_the_rest_wait() {
    let r = rig(true, true).await;
    r.fake.set_event_limit(2);
    for n in 0..3 {
        r.remind(HOUR + n * 60_000, &format!("item {n}"));
    }
    r.until("two events and a word in health", || {
        r.events().len() == 2
            && r.core
                .bindings
                .all()
                .first()
                .is_some_and(|b| b.last_error.as_deref().is_some_and(|e| e.contains("full")))
    })
    .await;
    let creates = r
        .fake
        .event_requests()
        .iter()
        .filter(|s| s.outcome == "limit")
        .count();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(r.events().len(), 2);
    // Told once, though it was refused again at later passes.
    assert_eq!(
        r.ledger("discord.error").len(),
        1,
        "after {creates} refusals"
    );
}

#[tokio::test]
async fn a_bot_without_manage_events_is_told_once_and_stops() {
    let r = rig(true, true).await;
    r.fake.refuse_events(true);
    r.remind(HOUR, "stretch");
    r.until("the refusal in health", || {
        r.core.bindings.all().first().is_some_and(|b| {
            b.last_error
                .as_deref()
                .is_some_and(|e| e.contains("Manage Events"))
        })
    })
    .await;
    assert!(r.events().is_empty());
    assert_eq!(r.ledger("discord.error").len(), 1);
}

#[tokio::test]
async fn a_routine_runs_as_an_event() {
    use crate::rpc_client::RpcClient;
    use theseus_core::approval::{Client, Surface};

    let r = rig(true, true).await;
    let (rpc, _n) = RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli));
    let made: Result<serde_json::Value, _> = rpc
        .call(
            "routine.create",
            json!({"title": "hourly sweep", "schedule": "every 1h", "brief": "look around"}),
        )
        .await;
    made.expect("a routine");
    r.until("the run's event", || {
        r.events().iter().any(|e| e.name == "hourly sweep")
    })
    .await;
    let e = r
        .events()
        .into_iter()
        .find(|e| e.name == "hourly sweep")
        .unwrap();
    assert!(
        e.description.starts_with("A Theseus routine."),
        "{}",
        e.description
    );
}

#[tokio::test]
async fn a_shared_places_reminder_is_named_neutrally_and_a_private_ones_by_title() {
    // `#lab` is bound shared here: the guild is trusted, its channel says
    // `private = false`.
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let b = bindings(true).replace(
        "mention_only = false\n",
        "mention_only = false\nprivate = false\n",
    );
    let r = Rig::start_tweaked(model, guild(true), &b, &[], |cfg| {
        PACES
            .lock()
            .unwrap()
            .push((cfg.server.state_dir.clone(), FAST));
        cfg.discord.calendar.enabled = true;
        cfg.discord.calendar.guild = DEFAULT_GUILD.to_string();
    })
    .await;
    r.say((ANA, "ana"), Some(LAB), "hello");
    r.until("lab's session", || {
        r.core
            .outbox
            .place_session(&format!("channel:{LAB}"))
            .ok()
            .flatten()
            .is_some()
    })
    .await;
    let sid = r
        .core
        .outbox
        .place_session(&format!("channel:{LAB}"))
        .unwrap()
        .unwrap();
    let exe = r
        .core
        .store
        .get_session::<theseus_core::session::SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    let due = r.core.kernel.now_ms() + HOUR;
    r.core
        .kernel
        .create_wake(
            &exe,
            None,
            theseus_kernel::wake_owner::reminder(due, "secret errand"),
            "the test",
        )
        .unwrap();
    r.remind(2 * HOUR, "open errand");
    r.until("both events", || r.events().len() == 2).await;
    let names: Vec<String> = r.events().iter().map(|e| e.name.clone()).collect();
    assert!(
        names.contains(&"Theseus · reminder".to_string()),
        "{names:?}"
    );
    assert!(names.contains(&"open errand".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("secret")), "{names:?}");
}
