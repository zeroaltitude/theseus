//! `[google] calendars`: the calendars the daemon reads (the Operations page,
//! theseus-jid7t.1). An entry is a calendar id, or a table that also names its
//! label, its colour, and whether it is **sensitive**: a read of it raises a
//! `personal` label (`google::private`), and its events never reach a shared
//! place or the ledger. Unset, the daemon reads every calendar the account's
//! `calendarList` holds (`google::calendar_cache`), so a calendar shared with
//! the account appears with no config edit; set, the list narrows it.
//!
//! **Sensitive by default**: a calendar whose id, or whose owner, is in a
//! Google Workspace domain (any domain but the consumer ones and Google's own
//! calendar ids) is sensitive with no config. `[google] sensitive_domains`,
//! when set, replaces that default with its own list.

use std::collections::BTreeSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The account's own calendar, as Google names it in a call.
pub const PRIMARY: &str = "primary";

/// The domains of consumer accounts: a calendar of one is not a workplace's.
const CONSUMER: [&str; 2] = ["gmail.com", "googlemail.com"];

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
    /// Whether the config named the colour (else Google's own wins over the
    /// palette's, once the list is read).
    pub color_set: bool,
    pub sensitive: bool,
}

/// Whether the calendar or address `addr` is sensitive by its domain: one of
/// `domains` (or a subdomain of one) when they are set, else any domain but
/// the consumer ones and Google's own (`*.calendar.google.com`: groups,
/// holidays, imports, rooms). An id with no domain (`primary`) is not.
pub fn sensitive_by_domain(addr: &str, domains: Option<&[String]>) -> bool {
    let Some((_, domain)) = addr.rsplit_once('@') else {
        return false;
    };
    let domain = domain.trim().to_ascii_lowercase();
    let under = |d: &str| {
        let d = d.trim().to_ascii_lowercase();
        domain == d || domain.ends_with(&format!(".{d}"))
    };
    match domains {
        Some(list) => list.iter().any(|d| under(d)),
        None => {
            !domain.is_empty()
                && !CONSUMER.iter().any(|d| under(d))
                && !under("calendar.google.com")
        }
    }
}

impl CalendarEntry {
    pub fn id(&self) -> &str {
        match self {
            CalendarEntry::Id(id) => id,
            CalendarEntry::Table(t) => &t.id,
        }
    }
}

/// The calendars `list` names, in order, with their colours; a calendar is
/// sensitive when its table says so or its domain is (`sensitive_by_domain`).
pub fn specs(list: &[CalendarEntry], domains: Option<&[String]>) -> Vec<CalendarSpec> {
    list.iter()
        .enumerate()
        .map(|(i, e)| match e {
            CalendarEntry::Id(id) => CalendarSpec {
                id: id.clone(),
                label: None,
                color: PALETTE[i % PALETTE.len()].into(),
                color_set: false,
                sensitive: sensitive_by_domain(id, domains),
            },
            CalendarEntry::Table(t) => CalendarSpec {
                id: t.id.clone(),
                label: t.label.clone(),
                color: t
                    .color
                    .clone()
                    .unwrap_or_else(|| PALETTE[i % PALETTE.len()].into()),
                color_set: t.color.is_some(),
                sensitive: t.sensitive || sensitive_by_domain(&t.id, domains),
            },
        })
        .collect()
}

/// The account's own calendar, alone: what a config that names none reads
/// until the account's list says more (or when the list cannot be read).
pub fn primary_spec() -> CalendarSpec {
    CalendarSpec {
        id: PRIMARY.into(),
        label: None,
        color: PALETTE[0].into(),
        color_set: false,
        sensitive: false,
    }
}

/// `#rrggbb`, the form a config colour and Google's own both take.
pub fn color_ok(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// The list's checks, at load: nothing is looked up. Empty is every calendar.
pub(super) fn validate(list: &[CalendarEntry]) -> Result<()> {
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

/// `[google] sensitive_domains`' checks: each a bare domain.
pub(super) fn validate_domains(list: Option<&[String]>) -> Result<()> {
    for d in list.unwrap_or_default() {
        let d = d.trim();
        let bare = !d.is_empty()
            && d.contains('.')
            && !d.starts_with('.')
            && !d.ends_with('.')
            && !d.contains(|c: char| c == '@' || c.is_whitespace() || c.is_control());
        if !bare {
            anyhow::bail!("google.sensitive_domains names {d:?}, which is not a bare domain");
        }
    }
    Ok(())
}
