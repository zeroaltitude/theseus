//! `calendar.window` (theseus-jid7t.1): the calendars the daemon reads, over a
//! window, for the Operations page. The owner's, on a private connection only;
//! never a model tool, so nothing it returns enters a session's context. Its
//! events hold titles, places and call links: they cross this one wire and are
//! in no ledger row, notification or log line.
//! `calendar.changed` carries no content: that the window moved, and which
//! calendars (by label) changed.

use serde::{Deserialize, Serialize};

/// `calendar.window`: events from `from_ms` to `to_ms` (a span of at most 62
/// days).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalendarWindowParams {
    pub from_ms: u64,
    pub to_ms: u64,
}

/// How a calendar read went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CalendarStatus {
    /// Events were read.
    #[default]
    Ok,
    /// Only free/busy is shared: blocks with no title.
    BusyOnly,
    /// Not shared with the account, or a wrong id.
    NotFound,
    Forbidden,
    /// The grant lapsed: `theseus google auth`.
    AuthLapsed,
    /// No `[google]`: nothing is read.
    Off,
}

/// One event, or one busy block (`busy`, with no title, place or call).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalendarEvent {
    /// Stable across reads and opaque: it names nothing.
    pub key: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub all_day: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub call: Option<String>,
    /// How many attendees, never who.
    pub attendees: u32,
    /// The account's own response (`accepted`, `declined`, `tentative`, `needsAction`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub self_response: Option<String>,
    pub organizer_self: bool,
    pub html_link: String,
    pub busy: bool,
}

/// One configured calendar over the window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalendarWindow {
    pub id: String,
    pub label: String,
    /// `#rrggbb`.
    pub color: String,
    pub sensitive: bool,
    pub status: CalendarStatus,
    /// The daemon's words for a status that is not `ok`, with its one fix.
    pub detail: String,
    pub events: Vec<CalendarEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalendarWindowResult {
    pub calendars: Vec<CalendarWindow>,
}

/// `calendar.changed`: a refresh found a change. The page reads the window again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalendarChanged {
    /// The labels of the calendars that changed.
    pub labels: Vec<String>,
}
