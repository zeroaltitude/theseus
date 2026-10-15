//! Home, and a notice wake's firing (theseus-8ux6x): a reminder always
//! reaches the owner.
//!
//! - **Home** is the conversation a reminder lives in when no place would hear
//!   it: the session the owner's Discord DM runs on, or, when no DM is bound
//!   (a headless daemon) or it has no session yet, a conversation labeled
//!   "Reminders", made on first use and kept in META `home.session`. A reminder
//!   set by `wake.create` with no session lives there; a `Remind` wake set in a
//!   placeless session stays where it was set and posts to Home when it fires.
//! - **Firing.** The due scan (`Core::fire_notices`, the harness's tick) takes a
//!   notice wake the moment it is due, whatever its conversation is doing: the
//!   kernel's one frame (`Kernel::fire_notices`) removes it (a series puts its
//!   next occurrence back), writes its `wake.fired` row and the post of its
//!   note. No turn, no model call, no admission slot. The post goes where the
//!   conversation posts, else where it posted when the wake was set, else to
//!   Home's DM; with no place at all the surfaces still hear `wake.noticed`,
//!   the Interrupt, with its four answers (`reminder_answers`).

use std::sync::Mutex;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Execution, FiredWake, NoticePost, PendingWake, Post, RetryClass};

use crate::fact::{self, wake::WakeFired};
use crate::outbox::NONCE_KEY;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The META key of the Reminders conversation, when no DM is Home.
pub const HOME_SESSION: &str = "home.session";

/// One Home at a time: two reminders set at once on a daemon with no
/// Reminders conversation yet must not make two.
static HOME_LOCK: Mutex<()> = Mutex::new(());

/// The owner's Home conversation, and where its notes are posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    pub session_id: String,
    pub execution_id: String,
    /// The owner's DM (`discord:dm:<user>`) when one is bound: where a reminder
    /// that no place hears is posted.
    pub target: Option<String>,
}

impl Core {
    /// The owner's Home, made if it is not yet. Never called while a thread
    /// holds an execution's lock: it may open a session.
    pub fn home(&self) -> Result<Home> {
        let _g = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut dm_target = None;
        for place in self.runner.place_rule.owner_dm_places() {
            let target = format!("discord:{place}");
            dm_target.get_or_insert(target.clone());
            if let Some(h) = self.live_home(self.outbox.place_session(&place)?, Some(&target))? {
                return Ok(h);
            }
        }
        let kept = self.store.get_meta::<String>(HOME_SESSION)?;
        if let Some(h) = self.live_home(kept, dm_target.as_deref())? {
            return Ok(h);
        }
        let rec = self.open_session(theseus_protocol::SessionOpenParams {
            dir: None,
            kind: None,
            label: Some("Reminders".into()),
            opened_from: None,
            persona: None,
        })?;
        self.store.put_meta(HOME_SESSION, &rec.session_id)?;
        let execution_id = rec
            .execution_id
            .ok_or_else(|| anyhow::anyhow!("the Reminders conversation has no execution"))?;
        Ok(Home {
            session_id: rec.session_id,
            execution_id,
            target: dm_target,
        })
    }

    /// `session` as a Home: it exists, is not retired, and has an execution.
    fn live_home(&self, session: Option<String>, target: Option<&str>) -> Result<Option<Home>> {
        let Some(id) = session else {
            return Ok(None);
        };
        let Some(rec) = self.store.get_session::<SessionRecord>(&id)? else {
            return Ok(None);
        };
        let Some(execution_id) = rec.execution_id.filter(|_| rec.retired.is_none()) else {
            return Ok(None);
        };
        let open = self
            .kernel
            .execution(&execution_id)?
            .is_some_and(|e| !e.state.is_terminal());
        Ok(open.then(|| Home {
            session_id: id,
            execution_id,
            target: target.map(str::to_string),
        }))
    }

    /// Where a notice's note is posted from `e`, if a place hears it: where its
    /// conversation posts now, else where it posted when the wake was set.
    fn notice_target(&self, e: &Execution, w: &PendingWake) -> Option<String> {
        self.outbox
            .target(&e.session_id)
            .or_else(|| w.target.clone())
    }

    /// The driver's due scan for what needs no turn: the reminders that are due
    /// post, and the watches that are due check (theseus-f2r2b).
    pub fn due_without_turns(self: &std::sync::Arc<Self>) {
        self.fire_notices();
        self.watch_tick();
    }

    /// The due scan's notice pass: every notice wake that is due, in any
    /// state of its conversation, taken in one frame each. Returns how many
    /// fired. A failure is logged and tried again on the next tick.
    pub fn fire_notices(&self) -> usize {
        let execs = match self.kernel.notice_executions() {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(error = %format!("{err:#}"), "the notice scan could not read");
                return 0;
            }
        };
        let now = self.kernel.now_ms();
        let mut fired = 0;
        for e in execs
            .iter()
            .filter(|e| e.wakes.iter().any(|w| w.notice_due(now)))
        {
            match self.fire_notices_of(e, now) {
                Ok(n) => fired += n,
                Err(err) => tracing::warn!(
                    execution_id = %e.id, error = %format!("{err:#}"),
                    "a due reminder could not be posted"
                ),
            }
        }
        // And the routines due, found by the same term (theseus-pvm5c).
        fired + self.fire_routines_in(&execs, now)
    }

    fn fire_notices_of(&self, e: &Execution, now: u64) -> Result<usize> {
        // Home is read before the frame: it may open a session, and the
        // kernel holds the execution's lock while it builds the posts.
        let placeless = e
            .wakes
            .iter()
            .filter(|w| w.notice_due(now))
            .any(|w| self.notice_target(e, w).is_none());
        let home = if placeless { Some(self.home()?) } else { None };
        let sent = self.kernel.fire_notices(&e.id, |e, f| {
            let own = self.notice_target(e, &f.wake);
            let target = own.or_else(|| home.as_ref().and_then(|h| h.target.clone()));
            Ok(NoticePost {
                post: target.map(|target| Post {
                    session_id: e.session_id.clone(),
                    execution_id: e.id.clone(),
                    target,
                    body: reminder_body(f),
                    retry_class: RetryClass::IdempotentWithKey {
                        key: NONCE_KEY.into(),
                    },
                }),
                extra: vec![],
            })
        })?;
        for n in &sent {
            let posted_to = n.post.as_ref().map(crate::outbox::target_of);
            if let Some(a) = &n.post {
                self.outbox.posted(a);
            }
            let in_home = home.as_ref().is_some_and(|h| h.execution_id == e.id)
                || self.notice_target(e, &n.fired.wake).is_none();
            let fact = WakeFired {
                fired: &n.fired,
                session_id: &e.session_id,
                execution_id: &e.id,
                posted_to,
                home: in_home,
            };
            fact::Rec {
                narrator: &self.narrator,
                session: Some(&e.session_id),
                turn: None,
                to: fact::To::Session(&self.bus, &e.session_id),
                store: &self.store,
            }
            .announce(&fact);
        }
        Ok(sent.len())
    }
}

/// A reminder's post: its note whole, and what the courier needs to draw its
/// four buttons (`answers`) and to say which wake it is.
pub fn reminder_body(f: &FiredWake) -> Value {
    let w = &f.wake;
    let mut body = json!({
        "kind": "reminder", "text": w.note, "wake_id": w.id,
        "short": crate::task::short(&w.id), "purpose": w.purpose.as_str(),
        "due_at_ms": w.due_at_ms, "late_ms": f.late_ms,
        "answers": theseus_protocol::reminder_answers(),
    });
    if f.while_down {
        body["while_down"] = json!(true);
    }
    if let Some(r) = &w.repeat {
        body["series"] = json!(crate::wake::series_of(r));
        body["occurrence"] = json!(w.occurrence);
    }
    body
}
