//! What a stop or a cancel does about the jobs a turn left running
//! (theseus-7kha): the sibling of `Core::end_terminals_left`, for jobs.
//!
//! A job's command can report while something it started runs on: a server,
//! a `&`-backgrounded process, a descendant a double fork orphaned. The
//! wrapper is a child subreaper (theseus-6qy), so the orphan stays under it
//! and the wrapper lingers for it, marking itself `lingering/<job>` in the
//! spool. **That is what `proc.run` leaves behind**, and until this row a
//! stop reached none of it: the job is not in `to_kill` (its action has
//! settled, and `Kernel::stop_call` takes only a `Dispatched` one), and
//! `Cancels::stop_backends` looks for a wrapper with `Spool::read_pid`,
//! which a lingering wrapper removes at its report (`Spool::wrapper_lives`:
//! "a reader that stops there reads a lingering wrapper as gone"). So the
//! loop fell through every arm and did nothing, with no row and no verdict.
//! Terminals' leftovers were ended on both paths (`term::left`,
//! theseus-ggqf) and jobs' were not, which is exactly what the row asked:
//! "or only what a terminal left".
//!
//! **What a stop reaches now.** Each job gets a cgroup of its own under the
//! daemon's delegated one (theseus-a5nv), so "everything the turn started"
//! is a kernel-held set and not a guess: a wrapper asked to stop ends every
//! process in its cgroup and then whatever moved out of it
//! (`job::stop_tree`, `scope: cgroup`), and on a daemon with no delegated
//! cgroup its tree instead, which the subreaper keeps whole.
//!
//! **What a stop keeps: an approved detached task, and nothing else.** Both
//! words carry, and each is a marking the model cannot forge:
//!
//! - **detached** -- the job has a **short id** (`j1`), which only a job
//!   that went on in the background gets (`toolrun::handles`, theseus-n8gk).
//!   The `keep_jobs` branch of `stop_execution_with` already names that same
//!   set the same way.
//! - **approved** -- `Action::confirm` is `Some`: written only by
//!   `Kernel::bind_confirm`, bound to that call's own `args_digest`, naming
//!   the operator who answered.
//!
//! **Why both.** An approval is the word *before* the stop, and a stop is the
//! word after it, so for an ordinary call the stop wins: approving `cargo
//! build` means "run that", not "keep building once I say stop". Only work
//! meant from the start to outlive its turn still speaks through a stop, and
//! that is what a background job is. The approval alone would not do:
//! `[policy.tools] "proc.run" = "approve"` is a supported setting, and under
//! it every `proc.run` carries a `confirm`, so keying on the approval alone
//! would let one config line quietly undo this row. The detachment alone
//! would not do either: the model chooses `background: true` itself, and a
//! model must not be able to opt its own leftovers out of the owner's stop.
//! The owner's standing comforts do not count here, exactly as `[tools.term]
//! keep_background` does not save a terminal's leftovers from a stop
//! (`term::Terms::close`): a stop is deliberately stronger than a session's
//! end.
//!
//! **And a stop says what it did.** Both lists reach the caller
//! (`ExecutionStopResult::left_ended`, `left_kept`), the CLI's words
//! (`theseus_client::render::stop_line`) and the session's record
//! (`fact::driver::JobsLeft`). A stop that ended more than it used to and
//! said nothing would be a worse product than the one it replaces, even
//! though it is more correct.

use theseus_kernel::job::Stopping;

use crate::fact;

/// A job whose command has reported while something it started runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    /// Its correlation id, which names its spool marker and its cgroup.
    pub job: String,
    /// Its lingering wrapper's pid, as the spool's marker named it. Only
    /// `Spool::lingering` writes this, and it returns a marker's pid solely
    /// while `job::wrapper_job(pid)` still names that job, so a pid handed
    /// to another process after the wrapper was reaped never reaches here.
    pub pid: u32,
    /// Its short id (`j1`), where it has one: a job that went on in the
    /// background (theseus-n8gk). `Some` is what makes a job detached.
    pub short: Option<String>,
    /// The operator who approved the call at the floor (`Action::confirm`),
    /// where one did.
    pub approved_by: Option<String>,
}

impl Leftover {
    /// What the caller calls it: its short id where it has one.
    pub fn name(&self) -> &str {
        self.short.as_deref().unwrap_or(&self.job)
    }

    /// An approved detached task: the one thing a stop keeps. Both halves
    /// must hold -- see this module's head for why neither alone does.
    pub fn kept(&self) -> bool {
        self.short.is_some() && self.approved_by.is_some()
    }
}

/// Which leftovers a stop ends, and which it keeps (theseus-7kha). The whole
/// exemption is `Leftover::kept`, in one place, over markings an operator
/// made and a model cannot.
pub fn split(found: Vec<Leftover>) -> (Vec<Leftover>, Vec<Leftover>) {
    found.into_iter().partition(|l| !l.kept())
}

/// What a stop did about what the turn left running.
#[derive(Debug, Clone, Default)]
pub struct JobsLeft {
    pub ended: Vec<String>,
    pub kept: Vec<theseus_protocol::JobKept>,
}

impl JobsLeft {
    pub fn is_empty(&self) -> bool {
        self.ended.is_empty() && self.kept.is_empty()
    }
}

impl crate::rpc::Core {
    /// What the turn's jobs left running, classified (theseus-7kha): its
    /// execution's session's, and its tasks'.
    ///
    /// Read **before** the kernel's stop, so that a leftover the stop keeps
    /// can go in `stop_execution_keeping`'s `keep` list. A background job
    /// whose command has reported but whose completion the drain has not
    /// taken yet still has a `Dispatched` action, so without this the kernel
    /// would cancel the action of a job whose process this keeps running --
    /// a stop that half-kept it. `keep` is the mechanism that already exists
    /// for exactly this (`keep_jobs`, theseus-wy6v.1).
    pub fn jobs_left(&self, execution_id: &str) -> Vec<Leftover> {
        let lingering = self.spool.lingering();
        if lingering.is_empty() {
            return Vec::new();
        }
        // The execution's session and its tasks', as `end_terminals_left`
        // walks them: a task's job lingers under the conversation stopped.
        let mut sessions = Vec::new();
        let mut under = vec![execution_id.to_string()];
        while let Some(id) = under.pop() {
            if let Ok(Some(e)) = self.kernel.execution(&id) {
                sessions.push(e.session_id);
            }
            under.extend(
                self.kernel
                    .tasks(Some(&id))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.id),
            );
        }
        lingering
            .into_iter()
            .filter_map(|(job, pid)| {
                let a = self.kernel.action(&job).ok().flatten()?;
                sessions.contains(&a.session_id).then_some(())?;
                let short = self
                    .tools
                    .jobs
                    .handles
                    .of_session(&self.store, &a.session_id)
                    .into_iter()
                    .find(|(_, e)| e.corr == job)
                    .map(|(short, _)| short);
                Some(Leftover {
                    job,
                    pid,
                    short,
                    approved_by: a.confirm.as_ref().map(|c| c.by.clone()),
                })
            })
            .collect()
    }

    /// End what the turn left running, keeping the approved detached tasks
    /// (theseus-7kha). Each wrapper it ends is asked to stop its whole tree
    /// through the same `job::Stopping` the cancel path uses, which is what
    /// keeps a stop from ever signalling a pid its wrapper no longer holds.
    /// Returns both lists, for the answer and the record.
    pub async fn end_jobs_left(&self, found: Vec<Leftover>, by: &str) -> JobsLeft {
        if found.is_empty() {
            return JobsLeft::default();
        }
        let (end, keep) = split(found);
        let left = JobsLeft {
            ended: end.iter().map(|l| l.name().to_string()).collect(),
            kept: keep
                .iter()
                .map(|l| theseus_protocol::JobKept {
                    job: l.name().to_string(),
                    approved_by: l.approved_by.clone().unwrap_or_default(),
                })
                .collect(),
        };
        if !end.is_empty() {
            // Off the runtime's workers: a stop holds none while it waits
            // (review 2's S2), and `Stopping` polls with sleeps.
            let spool = self.spool.clone();
            let jobs: Vec<(u32, String)> = end.iter().map(|l| (l.pid, l.job.clone())).collect();
            let _ = tokio::task::spawn_blocking(move || {
                let mut stopping = Stopping::start(&spool, jobs, theseus_kernel::job::STOP_GRACE);
                while let Some(wait) = stopping.poll() {
                    std::thread::sleep(wait);
                }
            })
            .await;
        }
        for l in end.iter().chain(keep.iter()) {
            if let Ok(Some(a)) = self.kernel.action(&l.job) {
                self.session_rec(&a.session_id)
                    .record(&fact::driver::JobsLeft {
                        job: l.name(),
                        by,
                        approved_by: l.approved_by.as_deref().filter(|_| l.kept()),
                    });
            }
        }
        left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leftover(job: &str, short: Option<&str>, approved_by: Option<&str>) -> Leftover {
        Leftover {
            job: job.into(),
            pid: 1,
            short: short.map(Into::into),
            approved_by: approved_by.map(Into::into),
        }
    }

    /// The row's rule over the selection alone: a plain leftover is ended,
    /// and an approved detached one is kept.
    #[test]
    fn a_stop_ends_a_leftover_and_keeps_an_approved_detached_one() {
        let (end, keep) = split(vec![
            leftover("act_plain", None, None),
            leftover("act_kept", Some("j1"), Some("the owner")),
        ]);
        assert_eq!(
            end.iter().map(|l| l.name()).collect::<Vec<_>>(),
            ["act_plain"]
        );
        assert_eq!(keep.iter().map(|l| l.name()).collect::<Vec<_>>(), ["j1"]);
        assert_eq!(keep[0].approved_by.as_deref(), Some("the owner"));
    }

    /// Both halves of "an approved detached task" are load-bearing, and
    /// neither alone keeps a leftover.
    ///
    /// **Approved but not detached** is the case an approval alone would get
    /// wrong: an operator approving `cargo build` said "run that", not "keep
    /// building once I say stop", and the stop is the later word. It also
    /// keeps `[policy.tools] "proc.run" = "approve"` -- under which EVERY
    /// call carries a `confirm` -- from quietly undoing this whole row.
    ///
    /// **Detached but not approved** is the case detachment alone would get
    /// wrong: `background: true` is the model's own choice, and a model must
    /// not be able to opt its leftovers out of the owner's stop.
    #[test]
    fn neither_half_of_an_approved_detached_task_keeps_a_leftover_alone() {
        let (end, keep) = split(vec![
            leftover("act_fg_approved", None, Some("the owner")),
            leftover("act_bg_unapproved", Some("j1"), None),
        ]);
        assert_eq!(
            end.iter().map(|l| l.job.as_str()).collect::<Vec<_>>(),
            ["act_fg_approved", "act_bg_unapproved"],
            "both were kept or reordered: {end:?}"
        );
        assert!(keep.is_empty(), "{keep:?}");
    }

    /// The discriminating pair: two leftovers the same operator approved,
    /// differing only in whether the job went on in the background. No
    /// heuristic on the command could part these, and the approval alone
    /// could not either.
    #[test]
    fn two_leftovers_the_same_operator_approved_part_on_detachment() {
        let (end, keep) = split(vec![
            leftover("act_fg", None, Some("the owner")),
            leftover("act_bg", Some("j2"), Some("the owner")),
        ]);
        assert_eq!(end.len(), 1, "{end:?}");
        assert_eq!(end[0].job, "act_fg");
        assert_eq!(keep.len(), 1, "{keep:?}");
        assert_eq!(keep[0].name(), "j2");
    }

    /// Nothing approved: a stop ends every leftover and keeps none.
    #[test]
    fn with_no_approval_a_stop_keeps_nothing() {
        let (end, keep) = split(vec![
            leftover("act_1", Some("j1"), None),
            leftover("act_2", None, None),
            leftover("act_3", Some("j2"), None),
        ]);
        assert_eq!(end.len(), 3);
        assert!(keep.is_empty(), "{keep:?}");
    }

    /// A leftover with no short id is named by its correlation id, so the
    /// stop's report never has a blank where a job should be.
    #[test]
    fn a_leftover_without_a_short_id_is_named_by_its_correlation_id() {
        assert_eq!(leftover("act_7", None, None).name(), "act_7");
        assert_eq!(leftover("act_7", Some("j3"), None).name(), "j3");
    }
}
