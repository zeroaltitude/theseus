//! `people.v1` live (theseus-wy7y): at a private conversation's exchange
//! end, beside `categorize.v1` and on the same discipline: never a task's,
//! never a shared place's (the place rule), off the turn's path (the turn's
//! end spawns it), one decision a session at a time. Its own due rule and
//! mark: `categorize::due` over the records after the session's mark (META
//! `judge.people.<session>`: 10 human messages since, or an exchange after
//! 30 minutes' quiet), and the mark moves in the extraction's frame, so a
//! crash loses the two together. Live, not shadow: it writes proposals the
//! owner sees; `[people] live = false` turns it off.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use theseus_store::kinds;

use super::run::{meta, Session};
use super::{lines, PACK};
use crate::judge::categorize::{due, is_human, ExchangeEnd, Mark};
use crate::judge::JudgeService;
use crate::node::Node;
use crate::places::PlaceClass;
use crate::rpc::Core;

/// A session's mark: its last extraction, at META `judge.people.<session>`.
pub const MARK_PREFIX: &str = "judge.people.";

/// The sessions whose decision is running.
#[derive(Default)]
pub struct Point {
    deciding: Mutex<HashSet<String>>,
}

impl JudgeService {
    /// A conversation's exchange end: whether `people.v1` proposes is
    /// decided in a task of its own. Returns at once.
    pub fn people_at_exchange_end(&self, end: &ExchangeEnd, task: bool) {
        if task || !self.pack_on(PACK) {
            return;
        }
        let Some(core) = self.core() else { return };
        if !core.runner.cfg.people.live {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let sid = end.session_id.clone();
        if !self.people.deciding.lock().unwrap().insert(sid.clone()) {
            return;
        }
        let weak = Arc::downgrade(&core);
        drop(core);
        rt.spawn(async move {
            if let Some(core) = weak.upgrade() {
                core.people_live(&sid).await;
                core.runner
                    .judge
                    .people
                    .deciding
                    .lock()
                    .unwrap()
                    .remove(&sid);
            }
        });
    }
}

impl Core {
    /// The decision and, when due, the pass.
    async fn people_live(&self, sid: &str) {
        if self.runner.class_of(sid) != PlaceClass::Private {
            return;
        }
        let key = format!("{MARK_PREFIX}{sid}");
        let read = theseus_store::blocking(|| -> anyhow::Result<_> {
            let mark: Option<Mark> = self.store.get_meta(&key)?;
            let after: Vec<(u64, Node)> = self
                .store
                .scope_after(sid, mark.as_ref().map_or(0, |m| m.through))?
                .iter()
                .filter(|r| r.kind == kinds::NODE)
                .filter_map(|r| Some((r.position, r.decode::<Node>().ok()?)))
                .collect();
            let title = self
                .store
                .get_session::<crate::session::SessionRecord>(sid)?
                .and_then(|s| s.title)
                .unwrap_or_default();
            Ok((mark, after, title))
        });
        let (mark, after, title) = match read {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "people: the session's records were not read");
                return;
            }
        };
        if due(&after, mark.as_ref()).is_none() {
            return;
        }
        let found = lines(&after);
        let Some((through, through_ms)) = after
            .iter()
            .rev()
            .find(|(_, n)| is_human(n))
            .map(|(p, n)| (*p, n.created_at_ms))
        else {
            return;
        };
        if found.is_empty() {
            return;
        }
        let known = match theseus_store::blocking(|| self.people_known()) {
            Ok(mut k) => k.remove(sid).unwrap_or_default(),
            Err(_) => return,
        };
        let moved = Mark {
            judgment: String::new(),
            through,
            through_ms,
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let Ok(mark) = meta(&key, &moved) else { return };
        let pass = self
            .propose_people(Session {
                sid,
                title: &title,
                lines: found,
                purpose: "live",
                mark: Some(mark),
                known,
            })
            .await;
        if let Err(why) = pass {
            tracing::info!(session = %sid, why = %why, "people: nothing proposed");
        }
    }
}
