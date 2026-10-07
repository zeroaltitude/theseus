//! Jobs whose wrappers went while no daemon ran (theseus-vej5).
//!
//! The reaper hears only the wrappers this daemon spawned or adopted, and the
//! reconciler probes only overdue actions: a job whose wrapper died while no
//! daemon ran (a unit stop that killed the whole cgroup, an OOM kill, a
//! machine's restart) stayed `dispatched` until its deadline, six hours for
//! a long job, and the watch showed it working all that time.
//!
//! Startup's scan already reads every dispatched action, and notes each one
//! not yet overdue that is not an in-process call (`Earlier::jobs`), writing
//! nothing and reading nothing more. Once the socket serves, the harness's
//! first beat probes each one's wrapper once ([`at_start`]): its completion on
//! the spool first (a finished job is reported, never unknown), then its pid
//! file and its lingering marker (`Spool::wrapper_lives`: either naming a live
//! wrapper of that job is alive, and a reused pid is not), then the
//! completion again, which a wrapper that reported between the two reads
//! wrote before it removed its pid file. What has a pid file or a marker and
//! no live wrapper is gone, and is marked `outcome_unknown` with
//! [`WRAPPER_GONE`] as its reason. What has neither (an in-process tool call,
//! an AWS hand, a job whose daemon died before it wrote the pid) is left to
//! its deadline, as before: nothing says it was a job, and a hand's backend
//! is its own reconciler's. A live wrapper is never touched. The finished and
//! the gone settle in one frame.

use anyhow::Result;

use crate::kernel::{Accepted, Kernel, KernelError};
use crate::spool::Spool;
use crate::types::{Action, Completion, CorrelationId};
use theseus_store::NewRecord;

/// The reason a job whose wrapper went while no daemon ran is marked
/// unknown: its completion's producer is `reconciler:wrapper_gone_at_start`.
pub const WRAPPER_GONE: &str = "wrapper_gone_at_start";

/// What a job's evidence says at a start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtStart {
    /// Its wrapper reported before it went: the completion on the spool.
    Finished(Completion),
    /// A live wrapper of the job: its pid file's or its lingering marker's.
    Alive,
    /// A pid file or a marker, and no live wrapper of the job: gone, with the
    /// pid the spool named.
    Gone { pid: u32 },
    /// No completion, no pid file, and no marker: nothing says it is a job.
    Unwitnessed,
}

/// Job `id`'s evidence at a start.
pub fn at_start(spool: &Spool, id: &str) -> AtStart {
    let completion = || spool.read_completion(id).ok().flatten();
    if let Some(c) = completion() {
        return AtStart::Finished(c);
    }
    if spool.wrapper_lives(id) {
        return AtStart::Alive;
    }
    // A wrapper writes its completion before it removes its pid file: one
    // that reported between the two reads is finished, not gone.
    if let Some(c) = completion() {
        return AtStart::Finished(c);
    }
    match spool.read_pid(id).or_else(|| spool.read_lingering(id)) {
        Some(pid) => AtStart::Gone { pid },
        None => AtStart::Unwitnessed,
    }
}

/// What `settle_gone_jobs` found and did.
#[derive(Debug, Default)]
pub struct GoneJobs {
    /// The dispatched actions startup noted, each probed once.
    pub probed: u32,
    /// Settled from the completion each wrapper spooled before it went.
    pub finished: Vec<CorrelationId>,
    /// A live wrapper, left as it is.
    pub alive: Vec<CorrelationId>,
    /// Marked unknown, each with the pid the spool named.
    pub gone: Vec<(Action, u32)>,
    /// No evidence of a job, left to its deadline.
    pub unwitnessed: u32,
    /// The probe and its frame.
    pub elapsed_us: u64,
}

impl Kernel {
    /// Probe, once, the wrapper of each job startup found dispatched, and
    /// settle in one frame those whose wrapper went while no daemon ran: a
    /// spooled completion is taken, and the rest are marked unknown
    /// ([`WRAPPER_GONE`]). A live wrapper, and an action settled or cancelled
    /// since the scan, is left as it is. Each taken completion's file is
    /// removed once the frame commits. A second call probes nothing.
    pub fn settle_gone_jobs(&self, spool: &Spool) -> Result<GoneJobs> {
        self.settle_gone_jobs_with(spool, |_, _| Ok(vec![]))
    }

    /// `settle_gone_jobs`, with the records `rows` builds for each gone
    /// job, as it is marked and with the pid its spool named, in the same
    /// frame: the core's `job.wrapper_gone` row.
    pub fn settle_gone_jobs_with(
        &self,
        spool: &Spool,
        rows: impl Fn(&Action, u32) -> Result<Vec<NewRecord>>,
    ) -> Result<GoneJobs> {
        let t0 = std::time::Instant::now();
        let mut ids = std::mem::take(&mut self.earlier_noted().jobs);
        // A start that ran its scan twice (a crash inside it) noted twice.
        ids.sort();
        ids.dedup();
        let mut out = GoneJobs {
            probed: ids.len() as u32,
            ..GoneJobs::default()
        };
        let mut finished: Vec<Completion> = Vec::new();
        let mut gone: Vec<(CorrelationId, u32)> = Vec::new();
        for id in ids {
            match at_start(spool, &id) {
                AtStart::Finished(c) => finished.push(c),
                AtStart::Alive => out.alive.push(id),
                AtStart::Gone { pid } => gone.push((id, pid)),
                AtStart::Unwitnessed => out.unwitnessed += 1,
            }
        }
        if finished.is_empty() && gone.is_empty() {
            out.elapsed_us = t0.elapsed().as_micros() as u64;
            return Ok(out);
        }
        let mut execs: Vec<String> = Vec::new();
        for id in finished
            .iter()
            .map(|c| &c.correlation_id)
            .chain(gone.iter().map(|(id, _)| id))
        {
            if let Some(a) = self.action(id)? {
                if !execs.contains(&a.execution_id) {
                    execs.push(a.execution_id);
                }
            }
        }
        let execs: Vec<&str> = execs.iter().map(String::as_str).collect();
        let (taken, marked) = self.frame(&execs, |k| {
            let mut taken = Vec::new();
            for c in &finished {
                match k.take_completion_with(c, vec![])? {
                    Accepted::Taken { .. } => {}
                    _ => taken.push(c.correlation_id.clone()),
                }
            }
            let mut marked = Vec::new();
            for (id, pid) in &gone {
                // Checked again under the lock: one settled since is left.
                match k.mark_unknown(id, WRAPPER_GONE) {
                    Ok(a) => {
                        k.stage(&rows(&a, *pid)?)?;
                        marked.push((a, *pid));
                    }
                    Err(e)
                        if matches!(
                            e.downcast_ref::<KernelError>(),
                            Some(KernelError::ActionState { .. } | KernelError::UnknownAction(_))
                        ) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok((taken, marked))
        })?;
        for c in &finished {
            spool.remove(&spool.completion_path(&c.correlation_id))?;
        }
        out.finished = taken;
        out.gone = marked;
        out.elapsed_us = t0.elapsed().as_micros() as u64;
        Ok(out)
    }
}
