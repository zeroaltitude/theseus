//! A running task's checkpoints (theseus-ypnxt.9): the pure half. A task is
//! one long turn of many loops, so its model, picked once from its brief
//! ([`super::brief`]), would never move. At a loop boundary a task asks
//! the route pack about its recent state, and this module says when to ask
//! ([`Tally`]), what the pack reads ([`text`], at most [`MAX_TOKENS`]), and
//! what the verdict does ([`decide`]), by routing's own rules, reused:
//!
//! - **Jev decides** at each trigger (`failing`, `quiet`, `phase`, `report`):
//!   the verdict's pick is where the task goes, up or down. There is no base
//!   the task returns to, no count of moves (`[routing.task_checkpoints]
//!   max_switches` is read by nothing now), and no second verdict to step
//!   down on (theseus-ej6bj).
//! - **The cache hold** is the one hysteresis, and it is about money: a move
//!   whose estimate (the warm prefix taken off by the caller) is at or above
//!   `[routing] cold_switch_tokens` waits for a second agreeing verdict.

use super::effort;
use super::{decide as route_decide, Ask, Decision, Hold, HoldNext, Profiles, Reason, Verdict};
use crate::config::routing::TaskCheckpoints;
use crate::config::{Effort, RoutingConfig};

/// The most the pack reads, in tokens (estimated at [`CHARS_PER_TOKEN`] characters, which is
/// conservative for English).
pub const MAX_TOKENS: usize = 1_500;
/// A token, in characters, for the bound.
pub const CHARS_PER_TOKEN: usize = 3;
/// The brief's share of the text, in characters.
pub const BRIEF_CHARS: usize = 600;
/// The plan's steps the text shows, at most.
pub const MAX_STEPS: usize = 12;
/// The loops the text shows, at most.
pub const LOOP_LINES: usize = 6;
const STEP_CHARS: usize = 100;
const FAILED_CHARS: usize = 160;

/// Why a checkpoint is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// This many loops in a row each had a failed tool result.
    Failing(u32),
    /// This many loops in a row made no progress.
    Quiet(u32),
    /// The task's current step moved from one to another.
    Phase { from: String, to: String },
    /// The last open step of the plan was marked done: the report is next.
    Report,
}

impl Trigger {
    /// The row's `trigger`.
    pub fn key(&self) -> &'static str {
        match self {
            Trigger::Failing(_) => "failing",
            Trigger::Quiet(_) => "quiet",
            Trigger::Phase { .. } => "phase",
            Trigger::Report => "report",
        }
    }

    /// The one plain line the pack reads.
    pub fn line(&self) -> String {
        match self {
            Trigger::Failing(n) => format!("failing: {n} failed loops in a row"),
            Trigger::Quiet(n) => format!("no progress for {n} loops"),
            Trigger::Phase { from, to } => format!("phase: {} → {}", cut(from, 60), cut(to, 60)),
            Trigger::Report => "before the report".into(),
        }
    }
}

/// What one loop showed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// A tool result of the loop was an error, as the runner marks one.
    pub failed: bool,
    /// A plan step was marked done, or the progress words changed.
    pub advanced: bool,
    /// The task's current step moved: (from, to).
    pub phase: Option<(String, String)>,
    /// The last open step of the plan was marked done.
    pub report: bool,
}

/// The counts that decide when a task asks, one per task.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    failing: u32,
    quiet: u32,
    /// Loops since the last ask; none before the first.
    since: Option<u32>,
    /// A phase change or a report that came inside the window: asked when it
    /// closes.
    pending: Option<Trigger>,
}

impl Tally {
    /// Take a loop's observation. `flight`: a checkpoint is in flight, so none
    /// is asked (the counts keep running). Returns the trigger to ask for, if
    /// any, and marks the ask: at most one per `min_loops_between` loops, the
    /// first at once.
    pub fn observe(&mut self, cfg: &TaskCheckpoints, seen: &Seen, flight: bool) -> Option<Trigger> {
        let recovered = !seen.failed && self.failing > 0;
        self.failing = if seen.failed { self.failing + 1 } else { 0 };
        // A recovery after a failing run is progress.
        self.quiet = if seen.advanced || recovered {
            0
        } else {
            self.quiet + 1
        };
        self.since = self.since.map(|s| s.saturating_add(1));
        if let Some((from, to)) = &seen.phase {
            self.pending = Some(Trigger::Phase {
                from: from.clone(),
                to: to.clone(),
            });
        }
        if seen.report {
            self.pending = Some(Trigger::Report);
        }
        let open = self.since.is_none_or(|s| s >= cfg.min_loops_between);
        if flight || !open {
            return None;
        }
        let trigger = match (&self.pending, self.failing, self.quiet) {
            (Some(Trigger::Report), _, _) => Some(Trigger::Report),
            (_, f, _) if f >= cfg.failing_streak => Some(Trigger::Failing(f)),
            (_, _, q) if q >= cfg.quiet_loops => Some(Trigger::Quiet(q)),
            (Some(p), _, _) => Some(p.clone()),
            _ => None,
        }?;
        self.pending = None;
        self.since = Some(0);
        Some(trigger)
    }
}

/// One loop's own line, for the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopLine {
    pub index: u32,
    /// The loop's `say`.
    pub say: Option<String>,
    /// The first line of its first failed result.
    pub failed: Option<String>,
}

fn cut(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.chars().count() <= n {
        true => s,
        false => format!(
            "{}…",
            s.chars().take(n.saturating_sub(1)).collect::<String>()
        ),
    }
}

/// The estimated tokens of `s`: conservative.
pub fn tokens(s: &str) -> usize {
    s.chars().count().div_ceil(CHARS_PER_TOKEN)
}

/// What the pack reads about a task: the brief's first [`BRIEF_CHARS`]
/// characters, the plan's steps with their states (at most [`MAX_STEPS`]),
/// the last [`LOOP_LINES`] loops' lines each with its first failed result's
/// line, and the trigger, in at most [`MAX_TOKENS`]: the oldest loops go
/// first, then the plan's last steps, then the brief's tail; the trigger's
/// line stays.
pub fn text(brief: &str, plan: &[(String, &str)], loops: &[LoopLine], trigger: &Trigger) -> String {
    render(
        brief,
        plan,
        loops,
        trigger,
        (None, MAX_TOKENS * CHARS_PER_TOKEN),
    )
}

/// What the work shows of its own effort, for the pack to read beside the
/// loops (theseus-ujzuh): the level the loops ran at, and whether a checkpoint
/// raised it; how many checkpoints this turn (or task) has asked, and how many
/// in a row found the work failing. A failure after more careful effort reads
/// differently from one at the first level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evidence {
    /// The effort the last loop's request carried; none when unknown.
    pub effort: Option<Effort>,
    /// The level before a checkpoint raised it, and the loop it was raised at.
    pub raised: Option<(Effort, u32)>,
    /// Checkpoints asked so far, this one included.
    pub checkpoints: u32,
    /// Checkpoints in a row, this one included, that found the work failing.
    pub failing: u32,
}

impl Evidence {
    /// A loop's request ran at `level`: a rise is a raise from the level
    /// before, and a fall to or below where the raise started ends it, so the
    /// text never says "raised" of work that came back down.
    pub fn note(&mut self, level: Effort, index: u32) {
        if let Some(was) = self.effort.filter(|was| level > *was) {
            self.raised = Some((was, index));
        }
        if self.raised.is_some_and(|(from, _)| level <= from) {
            self.raised = None;
        }
        self.effort = Some(level);
    }

    /// The lines the text carries: the effort (when known) and the checkpoints.
    fn lines(&self) -> String {
        let mut out = String::new();
        if let Some(e) = self.effort {
            out.push_str(&format!("Effort: {}", effort::name(e)));
            if let Some((from, at)) = self.raised {
                out.push_str(&format!(
                    ", raised from {} at loop {at}",
                    effort::name(from)
                ));
            }
            out.push('\n');
        }
        out.push_str(&format!(
            "Checkpoints this turn: {}, failing {} in a row\n",
            self.checkpoints, self.failing
        ));
        out
    }
}

/// [`text`] with the work's effort evidence, kept whole when the text is cut
/// (as the trigger's line is).
pub fn text_with(
    brief: &str,
    plan: &[(String, &str)],
    loops: &[LoopLine],
    trigger: &Trigger,
    evidence: &Evidence,
) -> String {
    render(
        brief,
        plan,
        loops,
        trigger,
        (Some(evidence), MAX_TOKENS * CHARS_PER_TOKEN),
    )
}

/// [`text`] within `max` characters.
#[cfg(test)]
fn text_within(
    brief: &str,
    plan: &[(String, &str)],
    loops: &[LoopLine],
    trigger: &Trigger,
    max: usize,
) -> String {
    render(brief, plan, loops, trigger, (None, max))
}

fn render(
    brief: &str,
    plan: &[(String, &str)],
    loops: &[LoopLine],
    trigger: &Trigger,
    (evidence, max): (Option<&Evidence>, usize),
) -> String {
    let steps: Vec<String> = plan
        .iter()
        .take(MAX_STEPS)
        .map(|(title, state)| format!("- [{state}] {}", cut(title, STEP_CHARS)))
        .collect();
    let recent = &loops[loops.len().saturating_sub(LOOP_LINES)..];
    let lines: Vec<String> = recent
        .iter()
        .map(|l| {
            let say = l
                .say
                .as_deref()
                .map_or("(no words)".into(), |s| cut(s, 120));
            match &l.failed {
                Some(f) => format!(
                    "- loop {}: {say} — failed: {}",
                    l.index,
                    cut(f, FAILED_CHARS)
                ),
                None => format!("- loop {}: {say}", l.index),
            }
        })
        .collect();
    let (mut from, mut steps_n, mut brief_n) = (0, steps.len(), BRIEF_CHARS);
    loop {
        let mut out = format!("Task: {}\n", cut(brief, brief_n));
        if steps_n > 0 {
            out.push_str("Plan:\n");
            steps[..steps_n]
                .iter()
                .for_each(|s| out.push_str(&format!("{s}\n")));
        }
        if from < lines.len() {
            out.push_str("Recent loops:\n");
            lines[from..]
                .iter()
                .for_each(|s| out.push_str(&format!("{s}\n")));
        }
        if let Some(ev) = evidence {
            out.push_str(&ev.lines());
        }
        out.push_str(&format!("Now: {}", trigger.line()));
        if out.chars().count() <= max {
            return out;
        }
        if from < lines.len() {
            from += 1;
        } else if steps_n > 0 {
            steps_n -= 1;
        } else if brief_n > 40 {
            brief_n /= 2;
        } else {
            return out.chars().take(max).collect();
        }
    }
}

/// What a checkpoint's verdict asks the rule.
#[derive(Debug, Clone)]
pub struct CheckpointAsk<'a> {
    pub verdict: &'a Verdict,
    /// The profile the task runs on now.
    pub current: &'a str,
    /// The last compile's estimate, less the warm prefix on the target model.
    pub est_tokens: u64,
    /// The hold the last checkpoint left.
    pub hold: Option<&'a Hold>,
    pub images: bool,
    pub cap: Option<f64>,
}

/// The rule's answer for a checkpoint verdict. `switch` says the task moves
/// to `profile`; a stay names `current`.
///
/// Jev decides at each trigger (theseus-ej6bj, the owner: "I categorically
/// disagree with caps"): the verdict's pick is where the task goes, up or
/// down, with no base to return to, no count of moves, and no second verdict
/// to step down on. The one hysteresis is the cache hold, which is about money:
/// a move whose estimate is at or above `cold_switch_tokens` waits for a second
/// agreeing verdict (the next one, naming the same profile, makes it).
pub fn decide(cfg: &RoutingConfig, profiles: &Profiles, a: &CheckpointAsk<'_>) -> Decision {
    let stay = |reason, hold| Decision {
        profile: a.current.to_string(),
        reason,
        detour: false,
        switch: false,
        hold,
    };
    // The verdict's pick, by the conversation's own rule with the cold hold
    // left to this one: the profile the task is on is the rule's base.
    let warm = RoutingConfig {
        cold_switch_tokens: u64::MAX,
        ..cfg.clone()
    };
    let d = route_decide(
        &warm,
        profiles,
        &Ask {
            verdict: a.verdict,
            base: a.current,
            est_tokens: a.est_tokens,
            hold: None,
            from: None,
            images: a.images,
            cap: a.cap,
            cleared: false,
        },
    );
    let target = match (d.switch || d.detour, d.reason) {
        (true, _) | (_, Reason::Verdict) => d.profile.clone(),
        // Under its bar, capped or none usable: the task stays.
        _ => return stay(d.reason, HoldNext::Clear),
    };
    if target == a.current {
        return stay(Reason::Verdict, HoldNext::Clear);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == target);
    if a.est_tokens < cfg.cold_switch_tokens || agreed {
        // A detour's class is a pick like another for a task: its move is a
        // verdict's.
        let reason = match d.reason {
            Reason::Detour => Reason::Verdict,
            other => other,
        };
        return Decision {
            profile: target,
            reason,
            detour: false,
            switch: true,
            hold: HoldNext::Clear,
        };
    }
    stay(
        Reason::CacheHold,
        HoldNext::Set(Hold {
            mode: a.verdict.mode.clone(),
            profile: target,
            turn: a.verdict.turn.clone(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{five, verdict};
    use super::*;

    fn cfg() -> RoutingConfig {
        RoutingConfig::default()
    }

    fn go(mode: &str, current: &str, est: u64, hold: Option<&Hold>) -> Decision {
        let (ps, v) = (five(), verdict(mode, 0.9));
        decide(
            &cfg(),
            &ps,
            &CheckpointAsk {
                verdict: &v,
                current,
                est_tokens: est,
                hold,
                images: false,
                cap: None,
            },
        )
    }

    fn up(d: &Decision) -> (&str, Reason, bool) {
        (d.profile.as_str(), d.reason, d.switch)
    }

    /// A class that picks a dearer profile moves the task up.
    #[test]
    fn a_dearer_pick_moves_a_task_up() {
        let d = go("sophisticated", "glm", 1_000, None);
        assert_eq!(up(&d), ("opus", Reason::Verdict, true));
    }

    /// Jev decides at each trigger (theseus-ej6bj): a cheaper pick moves the
    /// task down at once, as a dearer one moves it up. There is no base to go
    /// back to, no step down, and no hold toward one.
    #[test]
    fn a_cheaper_pick_moves_a_task_down_at_once() {
        let d = go("routine_coding", "opus", 1_000, None);
        assert_eq!(up(&d), ("sonnet", Reason::Verdict, true));
        assert_eq!(d.hold, HoldNext::Clear);
        // Below where the task started is no different: nothing is a floor.
        let d = go("trivial", "sonnet", 1_000, None);
        assert_eq!(up(&d), ("glm", Reason::Verdict, true));
    }

    /// The cache hold is the one hysteresis, and it is about money, so it
    /// holds a cold move either way: the first agreeing verdict waits, the
    /// second moves.
    #[test]
    fn a_cold_move_either_way_waits_for_a_second_agreeing_verdict() {
        for (mode, from, to) in [
            ("sophisticated", "glm", "opus"),
            ("routine_coding", "opus", "sonnet"),
        ] {
            let first = go(mode, from, 100_000, None);
            assert_eq!(up(&first), (from, Reason::CacheHold, false), "{mode}");
            let HoldNext::Set(h) = &first.hold else {
                panic!("a hold")
            };
            assert_eq!((h.mode.as_str(), h.profile.as_str()), (mode, to));
            let second = go(mode, from, 100_000, Some(h));
            assert_eq!(up(&second), (to, Reason::Verdict, true), "{mode}");
        }
    }

    /// A hold toward one pick does not carry a different one: the row breaks.
    #[test]
    fn a_hold_toward_another_profile_does_not_move_the_task() {
        let first = go("sophisticated", "glm", 100_000, None);
        let HoldNext::Set(h) = &first.hold else {
            panic!("a hold")
        };
        let other = go("routine_coding", "glm", 100_000, Some(h));
        assert!(!other.switch, "{other:?}");
        assert_eq!(other.reason, Reason::CacheHold);
    }

    /// A verdict that picks the profile the task is on moves nothing and
    /// clears the hold.
    #[test]
    fn the_tasks_own_pick_stays_and_clears_the_hold() {
        let d = go("routine_coding", "sonnet", 100_000, None);
        assert_eq!(up(&d), ("sonnet", Reason::Verdict, false));
        assert_eq!(d.hold, HoldNext::Clear);
    }

    /// A detour's class is a pick like another for a task: a trivial verdict
    /// from a task on opus moves it to the cheap profile Jev named, as a
    /// verdict, not a detour.
    #[test]
    fn a_detour_class_is_a_pick_like_another() {
        let d = go("trivial", "opus", 1_000, None);
        assert_eq!(up(&d), ("glm", Reason::Verdict, true));
        assert!(!d.detour, "{d:?}");
    }

    /// No count of moves is an input: the same verdict answers the same at
    /// any point in a task's life (there is no `max_switches`).
    #[test]
    fn the_rule_has_no_cap_on_moves() {
        for _ in 0..5 {
            let d = go("sophisticated", "glm", 1_000, None);
            assert_eq!(up(&d), ("opus", Reason::Verdict, true));
        }
    }

    #[test]
    fn a_verdict_under_its_bar_moves_nothing_and_breaks_the_row() {
        let (ps, v) = (five(), verdict("sophisticated", 0.1));
        let d = decide(
            &cfg(),
            &ps,
            &CheckpointAsk {
                verdict: &v,
                current: "glm",
                est_tokens: 0,
                hold: None,
                images: false,
                cap: None,
            },
        );
        assert_eq!(
            (d.reason, d.switch, d.hold),
            (Reason::Unsure, false, HoldNext::Clear)
        );
    }

    fn tc() -> TaskCheckpoints {
        TaskCheckpoints::default()
    }

    fn failing() -> Seen {
        Seen {
            failed: true,
            ..Seen::default()
        }
    }

    /// A task that fails for 12 loops asks at most three times (K = 3, M = 4:
    /// at loops 3, 7 and 11), and never while one is in flight.
    #[test]
    fn a_failing_task_asks_once_per_window() {
        let (mut t, cfg) = (Tally::default(), tc());
        let asked: Vec<u32> = (1..=12)
            .filter(|_| t.observe(&cfg, &failing(), false).is_some())
            .collect();
        assert_eq!(asked, [3, 7, 11]);
        let mut t = Tally::default();
        assert!((1..=12).all(|_| t.observe(&cfg, &failing(), true).is_none()));
    }

    #[test]
    fn a_quiet_run_asks_and_a_recovery_is_progress() {
        let (mut t, cfg) = (Tally::default(), tc());
        let quiet = Seen::default();
        let first: Vec<_> = (1..=8).map(|_| t.observe(&cfg, &quiet, false)).collect();
        assert_eq!(first[7], Some(Trigger::Quiet(8)));
        assert!(first[..7].iter().all(Option::is_none));
        // Two failing loops, then one clean: a recovery, so the quiet count starts over.
        let mut t = Tally::default();
        for _ in 0..5 {
            t.observe(&cfg, &quiet, false);
        }
        t.observe(&cfg, &failing(), false);
        t.observe(&cfg, &failing(), false);
        t.observe(&cfg, &quiet, false);
        assert_eq!((t.failing, t.quiet), (0, 0));
    }

    /// A phase change asks at once, a report before it, and one that comes
    /// inside the window waits for it.
    #[test]
    fn a_phase_change_and_the_report_ask_and_wait_for_the_window() {
        let (mut t, cfg) = (Tally::default(), tc());
        let phase = Seen {
            phase: Some(("plan".into(), "implement".into())),
            advanced: true,
            ..Seen::default()
        };
        assert_eq!(
            t.observe(&cfg, &phase, false),
            Some(Trigger::Phase {
                from: "plan".into(),
                to: "implement".into()
            })
        );
        let report = Seen {
            report: true,
            advanced: true,
            ..Seen::default()
        };
        assert_eq!(t.observe(&cfg, &report, false), None, "inside the window");
        let quiet = Seen {
            advanced: true,
            ..Seen::default()
        };
        assert_eq!(t.observe(&cfg, &quiet, false), None);
        assert_eq!(t.observe(&cfg, &quiet, false), None);
        assert_eq!(t.observe(&cfg, &quiet, false), Some(Trigger::Report));
    }

    /// The text is bounded whatever the task wrote, drops the oldest loops
    /// first and keeps the trigger.
    #[test]
    fn the_text_is_bounded_and_keeps_its_trigger() {
        let long = "word ".repeat(400);
        let plan: Vec<(String, &str)> = (0..30)
            .map(|n| (format!("step {n} {long}"), "todo"))
            .collect();
        let loops: Vec<LoopLine> = (0..40)
            .map(|index| LoopLine {
                index,
                say: Some(long.clone()),
                failed: Some(long.clone()),
            })
            .collect();
        let trigger = Trigger::Failing(3);
        let s = text(&long.repeat(3), &plan, &loops, &trigger);
        assert!(tokens(&s) <= MAX_TOKENS, "{} tokens", tokens(&s));
        assert!(s.ends_with(&trigger.line()), "{s}");
        assert!(s.contains("loop 39"), "the newest loop is kept");
        assert!(!s.contains("loop 0:"), "the oldest goes first");
        // A small task shows everything, in order.
        let s = text(
            "Fix the quay notice.",
            &[("find it".into(), "done"), ("fix it".into(), "doing")],
            &[LoopLine {
                index: 1,
                say: Some("Running the tests".into()),
                failed: Some("error: boom".into()),
            }],
            &Trigger::Report,
        );
        assert_eq!(
            s,
            "Task: Fix the quay notice.\nPlan:\n- [done] find it\n- [doing] fix it\nRecent loops:\n- loop 1: Running the tests — failed: error: boom\nNow: before the report"
        );
    }

    /// Over a bound the text gives up the oldest loops first, then the plan's last
    /// steps, then the brief's tail, and never its trigger line.
    #[test]
    fn a_tight_bound_cuts_the_oldest_loops_then_the_plan_then_the_brief() {
        let plan: Vec<(String, &str)> = (1..=4).map(|n| (format!("step {n}"), "todo")).collect();
        let loops: Vec<LoopLine> = (1..=6)
            .map(|index| LoopLine {
                index,
                say: Some(format!("words of loop {index}")),
                failed: None,
            })
            .collect();
        let trigger = Trigger::Quiet(8);
        let whole = text_within("Fix the quay notice.", &plan, &loops, &trigger, 10_000);
        assert!(whole.contains("loop 1:") && whole.contains("step 4"));
        let at = |max| text_within("Fix the quay notice.", &plan, &loops, &trigger, max);
        let loops_cut = at(whole.chars().count() - 1);
        assert!(
            !loops_cut.contains("loop 1:") && loops_cut.contains("loop 2:"),
            "{loops_cut}"
        );
        let tight = at(60);
        assert!(
            !tight.contains("- loop") && !tight.contains("- ["),
            "{tight}"
        );
        assert!(tight.ends_with("Now: no progress for 8 loops"), "{tight}");
        assert!(tight.chars().count() <= 60);
        assert!(at(1).chars().count() <= 1, "never over, whatever the bound");
    }

    fn raised_twice() -> Evidence {
        Evidence {
            effort: Some(Effort::Xhigh),
            raised: Some((Effort::High, 4)),
            checkpoints: 3,
            failing: 2,
        }
    }

    /// The text says the effort the loops ran at, that a checkpoint raised
    /// it, and how many checkpoints found the work failing in a row
    /// (theseus-ujzuh); a first checkpoint says one; a turn whose effort is
    /// unknown leaves that line out; a text without evidence is unchanged.
    #[test]
    fn the_text_carries_the_works_effort_evidence() {
        let loops = [LoopLine {
            index: 5,
            say: Some("ran the checks".into()),
            failed: Some("check failed".into()),
        }];
        let t = Trigger::Failing(3);
        let s = text_with("Fix the build.", &[], &loops, &t, &raised_twice());
        assert!(
            s.contains("Effort: xhigh, raised from high at loop 4\n"),
            "{s}"
        );
        assert!(
            s.contains("Checkpoints this turn: 3, failing 2 in a row\n"),
            "{s}"
        );
        assert!(s.ends_with("Now: failing: 3 failed loops in a row"), "{s}");
        let first = Evidence {
            effort: Some(Effort::High),
            checkpoints: 1,
            failing: 1,
            ..Evidence::default()
        };
        let s = text_with("Fix the build.", &[], &loops, &t, &first);
        assert!(s.contains("Effort: high\nCheckpoints this turn: 1, failing 1 in a row"));
        assert!(!s.contains("raised"));
        let s = text_with("Fix the build.", &[], &loops, &t, &Evidence::default());
        assert!(!s.contains("Effort:") && s.contains("Checkpoints this turn: 0"));
        assert_eq!(
            text("Fix the build.", &[], &loops, &t),
            text_within("Fix the build.", &[], &loops, &t, 4_500),
            "no evidence, no lines"
        );
    }

    /// A long brief and many loops are cut, never the evidence lines.
    #[test]
    fn the_evidence_survives_the_cut() {
        let long = "word ".repeat(400);
        let plan: Vec<(String, &str)> = (0..30)
            .map(|n| (format!("step {n} {long}"), "todo"))
            .collect();
        let loops: Vec<LoopLine> = (0..40)
            .map(|index| LoopLine {
                index,
                say: Some(long.clone()),
                failed: Some(long.clone()),
            })
            .collect();
        let s = text_with(
            &long.repeat(3),
            &plan,
            &loops,
            &Trigger::Report,
            &raised_twice(),
        );
        assert!(tokens(&s) <= MAX_TOKENS, "{}", tokens(&s));
        assert!(
            s.contains("Effort: xhigh, raised from high at loop 4"),
            "{s}"
        );
        assert!(s.contains("Checkpoints this turn: 3, failing 2 in a row"));
        assert!(s.ends_with("Now: before the report"));
    }

    /// A raise ends when the work comes back down to where it started: high,
    /// xhigh, high says no raise (and the text says none).
    #[test]
    fn a_raise_ends_when_the_effort_falls_back_to_its_start() {
        let mut ev = Evidence::default();
        ev.note(Effort::High, 1);
        ev.note(Effort::Xhigh, 4);
        assert_eq!(ev.raised, Some((Effort::High, 4)));
        ev.note(Effort::High, 6);
        assert_eq!((ev.effort, ev.raised), (Some(Effort::High), None));
        assert!(!ev.lines().contains("raised"), "{}", ev.lines());
        ev.note(Effort::Xhigh, 7);
        ev.note(Effort::Medium, 9);
        assert_eq!((ev.effort, ev.raised), (Some(Effort::Medium), None));
    }
}
