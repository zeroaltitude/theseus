//! The calendar cache, `calendar.window` and the reachability check
//! (theseus-jid7t.1), against a loopback Google that serves two calendars
//! under invented ids: a personal one and a work one marked sensitive. The
//! fixture here (`install`, `cal_rig`) is shared with the sensitive-level and
//! morning tests.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_google::fake::{Fake, Reply, REFRESH_TOKEN};
use theseus_protocol::calendar::{CalendarStatus, CalendarWindowResult};
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::tests::*;
use crate::approval::{Client, Surface};
use crate::config::{CalendarEntry, CalendarTable};
use crate::provider::Scripted;
use crate::{Config, Core};

pub(super) const PERSONAL: &str = "personal.cal.example.test";
pub(super) const WORK: &str = "work.cal.example.test";

/// How the fake answers one calendar's `events.list`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Ok,
    NotFound,
    /// 403 for the events; `freebusy.query` answers.
    BusyOnly,
    /// 403 for the events, and `freebusy.query` fails too.
    Forbidden,
}

pub(super) struct Cal {
    pub items: Vec<Value>,
    /// What a `syncToken` read returns.
    pub delta: Vec<Value>,
    pub mode: Mode,
    /// A `syncToken` read answers 410.
    pub gone: bool,
    /// The busy blocks `freebusy.query` answers (RFC 3339 start, end).
    pub busy: Vec<(String, String)>,
}

pub(super) struct Cals {
    pub personal: Arc<Mutex<Cal>>,
    pub work: Arc<Mutex<Cal>>,
}

pub(super) fn ev(id: &str, title: &str, start: &str, end: &str) -> Value {
    json!({"id": id, "summary": title, "status": "confirmed",
        "updated": "2026-10-20T10:00:00Z", "location": "Room 4",
        "htmlLink": format!("https://calendar.example.test/e/{id}"),
        "start": {"dateTime": start}, "end": {"dateTime": end},
        "organizer": {"email": ACCOUNT, "self": true},
        "hangoutLink": "https://meet.example.test/x-y-z",
        "attendees": [{"email": "guest.one@example.test", "self": true, "responseStatus": "accepted"},
                      {"email": "guest.two@example.test"}]})
}

fn error(code: u16, reason: &str) -> Reply {
    Reply::json(
        code,
        json!({"error": {"code": code, "message": reason, "errors": [{"reason": reason}]}}),
    )
}

fn calendar(fake: &Fake, id: &str, items: Vec<Value>) -> Arc<Mutex<Cal>> {
    let cal = Arc::new(Mutex::new(Cal {
        items,
        delta: Vec::new(),
        mode: Mode::Ok,
        gone: false,
        busy: Vec::new(),
    }));
    let c = cal.clone();
    fake.route(
        "GET",
        &format!("/calendar/v3/calendars/{id}/events"),
        move |req| {
            let c = c.lock().unwrap();
            let sync = req.query().iter().any(|(k, _)| k == "syncToken");
            match c.mode {
                Mode::NotFound => error(404, "notFound"),
                Mode::BusyOnly | Mode::Forbidden => error(403, "forbidden"),
                Mode::Ok if sync && c.gone => error(410, "gone"),
                Mode::Ok if sync => {
                    Reply::json(200, json!({"items": c.delta, "nextSyncToken": "sync-2"}))
                }
                Mode::Ok => Reply::json(200, json!({"items": c.items, "nextSyncToken": "sync-1"})),
            }
        },
    );
    cal
}

/// Serve the two calendars, their names (`calendarList`) and free/busy.
pub(super) fn install(fake: &Fake) -> Cals {
    let personal = calendar(fake, PERSONAL, Vec::new());
    let work = calendar(fake, WORK, Vec::new());
    fake.json(
        "GET",
        "/calendar/v3/users/me/calendarList",
        json!({"items": [
            {"id": PERSONAL, "summary": "Personal", "primary": true},
            {"id": WORK, "summary": "Team calendar"}]}),
    );
    let both = [(PERSONAL, personal.clone()), (WORK, work.clone())];
    fake.route("POST", "/calendar/v3/freeBusy", move |req| {
        let asked = req.json()["items"][0]["id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let Some((_, cal)) = both.iter().find(|(id, _)| *id == asked) else {
            return error(404, "notFound");
        };
        let c = cal.lock().unwrap();
        let busy: Vec<Value> = c
            .busy
            .iter()
            .map(|(s, e)| json!({"start": s, "end": e}))
            .collect();
        let entry = match c.mode {
            Mode::Forbidden => json!({"errors": [{"reason": "notFound"}], "busy": []}),
            _ => json!({"busy": busy}),
        };
        Reply::json(200, json!({"calendars": {asked: entry}}))
    });
    Cals { personal, work }
}

/// The two calendars, the second sensitive and labelled `Work`.
pub(super) fn two(c: &mut Config) {
    c.policy.enforcement = crate::policy::Posture::Open;
    c.policy
        .tools
        .insert("calendar.events".into(), crate::policy::Posture::Open);
    c.google.calendars = vec![
        CalendarEntry::Table(CalendarTable {
            id: PERSONAL.into(),
            label: None,
            color: None,
            sensitive: false,
        }),
        CalendarEntry::Table(CalendarTable {
            id: WORK.into(),
            label: Some("Work".into()),
            color: None,
            sensitive: true,
        }),
    ];
}

pub(super) async fn cal_rig_with(
    script: Vec<Scripted>,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, Cals) {
    let r = rig_with(
        Some(REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        |c| {
            two(c);
            tweak(c);
        },
    )
    .await;
    let cals = install(&r.fake);
    (r, cals)
}

pub(super) async fn cal_rig() -> (Rig, Cals) {
    cal_rig_with(vec![], |_| {}).await
}

/// The window of 2026-10-22 (UTC midnight to midnight).
pub(super) fn day() -> (u64, u64) {
    let from = crate::wake::parse_at("2026-10-22T00:00:00Z").unwrap();
    (from, from + 86_400_000)
}

pub(super) const NOON: (&str, &str) = ("2026-10-22T16:00:00Z", "2026-10-22T16:30:00Z");

/// The `events.list` requests the fake saw for `id`.
pub(super) fn lists(fake: &Fake, id: &str) -> Vec<Vec<(String, String)>> {
    fake.log()
        .into_iter()
        .filter(|l| l.verb == "GET" && l.path == format!("/calendar/v3/calendars/{id}/events"))
        .map(|l| l.query)
        .collect()
}

fn token_of(q: &[(String, String)]) -> Option<&str> {
    q.iter()
        .find(|(k, _)| k == "syncToken")
        .map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn a_window_read_twice_within_five_minutes_makes_one_list() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    let (from, to) = day();
    let first = r.g.calendar_window(from, to).await;
    let second = r.g.calendar_window(from + 3_600_000, to).await;
    assert_eq!(
        lists(&r.fake, PERSONAL).len(),
        1,
        "one list for the personal calendar"
    );
    assert_eq!(lists(&r.fake, WORK).len(), 1);
    assert_eq!(first.calendars[0].events.len(), 1);
    assert_eq!(
        second.calendars[0].events.len(),
        1,
        "the narrower window is served kept"
    );
    // The names: the configured label, else the calendar's own summary.
    let names: Vec<_> = first.calendars.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(names, ["Personal", "Work"]);
    assert_eq!(first.calendars[0].color, "#c9a24a");
    assert_eq!(first.calendars[1].color, "#5fa08d");
    assert!(first.calendars[1].sensitive && !first.calendars[0].sensitive);
    let e = &first.calendars[0].events[0];
    assert_eq!(
        (e.title.as_deref(), e.location.as_deref(), e.attendees),
        (Some("Dentist"), Some("Room 4"), 2)
    );
    assert_eq!(e.self_response.as_deref(), Some("accepted"));
    assert!(e.organizer_self && !e.all_day && !e.busy);
    // The key is stable across reads and opaque.
    assert_eq!(e.key, second.calendars[0].events[0].key);
    assert!(
        !e.key.contains("p1") && !e.key.contains(PERSONAL),
        "{}",
        e.key
    );
    // calendarList is read once, however many windows follow.
    let names_read = |f: &Fake| {
        f.log()
            .iter()
            .filter(|l| l.path == "/calendar/v3/users/me/calendarList")
            .count()
    };
    assert_eq!(names_read(&r.fake), 1);
    r.g.calendar_window(from, to).await;
    assert_eq!(names_read(&r.fake), 1);
}

#[tokio::test]
async fn after_five_minutes_the_refresh_sends_the_sync_token_and_merges_the_changes() {
    let (r, cals) = cal_rig().await;
    {
        let mut p = cals.personal.lock().unwrap();
        p.items = vec![
            ev("p1", "Dentist", NOON.0, NOON.1),
            ev(
                "p2",
                "Lunch",
                "2026-10-22T19:00:00Z",
                "2026-10-22T20:00:00Z",
            ),
        ];
        p.delta = vec![
            ev(
                "p1",
                "Dentist (moved)",
                "2026-10-22T17:00:00Z",
                "2026-10-22T17:30:00Z",
            ),
            json!({"id": "p2", "status": "cancelled"}),
        ];
    }
    let (from, to) = day();
    let first = r.g.calendar_window(from, to).await;
    assert!(first.changed.is_empty(), "a first read is no change");
    r.g.calendars.age(Duration::from_secs(299));
    r.g.calendar_window(from, to).await;
    assert_eq!(lists(&r.fake, PERSONAL).len(), 1, "299 s is still fresh");
    r.g.calendars.age(Duration::from_secs(2));
    let again = r.g.calendar_window(from, to).await;
    let reads = lists(&r.fake, PERSONAL);
    assert_eq!(reads.len(), 2);
    assert_eq!(token_of(&reads[0]), None);
    assert_eq!(
        token_of(&reads[1]),
        Some("sync-1"),
        "the refresh sends the token"
    );
    let titles: Vec<_> = again.calendars[0]
        .events
        .iter()
        .map(|e| e.title.clone().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Dentist (moved)"],
        "the change merged, the cancelled one gone"
    );
    assert_eq!(again.changed, ["Personal"]);
    // Nothing changed since: the next refresh finds none.
    r.g.calendars.age(Duration::from_secs(301));
    cals.personal.lock().unwrap().delta = Vec::new();
    let quiet = r.g.calendar_window(from, to).await;
    assert!(quiet.changed.is_empty(), "{:?}", quiet.changed);
    assert_eq!(token_of(&lists(&r.fake, PERSONAL)[2]), Some("sync-2"));
}

#[tokio::test]
async fn a_410_drops_the_token_and_reads_the_window_again() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    let (from, to) = day();
    r.g.calendar_window(from, to).await;
    {
        let mut p = cals.personal.lock().unwrap();
        p.gone = true;
        p.items = vec![ev("p9", "Physio", NOON.0, NOON.1)];
    }
    r.g.calendars.age(Duration::from_secs(301));
    let again = r.g.calendar_window(from, to).await;
    let reads = lists(&r.fake, PERSONAL);
    assert_eq!(reads.len(), 3, "the read, the stale token, the read again");
    assert_eq!(token_of(&reads[1]), Some("sync-1"));
    assert_eq!(token_of(&reads[2]), None);
    assert_eq!(again.calendars[0].status, CalendarStatus::Ok);
    assert_eq!(
        again.calendars[0].events[0].title.as_deref(),
        Some("Physio")
    );
    assert_eq!(again.changed, ["Personal"], "a different set is a change");
}

#[tokio::test]
async fn a_403_gives_busy_blocks_with_no_title() {
    let (r, cals) = cal_rig().await;
    {
        let mut w = cals.work.lock().unwrap();
        w.mode = Mode::BusyOnly;
        w.busy = vec![(NOON.0.into(), NOON.1.into())];
    }
    let (from, to) = day();
    let read = r.g.calendar_window(from, to).await;
    let work = &read.calendars[1];
    assert_eq!(work.status, CalendarStatus::BusyOnly);
    assert!(work.detail.contains("free/busy"), "{}", work.detail);
    let e = &work.events[0];
    assert!(e.busy && e.title.is_none() && e.location.is_none() && e.call.is_none());
    assert_eq!((e.attendees, e.html_link.as_str()), (0, ""));
    // Neither the other calendar nor the next read is disturbed.
    assert_eq!(read.calendars[0].status, CalendarStatus::Ok);
    // A busy window is read from the start when it goes stale: events first.
    r.g.calendars.age(Duration::from_secs(301));
    r.g.calendar_window(from, to).await;
    assert_eq!(lists(&r.fake, WORK).len(), 2);
}

#[tokio::test]
async fn a_calendar_that_fails_says_why_and_blocks_no_other() {
    let (r, cals) = cal_rig().await;
    cals.work.lock().unwrap().mode = Mode::NotFound;
    cals.personal.lock().unwrap().mode = Mode::Forbidden;
    let (from, to) = day();
    let read = r.g.calendar_window(from, to).await;
    let [personal, work] = &read.calendars[..] else {
        panic!()
    };
    assert_eq!(personal.status, CalendarStatus::Forbidden);
    assert_eq!(work.status, CalendarStatus::NotFound);
    assert!(
        work.detail.contains("not shared") && work.detail.contains(ACCOUNT),
        "{}",
        work.detail
    );
    assert!(personal.events.is_empty() && work.events.is_empty());
}

#[tokio::test]
async fn the_check_reports_each_calendar_by_label() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    let (lines, rows) = r.g.calendar_reach().await;
    let said: Vec<_> = lines.iter().map(|(l, s, _)| (l.as_str(), *s)).collect();
    assert_eq!(
        said,
        [
            ("Personal", CalendarStatus::Ok),
            ("Work", CalendarStatus::Ok)
        ]
    );
    assert!(!rows.is_empty() && !serde_json::to_string(&rows).unwrap().contains("Dentist"));
    // 404: not shared, or a wrong id.
    cals.work.lock().unwrap().mode = Mode::NotFound;
    assert_eq!(r.g.calendar_reach().await.0[1].1, CalendarStatus::NotFound);
    // 403 with free/busy answering.
    cals.work.lock().unwrap().mode = Mode::BusyOnly;
    let (lines, _) = r.g.calendar_reach().await;
    assert_eq!(lines[1].1, CalendarStatus::BusyOnly);
    // 403 and free/busy fails too.
    cals.work.lock().unwrap().mode = Mode::Forbidden;
    assert_eq!(r.g.calendar_reach().await.0[1].1, CalendarStatus::Forbidden);
    // The probe asks for one event.
    assert!(lists(&r.fake, PERSONAL)
        .iter()
        .all(|q| q.iter().any(|(k, v)| k == "maxResults" && v == "1")));
}

#[tokio::test]
async fn the_check_says_auth_lapsed_when_consent_is_gone() {
    let (r, _cals) = cal_rig().await;
    r.fake.set_invalid_grant(true);
    let (lines, _) = r.g.calendar_reach().await;
    assert!(
        lines
            .iter()
            .all(|(_, s, d)| *s == CalendarStatus::AuthLapsed && d.contains("theseus google auth")),
        "{lines:?}"
    );
    let read = r.g.calendar_window(day().0, day().1).await;
    assert!(read
        .calendars
        .iter()
        .all(|c| c.status == CalendarStatus::AuthLapsed));
}

// ------------------------------------------------------------- calendar.window

/// One request on a connection of `client`'s surface: its result or error.
pub(super) async fn rpc_as(
    core: &Arc<Core>,
    client: Client,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (_, e) => Err(format!("{e:?}")),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

pub(super) fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

fn window_params() -> Value {
    let (from_ms, to_ms) = day();
    json!({"from_ms": from_ms, "to_ms": to_ms})
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_on_a_private_connection_reads_both_calendars() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    cals.work.lock().unwrap().mode = Mode::NotFound;
    let v = rpc_as(&r.core, cli(), "calendar.window", window_params())
        .await
        .unwrap();
    let w: CalendarWindowResult = serde_json::from_value(v).unwrap();
    assert_eq!(w.calendars.len(), 2);
    assert_eq!(w.calendars[0].id, PERSONAL);
    assert_eq!(w.calendars[0].status, CalendarStatus::Ok);
    assert_eq!(w.calendars[0].events[0].title.as_deref(), Some("Dentist"));
    assert_eq!(w.calendars[1].status, CalendarStatus::NotFound);
    assert!(w.calendars[1].sensitive && w.calendars[1].color == "#5fa08d");
    // The rows of the reads are in the ledger, and hold no event.
    let called = ledgered(&r.core, "google.called");
    assert!(called.iter().any(|c| c["method"] == "calendar.events.list"));
    assert!(!serde_json::to_string(&called).unwrap().contains("Dentist"));
}

#[tokio::test(flavor = "multi_thread")]
async fn anyone_else_is_refused_and_gets_no_event_data() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    for (client, said) in [
        // Discord with no private place named is no owner in a private place.
        (
            Client::new("discord", Surface::Discord),
            "calendar.window is the owner's",
        ),
        // An MCP client never reaches it at all.
        (
            Client::new("mcp#1", Surface::Mcp),
            "may not call calendar.window",
        ),
    ] {
        let e = rpc_as(&r.core, client, "calendar.window", window_params())
            .await
            .unwrap_err();
        assert!(e.contains(said), "{e}");
        assert!(!e.contains("Dentist") && !e.contains("Room 4"), "{e}");
    }
    assert!(
        r.fake.log().iter().all(|l| !l.path.contains("/events")),
        "nothing was read for them"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_span_over_62_days_is_refused() {
    let (r, _cals) = cal_rig().await;
    let (from, _) = day();
    let e = rpc_as(
        &r.core,
        cli(),
        "calendar.window",
        json!({"from_ms": from, "to_ms": from + 63 * 86_400_000}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("at most 62 days"), "{e}");
    let ok = rpc_as(
        &r.core,
        cli(),
        "calendar.window",
        json!({"from_ms": from, "to_ms": from + 62 * 86_400_000}),
    )
    .await;
    assert!(ok.is_ok(), "{ok:?}");
    let backwards = rpc_as(
        &r.core,
        cli(),
        "calendar.window",
        json!({"from_ms": from, "to_ms": from}),
    )
    .await;
    assert!(backwards.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_google_every_calendar_is_off_and_nothing_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap();
    let v = rpc_as(&core, cli(), "calendar.window", window_params())
        .await
        .unwrap();
    let w: CalendarWindowResult = serde_json::from_value(v).unwrap();
    assert_eq!(w.calendars.len(), 1);
    assert_eq!(w.calendars[0].status, CalendarStatus::Off);
    assert!(w.calendars[0].events.is_empty() && w.calendars[0].detail.contains("not set up"));
}

/// `calendar.changed` carries labels only, goes out once for a change and not
/// for a refresh that finds none, and not at all before a window was read.
#[tokio::test(flavor = "multi_thread")]
async fn calendar_changed_is_sent_once_on_a_change_and_never_with_content() {
    let (r, cals) = cal_rig().await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    *r.core.bus.tap.lock().unwrap() = Some(tx);
    let heard = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>| {
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            if let Message::Notification(n) = m {
                if n.method == "calendar.changed" {
                    out.push(n.params);
                }
            }
        }
        out
    };
    // Nothing was read: nothing is refreshed, nothing is sent.
    r.g.calendars.age(Duration::from_secs(900));
    assert!(r.g.calendar_refresh().await.changed.is_empty());
    assert!(heard(&mut rx).is_empty());

    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    rpc_as(&r.core, cli(), "calendar.window", window_params())
        .await
        .unwrap();
    assert!(heard(&mut rx).is_empty(), "a first read is no change");

    // A change: the refresh finds it, and one notification goes out.
    cals.personal.lock().unwrap().delta =
        vec![ev("p1", "Dentist (moved)", NOON.0, "2026-10-22T16:45:00Z")];
    r.g.calendars.age(Duration::from_secs(301));
    let read = r.g.calendar_refresh().await;
    r.core.calendar_told(read.rows, read.changed);
    let sent = heard(&mut rx);
    assert_eq!(sent, [json!({"labels": ["Personal"]})], "{sent:?}");
    assert!(!sent[0].to_string().contains("Dentist"));

    // Another refresh with nothing new: nothing is sent.
    cals.personal.lock().unwrap().delta = Vec::new();
    r.g.calendars.age(Duration::from_secs(301));
    let read = r.g.calendar_refresh().await;
    assert!(read.changed.is_empty());
    r.core.calendar_told(read.rows, read.changed);
    assert!(heard(&mut rx).is_empty());
}

/// `theseusd check`'s lines: one a calendar, by label, and never an event.
#[tokio::test]
async fn the_checks_lines_name_each_calendar_by_label() {
    let (r, cals) = cal_rig().await;
    cals.personal.lock().unwrap().items = vec![ev("p1", "Dentist", NOON.0, NOON.1)];
    cals.work.lock().unwrap().mode = Mode::NotFound;
    let lines = super::calendar_check(&r.core.cfg, &board(Some(REFRESH_TOKEN))).await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0], "google calendar Personal: ok");
    assert!(
        lines[1].starts_with("google calendar Work: not_found: "),
        "{}",
        lines[1]
    );
    assert!(!lines.join("\n").contains("Dentist"));
    // With no [google] there is nothing to say.
    let mut none = (*r.core.cfg).clone();
    none.google = Default::default();
    assert!(super::calendar_check(&none, &board(None)).await.is_empty());
}
