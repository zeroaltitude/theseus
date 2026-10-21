//! The place digest's turn side (theseus-98bz.9; `compiler::place_digest`).
//!
//! - **Asked on the one inbound request.** A message in a thread's own session
//!   asks classify.v4's `scope` without the list of threads (`in_thread`); a
//!   message the router placed asked it with the list (`Early.scope`, handed
//!   over). The turn holds the answer's channel (`Digest.scope`).
//! - **Read before the first loop's recall**, in a thread's session only: an
//!   answer already in is taken at once, else the turn waits at most
//!   `[threads] route_wait_ms`; a timeout, a dropped answer, or `thread` gives
//!   no digest. An unthreaded session holds no channel and waits for nothing.
//! - **On `place`** the turn's requests carry the digest after the last cache
//!   mark, every loop of this turn and no other, and a `threads` mark of kind
//!   `place_digest` on the trace says what was read, waited and shown.

use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::oneshot::error::TryRecvError;

use super::{Turn, TurnRunner};
use crate::compiler::Compiled;
use crate::judge::inbound::ScopeWait;
use crate::places::PlaceClass;

/// What the turn holds of the place digest.
#[derive(Default)]
pub(crate) struct Digest {
    /// classify.v4's `scope` answer, when it was asked.
    pub scope: Option<ScopeWait>,
    /// The digest this turn's requests carry.
    pub text: Option<String>,
}

impl Digest {
    /// Whether this turn's requests carry a digest: Jev's read of the near
    /// misses is then not asked (theseus-98bz.10).
    pub fn fired(&self) -> bool {
        self.text.is_some()
    }
}

impl TurnRunner {
    /// Whether a message in `session` asks `scope` itself: the session is a
    /// thread's own and the digest is on.
    pub(super) fn scope_in_thread(&self, session: &str) -> bool {
        let cfg = &self.cfg.threads;
        cfg.enabled
            && cfg.place_digest
            && self
                .threads
                .held()
                .is_some_and(|b| b.of_session(session).is_some())
    }

    /// The first loop's read of the `scope` answer, and the digest it gives.
    pub(super) async fn digest_first(&self, t: &mut Turn<'_>) {
        let Some(mut rx) = t.digest.scope.take() else {
            return;
        };
        let cfg = &self.cfg.threads;
        if !cfg.enabled || !cfg.place_digest {
            return;
        }
        let Some(board) = self.threads.held() else {
            return;
        };
        let Some(here) = board.of_session(t.tc.session_id).map(|h| h.slug.clone()) else {
            return;
        };
        let t0 = Instant::now();
        let answer = match rx.try_recv() {
            Ok(a) => Some(a),
            Err(TryRecvError::Closed) => None,
            Err(TryRecvError::Empty) => {
                let wait = Duration::from_millis(cfg.route_wait_ms);
                tokio::time::timeout(wait, &mut rx)
                    .await
                    .ok()
                    .and_then(Result::ok)
            }
        };
        let live = board.live(PlaceClass::Private);
        let digest = answer.as_ref().filter(|a| a.place).and_then(|_| {
            crate::compiler::place_digest::render(&live, &here, cfg.place_digest_line_chars)
        });
        t.trace.mark(
            "threads",
            "place_digest",
            json!({
                "scope": answer.as_ref().map(|a| a.choice.as_str()),
                "confidence": answer.as_ref().map(|a| a.confidence),
                "judgment": answer.as_ref().map(|a| a.judgment.as_str()),
                "waited_ms": t0.elapsed().as_millis() as u64,
                "answered": answer.is_some(),
                "shown": digest.as_ref().map_or(0, |d| d.shown),
                "embargoed": digest.as_ref().map_or(0, |d| d.embargoed),
            }),
        );
        t.digest.text = digest.map(|d| d.text);
    }

    /// The digest after the request's last cache mark, its tokens counted.
    pub(super) fn digest_line(t: &Turn<'_>, compiled: &mut Compiled) {
        let Some(text) = &t.digest.text else {
            return;
        };
        crate::task_graph::view::attach_text(&mut compiled.request, text);
        let n = (text.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
        compiled.est_tokens += n;
        compiled.estimate.tokens += n;
        compiled.estimate.estimated += n;
        compiled.estimate.upper += n + n * crate::compiler::MARGIN_PERCENT / 100;
    }
}
