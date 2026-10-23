//! `[google] calendars`: the calendars the daemon reads (the Operations page,
//! theseus-jid7t.1). An entry is a calendar id, or a table that also names its
//! label, its colour, and whether it is **sensitive**: its events never reach
//! a model that is not of US origin, a shared place, or the ledger
//! (`google::private`). The default is the account's own `primary` calendar,
//! so a config without the key reads exactly what it always has.

use std::collections::BTreeSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The calendar a config that names none reads.
pub const PRIMARY: &str = "primary";

/// The colours of calendars that name none, in the order the list gives them.
/// Four materials that stay clear of the cockpit's signal tones: brass,
/// verdigris, lapis and amethyst.
pub const PALETTE: [&str; 4] = ["#c9a24a", "#5fa08d", "#5f7fb8", "#9a6fa3"];

/// One entry of `[google] calendars`: an id alone, or a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CalendarEntry {
    Id(String),
    Table(CalendarTable),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarTable {
    pub id: String,
    /// What the page and the tool call it; the calendar's own summary, else
    /// its id, when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `#rrggbb`; the palette's, by position, when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sensitive: bool,
}

/// A configured calendar, its colour settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarSpec {
    pub id: String,
    pub label: Option<String>,
    pub color: String,
    pub sensitive: bool,
}

pub(super) fn default_calendars() -> Vec<CalendarEntry> {
    vec![CalendarEntry::Id(PRIMARY.into())]
}

pub(super) fn is_default(list: &[CalendarEntry]) -> bool {
    list == default_calendars()
}

impl CalendarEntry {
    pub fn id(&self) -> &str {
        match self {
            CalendarEntry::Id(id) => id,
            CalendarEntry::Table(t) => &t.id,
        }
    }
}

/// The calendars `list` names, in order, with their colours.
pub fn specs(list: &[CalendarEntry]) -> Vec<CalendarSpec> {
    list.iter()
        .enumerate()
        .map(|(i, e)| match e {
            CalendarEntry::Id(id) => CalendarSpec {
                id: id.clone(),
                label: None,
                color: PALETTE[i % PALETTE.len()].into(),
                sensitive: false,
            },
            CalendarEntry::Table(t) => CalendarSpec {
                id: t.id.clone(),
                label: t.label.clone(),
                color: t
                    .color
                    .clone()
                    .unwrap_or_else(|| PALETTE[i % PALETTE.len()].into()),
                sensitive: t.sensitive,
            },
        })
        .collect()
}

fn color_ok(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The list's checks, at load: nothing is looked up.
pub(super) fn validate(list: &[CalendarEntry]) -> Result<()> {
    if list.is_empty() {
        anyhow::bail!(
            "google.calendars is empty: name at least one calendar, or leave the key out"
        );
    }
    let mut ids = BTreeSet::new();
    let mut labels = BTreeSet::new();
    for e in list {
        let id = e.id().trim();
        if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
            anyhow::bail!(
                "google.calendars names {:?}, which is not a calendar id",
                e.id()
            );
        }
        if !ids.insert(id.to_ascii_lowercase()) {
            anyhow::bail!("google.calendars names {id:?} twice");
        }
        let CalendarEntry::Table(t) = e else { continue };
        if let Some(c) = t.color.as_deref().filter(|c| !color_ok(c)) {
            anyhow::bail!("google.calendars {id:?}: colour {c:?} is not #rrggbb");
        }
        if let Some(l) = &t.label {
            if l.trim().is_empty() || l.contains(|c: char| c.is_control()) {
                anyhow::bail!(
                    "google.calendars {id:?}: the label is empty or holds a control character"
                );
            }
            if !labels.insert(l.trim().to_lowercase()) {
                anyhow::bail!("google.calendars: the label {l:?} names two calendars");
            }
        }
    }
    Ok(())
}
