//! What the daemon does for Google after serving (the Google design §2.2):
//! the `google.check` startup phase, and the lapse's one row and one notice.
//! Neither runs without `[google]`.

use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use theseus_store::NewRecord;

use crate::fact::google::{GoogleAuthLapsed, GoogleAuthorized, GoogleRevoked};

impl crate::Core {
    /// After serving: the day's send count is kept in the store, the account's
    /// check runs as the `google.check` phase (the secrets, a refresh, the
    /// cheapest call among the granted services, which must name the account),
    /// and a lapse is told once. The tools fail closed until the check passes.
    pub fn google_after_serving(self: &Arc<Self>) {
        let Some(g) = self.tools.google.clone() else {
            return;
        };
        g.sends.attach(self.store.clone());
        let log = self.startup_log.clone();
        let checked = g.clone();
        tokio::spawn(async move {
            let phase = log.begin("google.check", true, Instant::now());
            let detail = checked.check().await;
            log.end(phase, detail);
        });
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                g.lapsed.notified().await;
                let Some(core) = weak.upgrade() else { return };
                core.google_lapse_told(g.account());
            }
        });
    }

    /// The lapse: `google.auth_lapsed` and, with a DM bound, one post to the
    /// owner, in one frame.
    pub(crate) fn google_lapse_told(&self, account: &str) {
        let text = format!("⚠️ {}", theseus_google::error::LAPSED);
        let told = (|| -> anyhow::Result<()> {
            let mut frame = Vec::new();
            let post = if self.runner.place_rule.owner_dm_bound() {
                let (a, records) = self
                    .outbox
                    .stage_to_operator(None, json!({"kind": "spend_ceiling", "text": text}))?;
                frame.extend(records);
                Some(a)
            } else {
                None
            };
            let f = GoogleAuthLapsed {
                account,
                posted: post.is_some(),
            };
            frame.push(crate::fact::row(&f, None, None)?);
            self.store.append(&frame)?;
            if let Some(a) = post {
                self.outbox.posted(&a);
            }
            Ok(())
        })();
        if let Err(e) = told {
            tracing::warn!(error = %format!("{e:#}"), "google: the lapse was not written");
        }
    }

    fn google_row<F: crate::fact::Fact>(&self, f: &F) {
        let written = crate::fact::row(f, None, None)
            .and_then(|r: NewRecord| self.store.append(&[r]).map(|_| ()));
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "google: a row was not written");
        }
    }

    pub(crate) fn google_authorized(&self, account: &str, scopes: &[String], store: &str) {
        self.google_row(&GoogleAuthorized {
            account,
            scopes,
            store,
        });
    }

    pub(crate) fn google_revoked(&self, account: &str, at_google: bool, cleared: bool) {
        self.google_row(&GoogleRevoked {
            account,
            at_google,
            cleared,
        });
    }
}
