//! `session.typing` (theseus-tnky): someone began to type. The answer is
//! immediate and what it starts runs beside it: the session's provider
//! connection opened if it is cold, and the index tender told to load its
//! model ([`crate::warm`]). Nothing is written, and no turn waits for it.

use std::sync::Arc;

use theseus_protocol::warm::{SessionTypingParams, SessionTypingResult};

use super::server::Conn;
use super::Core;
use crate::approval::Surface;
use crate::session::SessionRecord;

impl Core {
    pub(super) fn session_typing(
        self: &Arc<Self>,
        p: SessionTypingParams,
        conn: Conn<'_>,
    ) -> SessionTypingResult {
        if let Some(why) = self.typist_refused(&p, conn.surface) {
            self.warmth.refuse();
            return SessionTypingResult {
                started: false,
                why: Some(why),
            };
        }
        if self.outbox.stopping() {
            return SessionTypingResult {
                started: false,
                why: Some("the daemon is stopping".into()),
            };
        }
        let key = p.session_id.clone().unwrap_or_default();
        if !self.warmth.admit(&key, tokio::time::Instant::now()) {
            return SessionTypingResult {
                started: false,
                why: Some("this session was warmed within its idle spell".into()),
            };
        }
        // Only the daemon's own runtime runs it (a test's current-thread
        // one too); none, and there is nothing to warm.
        let me = Arc::clone(self);
        tokio::spawn(async move { me.warm_for(p.session_id).await });
        SessionTypingResult {
            started: true,
            why: None,
        }
    }

    /// Why this typist's notice warms nothing, or `None`. The CLI and the
    /// web UI are the owner's own surfaces; through Discord only an owner's
    /// typing counts, never another person's in a shared place (or any
    /// place). Theseus's own MCP server's clients are no typists: its
    /// allow-list (`mcp_admit`) never lets this method through.
    fn typist_refused(&self, p: &SessionTypingParams, surface: Surface) -> Option<String> {
        match surface {
            Surface::Discord => {
                let Some(d) = &p.discord else {
                    return Some("a Discord notice names no one".into());
                };
                let user = format!("discord:{}", d.user_id);
                (!self.runner.place_rule.owners(&self.cfg).contains(&user))
                    .then(|| format!("{user} is not an owner: nothing is warmed for them"))
            }
            Surface::Cli | Surface::Web | Surface::Unnamed | Surface::Mcp => None,
        }
    }

    /// The warm-ups, side by side: the provider the session's next turn
    /// calls, and the tender's model. Each records what it found.
    async fn warm_for(self: Arc<Self>, session: Option<String>) {
        let (provider, tender) =
            tokio::join!(self.warm_provider(session.as_deref()), self.warm_tender());
        tracing::debug!(provider, tender, "a first keystroke warmed");
    }

    /// The provider a session's next turn calls: where its last turn ran, else
    /// the live profile's.
    async fn warm_provider(&self, session: Option<&str>) -> &'static str {
        let sid = session.map(str::to_string);
        let store = self.store.clone();
        let last = tokio::task::spawn_blocking(move || {
            sid.and_then(|id| store.get_session::<SessionRecord>(&id).ok().flatten())
                .and_then(|s| s.last_target)
        })
        .await
        .ok()
        .flatten();
        let name = match last {
            Some(t) => t.provider,
            None => match self
                .runner
                .resolve_target(&self.live_profile().0, None, None, None)
            {
                Ok(t) => t.provider,
                Err(_) => return "no provider",
            },
        };
        let Some(provider) = self.runner.providers.get(&name).cloned() else {
            return "no provider";
        };
        let (outcome, took) = match provider.warm().await {
            Ok(w) if w.already => ("warm".to_string(), w.took),
            Ok(w) => ("opened".to_string(), w.took),
            Err(why) => (format!("failed: {why}"), std::time::Duration::ZERO),
        };
        let tag = if outcome.starts_with("failed") {
            "failed"
        } else {
            outcome.as_str()
        };
        self.telemetry().record_warm("provider", tag);
        self.warmth.provider_warmed(took, outcome);
        "done"
    }

    /// `index.warm`, to a tender that runs: the model starts loading if it
    /// was unloaded, and the tender's idle clock starts over.
    async fn warm_tender(&self) -> &'static str {
        let t0 = std::time::Instant::now();
        match self.index.warm().await {
            Ok(w) => {
                self.telemetry().record_warm("tender", &w.model);
                self.warmth.tender_warmed(t0.elapsed(), w.model);
                "done"
            }
            // Off, not started yet, or down: nothing to warm, and nothing to
            // say beyond health's own index block.
            Err(_) => "no tender",
        }
    }
}
