//! The calendar's two tools (the Google design §3.1, rows 6 and 7).
//!
//! - **`calendar.events`**: events in a window, with their attendees and
//!   responses, the call link and the organizer, in the daemon's own time zone
//!   (an all-day event is a date, never moved by the zone). A read; the result
//!   is outside text unless the account organizes every event in it
//!   (`organizer.self`, handed to [`finish`]).
//! - **`calendar.put`**: create, change or cancel an event. **Authority** (asked
//!   at every posture, whatever a `[policy.tools]` line says) whenever people
//!   may be told or added: attendees in the call, a cancellation, or a change
//!   that notifies the attendees of an event that already exists. Google's
//!   `sendUpdates` only decides whether it *emails*; an attendee's calendar
//!   gains or loses the event either way, so the plan never reads
//!   `notify_attendees: false` as "no one is reached". The plan sees the call,
//!   never the event, so a change to an existing event with `notify_attendees`
//!   false and no attendees named runs at its posture.
//!
//! Both go through [`Google::call`] and end in [`finish`].

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use theseus_google::methods::resolve;
use theseus_google::{Class, Repeat, Request};
use theseus_tools::{parse, AsyncRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure};

use super::external::{finish, Finished, Source};
use super::{Google, Meta};
use crate::recall::stamp::{self, Zone, ZoneAt};

pub const EVENTS: &str = "calendar.events";
pub const PUT: &str = "calendar.put";

/// The tools [`Google::tools`] adds for the calendar.
pub fn tools(g: &Arc<Google>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Events(g.clone())), Arc::new(Put(g.clone()))]
}

const DAY_MS: u64 = 86_400_000;
/// Events a call returns when `max` is not given, and at most.
const MAX_DEFAULT: u32 = 25;
const MAX_MOST: u32 = 100;

/// UTC, for a date: an all-day event's day is the date Google names.
fn utc(_: u64) -> Zone {
    Zone {
        offset_secs: 0,
        name: "UTC".into(),
    }
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-'
}

/// A date's UTC midnight, or the words for what is wrong with it.
fn date_ms(s: &str) -> Result<u64, String> {
    crate::wake::parse_at(&format!("{s}T00:00:00Z"))
        .map_err(|_| format!("`{s}` is not a date: write 2026-10-21"))
}

/// `2026-10-21` for the UTC day of `ms`.
fn date_of(ms: u64) -> String {
    stamp::day_in(ms, &utc)[4..].to_string()
}

/// The instant `s` names: an RFC 3339 time with its offset, or a date, which
/// is that day's first moment in the daemon's zone.
pub(super) fn instant(s: &str, zone: ZoneAt<'_>) -> Result<u64, String> {
    if !is_date(s) {
        return crate::wake::parse_at(s);
    }
    let guess = date_ms(s)?;
    // The offset at the midnight itself, which daylight saving may have moved.
    let off = |ms: u64| zone(ms).offset_secs * 1000;
    let first = guess.saturating_add_signed(-off(guess));
    Ok(guess.saturating_add_signed(-off(first)))
}

/// The first moment of `ms`'s day in `zone`.
pub(super) fn day_start(ms: u64, zone: ZoneAt<'_>) -> u64 {
    let off = zone(ms).offset_secs;
    let local = (ms / 1000) as i64 + off;
    let start = local - local.rem_euclid(86_400) - off;
    start.max(0) as u64 * 1000
}

/// `ms` as an RFC 3339 time with the daemon's offset, for a query.
pub(super) fn rfc3339(ms: u64) -> String {
    let full = crate::wake::local(ms).full();
    let (day, rest) = full.split_once(' ').unwrap_or((&full, ""));
    format!("{day}T{}", rest.replace(' ', ""))
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// An event's time as people read it: `Wed 2026-10-21 09:00 MST – 09:30 MST`,
/// or, for an all-day event, `Wed 2026-10-21, all day` (a span names its last
/// day: Google's `end.date` is the day after).
pub(super) fn when(e: &Value, zone: ZoneAt<'_>) -> String {
    if let Some(start) = text(e, &["start", "date"]) {
        let first_ms = date_ms(start);
        let first = first_ms.clone().map(|ms| stamp::day_in(ms, &utc));
        let last_ms = text(e, &["end", "date"])
            .and_then(|d| date_ms(d).ok())
            .map(|ms| ms.saturating_sub(DAY_MS));
        let last = last_ms.map(|ms| stamp::day_in(ms, &utc));
        return match (first, last) {
            (Ok(f), Some(l))
                if first_ms
                    .as_ref()
                    .is_ok_and(|f| last_ms.is_some_and(|l| l > *f)) =>
            {
                format!("{f} to {l}, all day")
            }
            (Ok(f), _) => format!("{f}, all day"),
            (Err(_), _) => format!("{start}, all day"),
        };
    }
    let at = |k: &str| {
        text(e, &[k, "dateTime"])
            .and_then(|t| crate::wake::parse_at(t).ok())
            .map(|ms| stamp::said_in(ms, zone))
    };
    match (at("start"), at("end")) {
        (Some(s), Some(e)) => match e.strip_prefix(&s[..14]) {
            Some(rest) => format!("{s} –{rest}"),
            None => format!("{s} – {e}"),
        },
        (Some(s), None) => s,
        _ => "(no time)".into(),
    }
}

/// Whether the account organizes `e` (Google's `organizer.self`).
fn organized_by(e: &Value, account: &str) -> bool {
    e["organizer"]["self"] == true
        || text(e, &["organizer", "email"]).is_some_and(|m| m.eq_ignore_ascii_case(account))
}

/// The event's video call: Meet's link, else a conference's video entry point.
fn call_link(e: &Value) -> Option<String> {
    text(e, &["hangoutLink"]).map(str::to_string).or_else(|| {
        e["conferenceData"]["entryPoints"]
            .as_array()?
            .iter()
            .find(|p| p["entryPointType"] == "video")
            .and_then(|p| text(p, &["uri"]))
            .map(str::to_string)
    })
}

/// An event as the morning note reads it (`initiative/morning_google.rs`).
pub(crate) struct Brief {
    /// The start and end (unix ms), none for an all-day event.
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    /// The event's time as people read it (`when`).
    pub when: String,
    pub title: String,
    pub attendees: usize,
    pub call: Option<String>,
}

/// The request for today's events: from the local midnight to the next.
pub(crate) fn today(now: u64) -> Result<Request, String> {
    let from = day_start(now, &stamp::system);
    // Noon of the next day is on that day, whatever daylight saving did.
    let to = day_start(from + DAY_MS + DAY_MS / 2, &stamp::system);
    let input = json!({"from": rfc3339(from), "to": rfc3339(to), "max": MAX_MOST});
    listing(&input, now, &stamp::system).map(|l| l.request)
}

/// The events a listing's answer holds (a cancelled one left out), as briefs.
pub(crate) fn briefs(reply: &Value) -> Vec<Brief> {
    let at =
        |e: &Value, k: &str| text(e, &[k, "dateTime"]).and_then(|t| crate::wake::parse_at(t).ok());
    reply["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["status"] != "cancelled")
        .map(|e| Brief {
            start_ms: at(e, "start"),
            end_ms: at(e, "end"),
            when: when(e, &stamp::system),
            title: text(e, &["summary"]).map_or("(no title)".into(), one_line),
            attendees: e["attendees"].as_array().map_or(0, Vec::len),
            call: call_link(e),
        })
        .collect()
}

/// An event in the lines the model reads.
fn render(e: &Value, account: &str, zone: ZoneAt<'_>) -> String {
    let title = text(e, &["summary"]).map_or("(no title)".into(), one_line);
    let mut out = format!(
        "- {} · {title} ({})\n",
        when(e, zone),
        text(e, &["id"]).unwrap_or("?")
    );
    let mut notes = Vec::new();
    if let Some(m) = text(e, &["organizer", "email"]) {
        let you = if organized_by(e, account) {
            " (you)"
        } else {
            ""
        };
        notes.push(format!("organizer: {m}{you}"));
    }
    if let Some(l) = text(e, &["location"]) {
        notes.push(format!("where: {}", one_line(l)));
    }
    if let Some(c) = call_link(e) {
        notes.push(format!("call: {c}"));
    }
    if !notes.is_empty() {
        out.push_str(&format!("  {}\n", notes.join(" · ")));
    }
    if let Some(people) = e["attendees"].as_array().filter(|a| !a.is_empty()) {
        let who: Vec<String> = people
            .iter()
            .map(|a| {
                let mail = text(a, &["email"]).unwrap_or("?");
                let you = if a["self"] == true { " (you)" } else { "" };
                let optional = if a["optional"] == true {
                    ", optional"
                } else {
                    ""
                };
                let said = text(a, &["responseStatus"]).unwrap_or("needsAction");
                format!("{mail}{you} {said}{optional}")
            })
            .collect();
        out.push_str(&format!("  attendees: {}\n", who.join(", ")));
    }
    if let Some(d) = text(e, &["description"]) {
        let d = one_line(d);
        let cut: String = d.chars().take(400).collect();
        let more = if cut.len() < d.len() { " …" } else { "" };
        out.push_str(&format!("  about: {cut}{more}\n"));
    }
    out
}

fn failed(f: super::CallFailed) -> ToolFailure {
    ToolFailure {
        message: f.message,
        meta: json!({"private": true, "google_called": f.row, "outcome_unknown": f.unknown}),
    }
}

// ------------------------------------------------------------- calendar.events

pub struct Events(Arc<Google>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsArgs {
    from: Option<String>,
    to: Option<String>,
    q: Option<String>,
    calendar: Option<String>,
    max: Option<u32>,
}

/// A listing as `plan` and `run` both read it: the request, and the window.
struct Listing {
    request: Request,
    calendar: String,
    window: (u64, u64),
}

fn listing(input: &Value, now: u64, zone: ZoneAt<'_>) -> Result<Listing, String> {
    let a: EventsArgs = parse(input)?;
    let from = match &a.from {
        Some(f) => instant(f, zone)?,
        None => day_start(now, zone),
    };
    let to = match &a.to {
        Some(t) => instant(t, zone)?,
        None => from + DAY_MS,
    };
    if to <= from {
        return Err("`to` must be after `from`".into());
    }
    let calendar = a
        .calendar
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "primary".into());
    let mut params = Map::new();
    params.insert("calendarId".into(), calendar.clone().into());
    params.insert("timeMin".into(), rfc3339(from).into());
    params.insert("timeMax".into(), rfc3339(to).into());
    params.insert("singleEvents".into(), true.into());
    params.insert("orderBy".into(), "startTime".into());
    params.insert(
        "maxResults".into(),
        a.max.unwrap_or(MAX_DEFAULT).clamp(1, MAX_MOST).into(),
    );
    if let Some(q) = a.q.filter(|q| !q.is_empty()) {
        params.insert("q".into(), q.into());
    }
    let resolved = resolve("calendar.events.list", Some(&Value::Object(params)), None)?;
    Ok(Listing {
        request: resolved.request,
        calendar,
        window: (from, to),
    })
}

/// The listing's text and its result: outside text unless the account
/// organizes every event in it.
fn listed(
    account: &str,
    calendar: &str,
    window: (u64, u64),
    reply: &Value,
    zone: ZoneAt<'_>,
) -> (String, Value, bool, String) {
    let events: Vec<&Value> = reply["items"]
        .as_array()
        .map(|a| a.iter().filter(|e| e["status"] != "cancelled").collect())
        .unwrap_or_default();
    let span = format!(
        "{calendar}, {} to {}",
        stamp::said_in(window.0, zone),
        stamp::said_in(window.1, zone)
    );
    let mut text = match events.len() {
        0 => format!("No events on {span}.\n"),
        n => format!("{n} event(s) on {span}:\n"),
    };
    for e in &events {
        text.push_str(&render(e, account, zone));
    }
    if reply.get("nextPageToken").is_some() {
        text.push_str("(more events follow: narrow from and to, or raise max)\n");
    }
    let ids: Vec<&str> = events.iter().filter_map(|e| text_of(e, "id")).collect();
    let stranger = events.iter().find(|e| !organized_by(e, account));
    let first = stranger.or(events.first()).and_then(|e| text_of(e, "id"));
    let meta = json!({"calendar": calendar, "count": events.len(), "events": ids});
    (
        text,
        meta,
        stranger.is_none(),
        first.unwrap_or("").to_string(),
    )
}

fn text_of<'a>(e: &'a Value, k: &str) -> Option<&'a str> {
    text(e, &[k])
}

impl Tool for Events {
    fn name(&self) -> &str {
        EVENTS
    }
    fn description(&self) -> &str {
        "Calendar events in a window (default today) with attendees and their responses, call \
         link and organizer, in local time. from/to: RFC 3339 or a date. Outside text unless \
         the owner organizes them."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
        "from": {"type": "string"}, "to": {"type": "string"}, "q": {"type": "string"},
        "calendar": {"type": "string"}, "max": {"type": "integer"}},
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let l = listing(input, theseus_protocol::now_unix_ms(), &stamp::system)?;
        Ok(Plan {
            summary: format!("read events on {}", l.calendar),
            class: Some(ToolClass::Read),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let planned = listing(input, theseus_protocol::now_unix_ms(), &stamp::system);
        let g = self.0.clone();
        Box::pin(async move {
            let l = planned.map_err(ToolFailure::new)?;
            let meta = Meta {
                tool: EVENTS,
                method: "calendar.events.list".into(),
                resource: Some(l.calendar.clone()),
                class: "read",
                recipients: None,
            };
            let done = g.call(meta, l.request).await.map_err(failed)?;
            let (text, meta, own, id) = listed(
                g.account(),
                &l.calendar,
                l.window,
                &done.response.json(),
                &stamp::system,
            );
            Ok(finish(
                g.account(),
                text,
                meta,
                done.row,
                Source::Event {
                    id: &id,
                    organizer_is_account: own,
                },
            ))
        })
    }
}

// --------------------------------------------------------------- calendar.put

pub struct Put(Arc<Google>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PutArgs {
    event_id: Option<String>,
    summary: Option<String>,
    start: Option<String>,
    end: Option<String>,
    attendees: Option<Vec<String>>,
    location: Option<String>,
    description: Option<String>,
    #[serde(default)]
    notify_attendees: bool,
    #[serde(default)]
    cancel: bool,
}

/// A put as `plan` and `run` both read it.
struct Change {
    request: Request,
    /// The card's one line.
    summary: String,
    /// Why it is authority, when it is.
    why: Option<&'static str>,
    event_id: Option<String>,
}

fn address(s: &str) -> Result<Value, String> {
    let s = s.trim();
    let ok = s.len() <= 254
        && s.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.contains('@'))
        && !s.contains(|c: char| c.is_whitespace() || c.is_control() || "<>,;\"".contains(c));
    ok.then(|| json!({"email": s}))
        .ok_or_else(|| format!("`{s}` is not an email address"))
}

/// An event's `start` and `end`: both dates (an all-day event, `end` its last
/// day, defaulting to `start`) or both times with their offsets.
fn span(start: &str, end: Option<&str>) -> Result<(Value, Value), String> {
    if is_date(start) != end.is_none_or(is_date) {
        return Err("`start` and `end` must both be dates or both be times".into());
    }
    if is_date(start) {
        let last = date_ms(end.unwrap_or(start))?;
        let first = date_ms(start)?;
        if last < first {
            return Err("`end` is before `start`".into());
        }
        return Ok((
            json!({"date": start}),
            json!({"date": date_of(last + DAY_MS)}),
        ));
    }
    let end = end.ok_or("an event with a time needs `end` too")?;
    if crate::wake::parse_at(end)? <= crate::wake::parse_at(start)? {
        return Err("`end` must be after `start`".into());
    }
    Ok((json!({"dateTime": start}), json!({"dateTime": end})))
}

/// A cancellation: `event_id` and whether to tell the attendees, nothing else.
fn cancellation(
    a: &PutArgs,
    id: Option<String>,
    params: Map<String, Value>,
) -> Result<Change, String> {
    let id = id.ok_or("cancelling needs `event_id`")?;
    let others = [&a.summary, &a.start, &a.end, &a.location, &a.description];
    if others.iter().any(|f| f.is_some()) || a.attendees.as_ref().is_some_and(|l| !l.is_empty()) {
        return Err("a cancellation takes only `event_id` and `notify_attendees`".into());
    }
    let mut resolved = resolve("calendar.events.delete", Some(&Value::Object(params)), None)?;
    resolved.request.repeat = Repeat::NonRepeatable;
    let told = if a.notify_attendees {
        ", telling the attendees"
    } else {
        ""
    };
    Ok(Change {
        request: resolved.request,
        summary: format!("cancel event {id}{told}"),
        why: Some("cancels an event others may attend"),
        event_id: Some(id),
    })
}

/// The event's fields as Google names them, and the card's words for its time.
fn body_of(
    a: &PutArgs,
    id: Option<&str>,
    attendees: &[String],
    zone: ZoneAt<'_>,
) -> Result<(Map<String, Value>, String), String> {
    let mut body = Map::new();
    for (k, v) in [
        ("summary", &a.summary),
        ("location", &a.location),
        ("description", &a.description),
    ] {
        if let Some(v) = v {
            body.insert(k.into(), v.clone().into());
        }
    }
    if !attendees.is_empty() {
        let people: Result<Vec<Value>, String> = attendees.iter().map(|m| address(m)).collect();
        body.insert("attendees".into(), people?.into());
    }
    let mut at = String::new();
    match (&a.start, id) {
        (Some(s), _) => {
            let (start, end) = span(s, a.end.as_deref())?;
            body.insert("start".into(), start);
            body.insert("end".into(), end);
            at = format!(", {}", stamp::said_in(instant(s, zone)?, zone));
        }
        (None, None) => return Err("a new event needs `summary` and `start`".into()),
        (None, Some(_)) if a.end.is_some() => return Err("`end` comes with `start`".into()),
        (None, Some(_)) => {}
    }
    if id.is_none() && !body.contains_key("summary") {
        return Err("a new event needs `summary` and `start`".into());
    }
    if id.is_some() && body.is_empty() {
        return Err("nothing to change: name a field to change, or `cancel`".into());
    }
    Ok((body, at))
}

fn change(input: &Value, zone: ZoneAt<'_>) -> Result<Change, String> {
    let a: PutArgs = parse(input)?;
    let id = a.event_id.clone().filter(|i| !i.is_empty());
    let attendees = a.attendees.clone().unwrap_or_default();
    // Google's `sendUpdates` has nothing to tell when no one is on a new event.
    let told = a.notify_attendees && (id.is_some() || !attendees.is_empty());
    let mut params = Map::new();
    params.insert(
        "sendUpdates".into(),
        if told { "all" } else { "none" }.into(),
    );
    if let Some(i) = &id {
        params.insert("eventId".into(), i.clone().into());
    }
    if a.cancel {
        return cancellation(&a, id, params);
    }
    let (body, at) = body_of(&a, id.as_deref(), &attendees, zone)?;
    let (method, verb) = match id {
        Some(_) => ("calendar.events.patch", "change"),
        None => ("calendar.events.insert", "create"),
    };
    let mut resolved = resolve(
        method,
        Some(&Value::Object(params)),
        Some(&Value::Object(body)),
    )?;
    if id.is_none() {
        // A repeated create is a second event.
        resolved.request.repeat = Repeat::NonRepeatable;
    }
    let title = a
        .summary
        .as_deref()
        .map(|t| format!(" «{}»", one_line(t)))
        .unwrap_or_default();
    let named = id.as_deref().map(|i| format!(" {i}")).unwrap_or_default();
    let invites = if attendees.is_empty() {
        String::new()
    } else {
        format!(", with {}", attendees.join(", "))
    };
    let why = if !attendees.is_empty() {
        Some("invites people to an event")
    } else if id.is_some() && told {
        Some("tells the attendees of a change")
    } else {
        None
    };
    debug_assert!(resolved.class.class != Class::Authority || why.is_some());
    let said = if told { ", emailing the attendees" } else { "" };
    Ok(Change {
        request: resolved.request,
        summary: format!("{verb} event{named}{title}{at}{invites}{said}"),
        why,
        event_id: id,
    })
}

impl Tool for Put {
    fn name(&self) -> &str {
        PUT
    }
    fn description(&self) -> &str {
        "Create, change (event_id) or cancel an event. start/end: RFC 3339 or dates (all day). \
         Inviting, cancelling or notifying asks the owner."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
        "event_id": {"type": "string"}, "summary": {"type": "string"},
        "start": {"type": "string"}, "end": {"type": "string"},
        "attendees": {"type": "array", "items": {"type": "string"}},
        "location": {"type": "string"}, "description": {"type": "string"},
        "notify_attendees": {"type": "boolean"}, "cancel": {"type": "boolean"}},
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A write may have run when a crash cut it: unknown, never run again.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let c = change(input, &stamp::system)?;
        let authority = c.why.map(|why| {
            format!(
                "{why}, so the owner decides it, every time, at every posture: {}",
                c.summary
            )
        });
        Ok(Plan {
            summary: c.summary,
            class: Some(ToolClass::Write),
            authority,
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let planned = change(input, &stamp::system);
        let g = self.0.clone();
        Box::pin(async move {
            let c = planned.map_err(ToolFailure::new)?;
            let cancel = c.request.verb == "DELETE";
            let meta = Meta {
                tool: PUT,
                method: match (cancel, &c.event_id) {
                    (true, _) => "calendar.events.delete",
                    (_, Some(_)) => "calendar.events.patch",
                    _ => "calendar.events.insert",
                }
                .into(),
                resource: c.event_id.clone(),
                class: if c.why.is_some() {
                    "authority"
                } else {
                    "write"
                },
                recipients: None,
            };
            let done = g.call(meta, c.request).await.map_err(failed)?;
            let event = done.response.json();
            let id = text_of(&event, "id")
                .or(c.event_id.as_deref())
                .unwrap_or("?");
            let (said, source): (String, Source<'_>) = if cancel {
                (format!("Cancelled event {id}.\n"), Source::None)
            } else {
                let verb = if c.event_id.is_some() {
                    "Changed"
                } else {
                    "Created"
                };
                (
                    format!("{verb}:\n{}", render(&event, g.account(), &stamp::system)),
                    Source::Event {
                        id,
                        organizer_is_account: organized_by(&event, g.account()),
                    },
                )
            };
            let out: Finished =
                finish(g.account(), said, json!({"event_id": id}), done.row, source);
            Ok(out)
        })
    }
}
