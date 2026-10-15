//! A running job's last output line on its work view (theseus-e1hu4): the
//! sampler. The board has no timer of its own and no frame is written for a
//! line, so a task on the runtime walks the running jobs once a second while
//! there are any, and waits on a `Notify` while there are none: the push's
//! applier wakes it when a frame leaves a job running. Each sample is a stat
//! and one read of at most 4 KiB from the end of the job's output file, on the
//! blocking pool, however much the job prints; a line is scrubbed by the
//! session's scrubber and cut to `LAST_LINE_CHARS`, and goes out only when it
//! changed, so a job costs the board one push a second at most.
//!
//! Private places only: a job of a session that speaks in a shared place is
//! never sampled (and a connection that reads no private text is given no
//! `last_line`, `place.rs`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_memory::recall::Place;
use theseus_protocol::job_follow::{cut_line, LAST_LINE_CHARS};

use super::Push;
use crate::Core;

/// How often the running jobs are sampled.
pub const TICK: Duration = Duration::from_secs(1);

impl Push {
    /// The jobs running now: (id, session).
    pub fn running_jobs(&self) -> Vec<(String, String)> {
        self.work.lock().unwrap().running_jobs()
    }

    /// The usual time of a job's program, as the board's view of it says.
    pub fn job_usual_ms(&self, id: &str) -> Option<u64> {
        self.work.lock().unwrap().usual_of(id)
    }

    /// Say a job's newest line on its view and tell the work watchers.
    pub fn sample_job(&self, core: &Core, id: &str, line: Option<String>) {
        let out = self
            .work
            .lock()
            .unwrap()
            .sample(id, line, theseus_protocol::now_unix_ms());
        if let Some(out) = out {
            core.bus.publish_work(&out, &core.tools.scrubber);
        }
    }

    /// Wake the sampler: a job may be running.
    pub(super) fn wake_sampler(&self) {
        self.sampler.notify_one();
    }
}

impl Core {
    /// Start the sampler after serving. It holds the core by `Weak`; a
    /// sample runs on the blocking pool.
    pub fn sample_last_lines_after_serving(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let wake = self.push.sampler.clone();
        let private: Arc<Mutex<HashMap<String, bool>>> = Arc::default();
        tokio::spawn(async move {
            loop {
                let jobs = match weak.upgrade() {
                    Some(c) => c.push.running_jobs(),
                    None => return,
                };
                if jobs.is_empty() {
                    wake.notified().await;
                    continue;
                }
                let (w, p) = (weak.clone(), private.clone());
                let done = tokio::task::spawn_blocking(move || {
                    if let Some(c) = w.upgrade() {
                        c.sample_last_lines(&jobs, &p);
                    }
                })
                .await;
                if done.is_err() {
                    return;
                }
                tokio::time::sleep(TICK).await;
            }
        });
    }

    /// One pass: each job of a private place, its newest line.
    pub fn sample_last_lines(
        &self,
        jobs: &[(String, String)],
        private: &Mutex<HashMap<String, bool>>,
    ) {
        let Some(spool) = self.tools.spool.as_ref() else {
            return;
        };
        let mut known = private.lock().unwrap();
        known.retain(|s, _| jobs.iter().any(|(_, js)| js == s));
        for (id, session) in jobs {
            let ok = *known
                .entry(session.clone())
                .or_insert_with(|| !matches!(self.runner.place_of(session), Place::Shared(_)));
            if !ok {
                continue;
            }
            let line = crate::toolrun::last_line(&spool.result_path(id))
                .map(|l| cut_line(&self.tools.scrubber.scrub(&l).0, LAST_LINE_CHARS));
            self.push.sample_job(self, id, line);
        }
    }
}
