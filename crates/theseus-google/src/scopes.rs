//! The services, their scopes, and the scopes each tool accepts (Google
//! design §2.4). The consent asks for exactly the declared set, one service at
//! a time, so the consent screen and the request cannot drift apart.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// `openid`, and the account's address in the `id_token`.
pub const OPENID: &str = "openid";
pub const EMAIL: &str = "https://www.googleapis.com/auth/userinfo.email";

const GMAIL: &str = "https://www.googleapis.com/auth/gmail.modify";
const GMAIL_FULL: &str = "https://mail.google.com/";
const DRIVE: &str = "https://www.googleapis.com/auth/drive";
const DRIVE_READ: &str = "https://www.googleapis.com/auth/drive.readonly";
const SHEETS: &str = "https://www.googleapis.com/auth/spreadsheets";
const SHEETS_READ: &str = "https://www.googleapis.com/auth/spreadsheets.readonly";
const SLIDES: &str = "https://www.googleapis.com/auth/presentations";
const DOCS: &str = "https://www.googleapis.com/auth/documents";
const CALENDAR: &str = "https://www.googleapis.com/auth/calendar";
const CALENDAR_EVENTS: &str = "https://www.googleapis.com/auth/calendar.events";

/// A Workspace service the config can turn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    Gmail,
    Drive,
    Sheets,
    Slides,
    Docs,
    Calendar,
}

impl Service {
    /// Every service, in the order the config lists them by default.
    pub const ALL: [Service; 6] = [
        Service::Gmail,
        Service::Drive,
        Service::Sheets,
        Service::Slides,
        Service::Docs,
        Service::Calendar,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Service::Gmail => "gmail",
            Service::Drive => "drive",
            Service::Sheets => "sheets",
            Service::Slides => "slides",
            Service::Docs => "docs",
            Service::Calendar => "calendar",
        }
    }

    pub fn parse(s: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|v| v.name() == s)
    }

    /// The scope the consent asks for (§2.4's table).
    pub fn scope(self) -> &'static str {
        match self {
            Service::Gmail => GMAIL,
            Service::Drive => DRIVE,
            Service::Sheets => SHEETS,
            Service::Slides => SLIDES,
            Service::Docs => DOCS,
            Service::Calendar => CALENDAR,
        }
    }

    /// Every scope that lets a call of this service through.
    fn accepts(self) -> &'static [&'static str] {
        match self {
            Service::Gmail => &[GMAIL, GMAIL_FULL],
            Service::Drive => &[DRIVE],
            Service::Sheets => &[SHEETS, DRIVE],
            Service::Slides => &[SLIDES, DRIVE],
            Service::Docs => &[DOCS, DRIVE],
            Service::Calendar => &[CALENDAR, CALENDAR_EVENTS],
        }
    }

    /// What Google calls the API, for "enable the <name> API".
    pub fn api_label(self) -> &'static str {
        match self {
            Service::Gmail => "Gmail API",
            Service::Drive => "Google Drive API",
            Service::Sheets => "Google Sheets API",
            Service::Slides => "Google Slides API",
            Service::Docs => "Google Docs API",
            Service::Calendar => "Google Calendar API",
        }
    }

    /// The name a service goes by in Google's own errors (`gmail.googleapis.com`).
    pub fn api_host(self) -> &'static str {
        match self {
            Service::Gmail => "gmail.googleapis.com",
            Service::Drive => "drive.googleapis.com",
            Service::Sheets => "sheets.googleapis.com",
            Service::Slides => "slides.googleapis.com",
            Service::Docs => "docs.googleapis.com",
            Service::Calendar => "calendar-json.googleapis.com",
        }
    }

    /// The service a Google API host names.
    pub fn of_api_host(host: &str) -> Option<Service> {
        Service::ALL.into_iter().find(|s| s.api_host() == host)
    }

    /// The words for what a missing grant turns off.
    pub fn display(self) -> &'static str {
        match self {
            Service::Gmail => "Gmail",
            Service::Drive => "Drive",
            Service::Sheets => "Sheets",
            Service::Slides => "Slides",
            Service::Docs => "Docs",
            Service::Calendar => "Calendar",
        }
    }
}

/// The scopes the consent asks for: the account's two, then each service's.
pub fn requested(services: &[Service]) -> Vec<String> {
    let mut v = vec![OPENID.to_string(), EMAIL.to_string()];
    for s in services {
        let scope = s.scope().to_string();
        if !v.contains(&scope) {
            v.push(scope);
        }
    }
    v
}

/// Each tool and the scopes it accepts (§2.4: partial grants degrade per
/// tool). `google.call` is not here: it needs a granted service.
pub const TOOLS: [(&str, &[&str]); 14] = [
    ("gmail.search", &[GMAIL, GMAIL_FULL]),
    ("gmail.read", &[GMAIL, GMAIL_FULL]),
    ("gmail.draft", &[GMAIL, GMAIL_FULL]),
    ("gmail.send", &[GMAIL, GMAIL_FULL]),
    ("gmail.label", &[GMAIL, GMAIL_FULL]),
    ("calendar.events", &[CALENDAR, CALENDAR_EVENTS]),
    ("calendar.put", &[CALENDAR, CALENDAR_EVENTS]),
    ("drive.find", &[DRIVE, DRIVE_READ]),
    ("drive.read", &[DRIVE, DRIVE_READ]),
    ("drive.write", &[DRIVE]),
    ("drive.share", &[DRIVE]),
    ("sheets.read", &[SHEETS, SHEETS_READ, DRIVE]),
    ("sheets.write", &[SHEETS, DRIVE]),
    ("slides.write", &[SLIDES, DRIVE]),
];

/// `google.call`'s name.
pub const CALL: &str = "google.call";

/// Every tool's name, `google.call` included.
pub fn tool_names() -> impl Iterator<Item = &'static str> {
    TOOLS.iter().map(|(n, _)| *n).chain([CALL])
}

fn has(granted: &BTreeSet<String>, accepted: &[&str]) -> bool {
    accepted.iter().any(|s| granted.contains(*s))
}

/// The tools the granted scopes turn on.
pub fn tools_on(granted: &BTreeSet<String>) -> Vec<&'static str> {
    let mut on: Vec<&'static str> = TOOLS
        .iter()
        .filter(|(_, accepted)| has(granted, accepted))
        .map(|(n, _)| *n)
        .collect();
    if !services_on(granted).is_empty() {
        on.push(CALL);
    }
    on
}

/// Whether one tool runs on the granted scopes.
pub fn tool_on(granted: &BTreeSet<String>, tool: &str) -> bool {
    tools_on(granted).contains(&tool)
}

/// The services a grant lets a call through.
pub fn services_on(granted: &BTreeSet<String>) -> Vec<Service> {
    Service::ALL
        .into_iter()
        .filter(|s| has(granted, s.accepts()))
        .collect()
}

/// The services of `asked` whose scope the grant lacks, for the words "Drive
/// was not granted: the drive tools stay off".
pub fn missing(asked: &[Service], granted: &BTreeSet<String>) -> Vec<Service> {
    asked
        .iter()
        .copied()
        .filter(|s| !granted.contains(s.scope()) && !has(granted, s.accepts()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn requested_scopes_are_exactly_the_declared_set() {
        let all = requested(&Service::ALL);
        assert_eq!(
            all,
            [
                "openid",
                "https://www.googleapis.com/auth/userinfo.email",
                "https://www.googleapis.com/auth/gmail.modify",
                "https://www.googleapis.com/auth/drive",
                "https://www.googleapis.com/auth/spreadsheets",
                "https://www.googleapis.com/auth/presentations",
                "https://www.googleapis.com/auth/documents",
                "https://www.googleapis.com/auth/calendar",
            ]
        );
        assert_eq!(
            requested(&[Service::Gmail]),
            [
                "openid",
                "https://www.googleapis.com/auth/userinfo.email",
                "https://www.googleapis.com/auth/gmail.modify"
            ]
        );
    }

    #[test]
    fn partial_grant_turns_off_only_the_tools_missing_a_scope() {
        let granted = set(&["openid", EMAIL, GMAIL, SHEETS, SLIDES, DOCS, CALENDAR]);
        let on = tools_on(&granted);
        assert!(on.contains(&"gmail.send") && on.contains(&"calendar.put"));
        assert!(on.contains(&"sheets.read") && on.contains(&"slides.write"));
        assert!(on.contains(&"google.call"));
        for t in ["drive.find", "drive.read", "drive.write", "drive.share"] {
            assert!(!on.contains(&t), "{t} needs the drive scope");
        }
        assert_eq!(missing(&Service::ALL, &granted), [Service::Drive]);
        // With Drive and nothing else, Sheets and Slides still run on it.
        let drive = set(&[DRIVE]);
        assert!(tool_on(&drive, "sheets.write") && !tool_on(&drive, "gmail.read"));
        assert!(tools_on(&set(&["openid"])).is_empty());
    }

    #[test]
    fn every_service_has_a_table_row_and_a_name() {
        for s in Service::ALL {
            assert_eq!(Service::parse(s.name()), Some(s));
            assert_eq!(Service::of_api_host(s.api_host()), Some(s));
        }
        assert_eq!(tool_names().count(), 15);
    }
}
