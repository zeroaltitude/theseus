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
//! - **Idle.** A window nobody has read for [`IDLE`] (30 minutes) is dropped
//!   by the tender before it refreshes, so a page left closed costs Google
//!   nothing; the tender ends when none is kept, and the next read starts it.
//!   A sync delta is merged against the window's own bounds (`overlaps`), so
//!   an event outside it is never kept and never told as a change.
//! - **Memory only.** Nothing about an event is written to the store, the
//!   ledger, a log line or telemetry. Every call goes through
//!   [`Google::call`], whose `google.called` row holds a method, a status and
//!   the calendar's label, and those rows are handed back to the caller to
//!   write.
//! - **Which calendars.** The config's list, or, when it names none, every
//!   calendar of the account's `calendarList`, read again when a read finds it
//!   older than [`TTL`]: a calendar newly shared with the account appears
//!   (and is told as changed), one no longer listed goes. Until the list is
//!   read, or when it cannot be, the account's `primary` alone is read. The
//!   list also gives each calendar Google's own colour (a config colour wins)
//!   and its owner, whose domain can make it sensitive
//!   (`config::google_calendars::sensitive_by_domain`).
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
use crate::config::google_calendars::{color_ok, PALETTE};
use crate::config::{primary_spec, CalendarSpec, GoogleConfig};
use crate::recall::stamp;

/// How long a window is served as it was read.
pub const TTL: Duration = Duration::from_secs(300);
/// A window nobody has read for this long (6 x [`TTL`]) is dropped by the tender.
const IDLE: Duration = Duration::from_secs(1800);
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
    /// The test clock's age when it was last read (as `aged` is for `at`).
    used_aged: u64,
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

/// One calendar of the account's `calendarList`.
#[derive(Clone)]
struct Listed1 {
    id: String,
    primary: bool,
    name: Option<String>,
    color: Option<String>,
    owner: Option<String>,
}

/// The account's `calendarList` as last read.
struct Listing {
    at: Instant,
    aged: u64,
    names: HashMap<String, String>,
}

/// The cache: one slot per calendar read, each read one at a time.
pub struct Cache {
    /// The config names no calendar: the account's list says which.
    every: bool,
    domains: Option<Vec<String>>,
    /// The calendars read now, in order, beside their slots.
    specs: std::sync::RwLock<Vec<CalendarSpec>>,
    slots: std::sync::RwLock<Vec<Arc<tokio::sync::Mutex<Slot>>>>,
    listing: tokio::sync::Mutex<Option<Listing>>,
    /// The listed names, for the sync readers (`calendar_targets`).
    names: std::sync::RwLock<HashMap<String, String>>,
    /// The list was read once (a sync answer to "may a calendar be sensitive").
    listed: AtomicBool,
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

fn slot_of(spec: CalendarSpec) -> Arc<tokio::sync::Mutex<Slot>> {
    Arc::new(tokio::sync::Mutex::new(Slot {
        spec,
        summary: None,
        status: CalendarStatus::Ok,
        detail: String::new(),
        windows: Vec::new(),
    }))
}

impl Cache {
    pub fn new(cfg: &GoogleConfig) -> Self {
        let named = cfg.calendar_specs();
        let every = named.is_empty();
        let specs = if every { vec![primary_spec()] } else { named };
        Self {
            every,
            domains: cfg.sensitive_domains.clone(),
            slots: std::sync::RwLock::new(specs.iter().cloned().map(slot_of).collect()),
            specs: std::sync::RwLock::new(specs),
            listing: tokio::sync::Mutex::new(None),
            names: std::sync::RwLock::new(HashMap::new()),
            listed: AtomicBool::new(false),
            tending: AtomicBool::new(false),
            aged_ms: AtomicU64::new(0),
        }
    }

    /// The calendars read now.
    pub(crate) fn specs(&self) -> Vec<CalendarSpec> {
        self.specs.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn slots(&self) -> Vec<Arc<tokio::sync::Mutex<Slot>>> {
        self.slots.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether a calendar read may be of a sensitive calendar: one read now is,
    /// or the config names none and the account's list was never read (fail
    /// closed: the list may hold one).
    pub fn any_sensitive(&self) -> bool {
        (self.every && !self.listed.load(Ordering::SeqCst))
            || self.specs().iter().any(|s| s.sensitive)
    }

    /// Whether `l` is still served as it was read.
    fn listing_fresh(&self, l: &Listing) -> bool {
        let aged = self.aged_ms.load(Ordering::Relaxed).saturating_sub(l.aged);
        l.at.elapsed() + Duration::from_millis(aged) < TTL
    }

    /// Settle the calendars read on `entries`, the account's list: with no
    /// configured calendar, every listed one (the primary first, as `primary`),
    /// a slot kept for a calendar still listed; with a configured list, its
    /// calendars take Google's colour (unless the config named one) and
    /// their owner's domain. Returns the labels of calendars newly read.
    async fn settle(&self, entries: &[Listed1]) -> Vec<String> {
        let sensitive = |e: &Listed1| {
            let d = self.domains.as_deref();
            crate::config::google_calendars::sensitive_by_domain(&e.id, d)
                || e.owner
                    .as_deref()
                    .is_some_and(|o| crate::config::google_calendars::sensitive_by_domain(o, d))
        };
        let google = |e: &Listed1| e.color.clone().filter(|c| color_ok(c));
        if !self.every {
            for slot in self.slots() {
                let mut s = slot.lock().await;
                let id = s.spec.id.clone();
                let Some(e) = entries.iter().find(|e| {
                    e.id.eq_ignore_ascii_case(&id)
                        || (e.primary && id == crate::config::google_calendars::PRIMARY)
                }) else {
                    continue;
                };
                s.spec.sensitive |= sensitive(e);
                if !s.spec.color_set {
                    if let Some(c) = google(e) {
                        s.spec.color = c;
                    }
                }
            }
            let specs: Vec<CalendarSpec> = {
                let mut v = Vec::new();
                for slot in self.slots() {
                    v.push(slot.lock().await.spec.clone());
                }
                v
            };
            *self.specs.write().unwrap_or_else(|e| e.into_inner()) = specs;
            return Vec::new();
        }
        if entries.is_empty() {
            return Vec::new();
        }
        let mut ordered: Vec<&Listed1> = entries.iter().filter(|e| e.primary).collect();
        ordered.extend(entries.iter().filter(|e| !e.primary));
        let old = self.slots();
        let mut slots = Vec::new();
        let mut specs = Vec::new();
        let mut added = Vec::new();
        for (i, e) in ordered.into_iter().enumerate() {
            let id = match e.primary {
                true => crate::config::google_calendars::PRIMARY.to_string(),
                false => e.id.clone(),
            };
            let spec = CalendarSpec {
                id: id.clone(),
                label: None,
                color: google(e).unwrap_or_else(|| PALETTE[i % PALETTE.len()].into()),
                color_set: false,
                sensitive: sensitive(e),
            };
            let mut kept = None;
            for slot in &old {
                if slot.lock().await.spec.id == id {
                    kept = Some(slot.clone());
                    break;
                }
            }
            let slot = match kept {
                Some(slot) => {
                    let mut s = slot.lock().await;
                    if s.spec.sensitive != spec.sensitive {
                        s.windows.clear();
                    }
                    s.spec = spec.clone();
                    drop(s);
                    slot
                }
                None => {
                    added.push(e.name.clone().unwrap_or_else(|| id.clone()));
                    slot_of(spec.clone())
                }
            };
            slots.push(slot);
            specs.push(spec);
        }
        *self.slots.write().unwrap_or_else(|e| e.into_inner()) = slots;
        *self.specs.write().unwrap_or_else(|e| e.into_inner()) = specs;
        added
    }

    /// Whether `w` is still served as it was read.
    fn fresh(&self, w: &Win) -> bool {
        let aged = self.aged_ms.load(Ordering::Relaxed).saturating_sub(w.aged);
        w.at.elapsed() + Duration::from_millis(aged) < TTL
    }

    /// Whether nobody has read `w` for [`IDLE`].
    fn idle(&self, w: &Win) -> bool {
        let aged = self
            .aged_ms
            .load(Ordering::Relaxed)
            .saturating_sub(w.used_aged);
        w.used.elapsed() + Duration::from_millis(aged) >= IDLE
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
        out.changed.extend(summaries.2);
        for slot in self.calendars.slots() {
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
        if self.calendars.every || self.calendars.specs().len() > 1 {
            rows = self.calendar_summaries().await.1;
        }
        let all = self.calendar_targets(None, Some("all")).unwrap_or_default();
        (all, rows)
    }

    /// Refresh every kept window that is past its time: the tender's work
    /// while a page has read a window. Returns the changed labels and rows.
    pub async fn calendar_refresh(&self) -> Read {
        let mut out = Read::default();
        if self.calendars.every {
            let (_, rows, added) = self.calendar_summaries().await;
            out.rows.extend(rows);
            out.changed.extend(added);
        }
        for slot in self.calendars.slots() {
            let mut s = slot.lock().await;
            let label = label_of(&s);
            let mut changed = false;
            s.windows.retain(|w| !self.calendars.idle(w));
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
        for slot in self.calendars.slots() {
            if !slot.lock().await.windows.is_empty() {
                return true;
            }
        }
        false
    }

    /// The calendars' own names (`primary` is the entry marked so), from the
    /// account's `calendarList`: read once, and with no configured calendar
    /// read again once it is older than [`TTL`], each read settling which
    /// calendars are read (`Cache::settle`). Returns the names, the rows to
    /// write, and the labels of calendars a later read of the list added.
    pub(super) async fn calendar_summaries(
        &self,
    ) -> (HashMap<String, String>, Vec<Value>, Vec<String>) {
        let mut rows = Vec::new();
        let mut l = self.calendars.listing.lock().await;
        let stale = match &*l {
            None => true,
            Some(x) => self.calendars.every && !self.calendars.listing_fresh(x),
        };
        if !stale {
            return (
                l.as_ref().map(|x| x.names.clone()).unwrap_or_default(),
                rows,
                Vec::new(),
            );
        }
        let again = l.is_some();
        let entries = match self.calendar_list(&mut rows).await {
            Some(e) => e,
            None if again => {
                return (
                    l.as_ref().map(|x| x.names.clone()).unwrap_or_default(),
                    rows,
                    Vec::new(),
                )
            }
            None => Vec::new(),
        };
        let mut names = HashMap::new();
        for e in &entries {
            if let Some(n) = &e.name {
                names.insert(e.id.clone(), n.clone());
                if e.primary {
                    names.insert("primary".to_string(), n.clone());
                }
            }
        }
        let added = self.calendars.settle(&entries).await;
        *self
            .calendars
            .names
            .write()
            .unwrap_or_else(|e| e.into_inner()) = names.clone();
        self.calendars.listed.store(true, Ordering::SeqCst);
        *l = Some(Listing {
            at: Instant::now(),
            aged: self.calendars.aged_ms.load(Ordering::Relaxed),
            names: names.clone(),
        });
        (names, rows, if again { added } else { Vec::new() })
    }

    /// The account's `calendarList`, or `None` when it cannot be read.
    async fn calendar_list(&self, rows: &mut Vec<Value>) -> Option<Vec<Listed1>> {
        let r = resolve("calendar.calendarList.list", None, None).ok()?;
        let meta = meta("calendar.calendarList.list", None);
        let reply = match self.call(meta, r.request).await {
            Ok(done) => {
                rows.push(done.row);
                done.response.json()
            }
            Err(f) => {
                rows.push(f.row);
                return None;
            }
        };
        let mut out = Vec::new();
        for e in reply["items"].as_array().into_iter().flatten() {
            let Some(id) = text(e, &["id"]) else { continue };
            out.push(Listed1 {
                id: id.to_string(),
                primary: e["primary"] == true,
                name: text(e, &["summaryOverride"])
                    .or_else(|| text(e, &["summary"]))
                    .map(str::to_string),
                color: text(e, &["backgroundColor"]).map(str::to_ascii_lowercase),
                owner: text(e, &["dataOwner"]).map(str::to_string),
            });
        }
        Some(out)
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
        w.used_aged = self.calendars.aged_ms.load(Ordering::Relaxed);
        let events = w
            .items
            .iter()
            .filter(|i| overlaps(&i.event, from, to))
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
                    let mut changed =
                        merge(&mut w.items, &page.items, &account, &id, (w.from, w.to));
                    w.token = page.next_sync.or(w.token.take());
                    // A cut delta lost changes, and a cut window is cut as `fetch` cuts it: no
                    // token, so the next refresh reads it whole.
                    let cut = w.items.len() > MAX_EVENTS;
                    if cut {
                        w.items.truncate(MAX_EVENTS);
                        changed = true;
                    }
                    if cut || page.truncated {
                        w.token = None;
                    }
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
            used_aged: self.calendars.aged_ms.load(Ordering::Relaxed),
            token,
            items,
        };
        match self.list(s, Query::Window(wf, wt), rows).await {
            Ok(page) => {
                s.status = CalendarStatus::Ok;
                s.detail.clear();
                let mut items = Vec::new();
                merge(&mut items, &page.items, &account, &id, (wf, wt));
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
                CalendarStatus::Unreachable,
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
        for slot in self.calendars.slots() {
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
        let specs = self.calendars.specs();
        let direct = |n: &str| {
            specs.iter().any(|s| {
                s.id.eq_ignore_ascii_case(n)
                    || s.label
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case(n))
            })
        };
        let unlabelled = self.calendars.every || specs.iter().any(|s| s.label.is_none());
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
        let names = self
            .calendars
            .names
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let names = Some(&names).filter(|m| !m.is_empty());
        let specs = self.calendars.specs();
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
            (_, Some(a)) if a.eq_ignore_ascii_case("all") => Ok(specs.iter().map(target).collect()),
            (_, Some(a)) => Err(format!("`calendars` is \"all\" or left out, not {a:?}")),
            (None, None) => {
                let primary = specs.iter().find(|s| s.id == "primary");
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
                let found = specs
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
                let known: Vec<String> = specs.iter().map(|s| target(s).label).collect();
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

/// Whether `e` falls in `[from, to)`: the one test `serve` answers by and
/// `merge` keeps by (an event of no length counts as one millisecond long).
fn overlaps(e: &CalendarEvent, from: u64, to: u64) -> bool {
    e.start_ms < to && e.end_ms.max(e.start_ms + 1) > from
}

/// Merge Google's items into `kept` by event id (a cancelled one removes its
/// event, and so does one that no longer overlaps the window `bounds`, a
/// `[from, to)`: a sync delta covers the whole calendar); true when anything
/// differs. Sorted by start.
fn merge(
    kept: &mut Vec<Stored>,
    items: &[Value],
    account: &str,
    calendar: &str,
    bounds: (u64, u64),
) -> bool {
    let mut changed = false;
    for raw in items {
        let Some(id) = text(raw, &["id"]) else {
            continue;
        };
        let at = kept.iter().position(|k| k.id == id);
        let projected =
            project(raw, account, calendar).filter(|p| overlaps(&p.event, bounds.0, bounds.1));
        match (projected, at) {
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

#[cfg(test)]
impl Google {
    /// How many events the first kept window of every calendar holds, and its
    /// sync token (a test's view of the bounds).
    pub(crate) async fn kept_events(&self) -> Vec<(usize, Option<String>)> {
        let mut out = Vec::new();
        for slot in self.calendars.slots() {
            for w in &slot.lock().await.windows {
                out.push((w.items.len(), w.token.clone()));
            }
        }
        out
    }
}

/// A hash of what a window holds, to tell a full re-read that found a change.
fn fingerprint(items: &[Stored]) -> u64 {
    let mut h = DefaultHasher::new();
    for i in items {
        (&i.id, i.stamp, i.event.start_ms, i.event.end_ms).hash(&mut h);
    }
    h.finish()
}
