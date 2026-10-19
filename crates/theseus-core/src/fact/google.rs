//! Google Workspace's facts (the Google design, §4.4): `google.called`, one
//! row for each call a tool made, and the auth rows. A row holds the service,
//! the method, the resource's id, the class, the status, the attempts, the
//! quota units and, for a send, the recipients' **count**: never content, a
//! subject, an address, or a token.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// A call a Google tool made (`google::Google::call`'s row, which rides in
/// the tool result's meta and is recorded when the call's result is).
pub struct GoogleCalled<'a> {
    pub row: &'a Value,
}

impl Fact for GoogleCalled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GoogleCalled);

    fn row(&self) -> Value {
        self.row.clone()
    }
}

/// The consent finished: the account, the scopes Google granted, and where
/// the refresh token is kept. Never the token.
pub struct GoogleAuthorized<'a> {
    pub account: &'a str,
    pub scopes: &'a [String],
    pub store: &'a str,
}

impl Fact for GoogleAuthorized<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GoogleAuthorized);

    fn row(&self) -> Value {
        json!({"account": self.account, "scopes": self.scopes, "store": self.store})
    }
}

/// Google's grant lapsed (`invalid_grant`): the state is `needs_auth`.
pub struct GoogleAuthLapsed<'a> {
    pub account: &'a str,
    pub posted: bool,
}

impl Fact for GoogleAuthLapsed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GoogleAuthLapsed);

    fn row(&self) -> Value {
        json!({"account": self.account, "posted": self.posted})
    }
}

/// `theseus google revoke`: revoked at Google, cleared from the store.
pub struct GoogleRevoked<'a> {
    pub account: &'a str,
    pub at_google: bool,
    pub cleared: bool,
}

impl Fact for GoogleRevoked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GoogleRevoked);

    fn row(&self) -> Value {
        json!({"account": self.account, "at_google": self.at_google, "cleared": self.cleared})
    }
}

/// The `google.called` row a Google tool's result carries in its meta
/// (`google::Google::call` made it): recorded with the result, so a call that
/// failed is a row too. Nothing for any other tool's result.
pub fn of_result(rec: &super::Rec<'_>, meta: &Value) {
    if let Some(row) = meta.get("google_called") {
        rec.record(&GoogleCalled { row });
    }
}
