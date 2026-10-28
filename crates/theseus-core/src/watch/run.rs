//! The daemon's side of watches (theseus-f2r2b): the driver's tick hands the
//! due ones to the checks, and what a check comes to is written by the
//! kernel (`probe_result`) or not at all.

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use theseus_kernel::{
    Execution, PendingWake, Post, ProbeNews, ProbeVerdict, RetryClass, WatchState,
};
use theseus_protocol::WatchInfo;

use super::check::{self, Tools};
use super::cond::Observed;
use super::quiet::{self, Quiet};
use super::spec::Spec;
use super::step::{Entry, Step};
use crate::fact::{
    self,
    watch::{Ending, WatchEnded},
};
use crate::outbox::NONCE_KEY;
use crate::rpc::Core;

impl Core {
    /// The driver's tick: the first reads the open watches in; each after
    /// starts the checks that are due (at most four at once). Costs a lock and
    /// a short scan; writes nothing.
    pub fn watch_tick(self: &Arc<Self>) {
        if self.outbox.stopping() {
            return;
        }
        if self.watches.first_tick() {
            let weak = Arc::downgrade(self);
            drop(tokio::task::spawn_blocking(move || {
                if let Some(c) = weak.upgrade() {
                    c.watch_load();
                }
            }));
            return;
        }
        for e in self.watches.take_due(self.kernel.now_ms()) {
            self.watch_check(e);
        }
    }

    /// Fill the board from the open watches: each checks once at once.
    pub fn watch_load(&self) {
        let now = self.kernel.now_ms();
        match self.kernel.open_watches() {
            Ok(list) => {
                for (e, w) in &list {
                    self.watch_seed(e, w, now, false);
                }
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "the open watches could not be read")
            }
        }
    }

    /// Put `w` on the board, due at `now`. A watch made now replaces; one the
    /// load found does not undo one made while it read.
    pub fn watch_seed(&self, e: &Execution, w: &PendingWake, now: u64, fresh: bool) {
        let spec = match Spec::of(w) {
            Ok(s) => s,
            Err(why) => {
                tracing::warn!(wake_id = %w.id, error = %why, "a watch's record does not read; it is not checked");
                return;
            }
        };
        let state = if fresh {
            None
        } else {
            self.kernel.watch_state(&w.id).ok().flatten()
        };
        let mut entry = Entry::seed(e.id.clone(), w.id.clone(), spec, w.set_at_ms, state, now);
        if !fresh && matches!(entry.spec.check, super::spec::Check::Mail { .. }) {
            // A restart resumes from the cursor its last frame kept (empty:
            // cleared, or never written).
            let key = format!("{}{}", super::mail::CURSOR_PREFIX, w.id);
            entry.cursor = self
                .store
                .get_meta::<String>(&key)
                .ok()
                .flatten()
                .filter(|c| !c.is_empty());
        }
        if fresh {
            self.watches.insert(entry);
        } else {
            self.watches.insert_new(entry);
        }
    }

    pub(crate) fn watch_tools(&self) -> Tools {
        self.watches.tools(|hook| {
            let t = &self.cfg.tools;
            let sandbox = self.tools.sandbox.clone();
            Tools {
                web: crate::web::Web::new(
                    &t.web,
                    t.result_max_chars,
                    crate::cpu::CpuPool::new(1),
                    self.cfg.policy.private_addresses,
                ),
                run: hook.unwrap_or_else(|| {
                    Arc::new(move |argv: &[String]| {
                        theseus_kernel::probe_run::run(
                            &sandbox.job_view(),
                            argv,
                            check::CHECK_SECS * 1000,
                            check::OUTPUT_BYTES,
                        )
                    })
                }),
                google: self.tools.google.clone(),
            }
        })
    }

    /// One check, off the tick's thread; the core is held by `Weak`, so a
    /// check in flight never keeps the store open past a stop.
    fn watch_check(self: &Arc<Self>, e: Entry) {
        let tools = self.watch_tools();
        let weak = Arc::downgrade(self);
        drop(tokio::spawn(async move {
            let mut rows = Vec::new();
            let result = check::observe(&tools, &e, &mut rows).await;
            let Some(core) = weak.upgrade() else { return };
            let id = e.wake_id.clone();
            // A mail look's calls are rows like any tool's; who sent what is
            // decided here, where the config is.
            let result = core.watch_seen(&id, &rows, result);
            let _ = tokio::task::spawn_blocking(move || core.watch_finish(&id, result)).await;
        }));
    }

    /// A look finished, before an entry takes it: a mail look's calls are rows
    /// like any tool's, and who sent what is decided here, where the config is.
    /// A quiet look's rows go to its entry's tally instead (`quiet.rs`), which
    /// is written, ahead of the rows, with whatever a later look writes.
    pub(crate) fn watch_seen(
        &self,
        wake_id: &str,
        rows: &[serde_json::Value],
        result: Result<Observed, String>,
    ) -> Result<Observed, String> {
        let now = self.kernel.now_ms();
        let quiet = quiet::is_quiet(rows, &result);
        // The first look reads the profile: it is the baseline, never quiet.
        let tally = self.watches.with(wake_id, |e| {
            if quiet && e.cursor.is_some() {
                e.quiet.add(rows, now);
                (
                    e.quiet
                        .due(now)
                        .then(|| std::mem::take(&mut e.quiet).take())
                        .flatten(),
                    false,
                )
            } else {
                (std::mem::take(&mut e.quiet).take(), true)
            }
        });
        let (tally, with_rows) = tally.unwrap_or((None, true));
        // One frame for the look's rows, however many calls it made.
        let rec = self.rec(None);
        let rows = tally.iter().chain(rows.iter().filter(|_| with_rows));
        self.write_rows(&rec, rows);
        result.map(|mut seen| {
            if let Some(m) = &mut seen.mail {
                self.mail_trust(m);
            }
            seen
        })
    }

    /// `google.called` rows, in one frame.
    fn write_rows<'a>(
        &self,
        rec: &fact::Rec<'_>,
        rows: impl Iterator<Item = &'a serde_json::Value>,
    ) {
        let records: Vec<_> = rows
            .filter_map(|row| rec.row(&fact::google::GoogleCalled { row }).ok())
            .collect();
        if !records.is_empty() {
            if let Err(e) = self.store.append(&records) {
                tracing::warn!(error = %format!("{e:#}"), "a mail look's rows were not written");
            }
        }
    }

    /// A watch leaves the board: its tally of quiet looks is written first.
    fn watch_leave(&self, wake_id: &str) {
        let tally = self
            .watches
            .with(wake_id, |e| std::mem::take(&mut e.quiet).take());
        self.watches.remove(wake_id);
        if let Some(row) = tally.flatten() {
            self.write_rows(&self.rec(None), std::iter::once(&row));
        }
    }

    /// What a check came to: the entry takes it, and news is written.
    pub fn watch_finish(&self, wake_id: &str, result: Result<Observed, String>) {
        let now = self.kernel.now_ms();
        let Some((before, step)) = self.watches.with(wake_id, |e| {
            let before = e.clone();
            let step = e.apply(now, result);
            e.running = false;
            (before, step)
        }) else {
            return;
        };
        let Step::News(mut news) = step else { return };
        // A mail watch's task is started first, so the notice names it.
        let started = self
            .mail_then(&before, &mut news)
            .or(before.started.clone());
        // A watch that ends leaves the board before its row can be read
        // (theseus-1b2zm), as a reply watch does.
        let ends = matches!(
            news.verdict,
            ProbeVerdict::Met { .. } | ProbeVerdict::Expired
        );
        if ends {
            self.watch_leave(wake_id);
        }
        match self.watch_write(&before, &news) {
            Ok(true) => {}
            // Cancelled, or its conversation ended.
            Ok(false) => self.watch_leave(wake_id),
            Err(e) => {
                tracing::warn!(wake_id, error = %format!("{e:#}"), "a watch's news was not written; it is checked again");
                let again = Entry {
                    next_ms: now + 30_000,
                    running: false,
                    started,
                    // A leaving watch's tally was written as it left.
                    quiet: if ends {
                        Quiet::default()
                    } else {
                        before.quiet.clone()
                    },
                    ..before
                };
                match ends {
                    true => self.watches.insert(again),
                    false => drop(self.watches.with(wake_id, |e| *e = again)),
                }
            }
        }
    }

    /// Write `news` for the watch `e` was: false when the wake is gone.
    pub(super) fn watch_write(&self, e: &Entry, news: &ProbeNews) -> Result<bool> {
        let want_post = match &news.verdict {
            ProbeVerdict::Changed => false,
            ProbeVerdict::Met { turn } => !turn,
            ProbeVerdict::Expired | ProbeVerdict::Failing => true,
        };
        let Some(ex) = self.kernel.execution(&e.execution_id)? else {
            return Ok(false);
        };
        let Some(w) = ex.wakes.iter().find(|w| w.id == e.wake_id).cloned() else {
            return Ok(false);
        };
        // Where it is said is read before the frame: Home may open a session.
        let said_privately = matches!(
            e.spec.check,
            super::spec::Check::Reply { .. } | super::spec::Check::Mail { .. }
        );
        let target = if want_post && said_privately {
            // A reply's or a mail's notice is never posted into a shared place.
            self.reply_target(self.outbox.target(&ex.session_id), w.target)
        } else if want_post {
            self.outbox
                .target(&ex.session_id)
                .or_else(|| w.target.clone())
                .or_else(|| self.home().ok().and_then(|h| h.target))
        } else {
            None
        };
        let mut body = json!({"kind": "notice", "text": notice_text(&news.verdict, &e.spec.title, &news.text)});
        // A met watch pings as a reminder does (theseus-j90w2): the post names
        // its purpose. It offers no answer: a met watch's wake is gone, so
        // `work.answer` has nothing to find for Done, and "keep watching" has
        // no verb yet.
        if matches!(news.verdict, ProbeVerdict::Met { .. }) {
            body["wake_id"] = json!(e.wake_id);
            body["purpose"] = json!("watch.met");
        }
        let applied = self
            .kernel
            .probe_result(&e.execution_id, &e.wake_id, news, |ex, _| {
                Ok(target.map(|target| Post {
                    session_id: ex.session_id.clone(),
                    execution_id: ex.id.clone(),
                    target,
                    body,
                    retry_class: RetryClass::IdempotentWithKey {
                        key: NONCE_KEY.into(),
                    },
                }))
            })?;
        let Some(done) = applied else {
            return Ok(false);
        };
        #[cfg(test)]
        tests_hook::after_frame(&e.wake_id);
        let posted_to = done.post.as_ref().map(crate::outbox::target_of);
        if let Some(a) = &done.post {
            self.outbox.posted(a);
        }
        let ending = match &news.verdict {
            ProbeVerdict::Changed => return Ok(true),
            ProbeVerdict::Met { .. } => Ending::Met,
            ProbeVerdict::Expired => Ending::Expired,
            ProbeVerdict::Failing => Ending::Failing,
        };
        let home = posted_to.is_none() && ex.session_id == self.home_session_id();
        fact::Rec {
            narrator: &self.narrator,
            session: Some(&ex.session_id),
            turn: None,
            to: fact::To::Session(&self.bus, &ex.session_id),
            store: &self.store,
        }
        .announce(&WatchEnded {
            ending,
            wake_id: &e.wake_id,
            session_id: &ex.session_id,
            execution_id: &ex.id,
            text: &news.text,
            at_ms: self.kernel.now_ms(),
            posted_to,
            home,
        });
        Ok(true)
    }

    /// The Reminders conversation, when one is kept (not made by asking).
    fn home_session_id(&self) -> String {
        self.store
            .get_meta::<String>(crate::wake_home::HOME_SESSION)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// A watch as `watch.list` shows it: the wake and its last frame's state,
    /// with what the board knows beyond it.
    pub fn watch_info(&self, ex: &Execution, w: &PendingWake) -> Option<WatchInfo> {
        let spec = Spec::of(w).ok()?;
        let entry = self.watches.get(&w.id);
        let state: WatchState = match &entry {
            Some(e) => WatchState {
                first: e.first.clone(),
                last: e.last.clone(),
                checks: e.checks,
                checked_ms: e.checked_ms,
                failing: e.failing.clone(),
            },
            None => self
                .kernel
                .watch_state(&w.id)
                .ok()
                .flatten()
                .unwrap_or_default(),
        };
        Some(WatchInfo {
            wake_id: w.id.clone(),
            short: crate::task::short(&w.id),
            session_id: ex.session_id.clone(),
            execution_id: ex.id.clone(),
            title: spec.title.clone(),
            kind: spec.check.kind().into(),
            target: match &spec.check {
                super::spec::Check::Reply { place, .. } => {
                    spec.check
                        .target()
                        .replacen(place.as_str(), &self.place_name(place), 1)
                }
                c => c.target(),
            },
            until: spec.until.clone(),
            every: if matches!(spec.check, super::spec::Check::Reply { .. }) {
                "on arrival".into()
            } else {
                super::spec::every_word(spec.every_ms)
            },
            then: spec.then.word().into(),
            set_at_ms: w.set_at_ms,
            expires_at_ms: spec.expires_ms,
            value: state.last,
            checks: state.checks,
            last_checked_ms: (state.checked_ms > 0).then_some(state.checked_ms),
            next_check_ms: entry
                .as_ref()
                .filter(|_| !matches!(spec.check, super::spec::Check::Reply { .. }))
                .map(|e| e.next_ms)
                .filter(|n| *n != u64::MAX),
            failing: state.failing,
        })
    }

    /// Every open watch (of `session_id` when given), soonest set first.
    pub fn watch_list(&self, session_id: Option<&str>) -> Result<Vec<WatchInfo>> {
        Ok(self
            .kernel
            .open_watches()?
            .iter()
            .filter(|(e, _)| session_id.is_none_or(|s| e.session_id == s))
            .filter_map(|(e, w)| self.watch_info(e, w))
            .collect())
    }
}

/// What the commitments block says of a watch: its last value and when it
/// was seen (`OPEN at 14:05`), from its last frame.
pub fn shown_value(kernel: &theseus_kernel::Kernel, w: &PendingWake) -> Option<String> {
    w.probe()?;
    Some(match kernel.watch_state(&w.id).ok().flatten() {
        Some(s) if s.last.is_some() => format!(
            "{} at {}",
            crate::session::title_from(s.last.as_deref().unwrap_or("")),
            crate::wake::local(s.checked_ms).hm()
        ),
        _ => "not checked yet".into(),
    })
}

/// A test's look at the instant a watch's row is written (theseus-1b2zm): what
/// the board holds then is what a reader of the row finds. A hook is told the
/// wake's id and stays for the test binary's life, so it names its own wake.
/// A watch's post under its head (theseus-x8b9k): the verdict says the kind
/// and the title, the probe's own line follows.
pub(crate) fn notice_text(v: &ProbeVerdict, title: &str, text: &str) -> String {
    use crate::owner_note::{note, Kind};
    let (kind, what) = match v {
        ProbeVerdict::Met { .. } => (Kind::Fyi, "Watch met"),
        ProbeVerdict::Changed => (Kind::Fyi, "Watch saw a change"),
        ProbeVerdict::Expired => (Kind::Fyi, "Watch ended unmet"),
        ProbeVerdict::Failing => (Kind::Alert, "Watch cannot check"),
    };
    note(
        kind,
        &format!("{what}: {title}"),
        "a watch set for you, checked on its own",
        text,
    )
}

#[cfg(test)]
pub(super) mod tests_hook {
    use std::sync::Mutex;

    type Look = Box<dyn Fn(&str) + Send>;
    static LOOKS: Mutex<Vec<Look>> = Mutex::new(Vec::new());

    pub fn add(f: impl Fn(&str) + Send + 'static) {
        LOOKS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Box::new(f));
    }

    pub fn after_frame(wake_id: &str) {
        for f in LOOKS.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            f(wake_id);
        }
    }
}
