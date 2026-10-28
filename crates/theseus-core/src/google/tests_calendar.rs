//! The calendar's two tools through the whole core, against the loopback fake
//! (the Google design §3.1 rows 6 and 7, §4.2): an invitation or a
//! cancellation asks at every posture, the owner's own events are not outside
//! text and a stranger's invitation is, and times read in the daemon's zone.

use serde_json::{json, Value};
use theseus_google::fake::Reply;
use theseus_tools::ToolCtx;

use super::calendar::{instant, rfc3339, when};
use super::tests::*;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::recall::stamp::Zone;
use crate::test_support::turn;

const EVENTS_PATH: &str = "/calendar/v3/calendars/primary/events";

fn put(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "calendar_put", input)])
}

fn events(id: &str, input: Value) -> Scripted {
    Scripted::tools("", &[(id, "calendar_events", input)])
}

fn plan_of(r: &Rig, tool: &str, input: Value) -> Result<theseus_tools::Plan, String> {
    let tool = r.core.tools.registry.get(tool).expect("registered").clone();
    tool.plan(&input, &ToolCtx::for_tests(r.dir.path()))
}

/// The config that opens both tools, as the owner's can.
fn open(c: &mut crate::Config) {
    c.policy.enforcement = Posture::Open;
    c.policy.tools.insert("calendar.put".into(), Posture::Open);
    c.policy
        .tools
        .insert("calendar.events".into(), Posture::Open);
}

async fn rig_open(script: Vec<Scripted>) -> Rig {
    rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        open,
    )
    .await
}

/// The first `calendar.*` result in `sid`: its text, its outside-text marker, its status.
fn result_of(r: &Rig, sid: &str, tool: &str) -> (String, Option<String>, ResultStatus, Value) {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::ToolResult {
                tool: t,
                content,
                external,
                status,
                meta,
                ..
            } if t == tool => Some((
                content.clone(),
                external.as_ref().map(|e| e.url.clone()),
                *status,
                meta.clone(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a {tool} result"))
}

fn event(id: &str, organizer: &str, own: bool) -> Value {
    json!({"id": id, "summary": "Design review", "status": "confirmed",
        "start": {"dateTime": "2026-10-22T16:00:00Z"}, "end": {"dateTime": "2026-10-22T16:30:00Z"},
        "organizer": {"email": organizer, "self": own},
        "hangoutLink": "https://meet.example.test/abc-defg",
        "attendees": [
            {"email": "owner@example.test", "self": true, "responseStatus": "needsAction"},
            {"email": "collaborator@example.test", "responseStatus": "accepted"}]})
}

#[tokio::test]
async fn inviting_attendees_asks_whatever_the_posture() {
    let invite = json!({"summary": "Planning", "start": "2026-10-22T16:00:00Z",
        "end": "2026-10-22T16:30:00Z", "attendees": ["collaborator@example.test"],
        "notify_attendees": true});
    let alone = json!({"summary": "Focus time", "start": "2026-10-22T18:00:00Z",
        "end": "2026-10-22T19:00:00Z"});
    let r = rig_open(vec![
        put("c1", invite.clone()),
        put("c2", alone.clone()),
        Scripted::text("Booked."),
    ])
    .await;
    let sid = session(&r.core);
    // The plan names the authority, and with the posture and the lists all
    // `open` the decision is still Approve; the one question every loosening
    // layer (a mode, a routine's grant) asks
    // first is whether only the tool's posture decides, and here it does not.
    let tool = r.core.tools.registry.get("calendar.put").unwrap().clone();
    let plan = plan_of(&r, "calendar.put", invite.clone()).unwrap();
    assert!(
        plan.authority
            .as_deref()
            .is_some_and(|a| a.contains("invites")),
        "{plan:?}"
    );
    let d = r.core.tools.policy.decide(tool.as_ref(), &plan);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(!r.core.tools.policy.posture_only(tool.as_ref(), &plan));
    // An event with no attendees is decided by the posture alone.
    let plain = plan_of(&r, "calendar.put", alone).unwrap();
    assert!(plain.authority.is_none(), "{plain:?}");
    assert!(r.core.tools.policy.posture_only(tool.as_ref(), &plain));
    assert_eq!(
        r.core.tools.policy.decide(tool.as_ref(), &plain).posture,
        Posture::Open
    );
    // Through a whole turn: the invitation waits, and nothing was created.
    let res = turn(&r.core, &sid, "invite them").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert!(
        r.fake.log().iter().all(|l| l.verb != "POST"),
        "nothing was created"
    );
    let asked = ledgered(&r.core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0]["reason"].as_str().unwrap().contains("invites"),
        "{asked:?}"
    );
}

#[tokio::test]
async fn an_event_with_no_attendees_runs_without_asking() {
    let r = rig_open(vec![
        put(
            "c1",
            json!({"summary": "Focus time", "start": "2026-10-22T18:00:00Z", "end": "2026-10-22T19:00:00Z"}),
        ),
        Scripted::text("Booked."),
    ])
    .await;
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "block my afternoon").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    let posts: Vec<_> = r
        .fake
        .log()
        .into_iter()
        .filter(|l| l.verb == "POST")
        .collect();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].path, EVENTS_PATH);
    assert_eq!(
        posts[0].query,
        vec![("sendUpdates".to_string(), "none".to_string())]
    );
    assert_eq!(posts[0].body["summary"], "Focus time");
    assert_eq!(posts[0].body["start"]["dateTime"], "2026-10-22T18:00:00Z");
    assert!(posts[0].body.get("attendees").is_none());
    let (text, external, status, meta) = result_of(&r, &sid, "calendar.put");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.starts_with("Created:"), "{text}");
    // The owner organizes what it makes: not outside text, and still private.
    assert_eq!(external, None);
    assert!(super::private::marked(&meta), "{meta:?}");
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["class"], "write");
    assert_eq!(rows[0]["method"], "calendar.events.insert");
}

#[tokio::test]
async fn cancelling_an_event_with_attendees_asks() {
    let r = rig_open(vec![
        put(
            "c1",
            json!({"event_id": "evt9", "cancel": true, "notify_attendees": true}),
        ),
        Scripted::text("Cancelled."),
    ])
    .await;
    let sid = session(&r.core);
    let plan = plan_of(
        &r,
        "calendar.put",
        json!({"event_id": "evt9", "cancel": true, "notify_attendees": true}),
    )
    .unwrap();
    assert!(
        plan.authority
            .as_deref()
            .is_some_and(|a| a.contains("cancels")),
        "{plan:?}"
    );
    // A cancellation asks even when it tells no one: the plan cannot see who is invited.
    assert!(plan_of(
        &r,
        "calendar.put",
        json!({"event_id": "evt9", "cancel": true})
    )
    .unwrap()
    .authority
    .is_some());
    let res = turn(&r.core, &sid, "cancel the review").await;
    let corr = res.awaiting_confirm.clone().expect("a cancellation asks");
    assert!(
        r.fake.log().iter().all(|l| l.verb != "DELETE"),
        "nothing was cancelled yet"
    );
    // The owner's yes sends it, once, telling the attendees.
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core
        .continue_execution(&res.execution_id.clone().unwrap())
        .await
        .unwrap();
    let deletes: Vec<_> = r
        .fake
        .log()
        .into_iter()
        .filter(|l| l.verb == "DELETE")
        .collect();
    assert_eq!(deletes.len(), 1, "{deletes:?}");
    assert_eq!(deletes[0].path, format!("{EVENTS_PATH}/evt9"));
    assert_eq!(
        deletes[0].query,
        vec![("sendUpdates".to_string(), "all".to_string())]
    );
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.last().unwrap()["class"], "authority");
}

#[tokio::test]
async fn a_change_asks_when_it_tells_the_attendees_and_not_when_it_does_not() {
    let r = rig_open(vec![]).await;
    let change = |extra: Value| {
        let mut i = json!({"event_id": "evt9", "location": "Room 4"});
        i.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        plan_of(&r, "calendar.put", i).unwrap()
    };
    assert!(change(json!({"notify_attendees": true}))
        .authority
        .is_some());
    assert!(change(json!({"attendees": ["collaborator@example.test"]}))
        .authority
        .is_some());
    assert!(change(json!({})).authority.is_none());
}

#[tokio::test]
async fn an_event_the_account_organizes_is_not_external() {
    let r = rig_open(vec![
        events("c1", json!({"from": "2026-10-22", "to": "2026-10-23"})),
        Scripted::text("Done."),
    ])
    .await;
    r.fake.json(
        "GET",
        EVENTS_PATH,
        json!({"items": [event("own1", "owner@example.test", true)]}),
    );
    let sid = session(&r.core);
    turn(&r.core, &sid, "what is on tomorrow").await;
    let (text, external, status, meta) = result_of(&r, &sid, "calendar.events");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("Design review (own1)"), "{text}");
    assert!(
        text.contains("organizer: owner@example.test (you)"),
        "{text}"
    );
    assert!(
        text.contains("collaborator@example.test accepted"),
        "{text}"
    );
    assert!(
        text.contains("call: https://meet.example.test/abc-defg"),
        "{text}"
    );
    assert_eq!(external, None, "the owner's own event is not stranger text");
    assert!(super::private::marked(&meta), "{meta:?}");
    assert_eq!(meta["events"], json!(["own1"]));
    // The request asked for a window of single events in order.
    let list = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.path == EVENTS_PATH)
        .unwrap();
    let q = |k: &str| {
        list.query
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(q("singleEvents").as_deref(), Some("true"));
    assert_eq!(q("orderBy").as_deref(), Some("startTime"));
    assert!(
        q("timeMin").unwrap().starts_with("2026-10-22T"),
        "{:?}",
        list.query
    );
    assert!(
        q("timeMax").unwrap().starts_with("2026-10-23T"),
        "{:?}",
        list.query
    );
}

#[tokio::test]
async fn an_invitation_from_a_stranger_is_external() {
    let r = rig_open(vec![
        events("c1", json!({"from": "2026-10-22"})),
        Scripted::text("Done."),
    ])
    .await;
    r.fake.json(
        "GET",
        EVENTS_PATH,
        json!({"items": [event("own1", "owner@example.test", true), event("inv7", "stranger@example.test", false)]}),
    );
    let sid = session(&r.core);
    turn(&r.core, &sid, "what is on").await;
    let (text, external, _, _) = result_of(&r, &sid, "calendar.events");
    assert!(
        text.contains("organizer: stranger@example.test\n")
            || text.contains("organizer: stranger@example.test ·"),
        "{text}"
    );
    // One event the account does not organize makes the whole listing outside text,
    // marked by that event.
    assert_eq!(external.as_deref(), Some("calendar:inv7"));
    assert!(crate::external::read_any(&r.core.store, &sid));
}

#[tokio::test]
async fn an_empty_day_and_a_failure_say_so() {
    let r = rig_open(vec![
        events("c1", json!({})),
        Scripted::text("Done."),
        events("c2", json!({"calendar": "other@example.test"})),
        Scripted::text("Done."),
    ])
    .await;
    let sid = session(&r.core);
    turn(&r.core, &sid, "what is on").await;
    let (text, external, status, _) = result_of(&r, &sid, "calendar.events");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.starts_with("No events on primary,"), "{text}");
    assert_eq!(external, None);
    // A calendar Google does not know is its words, and a row.
    r.fake.route("GET", "/calendar/v3/calendars/other@example.test/events", |_| {
        Reply::json(404, json!({"error": {"code": 404, "message": "Not Found", "errors": [{"reason": "notFound"}]}}))
    });
    turn(&r.core, &sid, "and theirs").await;
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.last().unwrap()["outcome"], "failed", "{rows:?}");
    assert_eq!(rows.last().unwrap()["error"], "notFound");
}

#[tokio::test]
async fn calendar_acl_and_calendar_delete_are_authority() {
    // `google.call`'s classifier, G1's, checked from the calendar's side.
    let r = rig_open(vec![]).await;
    let authority = |m: &str, params: Value| {
        plan_of(&r, "google.call", json!({"method": m, "params": params}))
            .unwrap()
            .authority
            .is_some()
    };
    for (m, p) in [
        ("calendar.acl.insert", json!({})),
        (
            "calendar.acl.update",
            json!({"ruleId": "user:x@example.test"}),
        ),
        (
            "calendar.acl.delete",
            json!({"ruleId": "user:x@example.test"}),
        ),
        ("calendar.calendars.insert", json!({})),
        (
            "calendar.calendars.delete",
            json!({"calendarId": "team@example.test"}),
        ),
        ("calendar.calendars.clear", json!({})),
        ("calendar.events.delete", json!({"eventId": "e1"})),
        (
            "calendar.events.insert",
            json!({"sendUpdates": "externalOnly"}),
        ),
        ("DELETE /calendar/v3/calendars/team@example.test", json!({})),
        ("POST /calendar/v3/calendars/primary/acl", json!({})),
    ] {
        assert!(authority(m, p), "{m} is authority");
    }
    for (m, p) in [
        ("calendar.acl.list", json!({})),
        ("calendar.calendars.get", json!({})),
        ("calendar.events.list", json!({})),
        ("calendar.events.insert", json!({})),
        ("calendar.events.insert", json!({"sendUpdates": "none"})),
    ] {
        assert!(!authority(m, p), "{m} is not authority");
    }
}

/// Phoenix, which keeps no daylight saving.
fn phoenix(_: u64) -> Zone {
    Zone {
        offset_secs: -7 * 3600,
        name: "MST".into(),
    }
}

#[test]
fn an_all_day_event_reads_in_local_time() {
    let at = |start: Value, end: Value| when(&json!({"start": start, "end": end}), &phoenix);
    // A UTC event just after midnight is the evening before, here.
    assert_eq!(
        at(
            json!({"dateTime": "2026-10-22T03:30:00Z"}),
            json!({"dateTime": "2026-10-22T04:15:00Z"})
        ),
        "Wed 2026-10-21 20:30 MST – 21:15 MST"
    );
    assert_eq!(
        at(
            json!({"dateTime": "2026-10-21T16:30:00-07:00"}),
            json!({"dateTime": "2026-10-22T01:00:00-07:00"})
        ),
        "Wed 2026-10-21 16:30 MST – Thu 2026-10-22 01:00 MST"
    );
    // An all-day event is the date Google names, whatever the zone: its end is
    // the day after its last.
    assert_eq!(
        at(json!({"date": "2026-10-21"}), json!({"date": "2026-10-22"})),
        "Wed 2026-10-21, all day"
    );
    assert_eq!(
        at(json!({"date": "2026-10-21"}), json!({"date": "2026-10-24"})),
        "Wed 2026-10-21 to Fri 2026-10-23, all day"
    );
    // A date in a query is its local midnight; a time is itself.
    let utc_of = |s: &str| crate::wake::parse_at(s).unwrap();
    assert_eq!(
        instant("2026-10-21", &phoenix).unwrap(),
        utc_of("2026-10-21T07:00:00Z")
    );
    assert_eq!(
        instant("2026-10-21T10:00:00-07:00", &phoenix).unwrap(),
        utc_of("2026-10-21T17:00:00Z")
    );
    assert!(instant("2026-13-40", &phoenix).is_err());
    // The default window starts at the local midnight, so 03:30 UTC is still yesterday's.
    assert_eq!(
        super::calendar::day_start(utc_of("2026-10-22T03:30:00Z"), &phoenix),
        utc_of("2026-10-21T07:00:00Z")
    );
    // The query's times carry an offset Google reads.
    let q = rfc3339(utc_of("2026-10-22T03:30:00Z"));
    assert_eq!(
        crate::wake::parse_at(&q).unwrap(),
        utc_of("2026-10-22T03:30:00Z"),
        "{q}"
    );
}

#[tokio::test]
async fn an_event_reads_in_the_daemons_own_zone_through_the_tool() {
    let r = rig_open(vec![
        events("c1", json!({"from": "2026-10-21", "to": "2026-10-23"})),
        Scripted::text("Done."),
    ])
    .await;
    let mut e = event("own1", "owner@example.test", true);
    e["start"] = json!({"dateTime": "2026-10-22T03:30:00Z"});
    e["end"] = json!({"dateTime": "2026-10-22T04:00:00Z"});
    r.fake.json(
        "GET",
        EVENTS_PATH,
        json!({"items": [e, {"id": "day1", "summary": "Offsite", "organizer": {"email": "owner@example.test", "self": true},
            "start": {"date": "2026-10-21"}, "end": {"date": "2026-10-22"}}]}),
    );
    let sid = session(&r.core);
    turn(&r.core, &sid, "what is on").await;
    let (text, _, _, _) = result_of(&r, &sid, "calendar.events");
    let ms = crate::wake::parse_at("2026-10-22T03:30:00Z").unwrap();
    // Whatever the machine's zone is (the gate's is America/Phoenix, where this
    // reads `Wed 2026-10-21 20:30 MST`), it is the zone the daemon reads.
    assert!(
        text.contains(&crate::recall::stamp::said_in(
            ms,
            &crate::recall::stamp::system
        )),
        "{text}"
    );
    assert!(text.contains("Offsite (day1)"), "{text}");
    assert!(text.contains(", all day"), "{text}");
    assert!(
        text.contains("Wed 2026-10-21, all day"),
        "the date is the date: {text}"
    );
}

#[tokio::test]
async fn what_the_model_gets_wrong_is_invalid_input_not_a_call() {
    let r = rig_open(vec![]).await;
    let bad = |i: Value| plan_of(&r, "calendar.put", i).unwrap_err();
    assert!(
        bad(json!({"start": "2026-10-22T16:00:00Z", "end": "2026-10-22T17:00:00Z"}))
            .contains("summary")
    );
    assert!(bad(json!({"summary": "x"})).contains("start"));
    assert!(bad(json!({"summary": "x", "start": "2026-10-22T16:00:00Z"})).contains("end"));
    assert!(bad(
        json!({"summary": "x", "start": "2026-10-22T17:00:00Z", "end": "2026-10-22T16:00:00Z"})
    )
    .contains("after"));
    assert!(bad(
        json!({"summary": "x", "start": "2026-10-22T16:00:00", "end": "2026-10-22T17:00:00"})
    )
    .contains("RFC 3339"));
    assert!(
        bad(json!({"summary": "x", "start": "2026-10-22", "end": "2026-10-22T17:00:00Z"}))
            .contains("both")
    );
    assert!(
        bad(json!({"summary": "x", "start": "2026-10-22", "attendees": ["not an address"]}))
            .contains("not an email")
    );
    assert!(bad(json!({"cancel": true})).contains("event_id"));
    assert!(bad(json!({"cancel": true, "event_id": "e", "summary": "x"})).contains("only"));
    assert!(bad(json!({"event_id": "e"})).contains("nothing to change"));
    assert!(
        bad(json!({"summary": "x", "start": "2026-10-22", "extra": 1})).contains("invalid input")
    );
    let ev = |i: Value| plan_of(&r, "calendar.events", i).unwrap_err();
    assert!(ev(json!({"from": "tomorrow"})).contains("RFC 3339"));
    assert!(ev(json!({"from": "2026-10-23", "to": "2026-10-22"})).contains("after"));
    // An all-day event takes dates, its end the last day; a change names its fields.
    let day = plan_of(
        &r,
        "calendar.put",
        json!({"summary": "Offsite", "start": "2026-10-21", "end": "2026-10-23"}),
    )
    .unwrap();
    assert!(
        day.authority.is_none() && day.summary.starts_with("create event"),
        "{day:?}"
    );
}

#[tokio::test]
async fn an_all_day_event_is_sent_with_the_day_after_its_last() {
    let r = rig_open(vec![
        put(
            "c1",
            json!({"summary": "Offsite", "start": "2026-10-21", "end": "2026-10-23"}),
        ),
        Scripted::text("Booked."),
    ])
    .await;
    let sid = session(&r.core);
    turn(&r.core, &sid, "block the days").await;
    let post = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.verb == "POST")
        .expect("created");
    assert_eq!(post.body["start"], json!({"date": "2026-10-21"}));
    assert_eq!(post.body["end"], json!({"date": "2026-10-24"}));
}

#[test]
fn both_tools_are_deferred_in_the_calendar_family_and_small() {
    use crate::tools_defer::family::{of_wire, PlaceKind};
    // Tokens as the request estimates them: bytes at 2.4 to the token.
    const MAX_TOKENS: usize = 220;
    let fake = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let r = fake.block_on(rig(vec![]));
    for name in [super::calendar::EVENTS, super::calendar::PUT] {
        let t = r.core.tools.registry.get(name).expect("registered").clone();
        let wire = name.replace('.', "_");
        let def =
            json!({"name": wire, "description": t.description(), "input_schema": t.input_schema()});
        let tokens = def.to_string().len() * 10 / 24;
        assert!(
            tokens <= MAX_TOKENS,
            "{name} is {tokens} tokens, over {MAX_TOKENS}"
        );
        assert_eq!(of_wire(&wire), "calendar");
        for kind in PlaceKind::ALL {
            assert!(
                !kind.defaults().contains(&"calendar"),
                "{name} is loaded by default in {kind:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_gmail_added_event_is_outside_text() {
    let r = rig_open(vec![]).await;
    let own = |id: &str| {
        json!({"id": id, "summary": "Design review", "eventType": "default",
            "organizer": {"email": "owner@example.test", "self": true},
            "creator": {"email": "owner@example.test", "self": true},
            "start": {"dateTime": "2026-10-22T15:00:00Z"}, "end": {"dateTime": "2026-10-22T16:00:00Z"}})
    };
    let external = |items: Vec<Value>| {
        let r = &r;
        async move {
            r.fake.json("GET", EVENTS_PATH, json!({ "items": items }));
            let t = r
                .core
                .tools
                .registry
                .get("calendar.events")
                .unwrap()
                .clone();
            let (_, ext) = t
                .run_async(&json!({}), &ToolCtx::for_tests(r.dir.path()))
                .await
                .unwrap_or_else(|e| panic!("{}", e.message));
            ext.is_some()
        }
    };
    assert!(
        !external(vec![own("own1")]).await,
        "the account's own event"
    );
    let mut mailed = own("fl1");
    mailed["eventType"] = "fromGmail".into();
    assert!(
        external(vec![mailed]).await,
        "an event Gmail added from mail"
    );
    let mut theirs = own("cr1");
    theirs["creator"] = json!({"email": "stranger@example.test"});
    assert!(
        external(vec![theirs]).await,
        "an event another address created"
    );
    // A creator written in another case is still the account.
    let mut cased = own("cs1");
    cased["creator"] = json!({"email": "Owner@Example.test"});
    cased["organizer"] = json!({"email": "Owner@Example.test"});
    assert!(!external(vec![cased]).await);
}

// ------------------------------------------- the configured calendars (jid7t.1)

mod configured {
    use serde_json::{json, Value};
    use theseus_tools::ToolCtx;

    use super::super::tests::Rig;
    use super::super::tests_calendar_cache::{cal_rig, ev, lists, NOON, PERSONAL, WORK};

    async fn run(
        r: &Rig,
        input: Value,
    ) -> Result<
        (theseus_tools::ToolOutput, Option<theseus_tools::External>),
        theseus_tools::ToolFailure,
    > {
        let tool = r
            .core
            .tools
            .registry
            .get("calendar.events")
            .unwrap()
            .clone();
        tool.run_async(&input, &ToolCtx::for_tests(r.dir.path()))
            .await
    }

    async fn seeded() -> (Rig, super::super::tests_calendar_cache::Cals) {
        let (r, cals) = cal_rig().await;
        cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
        cals.work.lock().unwrap().items = vec![ev("w1", "Board prep", NOON.0, NOON.1)];
        (r, cals)
    }

    const WINDOW: (&str, &str) = ("2026-10-22", "2026-10-23");

    #[tokio::test]
    async fn a_calendar_is_read_by_label_or_by_id_and_a_sensitive_one_marks_the_result() {
        let (r, _cals) = seeded().await;
        let (from, to) = WINDOW;
        // By label, in any case: the sensitive calendar, so the result says so.
        let (out, _) = run(&r, json!({"calendar": "work", "from": from, "to": to}))
            .await
            .unwrap();
        assert!(
            out.text.contains("Board prep") && !out.text.contains("Dentist"),
            "{}",
            out.text
        );
        assert_eq!(out.meta["sensitive"], true);
        assert_eq!(out.meta["private"], true);
        assert_eq!(lists(&r.fake, WORK).len(), 1);
        assert!(lists(&r.fake, PERSONAL).is_empty());
        // By id, and by a summary the daemon read: the personal calendar is private only.
        for name in [PERSONAL, "Personal"] {
            let (out, _) = run(&r, json!({"calendar": name, "from": from, "to": to}))
                .await
                .unwrap();
            assert!(
                out.text.contains("Dentist") && !out.text.contains("Board prep"),
                "{}",
                out.text
            );
            assert_eq!(out.meta["private"], true, "{name}");
            assert!(out.meta.get("sensitive").is_none(), "{name}: {}", out.meta);
        }
        // The row names the label, never an event.
        assert_eq!(out_row(&r, "Work").await["resource"], "Work");
    }

    async fn out_row(r: &Rig, label: &str) -> Value {
        let (out, _) = run(
            r,
            json!({"calendar": label, "from": "2026-10-22", "to": "2026-10-23"}),
        )
        .await
        .unwrap();
        out.meta["google_called"].clone()
    }

    #[tokio::test]
    async fn calendars_all_reads_every_one_with_each_line_labelled() {
        let (r, _cals) = seeded().await;
        let (out, _) = run(
            &r,
            json!({"calendars": "all", "from": "2026-10-22", "to": "2026-10-23"}),
        )
        .await
        .unwrap();
        let lines: Vec<&str> = out.text.lines().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(lines.len(), 2, "{}", out.text);
        assert!(
            lines[0].starts_with("- [Personal] ") && lines[0].contains("Dentist"),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("- [Work] ") && lines[1].contains("Board prep"),
            "{lines:?}"
        );
        assert_eq!(
            out.meta["sensitive"], true,
            "one sensitive calendar marks the whole read"
        );
        // Every call has its row: the names, then the two lists.
        let also = out.meta["google_called_also"].as_array().unwrap().len();
        assert!(also >= 2, "{}", out.meta);
        // One calendar failing is said in a line and blocks no other.
        let (r2, cals2) = seeded().await;
        cals2.personal.lock().unwrap().mode = super::super::tests_calendar_cache::Mode::NotFound;
        let (out, _) = run(
            &r2,
            json!({"calendars": "all", "from": "2026-10-22", "to": "2026-10-23"}),
        )
        .await
        .unwrap();
        assert!(
            out.text.contains("Personal unreachable") && out.text.contains("[Work] "),
            "{}",
            out.text
        );
    }

    #[tokio::test]
    async fn an_unknown_label_is_refused_with_the_configured_labels() {
        let (r, _cals) = seeded().await;
        let e = run(&r, json!({"calendar": "Weekend"})).await.unwrap_err();
        assert!(e.message.contains("Personal, Work"), "{}", e.message);
        let both = run(&r, json!({"calendar": "Work", "calendars": "all"}))
            .await
            .unwrap_err();
        assert!(both.message.contains("not both"), "{}", both.message);
        assert!(
            r.fake.log().iter().all(|l| !l.path.ends_with("/events")),
            "nothing was read"
        );
        // An address the config does not list is read as it always was.
        let _ = run(&r, json!({"calendar": "other.cal@example.test"})).await;
        assert!(r
            .fake
            .log()
            .iter()
            .any(|l| l.path.contains("other.cal@example.test")
                || l.path.contains("other.cal%40example.test")));
    }

    #[tokio::test]
    async fn the_default_config_reads_primary_as_it_always_did() {
        let r = super::super::tests::rig(vec![]).await;
        r.fake.json(
            "GET",
            "/calendar/v3/calendars/primary/events",
            json!({"items": [ev("e1", "Standup", NOON.0, NOON.1)]}),
        );
        let (out, _) = run(&r, json!({"from": "2026-10-22", "to": "2026-10-23"}))
            .await
            .unwrap();
        assert!(out.text.contains("Standup") && out.meta.get("sensitive").is_none());
        assert!(
            r.fake
                .log()
                .iter()
                .all(|l| l.path != "/calendar/v3/users/me/calendarList"),
            "no names are read for the primary calendar alone"
        );
    }
}
