//! The one reconsideration point for every delay in a turn (theseus-5x8ry).
//!
//! The owner's standing rule (2026-10-25 14:16): *"Since we own the continual
//! gateway event loop, we should have a creative way to dynamically consider
//! what is happening during any kind of delay, and revise approach: provider
//! timeout? change models and retry. long task: is it worth it? if yes and
//! yes, keep going. if yes and no, consider what alternative approach to take
//! based on the details of what is delayed (a tool call, a busy system, etc).
//! A simple admission timeout that halts so much is certainly too blunt."*
//!
//! So: **a clock decides only when to ask, never what to do.** One delay
//! arrives here as a [`Delay`] and leaves as a [`Course`]. Three things make
//! that affordable and safe, in this order:
//!
//! 1. **The free pre-check** ([`pre_check`]), which answers most delays with
//!    no model call at all. Most delays are progress: a build compiling for
//!    forty minutes is not a delay, it is work.
//! 2. **The money lines, which speak before the adviser ever does.** A delay
//!    on an execution already past half its ten-minute burn line is stopped
//!    here, with the money notice, so Jev is never in a position to authorise
//!    spend. **This row raises no money guard**: the $200/10-min burn line,
//!    the $2,000 daily token runaway and the AWS lines are this check's
//!    *inputs*, never its subjects, and no course may be `KeepGoing` past one.
//! 3. **No adviser, no stop.** If Jev errors, times out or is over its day
//!    budget, the course is [`Course::KeepGoing`] once more with a notice; on
//!    the *second* such reconsideration of the same delay kind on the same
//!    execution, the site falls back to the behaviour it had before this
//!    module, with a notice. So the degraded path is exactly today's
//!    behaviour, the system can never deadlock on its own adviser, and the
//!    change cannot make anything worse than the status quo.
//!
//! **Where this lives, and why — the reason is NOT a crate cycle.** It would
//! be easy to assume `theseus-kernel` cannot take a `theseus-judge`
//! dependency because `theseus-core` depends on the kernel. **That is wrong,
//! and it is worth writing down because it wrongly closes off a design option
//! a later stops row may want.** `theseus-judge` is a *leaf*: it depends on no
//! theseus crate at all, so `theseus-kernel -> theseus-judge` would add no
//! cycle.
//!
//! The real constraints are two, and both are stronger:
//!
//! - **The kernel is synchronous**, by its own package description
//!   ("deterministic under an injected clock so the simulator can drive it"),
//!   and the code holds it: there is no `async` anywhere in
//!   `crates/theseus-kernel/src`, and `tokio` is a dev-dependency only. A
//!   judge call is async and does network I/O; asking from inside a kernel
//!   transition would make the kernel non-deterministic and break what
//!   `kernel-sim` relies on.
//! - **The judge *service* is core's, not the leaf crate's.** The day budget
//!   ([`crate::judge::spend`]), the scrubber, the blob staging and the
//!   recording sink all live in `theseus-core`. A kernel-side ask could only
//!   use the raw client, so it would be unbudgeted, unscrubbed and
//!   unrecorded - which would break the cost bound this module is built on.
//!
//! So the practical rule stands: a kernel deadline is reconsidered *before*
//! the kernel is handed it, never at the moment it fires. But it stands as a
//! trade, not as an impossibility, and a kernel that *reported* a pending cut
//! up to core and took an answer back would cost the kernel neither property.
//!
//! **What it does not do.** It writes no frame on a plain turn's path: it is
//! reached only on a delay, and a plain turn has none, so
//! `tests_m3::a_plain_turn_stays_within_its_frame_budget` is untouched. It
//! holds no kernel lock: it is `async` and may await a judge call, so every
//! caller places it outside a kernel transition, never between a read and its
//! frame. And it moves no Discord DM task message (memory
//! `theseus-discord-task-flow-locked`): its notice goes out through
//! `stage_to_operator`, which is not the task flow.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::judge::delay::{DelayInput, DelaySignal, DelayVerdict};

use super::TurnRunner;

/// What a delay may be reconsidered for. One per site, so the rate bound and
/// the fallback are per kind as well as per execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DelayKind {
    /// A turn waiting for a free admission slot (the slot queue's window).
    ///
    /// **Distinct from [`DelayKind::AdmissionTurn`] on purpose.** The stuck
    /// window governs two different waits through two different doors, and
    /// every bound here is keyed per kind. Were the two doors one kind, a
    /// course cached at one - possibly a `Stop` - would be replayed at the
    /// other unasked, which is the same defect as sharing a key across
    /// unrelated kinds. They are also genuinely different questions: one is
    /// "the machine is full", the other "this conversation's own last turn
    /// has not finished", and a judgment about one says nothing about the
    /// other.
    AdmissionSlot,
    /// A turn waiting for its own execution's previous turn to end.
    AdmissionTurn,
    /// A model call that failed or was cut.
    Provider,
    /// A job that has printed nothing for a beat.
    Job,
    /// A question nobody has answered.
    Question,
    /// A self build waiting for the host.
    SelfBuild,
    /// A self-extension trial.
    Trial,
}

impl DelayKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DelayKind::AdmissionSlot => "admission_slot",
            DelayKind::AdmissionTurn => "admission_turn",
            DelayKind::Provider => "provider",
            DelayKind::Job => "job",
            DelayKind::Question => "question",
            DelayKind::SelfBuild => "self_build",
            DelayKind::Trial => "trial",
        }
    }
}

/// One progress signal the site could read, sampled when the delay began and
/// again now. **A signal the site could not read is simply absent**: an
/// absent signal means *unknown*, and must never read as *not moving*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// What it is, in the owner's words ("bytes a backgrounded build printed").
    pub name: &'static str,
    /// Whether it changed since the delay began.
    pub moved: bool,
}

/// One delay, with everything the pre-check and the pack need. Self-contained
/// on purpose: it borrows no `Turn` (the admission site runs before a turn
/// exists) and holds no lock.
#[derive(Debug, Clone)]
pub struct Delay {
    pub kind: DelayKind,
    /// What is delayed, in the owner's words.
    pub what: String,
    /// The execution whose burn window this delay belongs to.
    pub execution: String,
    /// The session the delay is in, for the pack's mode and its row.
    pub session: String,
    pub elapsed: Duration,
    /// Every signal the site could read.
    pub signals: Vec<Signal>,
    /// The last thing the work was seen doing, where the site knows it.
    pub last_step: Option<String>,
    /// Why the work was started, where the site knows it.
    pub reason: Option<String>,
    /// A deterministic course the site already knows, which the pre-check
    /// takes without asking (a whole-call provider timeout with a fallback
    /// model is `SwitchModel`, which `turn/retry_step.rs` already performs).
    pub deterministic: Option<Course>,
}

/// What to do about a delay. [`Course::Stop`] is the only variant that stops,
/// and it stops because something judged the work stuck, never because a
/// number elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Course {
    /// Carry on unchanged and look again after the next window.
    KeepGoing,
    /// Make the same request on the catalog's fallback model, once.
    SwitchModel,
    /// Carry on, and tell the work what is delayed so it may plan
    /// differently. **This is not a stop**: it is a harness note.
    ChangeApproach,
    /// Carry on against a smaller objective.
    NarrowScope,
    /// Stop and report where it got to.
    Stop(StopWhy),
}

/// Why a [`Course::Stop`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopWhy {
    /// The execution is at or past half its ten-minute burn line. The keel,
    /// answering before the adviser was asked.
    MoneyRate,
    /// The adviser judged the work not worth finishing, or stuck.
    Adviser,
    /// The adviser could not answer twice for this delay, so the site falls
    /// back to the behaviour it had before this module.
    NoAdviserTwice,
}

impl Course {
    /// The course's id as `delay.v1` names it, for the notice and the row.
    pub fn as_str(self) -> &'static str {
        match self {
            Course::KeepGoing => "keep_going",
            Course::SwitchModel => "switch_model",
            Course::ChangeApproach => "change_approach",
            Course::NarrowScope => "narrow_scope",
            Course::Stop(_) => "stop_and_report",
        }
    }

    /// Whether this course ends the work.
    pub fn stops(self) -> bool {
        matches!(self, Course::Stop(_))
    }
}

/// What `delay.v1`'s `course` answer runs when it is under its confirm bar:
/// the pack's own `no_match`.
///
/// **One value in two places, the pack and here, because nothing on the live
/// path reads a pack's declared `no_match`** - `effort.v1` is the only pack
/// whose declared default is honoured, and it is honoured by exactly this
/// hand-copied-constant idiom (`routing/effort.rs:143`, theseus-h66v4.6.1);
/// its sibling `route.v10` declares one and the live path ignores it. So a
/// declared default here would be silently dropped without this constant, and
/// `the_daemons_no_match_is_the_packs` holds the two equal.
///
/// It is `keep_going` because under the bar nothing is known, and the owner's
/// rule under uncertainty is long work over stopped work.
pub const NO_MATCH: Course = Course::KeepGoing;

/// Reconsiderations of **one delay kind on one execution**, past which that
/// kind's own last course repeats unasked. Six decisions at the default
/// five-minute spacing covers a half-hour delay; past that the system has
/// said what it thinks about that kind.
///
/// Per kind rather than per execution, so that one kind's cap can never lock
/// out another kind or hand it a course judged about a different delay. The
/// per-execution worst case is therefore this many times the number of kinds;
/// the real ceiling on cost is the judge's own day budget, which every
/// reconsideration is made inside.
pub const MAX_RECONSIDERS: u32 = 6;

/// The share of the burn line at which a delay is stopped for money without
/// the adviser being asked.
const BURN_SHARE: u64 = 2;

/// What the free pre-check decided, or that it could not decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreCheck {
    /// Take this course; no model call was made.
    Took(Course),
    /// Nothing here settles it: ask the adviser.
    Ask,
}

/// The free pre-check, in order, with no model call. `burn` is the
/// execution's settled spend in its ten-minute window and the line it is
/// measured against, as `DayCeiling::burn_spent` reads them; a line of 0
/// means none is set and the money test does not apply.
pub fn pre_check(d: &Delay, burn: (u64, u64)) -> PreCheck {
    // 1. Did any readable signal move? A delay that is progress is not a
    //    delay. This is the arm that makes the feature affordable.
    if d.signals.iter().any(|s| s.moved) {
        return PreCheck::Took(Course::KeepGoing);
    }
    // 2. Is there a deterministic course? Take it; the adviser adds nothing
    //    and a provider timeout's retry must be fast.
    if let Some(c) = d.deterministic {
        return PreCheck::Took(c);
    }
    // 3. Is a money rate line within reach? The keel speaks before the
    //    adviser ever does, so no adviser can authorise spend.
    let (spent, line) = burn;
    if line > 0 && spent >= line / BURN_SHARE {
        return PreCheck::Took(Course::Stop(StopWhy::MoneyRate));
    }
    PreCheck::Ask
}

/// The course `delay.v1`'s answers mean, mapping the owner's clauses one for
/// one. `None` from the `course` answer is an answer under its confirm bar,
/// which takes [`NO_MATCH`].
pub fn course_of(v: &DelayVerdict) -> Course {
    // "if yes and yes, keep going"
    let worth = v.worth_it >= 0.5;
    let moving = v.progressing >= 0.5;
    if !worth {
        // Not worth finishing: the one place the adviser's own words stop it.
        return Course::Stop(StopWhy::Adviser);
    }
    if worth && moving {
        return Course::KeepGoing;
    }
    // "if yes and no, consider what alternative approach to take based on the
    // details of what is delayed": the course answer decides.
    match v.course.as_deref() {
        Some("keep_going") => Course::KeepGoing,
        Some("switch_model") => Course::SwitchModel,
        Some("change_approach") => Course::ChangeApproach,
        Some("narrow_scope") => Course::NarrowScope,
        Some("stop_and_report") => Course::Stop(StopWhy::Adviser),
        // Under the bar, or an option the build does not know: the pack's
        // own no-match, applied from `NO_MATCH` because the live path reads
        // no pack's declared one.
        _ => NO_MATCH,
    }
}

/// The notice's words. Jev returns a choice, a score or a probability and
/// **never a sentence** (`theseus_judge::Answer` has no text variant), so the
/// reason is composed here from the answers and the delay's own facts rather
/// than quoted from the adviser.
pub fn words(d: &Delay, c: Course, why: &str) -> String {
    let mins = d.elapsed.as_secs() / 60;
    format!(
        "A {} delay on {}: {}, because {}. {} min in.",
        d.kind.as_str(),
        d.what,
        c.as_str(),
        why,
        mins
    )
}

/// Why a course was taken, in the owner's words, for [`words`].
pub fn reason(v: &DelayVerdict) -> String {
    let worth = if v.worth_it >= 0.5 {
        "it is still worth finishing"
    } else {
        "it is no longer worth finishing"
    };
    let moving = if v.progressing >= 0.5 {
        "and it is still moving"
    } else {
        "and it is not moving"
    };
    format!("{worth} {moving}")
}

/// One delay kind's reconsiderations on one execution: how many have been
/// made, when it was last asked, how many times in a row the adviser could
/// not answer, and the course that repeats past the cap.
///
/// **Every field here is per kind, and that is load-bearing.** Keyed per
/// execution instead, one kind's cap would lock out every other kind on that
/// execution and hand each of them the *first* kind's cached course to replay
/// unasked - and that cache can hold a `Stop`, so a job delay could be stopped
/// by a judgment made about an admission delay. With one site wired that is
/// unreachable; it goes live the moment a second site is wired, which is
/// exactly what the rows built on this spine do.
#[derive(Debug, Default)]
struct Seen {
    /// Reconsiderations made of this kind, capped at [`MAX_RECONSIDERS`].
    made: u32,
    /// When this kind was last asked.
    last: Option<Instant>,
    /// Consecutive unavailable answers. Reset by any answer.
    unavailable: u32,
    /// The last course taken **for this kind**, which repeats unasked past
    /// [`MAX_RECONSIDERS`].
    last_course: Option<Course>,
}

/// How long a kind's record outlives its last reconsideration. Past it the
/// record is forgotten, so **a cached course can never be replayed an hour
/// after it was judged**: the cap restarts, the adviser gets a fresh chance,
/// and a `Stop` decided about a delay that is long over cannot stop anything.
/// An execution whose delays have gone quiet for this long also stops costing
/// memory.
const STALE: Duration = Duration::from_secs(3600);

/// Executions whose records are kept before the map is swept wholesale, as
/// the kernel's burn ring bounds itself.
const EXECUTIONS_KEPT: usize = 512;

/// Every execution's reconsiderations, per delay kind. Bounded two ways: a
/// kind's record expires [`STALE`] after its last reconsideration, and the
/// map is swept when it passes [`EXECUTIONS_KEPT`].
#[derive(Debug, Default)]
pub struct Reconsiders(Mutex<HashMap<String, HashMap<DelayKind, Seen>>>);

/// Drop every record whose last reconsideration is older than [`STALE`], and
/// every execution left with none.
fn expire(m: &mut HashMap<String, HashMap<DelayKind, Seen>>, now: Instant) {
    m.retain(|_, kinds| {
        kinds.retain(|_, s| {
            s.last
                .is_some_and(|at| now.saturating_duration_since(at) < STALE)
        });
        !kinds.is_empty()
    });
}

impl Reconsiders {
    /// Whether `kind` on `execution` may be asked now, and the course to
    /// repeat unasked when it may not. `every` is
    /// `[kernel] reconsider_every_secs`, which the caller has already found
    /// to be set: the off switch is the caller's, decided before any of this
    /// module's work, so that a site with the feature off always reaches its
    /// own fallback.
    ///
    /// Every bound read here is this kind's own: a kind is never held back,
    /// nor handed a course, by another kind's history.
    pub fn may_ask(
        &self,
        execution: &str,
        kind: DelayKind,
        every: Duration,
        now: Instant,
    ) -> Result<(), Course> {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // A record older than STALE is forgotten before it is read, so a
        // cached course can never be replayed long after it was judged.
        expire(&mut m, now);
        if m.len() > EXECUTIONS_KEPT {
            m.clear();
        }
        let s = m
            .entry(execution.to_string())
            .or_default()
            .entry(kind)
            .or_default();
        // The adviser was unavailable last time for this kind, so we are in
        // the degraded path and must re-probe at the delay's own window
        // rather than at the reconsideration rate. Without this the second
        // unavailable reconsideration could never be reached, and the wait
        // would be unbounded - which is the one property the no-adviser rule
        // exists to guarantee. An unavailable adviser costs nothing to ask,
        // so the rate bound buys nothing here either.
        if s.unavailable > 0 {
            return Ok(());
        }
        if s.made >= MAX_RECONSIDERS {
            // Past the cap the system has said what it thinks about this
            // kind: that kind's own last course repeats, and nothing is
            // asked or spent.
            return Err(s.last_course.unwrap_or(Course::KeepGoing));
        }
        if let Some(at) = s.last {
            if now.saturating_duration_since(at) < every {
                return Err(s.last_course.unwrap_or(Course::KeepGoing));
            }
        }
        Ok(())
    }

    /// Record that `kind` was asked on `execution` and what came back. An
    /// answer clears the kind's unavailable run; `None` (the adviser could
    /// not answer) lengthens it and returns how long it now is.
    pub fn asked(
        &self,
        execution: &str,
        kind: DelayKind,
        now: Instant,
        answered: Option<Course>,
    ) -> u32 {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let s = m
            .entry(execution.to_string())
            .or_default()
            .entry(kind)
            .or_default();
        s.made += 1;
        s.last = Some(now);
        match answered {
            Some(c) => {
                s.last_course = Some(c);
                s.unavailable = 0;
                0
            }
            None => {
                s.unavailable += 1;
                s.unavailable
            }
        }
    }

    /// How many executions hold a record (a test's view of the expiry).
    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// How the delay's input goes to `delay.v1`.
fn input_of(d: &Delay, cost_cents: u64) -> DelayInput {
    DelayInput {
        kind: d.kind.as_str().to_string(),
        what: d.what.clone(),
        elapsed_ms: d.elapsed.as_millis() as u64,
        cost_cents,
        signals: d
            .signals
            .iter()
            .map(|s| DelaySignal {
                name: s.name.to_string(),
                moved: s.moved,
            })
            .collect(),
        last_step: d.last_step.clone(),
        reason: d.reason.clone(),
    }
}

/// What a reconsideration decided, and whether the site must fall back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconsidered {
    pub course: Course,
    /// One line for the operator, at inform level. Never a DM task message.
    pub notice: String,
    /// The adviser could not answer, twice for this delay kind: the site
    /// restores the behaviour it had before this module.
    pub fall_back: bool,
}

impl TurnRunner {
    /// Reconsider one delay. **A clock decided when to ask; this decides what
    /// to do.** Never holds a kernel lock, writes no frame on a plain turn's
    /// path, and never stops without an adviser having spoken - except for
    /// the money line, which speaks first and alone.
    pub(crate) async fn reconsider(&self, d: Delay) -> Reconsidered {
        let now = Instant::now();
        // **The off switch is absolute, and it is read first.**
        // `reconsider_every_secs = 0` must mean this module does nothing at
        // all, so that every site - including the ones built on this spine
        // later - reaches its own pre-reconsideration fallback. Read after
        // the pre-check instead, a site with the feature off could be handed
        // `KeepGoing` by a moved signal and never fall back at all, which
        // would make the owner's escape hatch a half-measure on exactly the
        // sites it most needs to work on.
        let Some(every) = self.cfg.kernel.reconsider_every() else {
            return Reconsidered {
                course: Course::KeepGoing,
                notice: String::new(),
                fall_back: true,
            };
        };
        // **The free pre-check runs before the rate bound, not after it.**
        // It costs no model call and no money, so rate-limiting it would buy
        // nothing - and it would mean that a rate-limited or capped delay
        // skipped the money test and replayed a cached course instead. The
        // keel must not be reachable only on the turns we happen to be
        // willing to ask about.
        let burn = {
            let c = self.kernel.day_ceiling();
            c.burn_spent(theseus_protocol::now_unix_ms(), &d.execution)
        };
        if let PreCheck::Took(course) = pre_check(&d, burn) {
            let why = match course {
                Course::Stop(StopWhy::MoneyRate) => {
                    "it is over half its ten-minute spend line, which is the owner's to lift"
                }
                Course::KeepGoing => "a progress signal moved since the delay began",
                _ => "the delay has a course that needed no judgment",
            };
            return Reconsidered {
                notice: words(&d, course, why),
                course,
                fall_back: false,
            };
        }
        // Rate-limited, or past the cap for THIS KIND: that kind's own last
        // course repeats and nothing is asked or spent. A kind's record
        // expires after STALE, so the cached course is never an old one.
        if let Err(course) = self.reconsiders.may_ask(&d.execution, d.kind, every, now) {
            return Reconsidered {
                // A repeated course that stops is never silent: a stop the
                // owner cannot see is the defect this row exists to remove.
                notice: if course.stops() {
                    words(
                        &d,
                        course,
                        "this is the course already judged for this delay",
                    )
                } else {
                    String::new()
                },
                // Never a fallback: the feature is on, and this kind has
                // simply already been judged inside its window.
                fall_back: false,
                course,
            };
        }
        let cents = (burn.0 / 10_000).min(u64::from(u32::MAX));
        let verdict = self
            .judge
            .judge_delay(&d.session, input_of(&d, cents))
            .await
            .filter(|v| v.acts);
        let Some(v) = verdict else {
            // No adviser, no stop. Once: keep going with a notice. Twice for
            // this kind: the site restores its pre-reconsideration behaviour,
            // which is today's code path and so can be no worse than today.
            let run = self.reconsiders.asked(&d.execution, d.kind, now, None);
            let twice = run >= 2;
            let why = if twice {
                "the adviser could not answer twice, so this falls back to what it did before"
            } else {
                "the adviser could not answer, so the work goes on"
            };
            let course = if twice {
                Course::Stop(StopWhy::NoAdviserTwice)
            } else {
                Course::KeepGoing
            };
            return Reconsidered {
                notice: words(&d, course, why),
                course,
                fall_back: twice,
            };
        };
        let course = course_of(&v);
        self.reconsiders
            .asked(&d.execution, d.kind, now, Some(course));
        Reconsidered {
            notice: words(&d, course, &reason(&v)),
            course,
            fall_back: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay(signals: Vec<Signal>) -> Delay {
        Delay {
            kind: DelayKind::AdmissionSlot,
            what: "a wake turn on the morning digest".into(),
            execution: "e1".into(),
            session: "s1".into(),
            elapsed: Duration::from_secs(660),
            signals,
            last_step: None,
            reason: None,
            deterministic: None,
        }
    }

    fn verdict(worth: f64, moving: f64, course: Option<&str>) -> DelayVerdict {
        DelayVerdict {
            worth_it: worth,
            progressing: moving,
            course: course.map(str::to_string),
            acts: true,
        }
    }

    /// Pre-check 1: a signal that moved is progress, and progress is not a
    /// delay. No model call, whatever else is true.
    #[test]
    fn a_signal_that_moved_keeps_the_work_going_for_free() {
        let d = delay(vec![
            Signal {
                name: "the admission queue moved",
                moved: false,
            },
            Signal {
                name: "bytes a build printed",
                moved: true,
            },
        ]);
        assert_eq!(pre_check(&d, (0, 0)), PreCheck::Took(Course::KeepGoing));
    }

    /// An absent signal is unknown, never "stalled": a site that could read
    /// nothing asks the adviser rather than deciding on silence.
    #[test]
    fn a_site_that_read_no_signal_asks_rather_than_assuming_it_stalled() {
        assert_eq!(pre_check(&delay(Vec::new()), (0, 0)), PreCheck::Ask);
    }

    /// Pre-check 2: a deterministic course is taken without asking, so a
    /// provider timeout's retry stays fast.
    #[test]
    fn a_deterministic_course_is_taken_without_asking() {
        let mut d = delay(Vec::new());
        d.deterministic = Some(Course::SwitchModel);
        assert_eq!(pre_check(&d, (0, 0)), PreCheck::Took(Course::SwitchModel));
    }

    /// Pre-check 3, and the property the whole design turns on: **the money
    /// line answers before the adviser is ever asked**, so nothing Jev can
    /// say authorises spend.
    #[test]
    fn the_money_line_stops_the_delay_before_the_adviser_is_asked() {
        let d = delay(Vec::new());
        // Half the line is the bar: just under it asks, at it stops.
        assert_eq!(pre_check(&d, (99, 200)), PreCheck::Ask);
        assert_eq!(
            pre_check(&d, (100, 200)),
            PreCheck::Took(Course::Stop(StopWhy::MoneyRate))
        );
    }

    /// A moved signal does not rescue an execution that is burning: the money
    /// test is not reached, but neither does the money test ever let a burning
    /// execution keep going by its own words. Ordering is the contract.
    #[test]
    fn a_moved_signal_is_read_before_the_money_line_and_that_is_the_order() {
        let d = delay(vec![Signal {
            name: "bytes a build printed",
            moved: true,
        }]);
        // Documented, deliberate: progress wins over the *half*-line warning.
        // The real burn line is the kernel's own stop and is not this check's.
        assert_eq!(pre_check(&d, (200, 200)), PreCheck::Took(Course::KeepGoing));
    }

    /// The owner's clauses, one for one.
    #[test]
    fn the_owners_three_clauses_map_to_three_courses() {
        // "if yes and yes, keep going"
        assert_eq!(
            course_of(&verdict(0.9, 0.9, Some("narrow_scope"))),
            Course::KeepGoing
        );
        // "if yes and no, consider what alternative approach to take"
        assert_eq!(
            course_of(&verdict(0.9, 0.1, Some("change_approach"))),
            Course::ChangeApproach
        );
        // not worth it
        assert_eq!(
            course_of(&verdict(0.1, 0.9, Some("keep_going"))),
            Course::Stop(StopWhy::Adviser)
        );
    }

    /// An answer under the `course` question's confirm bar takes the pack's
    /// own no-match, which is `keep_going`: under uncertainty the owner's
    /// rule is long work over stopped work.
    #[test]
    fn an_answer_under_the_bar_takes_the_packs_no_match_and_never_stops() {
        assert_eq!(course_of(&verdict(0.9, 0.1, None)), NO_MATCH);
        assert_eq!(NO_MATCH, Course::KeepGoing);
        assert!(!NO_MATCH.stops());
    }

    /// `NO_MATCH` is the pack's declared `no_match`. Nothing on the live path
    /// reads a pack's declared default (theseus-h66v4.6.1), so these two are
    /// one value in two places and this test is what keeps them equal.
    #[test]
    fn the_daemons_no_match_is_the_packs() {
        let pack = theseus_judge::pack::by_name("delay.v1").unwrap();
        let want = pack.question("course").unwrap().no_match.clone().unwrap();
        assert_eq!(NO_MATCH.as_str(), want);
    }

    /// **The two admission doors are two kinds, not one.** Per-kind keying
    /// buys nothing if the two sites share a kind: a course cached at one
    /// door - possibly a `Stop` - would be replayed at the other unasked.
    /// This is the sibling of the cross-kind defect, and it is why
    /// `slot_or_reconsider` and `wait_out_its_own_turn` do not share a kind.
    #[test]
    fn the_two_admission_doors_do_not_lend_each_other_a_course() {
        let r = Reconsiders::default();
        let every = Duration::from_secs(300);
        let now = Instant::now();
        assert_ne!(DelayKind::AdmissionSlot, DelayKind::AdmissionTurn);
        assert_ne!(
            DelayKind::AdmissionSlot.as_str(),
            DelayKind::AdmissionTurn.as_str()
        );
        // Run the slot door to its cap with a Stop cached.
        let mut t = now;
        for _ in 0..MAX_RECONSIDERS {
            assert!(r.may_ask("e1", DelayKind::AdmissionSlot, every, t).is_ok());
            r.asked(
                "e1",
                DelayKind::AdmissionSlot,
                t,
                Some(Course::Stop(StopWhy::Adviser)),
            );
            t += Duration::from_secs(301);
        }
        assert_eq!(
            r.may_ask("e1", DelayKind::AdmissionSlot, every, t),
            Err(Course::Stop(StopWhy::Adviser))
        );
        // The other door is untouched and is never handed that Stop.
        assert!(r.may_ask("e1", DelayKind::AdmissionTurn, every, t).is_ok());
    }

    /// A kind's record expires, so **a cached course can never be replayed
    /// long after it was judged** and the map cannot grow without bound. An
    /// uncalled `forget` would have read as though expiry existed; this is
    /// expiry that actually runs, on the read path.
    #[test]
    fn a_cached_course_expires_and_the_map_does_not_grow_without_bound() {
        let r = Reconsiders::default();
        let every = Duration::from_secs(300);
        let now = Instant::now();
        let mut t = now;
        for _ in 0..MAX_RECONSIDERS {
            assert!(r.may_ask("e1", DelayKind::AdmissionSlot, every, t).is_ok());
            r.asked(
                "e1",
                DelayKind::AdmissionSlot,
                t,
                Some(Course::Stop(StopWhy::Adviser)),
            );
            t += Duration::from_secs(301);
        }
        // Capped, and the cached Stop repeats while it is fresh.
        assert_eq!(
            r.may_ask("e1", DelayKind::AdmissionSlot, every, t),
            Err(Course::Stop(StopWhy::Adviser))
        );
        assert_eq!(r.len(), 1);
        // Past STALE the record is gone: askable again, no cached Stop, and
        // the execution no longer costs anything.
        let later = t + STALE + Duration::from_secs(1);
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, later)
            .is_ok());
        assert_eq!(r.len(), 1, "the fresh entry this very call created");
        let r2 = Reconsiders::default();
        r2.asked("gone", DelayKind::Job, now, Some(Course::KeepGoing));
        assert_eq!(r2.len(), 1);
        assert!(r2
            .may_ask(
                "other",
                DelayKind::Job,
                every,
                now + STALE + Duration::from_secs(1)
            )
            .is_ok());
        assert_eq!(
            r2.len(),
            1,
            "the stale execution was swept, only the new one is held"
        );
    }

    /// One kind may be asked once per window, and another kind is not held
    /// back by it.
    #[test]
    fn one_kind_is_asked_once_a_window_and_another_kind_is_not_held_back() {
        let r = Reconsiders::default();
        let now = Instant::now();
        let every = Duration::from_secs(300);
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, now)
            .is_ok());
        r.asked("e1", DelayKind::AdmissionSlot, now, Some(Course::KeepGoing));
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, now)
            .is_err());
        assert!(r.may_ask("e1", DelayKind::Job, every, now).is_ok());
        let later = now + Duration::from_secs(301);
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, later)
            .is_ok());
    }

    /// **The landmine test.** Every bound is per delay kind, so one kind
    /// reaching its cap can neither lock out another kind nor hand it a
    /// course judged about a different delay. Keyed per execution this would
    /// hand a job delay a `Stop` decided about an admission delay - harmless
    /// while one site is wired, live the moment a second one is.
    #[test]
    fn one_kinds_cap_neither_blocks_another_kind_nor_lends_it_a_course() {
        let r = Reconsiders::default();
        let every = Duration::from_secs(1);
        let mut now = Instant::now();
        // Run Admission to its cap, and make its last course a Stop.
        for _ in 0..MAX_RECONSIDERS {
            assert!(r
                .may_ask("e1", DelayKind::AdmissionSlot, every, now)
                .is_ok());
            r.asked(
                "e1",
                DelayKind::AdmissionSlot,
                now,
                Some(Course::Stop(StopWhy::Adviser)),
            );
            now += Duration::from_secs(2);
        }
        assert_eq!(
            r.may_ask("e1", DelayKind::AdmissionSlot, every, now),
            Err(Course::Stop(StopWhy::Adviser)),
            "the capped kind replays its own course"
        );
        // Every other kind is untouched: still askable, and never handed
        // the admission kind's cached Stop.
        for kind in [
            DelayKind::Provider,
            DelayKind::Job,
            DelayKind::Question,
            DelayKind::SelfBuild,
            DelayKind::Trial,
        ] {
            assert!(
                r.may_ask("e1", kind, every, now).is_ok(),
                "{kind:?} was locked out by the admission kind's cap"
            );
        }
        // And an unavailable run is per kind too.
        assert_eq!(r.asked("e1", DelayKind::Job, now, None), 1);
        assert_eq!(r.asked("e1", DelayKind::Provider, now, None), 1);
    }

    /// Past the cap the last course repeats unasked, so reconsideration
    /// cannot bleed on one long execution.
    #[test]
    fn past_the_cap_the_last_course_repeats_unasked() {
        let r = Reconsiders::default();
        let every = Duration::from_secs(1);
        let mut now = Instant::now();
        for _ in 0..MAX_RECONSIDERS {
            assert!(r
                .may_ask("e1", DelayKind::AdmissionSlot, every, now)
                .is_ok());
            r.asked(
                "e1",
                DelayKind::AdmissionSlot,
                now,
                Some(Course::NarrowScope),
            );
            now += Duration::from_secs(2);
        }
        assert_eq!(
            r.may_ask("e1", DelayKind::AdmissionSlot, every, now),
            Err(Course::NarrowScope)
        );
    }

    /// The rate bound must not suppress the second ask after an unavailable
    /// adviser, or the fallback can never be reached and the wait is
    /// unbounded - which is the one property the no-adviser rule exists to
    /// guarantee. Found by `tests_admission`, which hung on exactly this.
    #[test]
    fn an_unavailable_adviser_is_re_asked_at_the_window_not_at_the_rate() {
        let r = Reconsiders::default();
        let now = Instant::now();
        let every = Duration::from_secs(300);
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, now)
            .is_ok());
        // Unavailable: the next ask must be allowed at once, though the rate
        // window has not passed.
        assert_eq!(r.asked("e1", DelayKind::AdmissionSlot, now, None), 1);
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, now)
            .is_ok());
        assert_eq!(r.asked("e1", DelayKind::AdmissionSlot, now, None), 2);
        // And once it answers, the rate bound applies again.
        r.asked("e1", DelayKind::AdmissionSlot, now, Some(Course::KeepGoing));
        assert!(r
            .may_ask("e1", DelayKind::AdmissionSlot, every, now)
            .is_err());
    }

    /// No adviser, no stop: the first unavailable answer keeps the work
    /// going, and only the second restores the old behaviour.
    #[test]
    fn the_first_unavailable_adviser_keeps_going_and_the_second_falls_back() {
        let r = Reconsiders::default();
        let now = Instant::now();
        assert_eq!(r.asked("e1", DelayKind::AdmissionSlot, now, None), 1);
        assert_eq!(r.asked("e1", DelayKind::AdmissionSlot, now, None), 2);
        // An answer clears the run, so a recovered adviser is trusted again.
        r.asked("e1", DelayKind::AdmissionSlot, now, Some(Course::KeepGoing));
        assert_eq!(r.asked("e1", DelayKind::AdmissionSlot, now, None), 1);
    }

    /// The notice names the kind, what was delayed, the course and how long,
    /// and composes its reason: Jev returns no sentence to quote.
    #[test]
    fn the_notice_composes_its_reason_because_jev_returns_no_text() {
        let d = delay(Vec::new());
        let v = verdict(0.9, 0.1, Some("change_approach"));
        let line = words(&d, course_of(&v), &reason(&v));
        assert!(line.contains("admission"));
        assert!(line.contains("a wake turn on the morning digest"));
        assert!(line.contains("change_approach"));
        assert!(line.contains("11 min in"));
        assert!(line.contains("it is still worth finishing and it is not moving"));
    }
}
