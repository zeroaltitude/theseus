//! Credential requests' facts (M4 18d): a job asked, and how it was
//! answered. Names only, never a value.

use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_protocol::cred::{asked, CredKind};
use theseus_protocol::{notify, Event, LedgerKind, NarrativePart::*, SecretRequested};

use super::{Fact, Say};
use crate::policy::Posture;

/// An L1 job asked for a secret through its socket (`secret.requested`),
/// and what decision 15 made of it at once: granted (open, notify),
/// waiting on a card (approve), or declined (an input error).
#[derive(Clone, Copy)]
pub struct CredRequested<'a> {
    pub job: &'a Action,
    pub command: &'a str,
    pub secret: &'a str,
    pub kind: CredKind,
    /// None when it was declined before any posture.
    pub posture: Option<Posture>,
    pub setting: &'a str,
    /// `granted`, `waiting`, or `declined`.
    pub outcome: &'a str,
    pub why: Option<&'a str>,
    /// The request's action; None for one declined at once.
    pub correlation_id: Option<&'a str>,
}

impl CredRequested<'_> {
    fn short(&self) -> String {
        crate::task::short(&self.job.correlation_id)
    }
}

impl Fact for CredRequested<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretRequested);
    const METHOD: Option<&'static str> = Some(notify::SECRET_REQUESTED);

    fn row(&self) -> Value {
        json!({"job": self.job.correlation_id, "secret": self.secret, "kind": self.kind.as_str(),
               "posture": self.posture.map(Posture::as_str), "setting": self.setting,
               "outcome": self.outcome, "why": self.why, "correlation_id": self.correlation_id,
               "command": self.command})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::SecretRequested(SecretRequested {
            session_id: self.job.session_id.clone(),
            correlation_id: self.correlation_id.unwrap_or_default().into(),
            job: self.job.correlation_id.clone(),
            short: self.short(),
            command: self.command.into(),
            kind: self.kind.as_str().into(),
            secret: self.secret.into(),
            posture: self.posture.map_or("", Posture::as_str).into(),
            setting: self.setting.into(),
            outcome: self.outcome.into(),
            why: self.why.map(str::to_string),
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = capital(&asked(&self.short(), self.command, self.secret));
        let line = match (self.outcome, self.posture) {
            ("declined", _) => format!("{what}: refused ({}).", self.why.unwrap_or("declined")),
            ("waiting", _) => format!("{what}: it waits for the operator ({}).", self.setting),
            (_, Some(p)) => format!("{what}: granted at {} ({}).", p.as_str(), self.setting),
            _ => format!("{what}."),
        };
        say.line(Job, line);
    }
}

/// A request granted (`secret.granted { via: request }`): at once, or by an
/// approval.
pub struct CredGranted<'a> {
    pub request: &'a Action,
    pub job: &'a str,
    pub secret: &'a str,
    /// "granted at notify", "approved by cli".
    pub by: &'a str,
}

impl Fact for CredGranted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretGranted);

    fn row(&self) -> Value {
        json!({"via": "request", "secret": self.secret, "job": self.job,
               "correlation_id": self.request.correlation_id, "by": self.by})
    }
}

/// A request declined (`secret.declined`): by the operator, at once as an
/// input error, at the job's deadline, or by the job's end.
pub struct CredDeclined<'a> {
    /// None for one declined at once, which has no action.
    pub request: Option<&'a Action>,
    pub job: &'a str,
    pub secret: &'a str,
    /// Who declined it: the operator's label, `harness`, or `expiry`.
    pub by: &'a str,
    pub why: &'a str,
}

impl Fact for CredDeclined<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SecretDeclined);

    fn row(&self) -> Value {
        json!({"secret": self.secret, "job": self.job, "by": self.by, "why": self.why,
               "correlation_id": self.request.map(|r| r.correlation_id.as_str())})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        if self.request.is_none() {
            return;
        }
        say.line(
            Approval,
            format!(
                "Job {}'s request for {} was declined by {}: {}.",
                crate::task::short(self.job),
                self.secret,
                self.by,
                self.why
            ),
        );
    }
}

/// `text` with a capital first letter.
fn capital(text: &str) -> String {
    let mut c = text.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
