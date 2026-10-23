//! The calendar cache (theseus-jid7t.1): the configured calendars' events over
//! the windows the Operations page asks for, kept in memory so a page that
//! reads on every focus costs Google one list.
//!
//! - **Per calendar and window.** `events.list` (`singleEvents`,
//!   `orderBy=startTime`) over the window asked for, widened by a week each
//!   side, kept [`TTL`] (5 minutes). After that a read refreshes it with
//!   Google's `syncToken` (only what changed); a 410 for a stale token drops
//!   the token and reads the window again. A 403 for the events asks
//!   `freebusy.query` instead: busy blocks with no title.
//! - **Bounds.** At most [`MAX_WINDOWS`] windows a calendar (the least
//!   recently read goes first), at most [`MAX_EVENTS`] events a window (past it
//!   the window is cut and carries no sync token, so its next refresh reads it
//!   again).
//! - **Memory only.** Nothing about an event is written to the store, the
//!   ledger, a log line or telemetry. Every call goes through
//!   [`Google::call`], whose `google.called` row holds a method, a status and
//!   the calendar's label, and those rows are handed back to the caller to
//!   write.
//! - **A change** found by a refresh is returned as the calendar's label; the
//!   caller tells the connections (`calendar.changed`, no content).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use theseus_google::methods::resolve;
use theseus_protocol::calendar::{CalendarEvent, CalendarStatus, CalendarWindow};
use tokio::time::{Duration, Instant};

use super::calendar::{call_link, instant, organized_by, rfc3339, text};
use super::{Google, Meta, Phase};
use crate::config::CalendarSpec;
use crate::recall::stamp;

/// How long a window is served as it was read.
pub const TTL: Duration = Duration::from_secs(300);
/// A window is read this much wider than asked, each side.
const WIDEN_MS: u64 = 7 * 86_400_000;
pub const MAX_WINDOWS: usize = 4;
pub const MAX_EVENTS: usize = 1500;
const PAGE: u32 = 250;
const MAX_PAGES: u32 = MAX_EVENTS.div_ceil(PAGE as usize) as u32;
/// The longest span `calendar.window` answers.
pub const MAX_SPAN_MS: u64 = 62 * 86_400_000;

/// One kept event: what the page needs, and what a sync merge matches on.
struct Stored {
    id: String,
    /// Changes when the event does (Google's `updated`), compared in memory only.
    stamp: u64,
    event: CalendarEvent,
}

struct Win {
    from: u64,
    to: u64,
    at: Instant,
    /// The test clock's age when it was read.
    aged: u64,
    used: Instant,
    token: Option<String>,
    items: Vec<Stored>,
}

struct Slot {
    spec: CalendarSpec,
    /// The calendar's own name, read once from `calendarList`.
    summary: Option<String>,
    status: CalendarStatus,
    detail: String,
    windows: Vec<Win>,
}

/// The cache: one slot per configured calendar, each read one at a time.
pub struct Cache {
    specs: Vec<CalendarSpec>,
    slots: Vec<tokio::sync::Mutex<Slot>>,
    summaries: tokio::sync::OnceCell<HashMap<String, String>>,
    /// The tender runs (see [`Google::calendar_tend`]).
    tending: AtomicBool,
    /// Milliseconds a test has aged every window by (the clock is real).
    aged_ms: AtomicU64,
}

/// Where a refresh with news goes: the `google.called` rows to write, and the
/// labels of the calendars that changed.
pub type Told = Arc<dyn Fn(Vec<Value>, Vec<String>) + Send + Sync>;

/// What a read gives back: the calendars, the `google.called` rows to write,
/// and the labels of those a refresh found changed.
#[derive(Default)]
pub struct Read {
    pub calendars: Vec<CalendarWindow>,
    pub rows: Vec<Value>,
    pub changed: Vec<String>,
}

impl Cache {
    pub fn new(specs: Vec<CalendarSpec>) -> Self {
        Self {
            specs: specs.clone(),
            slots: specs
                .into_iter()
                .map(|spec| {
                    tokio::sync::Mutex::new(Slot {
                        spec,
                        summary: None,
                        status: CalendarStatus::Ok,
                        detail: String::new(),
                        windows: Vec::new(),
                    })
                })
                .collect(),
            summaries: tokio::sync::OnceCell::new(),
            tending: AtomicBool::new(false),
            aged_ms: AtomicU64::new(0),
        }
    }

    /// Whether `w` is still served as it was read.
    fn fresh(&self, w: &Win) -> bool {
        let aged = self.aged_ms.load(Ordering::Relaxed).saturating_sub(w.aged);
        w.at.elapsed() + Duration::from_millis(aged) < TTL
    }

    /// A test's clock: every kept window is `by` older.
    #[cfg(test)]
    pub(crate) fn age(&self, by: Duration) {
        self.aged_ms
            .fetch_add(by.as_millis() as u64, Ordering::Relaxed);
    }
}

// ------------------------------------------------------------------- reads

impl Google {
    /// Every configured calendar over `from..to`, from the cache when it is
    /// fresh, refreshed when it is not. A calendar that fails says why in its
    /// status; none blocks the others.
    pub async fn calendar_window(&self, from: u64, to: u64) -> Read {
        let summaries = self.calendar_summaries().await;
        let mut out = Read::default();
        out.rows.extend(summaries.1);
        for slot in &self.calendars.slots {
            let mut s = slot.lock().await;
            if s.summary.is_none() {
                s.summary = summaries.0.get(&s.spec.id).cloned();
            }
            let label = label_of(&s);
            let (events, changed) = self.serve(&mut s, from, to, &mut out.rows).await;
            if changed {
                out.changed.push(label.clone());
            }
            out.calendars.push(CalendarWindow {
                id: s.spec.id.clone(),
                label,
                color: s.spec.color.clone(),
                sensitive: s.spec.sensitive,
                status: s.status,
                detail: s.detail.clone(),
                events,
            });
        }
        out
    }

    /// Every configured calendar with its name, for the morning note: the
    /// summaries are read once (and only when there is more than one calendar
    /// to tell apart), and their row comes back.
    pub async fn calendar_all(&self) -> (Vec<Target>, Vec<Value>) {
        let mut rows = Vec::new();
        if self.calendars.specs.len() > 1 {
            rows = self.calendar_summaries().await.1;
        }
        let all = self.calendar_targets(None, Some("all")).unwrap_or_default();
        (all, rows)
    }

    /// Refresh every kept window that is past its time: the tender's work
    /// while a page has read a window. Returns the changed labels and rows.
    pub async fn calendar_refresh(&self) -> Read {
        let mut out = Read::default();
        for slot in &self.calendars.slots {
            let mut s = slot.lock().await;
            let label = label_of(&s);
            let mut changed = false;
            for i in 0..s.windows.len() {
                if s.windows.get(i).is_some_and(|w| !self.calendars.fresh(w)) {
                    changed |= self.refresh(&mut s, i, &mut out.rows).await;
                }
            }
            if changed {
                out.changed.push(label);
            }
        }
        out
    }

    /// Keep the windows a page has read fresh, and tell `told` what changed.
    /// Started by the first window read, never before: until a connection has
    /// read one, nothing is refreshed and nothing is sent. One timer wait of
    /// [`TTL`] a round, no polling; it ends when no window is kept, and the
    /// next read starts it again.
    pub fn calendar_tend(self: &Arc<Self>, told: Told) {
        if self.calendars.tending.swap(true, Ordering::SeqCst) {
            return;
        }
        let g = self.clone();
        tokio::spawn(async move {
            while g.calendars_kept().await {
                tokio::time::sleep(TTL).await;
                let r = g.calendar_refresh().await;
                if !r.rows.is_empty() || !r.changed.is_empty() {
                    told(r.rows, r.changed);
                }
            }
            g.calendars.tending.store(false, Ordering::SeqCst);
        });
    }

    /// Whether any window is kept (the tender has something to refresh).
    pub async fn calendars_kept(&self) -> bool {
        for slot in &self.calendars.slots {
            if !slot.lock().await.windows.is_empty() {
                return true;
            }
        }
        false
    }

    /// The calendars' own names, once: `primary` is the entry marked so.
    pub(super) async fn calendar_summaries(&self) -> (HashMap<String, String>, Vec<Value>) {
        let mut rows = Vec::new();
        let map = self
            .calendars
            .summaries
            .get_or_init(|| async {
                let Ok(r) = resolve("calendar.calendarList.list", None, None) else {
                    return HashMap::new();
                };
                let meta = meta("calendar.calendarList.list", None);
                let reply = match self.call(meta, r.request).await {
                    Ok(done) => {
                        rows.push(done.row);
                        done.response.json()
                    }
                    Err(f) => {
                        rows.push(f.row);
                        return HashMap::new();
                    }
                };
                let mut names = HashMap::new();
                for e in reply["items"].as_array().into_iter().flatten() {
                    let Some(name) =
                        text(e, &["summaryOverride"]).or_else(|| text(e, &["summary"]))
                    else {
                        continue;
                    };
                    if let Some(id) = text(e, &["id"]) {
                        names.insert(id.to_string(), name.to_string());
                    }
                    if e["primary"] == true {
                        names.insert("primary".to_string(), name.to_string());
                    }
                }
                names
            })
            .await
            .clone();
        (map, rows)
    }

    /// The events of `slot` over `from..to`, and whether a refresh found a change.
    async fn serve(
        &self,
        s: &mut Slot,
        from: u64,
        to: u64,
        rows: &mut Vec<Value>,
    ) -> (Vec<CalendarEvent>, bool) {
        let mut changed = false;
        let at = s.windows.iter().position(|w| w.from <= from && to <= w.to);
        match at {
            Some(i) if self.calendars.fresh(&s.windows[i]) => {}
            Some(i) => changed = self.refresh(s, i, rows).await,
            None => self.read_new(s, from, to, rows).await,
        }
        let Some(w) = s.windows.iter_mut().find(|w| w.from <= from && to <= w.to) else {
            return (Vec::new(), changed);
        };
        w.used = Instant::now();
        let events = w
            .items
            .iter()
            .filter(|i| i.event.start_ms < to && i.event.end_ms.max(i.event.start_ms + 1) > from)
            .map(|i| i.event.clone())
            .collect();
        (events, changed)
    }

    /// A window nothing kept covers: read wider than asked, keep it.
    async fn read_new(&self, s: &mut Slot, from: u64, to: u64, rows: &mut Vec<Value>) {
        let (wf, wt) = (from.saturating_sub(WIDEN_MS), to.saturating_add(WIDEN_MS));
        if let Some(w) = self.fetch(s, wf, wt, rows).await {
            s.windows.retain(|k| !(wf <= k.from && k.to <= wt));
            if s.windows.len() >= MAX_WINDOWS {
                if let Some(oldest) = (0..s.windows.len()).min_by_key(|i| s.windows[*i].used) {
                    s.windows.remove(oldest);
                }
            }
            s.windows.push(w);
        }
    }

    /// Bring window `i` up to date. True when it differs from what was kept.
    async fn refresh(&self, s: &mut Slot, i: usize, rows: &mut Vec<Value>) -> bool {
        let (wf, wt, token) = {
            let w = &s.windows[i];
            (w.from, w.to, w.token.clone())
        };
        if let Some(token) = token {
            match self.list(s, Query::Sync(&token), rows).await {
                Ok(page) => {
                    s.status = CalendarStatus::Ok;
                    s.detail.clear();
                    let account = self.account().to_string();
                    let id = s.spec.id.clone();
                    let w = &mut s.windows[i];
                    let changed = merge(&mut w.items, &page.items, &account, &id);
                    w.token = page.next_sync.or(w.token.take());
                    w.at = Instant::now();
                    w.aged = self.calendars.aged_ms.load(Ordering::Relaxed);
                    return changed;
                }
                Err(Fail::Gone) => {}
                Err(f) => {
                    // A stale window is better than none; say what is wrong.
                    self.failed(s, f);
                    return false;
                }
            }
        }
        let before = fingerprint(&s.windows[i].items);
        match self.fetch(s, wf, wt, rows).await {
            Some(fresh) => {
                let changed = fingerprint(&fresh.items) != before;
                s.windows[i] = fresh;
                changed
            }
            None => false,
        }
    }

    /// A whole window from Google: events, else (403) busy blocks.
    async fn fetch(&self, s: &mut Slot, wf: u64, wt: u64, rows: &mut Vec<Value>) -> Option<Win> {
        let account = self.account().to_string();
        let id = s.spec.id.clone();
        let win = |items: Vec<Stored>, token| Win {
            from: wf,
            to: wt,
            at: Instant::now(),
            aged: self.calendars.aged_ms.load(Ordering::Relaxed),
            used: Instant::now(),
            token,
            items,
        };
        match self.list(s, Query::Window(wf, wt), rows).await {
            Ok(page) => {
                s.status = CalendarStatus::Ok;
                s.detail.clear();
                let mut items = Vec::new();
                merge(&mut items, &page.items, &account, &id);
                let token = page.next_sync.filter(|_| !page.truncated);
                Some(win(items, token))
            }
            Err(Fail::Status(CalendarStatus::Forbidden)) => {
                match self.busy(s, wf, wt, rows).await {
                    Ok(items) => {
                        s.status = CalendarStatus::BusyOnly;
                        s.detail = busy_detail(&label_of(s));
                        Some(win(items, None))
                    }
                    Err(f) => {
                        self.failed(s, f);
                        None
                    }
                }
            }
            Err(f) => {
                self.failed(s, f);
                None
            }
        }
    }

    fn failed(&self, s: &mut Slot, f: Fail) {
        let label = label_of(s);
        let (status, detail) = match f {
            Fail::Status(CalendarStatus::NotFound) => (
                CalendarStatus::NotFound,
                format!(
                    "`{label}` is not shared with {}, or its id is wrong: share it with that \
                     account in Google Calendar, or fix `[google] calendars`",
                    self.account()
                ),
            ),
            Fail::Status(CalendarStatus::AuthLapsed) => (
                CalendarStatus::AuthLapsed,
                "Google's consent lapsed: run `theseus google auth`".to_string(),
            ),
            Fail::Status(CalendarStatus::Off) => (
                CalendarStatus::Off,
                "Calendar is off: its scope was not granted. `theseus google auth` asks \
                 again, and `theseus google status` shows what is on"
                    .to_string(),
            ),
            Fail::Status(other) => (other, format!("Google refused the read of `{label}`")),
            Fail::Gone | Fail::Other => (
                CalendarStatus::Forbidden,
                format!("`{label}` could not be read just now; it is tried again on the next read"),
            ),
        };
        s.status = status;
        s.detail = detail;
    }

    /// One `events.list` (every page, to the bound), or the failure's status.
    async fn list(&self, s: &Slot, q: Query<'_>, rows: &mut Vec<Value>) -> Result<Listed, Fail> {
        let mut out = Listed::default();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut p = Map::new();
            p.insert("calendarId".into(), s.spec.id.clone().into());
            p.insert("singleEvents".into(), true.into());
            match q {
                Query::Window(f, t) => {
                    p.insert("timeMin".into(), rfc3339(f).into());
                    p.insert("timeMax".into(), rfc3339(t).into());
                    p.insert("orderBy".into(), "startTime".into());
                    p.insert("maxResults".into(), PAGE.into());
                }
                Query::Sync(tok) => {
                    p.insert("syncToken".into(), tok.into());
                    p.insert("maxResults".into(), PAGE.into());
                }
                Query::Probe => {
                    p.insert("maxResults".into(), 1.into());
                }
            }
            if let Some(t) = &page_token {
                p.insert("pageToken".into(), t.clone().into());
            }
            let r = resolve("calendar.events.list", Some(&Value::Object(p)), None)
                .map_err(|_| Fail::Other)?;
            let m = meta("calendar.events.list", Some(label_of(s)));
            let reply = match self.call(m, r.request).await {
                Ok(done) => {
                    rows.push(done.row);
                    done.response.json()
                }
                Err(f) => {
                    let status = f.row["status"].as_u64().unwrap_or(0);
                    rows.push(f.row);
                    return Err(self.fail_of(status));
                }
            };
            out.items
                .extend(reply["items"].as_array().cloned().unwrap_or_default());
            if out.items.len() > MAX_EVENTS {
                out.items.truncate(MAX_EVENTS);
                out.truncated = true;
                return Ok(out);
            }
            match text(&reply, &["nextPageToken"]) {
                Some(t) if !matches!(q, Query::Probe) => page_token = Some(t.to_string()),
                _ => {
                    out.next_sync = text(&reply, &["nextSyncToken"]).map(str::to_string);
                    return Ok(out);
                }
            }
        }
        out.truncated = true;
        Ok(out)
    }

    /// `freebusy.query`: the calendar's busy blocks, with no title.
    async fn busy(
        &self,
        s: &Slot,
        wf: u64,
        wt: u64,
        rows: &mut Vec<Value>,
    ) -> Result<Vec<Stored>, Fail> {
        let body = json!({"timeMin": rfc3339(wf), "timeMax": rfc3339(wt),
            "items": [{"id": s.spec.id}]});
        let r = resolve("calendar.freebusy.query", None, Some(&body)).map_err(|_| Fail::Other)?;
        let m = meta("calendar.freebusy.query", Some(label_of(s)));
        let reply = match self.call(m, r.request).await {
            Ok(done) => {
                rows.push(done.row);
                done.response.json()
            }
            Err(f) => {
                let status = f.row["status"].as_u64().unwrap_or(0);
                rows.push(f.row);
                return Err(self.fail_of(status));
            }
        };
        let entry = reply["calendars"]
            .as_object()
            .and_then(|m| m.values().next())
            .cloned()
            .unwrap_or(Value::Null);
        if entry["errors"].as_array().is_some_and(|e| !e.is_empty()) {
            return Err(Fail::Status(CalendarStatus::Forbidden));
        }
        let at = |v: &Value| v.as_str().and_then(|t| crate::wake::parse_at(t).ok());
        Ok(entry["busy"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| Some((at(&b["start"])?, at(&b["end"])?)))
            .enumerate()
            .map(|(n, (start_ms, end_ms))| Stored {
                id: format!("busy:{n}"),
                stamp: 0,
                event: CalendarEvent {
                    key: key(&s.spec.id, &format!("busy:{n}"), start_ms),
                    start_ms,
                    end_ms,
                    busy: true,
                    ..Default::default()
                },
            })
            .collect())
    }

    /// What a failed call means for the calendar.
    fn fail_of(&self, http: u64) -> Fail {
        if let Phase::NeedsAuth(_) = self.check_now().phase {
            return Fail::Status(CalendarStatus::AuthLapsed);
        }
        match http {
            410 => Fail::Gone,
            404 => Fail::Status(CalendarStatus::NotFound),
            403 => Fail::Status(CalendarStatus::Forbidden),
            0 if !theseus_google::scopes::services_on(&self.check_now().granted)
                .contains(&theseus_google::Service::Calendar) =>
            {
                Fail::Status(CalendarStatus::Off)
            }
            _ => Fail::Other,
        }
    }

    /// Each configured calendar's reachability, for `theseusd check`: one
    /// `events.list` with `maxResults=1`, and the free/busy fallback. Names the
    /// calendar by label, never an event.
    pub async fn calendar_reach(&self) -> (Vec<(String, CalendarStatus, String)>, Vec<Value>) {
        let summaries = self.calendar_summaries().await;
        let mut rows = summaries.1;
        let mut out = Vec::new();
        for slot in &self.calendars.slots {
            let mut s = slot.lock().await;
            if s.summary.is_none() {
                s.summary = summaries.0.get(&s.spec.id).cloned();
            }
            match self.list(&s, Query::Probe, &mut rows).await {
                Ok(_) => {
                    s.status = CalendarStatus::Ok;
                    s.detail.clear();
                }
                Err(Fail::Status(CalendarStatus::Forbidden)) => {
                    let now = theseus_protocol::now_unix_ms();
                    match self.busy(&s, now, now + 86_400_000, &mut rows).await {
                        Ok(_) => {
                            s.status = CalendarStatus::BusyOnly;
                            s.detail = busy_detail(&label_of(&s));
                        }
                        Err(f) => self.failed(&mut s, f),
                    }
                }
                Err(f) => self.failed(&mut s, f),
            }
            out.push((label_of(&s), s.status, s.detail.clone()));
        }
        (out, rows)
    }
}

/// A calendar a tool call reads: its id, its name, and whether it is sensitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub sensitive: bool,
}

impl Google {
    /// Whether `calendar_targets(name, all)` reads the calendars' own names:
    /// `"all"`, or a name no configured id or label answers to. Nothing is
    /// read for the primary calendar alone.
    pub fn calendar_needs_names(&self, name: Option<&str>, all: Option<&str>) -> bool {
        let direct = |n: &str| {
            self.calendars.specs.iter().any(|s| {
                s.id.eq_ignore_ascii_case(n)
                    || s.label
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case(n))
            })
        };
        let unlabelled = self.calendars.specs.iter().any(|s| s.label.is_none());
        unlabelled && (all.is_some() || name.is_some_and(|n| !n.contains('@') && !direct(n)))
    }

    /// The calendars a `calendar.events` call names: one by label or id (case
    /// does not matter), or `"all"`. Nothing named is `primary`. An id of a
    /// calendar the config does not list is read as it always was, and is not
    /// sensitive; a name that is no label, no id and no address is refused,
    /// with the configured labels listed.
    pub fn calendar_targets(
        &self,
        name: Option<&str>,
        all: Option<&str>,
    ) -> Result<Vec<Target>, String> {
        let names = self.calendars.summaries.get();
        let target = |s: &CalendarSpec| Target {
            id: s.id.clone(),
            label: s
                .label
                .clone()
                .or_else(|| names.and_then(|m| m.get(&s.id).cloned()))
                .unwrap_or_else(|| s.id.clone()),
            sensitive: s.sensitive,
        };
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        match (name, all) {
            (Some(_), Some(_)) => Err("name `calendar` or `calendars`, not both".into()),
            (_, Some(a)) if a.eq_ignore_ascii_case("all") => {
                Ok(self.calendars.specs.iter().map(target).collect())
            }
            (_, Some(a)) => Err(format!("`calendars` is \"all\" or left out, not {a:?}")),
            (None, None) => {
                let primary = self.calendars.specs.iter().find(|s| s.id == "primary");
                Ok(vec![primary.map_or_else(
                    || Target {
                        id: "primary".into(),
                        label: "primary".into(),
                        sensitive: false,
                    },
                    target,
                )])
            }
            (Some(n), None) => {
                let found = self
                    .calendars
                    .specs
                    .iter()
                    .map(target)
                    .find(|t| t.label.eq_ignore_ascii_case(n) || t.id.eq_ignore_ascii_case(n));
                if let Some(t) = found {
                    return Ok(vec![t]);
                }
                if n.contains('@') {
                    return Ok(vec![Target {
                        id: n.into(),
                        label: n.into(),
                        sensitive: false,
                    }]);
                }
                let known: Vec<String> = self
                    .calendars
                    .specs
                    .iter()
                    .map(|s| target(s).label)
                    .collect();
                Err(format!(
                    "no calendar is called {n:?}; the configured calendars are: {}",
                    known.join(", ")
                ))
            }
        }
    }
}

enum Query<'a> {
    Window(u64, u64),
    Sync(&'a str),
    Probe,
}

#[derive(Default)]
struct Listed {
    items: Vec<Value>,
    next_sync: Option<String>,
    truncated: bool,
}

enum Fail {
    /// 410: the sync token is stale.
    Gone,
    Status(CalendarStatus),
    Other,
}

fn meta(method: &str, resource: Option<String>) -> Meta {
    Meta {
        tool: super::calendar::EVENTS,
        method: method.into(),
        resource,
        class: "read",
        recipients: None,
    }
}

/// The calendar's name: the configured label, else its own summary, else its id.
fn label_of(s: &Slot) -> String {
    s.spec
        .label
        .clone()
        .or_else(|| s.summary.clone())
        .unwrap_or_else(|| s.spec.id.clone())
}

fn busy_detail(label: &str) -> String {
    format!(
        "only free/busy of `{label}` is shared: ask its owner to share event details with \
         this account in Google Calendar"
    )
}

/// An opaque, stable name for an event: a hash of the calendar, the event and
/// its start.
fn key(calendar: &str, event: &str, start_ms: u64) -> String {
    let mut h = Sha256::new();
    h.update(calendar.as_bytes());
    h.update([0]);
    h.update(event.as_bytes());
    h.update([0]);
    h.update(start_ms.to_be_bytes());
    h.finalize()[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One Google event as the page shows it, or None for a cancelled one or one
/// with no time.
fn project(e: &Value, account: &str, calendar: &str) -> Option<Stored> {
    if e["status"] == "cancelled" {
        return None;
    }
    let id = text(e, &["id"])?;
    let all_day = text(e, &["start", "date"]).is_some();
    let at = |k: &str| -> Option<u64> {
        let (date, time) = (text(e, &[k, "date"]), text(e, &[k, "dateTime"]));
        match (date, time) {
            (Some(d), _) => instant(d, &stamp::system).ok(),
            (_, Some(t)) => crate::wake::parse_at(t).ok(),
            _ => None,
        }
    };
    let (start_ms, end_ms) = (at("start")?, at("end")?);
    let people = e["attendees"].as_array();
    let mut h = DefaultHasher::new();
    text(e, &["updated"]).hash(&mut h);
    Some(Stored {
        id: id.to_string(),
        stamp: h.finish(),
        event: CalendarEvent {
            key: key(calendar, id, start_ms),
            start_ms,
            end_ms,
            all_day,
            title: Some(text(e, &["summary"]).unwrap_or("(no title)").to_string()),
            location: text(e, &["location"]).map(str::to_string),
            call: call_link(e),
            attendees: people.map_or(0, Vec::len) as u32,
            self_response: people
                .into_iter()
                .flatten()
                .find(|a| a["self"] == true)
                .and_then(|a| text(a, &["responseStatus"]))
                .map(str::to_string),
            organizer_self: organized_by(e, account),
            html_link: text(e, &["htmlLink"]).unwrap_or_default().to_string(),
            busy: false,
        },
    })
}

/// Merge Google's items into `kept` by event id (a cancelled one removes its
/// event); true when anything differs. Sorted by start.
fn merge(kept: &mut Vec<Stored>, items: &[Value], account: &str, calendar: &str) -> bool {
    let mut changed = false;
    for raw in items {
        let Some(id) = text(raw, &["id"]) else {
            continue;
        };
        let at = kept.iter().position(|k| k.id == id);
        match (project(raw, account, calendar), at) {
            (Some(new), Some(i)) => {
                changed |= kept[i].stamp != new.stamp || kept[i].event != new.event;
                kept[i] = new;
            }
            (Some(new), None) => {
                kept.push(new);
                changed = true;
            }
            (None, Some(i)) => {
                kept.remove(i);
                changed = true;
            }
            (None, None) => {}
        }
    }
    kept.sort_by_key(|k| (k.event.start_ms, k.event.end_ms));
    changed
}

/// A hash of what a window holds, to tell a full re-read that found a change.
fn fingerprint(items: &[Stored]) -> u64 {
    let mut h = DefaultHasher::new();
    for i in items {
        (&i.id, i.stamp, i.event.start_ms, i.event.end_ms).hash(&mut h);
    }
    h.finish()
}
