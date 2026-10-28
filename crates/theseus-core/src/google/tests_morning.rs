//! The morning note's day (theseus-batvg.6), against the loopback fake: today's
//! meetings (with a conflict mark), the unread mail that matters and the drafts
//! waiting, each read through `Google::call` with its `google.called` row; the
//! note's model call made private, so a restricted-origin model never sees it;
//! and, with no `[google]`, the note it always was.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::gmail::MsgSpec;
use theseus_google::fake::{Fake, Reply, REFRESH_TOKEN};
use theseus_protocol::Initiative;

use super::tests::{board, config, ledgered};
use crate::config::PersonaConfig;
use crate::places::BoundPlace;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::test_support::{rows_100k as rows, until_within as until};
use crate::{Config, Core};

const EVENTS_PATH: &str = "/calendar/v3/calendars/primary/events";
const OWNER: u64 = 271_828_182_845_904_523;
const SAID: &str = "Your day is laid out.";

struct Morning {
    core: Arc<Core>,
    fake: Fake,
    model: Arc<FakeProvider>,
    zai: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

/// A core with `[google]` at a fake, a persona that speaks the morning note,
/// the owner's DM bound, and `tweak` on the config; `google` off builds it with
/// no `[google]` at all.
async fn rig(google: bool, script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Morning {
    rig_as(google, script, tweak, true).await
}

/// [`rig`], the config's load checks run only when `checked`: a test of the
/// run time's own floor builds a config the load refuses.
async fn rig_as(
    google: bool,
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
    checked: bool,
) -> Morning {
    let fake = Fake::start().await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    if !google {
        cfg.google = Default::default();
        cfg.secrets.remove("google_client");
        cfg.secrets.remove("google_refresh_token");
    }
    // The account is a consumer one here: its own calendar (the fake lists the
    // account, of a domain the default rule counts as a workplace's) is not
    // sensitive. The calendar tests below mark one sensitive themselves.
    cfg.google.sensitive_domains = Some(Vec::new());
    cfg.initiative.morning_hour = 0;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.personas.insert(
        "ondine".into(),
        PersonaConfig {
            name: Some("Ondine".into()),
            emoji: Some("🌊".into()),
            initiative: Initiative::Normal,
            ..Default::default()
        },
    );
    cfg.context.default_persona = Some("ondine".into());
    tweak(&mut cfg);
    if checked {
        cfg.validate().unwrap();
    }
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(script));
    let zai = Arc::new(FakeProvider::scripted(vec![Scripted::text(SAID); 4]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.providers.insert("zai".into(), zai.clone());
    parts.secrets = board(Some(REFRESH_TOKEN));
    let core = Core::build(parts).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:dm:{OWNER}"),
        name: "DM @wren".into(),
        ..Default::default()
    }]);
    Morning {
        core,
        fake,
        model,
        zai,
        _dir: dir,
    }
}

/// Ask for the note and wait for it: the facts the model was given.
async fn note(m: &Morning, who: &Arc<FakeProvider>) -> String {
    assert!(m.core.post_morning_if_due(), "the day is claimed");
    until("the morning note", 20, || {
        !rows(&m.core, "initiative.spoke").is_empty()
    })
    .await;
    let requests = who.requests();
    let body = requests
        .last()
        .map(|r| serde_json::to_string(&r.body()).unwrap())
        .unwrap_or_default();
    body.replace("\\n", "\n")
}

fn said() -> Vec<Scripted> {
    vec![Scripted::text(SAID); 4]
}

fn event(id: &str, start: &str, end: &str, title: &str, attendees: usize, call: bool) -> Value {
    let mut e = json!({"id": id, "summary": title, "status": "confirmed",
        "start": {"dateTime": start}, "end": {"dateTime": end},
        "organizer": {"email": "owner@example.test", "self": true},
        "attendees": (0..attendees).map(|i| json!({"email": format!("p{i}@example.test")})).collect::<Vec<_>>()});
    if call {
        e["hangoutLink"] = "https://meet.example.test/abc-defg".into();
    }
    e
}

fn unread(id: &str, from: &str, subject: &str) -> MsgSpec {
    MsgSpec::new(id, from, subject, "Hello.").label("UNREAD")
}

#[tokio::test(flavor = "multi_thread")]
async fn morning_note_lists_todays_events() {
    let m = rig(true, said(), |_| {}).await;
    m.fake.json(
        "GET",
        EVENTS_PATH,
        json!({"items": [
            event("e1", "2026-10-22T16:00:00Z", "2026-10-22T16:30:00Z", "Design review", 3, true),
            event("e2", "2026-10-22T16:15:00Z", "2026-10-22T16:45:00Z", "Budget call", 0, false),
            event("e3", "2026-10-22T18:00:00Z", "2026-10-22T18:30:00Z", "Focus time", 1, false)]}),
    );
    let brief = note(&m, &m.model).await;
    // In the daemon's zone (the gate's TZ is Phoenix, UTC-7), one line each.
    assert!(
        brief.contains("09:00–09:30 · Design review · 3 attendees · call: https://meet.example.test/abc-defg · conflict"),
        "{brief}"
    );
    assert!(
        brief.contains("09:15–09:45 · Budget call · conflict"),
        "{brief}"
    );
    assert!(
        brief.contains("11:00–11:30 · Focus time · 1 attendees")
            && !brief.contains("Focus time · 1 attendees · conflict"),
        "{brief}"
    );
    // A day with meetings is a day something happened, and the calls are rows.
    let called = ledgered(&m.core, "google.called");
    assert!(
        called.iter().any(|c| c["method"] == "calendar.events.list"),
        "{called:?}"
    );
    assert!(called
        .iter()
        .all(|c| c["class"] == "read" && !c.to_string().contains("Design")));
}

#[tokio::test(flavor = "multi_thread")]
async fn morning_note_counts_unread_and_drafts_waiting() {
    let m = rig(true, said(), |_| {}).await;
    let mb = m.fake.mail();
    mb.thread(
        "t1",
        vec![unread(
            "m1",
            "Dana Colleague <dana@example.test>",
            "Contract",
        )],
    );
    mb.thread(
        "t2",
        vec![unread("m2", "Pat Peer <pat@example.test>", "Lunch?")],
    );
    mb.thread(
        "t3",
        vec![unread("m3", "Sam Stranger <sam@example.test>", "Question")],
    );
    mb.thread(
        "t4",
        vec![unread("m4", "Shop <deals@example.test>", "Sale").label("CATEGORY_PROMOTIONS")],
    );
    mb.thread(
        "t5",
        vec![MsgSpec::new(
            "m5",
            "Old <old@example.test>",
            "Read already",
            "Hi.",
        )],
    );
    for d in ["d1", "d2"] {
        mb.put_draft(
            d,
            MsgSpec::new(&format!("x{d}"), "owner@example.test", "Draft", "Hi."),
        );
    }
    let brief = note(&m, &m.model).await;
    assert!(
        brief.contains("Unread mail that matters (last 12 hours): 3"),
        "{brief}"
    );
    assert!(
        brief.contains("- Dana Colleague <dana@example.test> · Contract"),
        "{brief}"
    );
    assert!(
        !brief.contains("Sale") && !brief.contains("Read already"),
        "{brief}"
    );
    assert!(brief.contains("Drafts waiting for a send: 2"), "{brief}");
    assert!(
        !brief.contains("Nothing is waiting on the owner.")
            && brief.contains("2 drafts wait for the owner to send"),
        "{brief}"
    );
    // The template, when no model speaks, no longer says nothing needs the owner.
    let spoke = rows(&m.core, "initiative.spoke");
    assert_eq!(spoke[0]["text"], SAID);
}

/// A Google read that fails leaves its line out and says so; it never stops
/// the note.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_google_read_is_said_and_never_blocks_the_note() {
    let m = rig(true, said(), |_| {}).await;
    m.fake.json(
        "GET",
        EVENTS_PATH,
        json!({"items": [event("e1", "2026-10-22T16:00:00Z", "2026-10-22T16:30:00Z", "Standup", 0, false)]}),
    );
    m.fake.route("GET", "/gmail/v1/users/me/messages", |_| {
        Reply::json(
            403,
            json!({"error": {"code": 403, "message": "no", "errors": [{"reason": "forbidden"}]}}),
        )
    });
    let mb = m.fake.mail();
    mb.thread(
        "t1",
        vec![unread("m1", "Dana <dana@example.test>", "Contract")],
    );
    let brief = note(&m, &m.model).await;
    assert!(brief.contains("mail unreachable"), "{brief}");
    assert!(!brief.contains("calendar unreachable"), "{brief}");
}

/// The note's model call carries the owner's private text, so a model of
/// restricted origin is never asked: the load refuses the profile
/// (theseus-a1wvq.3's data floor), and a core built past that check still
/// sends the note as its template.
#[tokio::test(flavor = "multi_thread")]
async fn the_morning_note_never_uses_a_restricted_model() {
    let glm = |c: &mut Config| c.initiative.profile = Some("glm53".into());
    let mut cfg = Config::example();
    cfg.initiative.profile = Some("glm".into());
    let e = cfg.validate().unwrap_err().to_string();
    assert!(e.contains("[initiative] profile"), "{e}");
    let m = rig_as(true, said(), glm, false).await;
    let mb = m.fake.mail();
    mb.thread(
        "t1",
        vec![unread("m1", "Dana <dana@example.test>", "Contract")],
    );
    assert!(m.core.post_morning_if_due());
    until("the morning note", 20, || {
        !rows(&m.core, "initiative.spoke").is_empty()
    })
    .await;
    assert!(
        m.zai.requests().is_empty(),
        "nothing went to the restricted provider"
    );
    assert!(
        m.model.requests().is_empty(),
        "and it did not retry on another profile"
    );
    let spoke = rows(&m.core, "initiative.spoke");
    assert!(
        spoke[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("🌊 Ondine: "),
        "{spoke:?}"
    );
    assert!(!spoke[0]["text"].as_str().unwrap().contains(SAID));
}

/// With no `[google]` section nothing here reads or adds anything: the note's
/// facts are what they were, no Google call is made, and its sentence still
/// ends as it did.
#[tokio::test(flavor = "multi_thread")]
async fn without_google_the_morning_note_is_what_it_was() {
    let promise = Scripted::text("Pushed. I'll let you know when it lands.");
    let m = rig(false, vec![promise, Scripted::text(SAID)], |_| {}).await;
    let sid = {
        let rec =
            crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        m.core.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let (live, _) = m.core.live_profile();
    let target = m
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = crate::bus::EventSink::new(m.core.bus.clone(), &sid, None);
    let rec = m.core.store.get_session(&sid).unwrap().unwrap();
    m.core
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some("hello".into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
    let brief = note(&m, &m.model).await;
    assert!(
        brief.contains("Nothing is waiting on the owner."),
        "{brief}"
    );
    assert!(
        !brief.contains("Today") && !brief.contains("unreachable"),
        "{brief}"
    );
    assert!(ledgered(&m.core, "google.called").is_empty());
    assert!(m.fake.log().is_empty(), "no request left for Google");
}

// --------------------------------------- every configured calendar (jid7t.1)

mod calendars {
    use super::*;
    use crate::google::tests_calendar_cache::{cli, ev, install, rpc_as, two, Mode};

    /// The two calendars, a note with the events of each.
    async fn rigged(
        script: Vec<Scripted>,
        tweak: impl FnOnce(&mut Config),
    ) -> (Morning, crate::google::tests_calendar_cache::Cals) {
        rigged_as(script, tweak, true).await
    }

    async fn rigged_as(
        script: Vec<Scripted>,
        tweak: impl FnOnce(&mut Config),
        checked: bool,
    ) -> (Morning, crate::google::tests_calendar_cache::Cals) {
        let m = rig_as(
            true,
            script,
            |c| {
                two(c);
                tweak(c);
            },
            checked,
        )
        .await;
        let cals = install(&m.fake);
        cals.personal.lock().unwrap().items = vec![ev(
            "p1",
            "Dentist",
            "2026-10-22T16:00:00Z",
            "2026-10-22T16:30:00Z",
        )];
        cals.work.lock().unwrap().items = vec![ev(
            "w1",
            "Board prep",
            "2026-10-22T16:15:00Z",
            "2026-10-22T16:45:00Z",
        )];
        (m, cals)
    }

    async fn spoken(m: &Morning) {
        assert!(m.core.post_morning_if_due(), "the day is claimed");
        until("the morning note", 20, || {
            !rows(&m.core, "initiative.spoke").is_empty()
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_brief_labels_each_line_by_calendar() {
        let (m, _cals) = rigged(said(), |_| {}).await;
        let brief = note(&m, &m.model).await;
        assert!(
            brief.contains("- [Personal] 09:00–09:30 · Dentist"),
            "{brief}"
        );
        assert!(
            brief.contains("- [Work] 09:15–09:45 · Board prep"),
            "{brief}"
        );
        // A meeting on one calendar that overlaps one on the other is a conflict.
        assert_eq!(brief.matches("· conflict").count(), 2, "{brief}");
        assert!(!brief.contains("unreachable"), "{brief}");
    }

    /// With the one default calendar the note is what it was: no label.
    #[tokio::test(flavor = "multi_thread")]
    async fn one_calendar_is_told_as_it_always_was() {
        let m = rig(true, said(), |_| {}).await;
        m.fake.json(
            "GET",
            EVENTS_PATH,
            json!({"items": [event("e1", "2026-10-22T16:00:00Z", "2026-10-22T16:30:00Z", "Standup", 0, false)]}),
        );
        let brief = note(&m, &m.model).await;
        assert!(brief.contains("- 09:00–09:30 · Standup"), "{brief}");
        assert!(
            !brief.contains("[primary]") && !brief.contains("unreachable"),
            "{brief}"
        );
    }

    /// A sensitive calendar in the brief: a model of China origin is never
    /// asked (the call is made for no session, which the chokepoint refuses to
    /// one), a US one is, and the note's ledger row holds no text.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sensitive_calendar_makes_the_brief_us_only() {
        // The US model is asked, and its words reach the DM and no ledger row.
        let titled = vec![Scripted::text("Board prep at nine, then the Dentist."); 4];
        let (m, _) = rigged(titled, |_| {}).await;
        spoken(&m).await;
        assert!(
            !m.model.requests().is_empty(),
            "the US model wrote the note"
        );
        let spoke = rows(&m.core, "initiative.spoke");
        assert_eq!(spoke.len(), 1);
        assert!(spoke[0].get("text").is_none(), "{spoke:?}");
        assert!(!serde_json::to_string(&all_rows(&m.core))
            .unwrap()
            .contains("Board prep"));
        // China origin: refused, the template speaks, and nothing else is tried.
        let (m, _) = rigged_as(
            said(),
            |c| c.initiative.profile = Some("glm53".into()),
            false,
        )
        .await;
        spoken(&m).await;
        assert!(m.zai.requests().is_empty() && m.model.requests().is_empty());
        // The template spoke: its row holds no text either.
        assert!(rows(&m.core, "initiative.spoke")[0].get("text").is_none());
    }

    /// A calendar that cannot be read says so by label and leaves its lines
    /// out; the others and the note go on, and with no sensitive calendar read
    /// the note's row keeps its text.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_unreachable_calendar_is_said_and_blocks_nothing() {
        let (m, cals) = rigged(said(), |_| {}).await;
        cals.work.lock().unwrap().mode = Mode::NotFound;
        let brief = note(&m, &m.model).await;
        assert!(brief.contains("Work unreachable"), "{brief}");
        assert!(
            brief.contains("[Personal] 09:00–09:30 · Dentist") && !brief.contains("Board prep"),
            "{brief}"
        );
        assert_eq!(rows(&m.core, "initiative.spoke")[0]["text"], SAID);
    }

    /// Both calendars read through the page's method, the tool and the brief:
    /// not one event's title, place, attendee address or call link is in any
    /// ledger row.
    #[tokio::test(flavor = "multi_thread")]
    async fn no_event_content_is_in_any_ledger_row() {
        // The model's tool path first: a turn that calls `calendar_events`, then
        // the brief's four replies.
        let mut script = vec![
            Scripted::tools(
                "",
                &[("c1", "calendar_events", json!({"calendars": "all"}))],
            ),
            Scripted::text("Read."),
        ];
        script.extend(vec![
            Scripted::text("Board prep and the Dentist in Room 4.");
            4
        ]);
        let (m, cals) = rigged(script, |_| {}).await;
        cals.work.lock().unwrap().busy =
            vec![("2026-10-22T16:00:00Z".into(), "2026-10-22T17:00:00Z".into())];
        let (from, to) = crate::google::tests_calendar_cache::day();
        rpc_as(
            &m.core,
            cli(),
            "calendar.window",
            json!({"from_ms": from, "to_ms": to}),
        )
        .await
        .unwrap();
        cals.work.lock().unwrap().mode = Mode::BusyOnly;
        m.core
            .tools
            .google
            .as_ref()
            .unwrap()
            .calendars
            .age(std::time::Duration::from_secs(400));
        rpc_as(
            &m.core,
            cli(),
            "calendar.window",
            json!({"from_ms": from, "to_ms": to}),
        )
        .await
        .unwrap();
        cals.work.lock().unwrap().mode = Mode::Ok;
        // A turn in a private session where the model reads the calendars itself.
        crate::test_support::turn_in(&m.core, None, "what is on my calendar?").await;
        assert!(
            m.model
                .requests()
                .iter()
                .any(|r| serde_json::to_string(&r.body())
                    .unwrap()
                    .contains("Board prep")),
            "the model was given the events (the tool ran)"
        );
        spoken(&m).await;
        let rows = all_rows(&m.core);
        assert!(
            rows.iter().any(|r| r.contains("google.called")),
            "the reads are rows"
        );
        assert!(
            rows.iter()
                .any(|r| r.contains("tool.ended") || r.contains("action.")),
            "the tool run wrote its rows"
        );
        let wrote = rows.join("\n");
        for secret in [
            "Dentist",
            "Board prep",
            "Room 4",
            "guest.one@example.test",
            "guest.two@example.test",
            "meet.example.test",
            "calendar.example.test/e/",
        ] {
            assert!(!wrote.contains(secret), "{secret} is in a ledger row");
        }
    }

    /// Every ledger row, as its JSON text.
    fn all_rows(core: &Core) -> Vec<String> {
        core.store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| serde_json::to_string(&r).unwrap())
            .collect()
    }
}
