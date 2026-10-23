//! `calendar.window` (theseus-jid7t.1): the configured calendars over a
//! window, for the Operations page. **The owner's, in a private place**
//! (`places::owner_in_private`, as `confirm.list` judges the asker): anyone
//! else gets a refusal in words and no data. It is no model tool, so nothing it
//! returns enters a session's context, and the events cross this one wire and
//! no other: the `google.called` rows of its reads hold a method, a status and
//! a calendar's label, and `calendar.changed` carries only labels.

use std::sync::Arc;

use serde_json::Value;
use theseus_google::Service;
use theseus_protocol::calendar::{
    CalendarChanged, CalendarStatus, CalendarWindow, CalendarWindowParams, CalendarWindowResult,
};
use theseus_protocol::{error_code, Event};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::fact::google::GoogleCalled;

impl Core {
    /// `calendar.window`, from `rpc_google`'s arm.
    pub(super) async fn calendar_window(
        self: &Arc<Self>,
        p: CalendarWindowParams,
        conn: Conn<'_>,
    ) -> Result<CalendarWindowResult, RpcFailure> {
        let who = conn.answerer(None, None);
        crate::places::owner_in_private(&who, &self.runner.place_rule, &self.cfg).map_err(
            |why| {
                RpcFailure::new(
                    error_code::REFUSED,
                    format!("calendar.window is the owner's, in a private place: {why}"),
                )
            },
        )?;
        if p.to_ms <= p.from_ms {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "`to_ms` must be after `from_ms`",
            ));
        }
        if p.to_ms - p.from_ms > crate::google::calendar_cache::MAX_SPAN_MS {
            return Err(RpcFailure::new(
                error_code::INVALID_PARAMS,
                "a window is at most 62 days: ask for a shorter span",
            ));
        }
        let google = self
            .tools
            .google
            .clone()
            .filter(|g| g.services().contains(&Service::Calendar));
        let Some(g) = google else {
            return Ok(self.calendars_off());
        };
        let read = g.calendar_window(p.from_ms, p.to_ms).await;
        self.calendar_told(read.rows, read.changed);
        let weak = Arc::downgrade(self);
        g.calendar_tend(Arc::new(move |rows, changed| {
            if let Some(core) = weak.upgrade() {
                core.calendar_told(rows, changed);
            }
        }));
        Ok(CalendarWindowResult {
            calendars: read.calendars,
        })
    }

    /// With no `[google]` (or no calendar service), every configured calendar
    /// is `off` and nothing was read.
    fn calendars_off(&self) -> CalendarWindowResult {
        let g = &self.cfg.google;
        let why = if g.is_empty() {
            "Google is not set up: add `[google] account`, then `theseus google auth`"
        } else {
            "Calendar is not in `[google] services`"
        };
        CalendarWindowResult {
            calendars: g
                .calendar_specs()
                .into_iter()
                .map(|s| CalendarWindow {
                    label: s.label.clone().unwrap_or_else(|| s.id.clone()),
                    id: s.id,
                    color: s.color,
                    sensitive: s.sensitive,
                    status: CalendarStatus::Off,
                    detail: why.into(),
                    events: Vec::new(),
                })
                .collect(),
        }
    }

    /// A read's `google.called` rows into the ledger, one frame, and
    /// `calendar.changed` to the connections that watch every session when a
    /// refresh found a change.
    pub(crate) fn calendar_told(&self, rows: Vec<Value>, changed: Vec<String>) {
        let rec = self.rec(None);
        let records: Vec<_> = rows
            .iter()
            .filter_map(|row| rec.row(&GoogleCalled { row }).ok())
            .collect();
        if !records.is_empty() {
            if let Err(e) = self.store.append(&records) {
                tracing::warn!(error = %format!("{e:#}"), "the calendar's Google rows were not written");
            }
        }
        if !changed.is_empty() {
            let note = Event::CalendarChanged(CalendarChanged { labels: changed });
            self.bus.publish_watchers(&note.into());
        }
    }
}
