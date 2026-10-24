//! `[google]`: Google Workspace as a native capability (the Google design,
//! §2.1). The section holds **names** of `[secrets]` entries, never values, as
//! `[aws.accounts.<id>]` does. With no `account`, no Google tool is registered,
//! no startup phase runs, and no secret is read.

use std::collections::BTreeSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_google::Service;

use super::google_calendars::{self as cals, CalendarEntry, CalendarSpec};

/// The one configured account (a second is a rename to
/// `[google.accounts.<email>]` later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleConfig {
    /// The account's address: the consent hints it, and the check refuses a
    /// token for any other. Empty: Google is off.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
    /// The services whose tools are on; the scopes the consent asks for derive
    /// from them. All six by default.
    #[serde(default = "default_services")]
    pub services: Vec<String>,
    /// Sends per local day (`gmail.send` and `google.call`'s sends), a runaway
    /// brake. 0: no sends.
    #[serde(default = "default_max_sends")]
    pub max_sends_per_day: u32,
    /// Addresses whose mail may start a task through a mail watch (`then:
    /// task`), compared in lower case. Empty: no mail starts anything; a mail
    /// watch only notices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_senders: Vec<String>,
    /// The `[secrets]` entries: the client (id and secret, two labelled lines
    /// or the console's JSON) and the refresh token `theseus google auth` mints.
    #[serde(default)]
    pub credentials: GoogleCredentialNames,
    /// A loopback stand-in for Google, `http://127.0.0.1:<port>`: tests and
    /// scratch daemons only, so that no request leaves the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// The calendars read: an id, or `{ id, label, color, sensitive }`. Unset
    /// (or empty): every calendar the account's `calendarList` holds, read
    /// again with the list, so a newly shared calendar appears with no edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calendars: Vec<CalendarEntry>,
    /// The domains whose calendars are sensitive by default (an id's or its
    /// owner's). Unset: every Google Workspace domain, that is any domain but
    /// the consumer ones and Google's own calendar ids (`cals::sensitive_by_domain`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitive_domains: Option<Vec<String>>,
}

fn default_services() -> Vec<String> {
    Service::ALL.iter().map(|s| s.name().to_string()).collect()
}

fn default_max_sends() -> u32 {
    25
}

impl Default for GoogleConfig {
    fn default() -> Self {
        Self {
            account: String::new(),
            services: default_services(),
            max_sends_per_day: default_max_sends(),
            trusted_senders: Vec::new(),
            credentials: GoogleCredentialNames::default(),
            endpoint: None,
            calendars: Vec::new(),
            sensitive_domains: None,
        }
    }
}

impl GoogleConfig {
    /// No account: Google is off.
    pub fn is_empty(&self) -> bool {
        self.account.is_empty()
    }

    /// The configured calendars, in order, with their colours; empty when the
    /// config names none (every calendar of the account's list is read).
    pub fn calendar_specs(&self) -> Vec<CalendarSpec> {
        cals::specs(&self.calendars, self.sensitive_domains.as_deref())
    }

    /// Whether a calendar `id` (or one its `owner` holds) is sensitive by its
    /// domain, under `sensitive_domains`.
    pub fn calendar_sensitive_by_domain(&self, id: &str, owner: Option<&str>) -> bool {
        let d = self.sensitive_domains.as_deref();
        cals::sensitive_by_domain(id, d) || owner.is_some_and(|o| cals::sensitive_by_domain(o, d))
    }

    /// The services the config turns on, in the order it lists them.
    pub fn service_list(&self) -> Vec<Service> {
        self.services
            .iter()
            .filter_map(|s| Service::parse(s))
            .collect()
    }
}

/// The `[secrets]` names of the client and of the refresh token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleCredentialNames {
    #[serde(default = "default_client")]
    pub client: String,
    #[serde(default = "default_refresh_token")]
    pub refresh_token: String,
}

fn default_client() -> String {
    "google_client".into()
}

fn default_refresh_token() -> String {
    "google_refresh_token".into()
}

impl Default for GoogleCredentialNames {
    fn default() -> Self {
        Self {
            client: default_client(),
            refresh_token: default_refresh_token(),
        }
    }
}

/// An address's form: one `@`, text either side, a dot in the domain.
fn email_ok(s: &str) -> bool {
    match s.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && !s.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

impl super::Config {
    /// `[google]`, checked as it is read: nothing is looked up, and no secret
    /// is read (§2.1).
    pub(super) fn validate_google(&self) -> Result<()> {
        let g = &self.google;
        if g.is_empty() {
            return Ok(());
        }
        if !email_ok(&g.account) {
            anyhow::bail!("google.account = {:?} is not an email address", g.account);
        }
        if let Some(a) = g.trusted_senders.iter().find(|a| !email_ok(a)) {
            anyhow::bail!("google.trusted_senders names {a:?}, which is not an email address");
        }
        cals::validate(&g.calendars)?;
        cals::validate_domains(g.sensitive_domains.as_deref())?;
        let mut seen = BTreeSet::new();
        for s in &g.services {
            if Service::parse(s).is_none() {
                let known: Vec<&str> = Service::ALL.iter().map(|s| s.name()).collect();
                anyhow::bail!(
                    "google.services names {s:?}, which is not a service: {}",
                    known.join(", ")
                );
            }
            if !seen.insert(s) {
                anyhow::bail!("google.services names {s:?} twice");
            }
        }
        for (key, name) in [
            ("client", &g.credentials.client),
            ("refresh_token", &g.credentials.refresh_token),
        ] {
            if !self.secrets.contains_key(name) {
                anyhow::bail!(
                    "google.credentials.{key} = {name:?} has no matching entry under [secrets]"
                );
            }
        }
        for name in [&g.credentials.client, &g.credentials.refresh_token] {
            let named = self.broker.secrets.contains_key(name)
                || self
                    .broker
                    .programs
                    .values()
                    .any(|p| p.env.values().any(|s| s == name));
            if named {
                anyhow::bail!(
                    "[broker] names the Google secret {name:?}: no job is ever handed a Google \
                     secret, so the grant is refused"
                );
            }
        }
        if let Some(e) = &g.endpoint {
            if !super::dev_origin_ok(e) {
                anyhow::bail!(
                    "google.endpoint = {e:?} is not a stand-in on this machine: it must be \
                     http://, then localhost or a loopback address, then a port"
                );
            }
        }
        Ok(())
    }
}
