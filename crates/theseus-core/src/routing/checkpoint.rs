//! A running task's checkpoints (theseus-ypnxt.9): the pure half. A task is
//! one long turn of many loops, so its model, picked once from its brief
//! ([`super::brief`]), would never move. At a loop boundary a task asks
//! route.v6 about its recent state, and this module says when to ask
//! ([`Tally`]), what the pack reads ([`text`], at most [`MAX_TOKENS`]), and
//! what the verdict does ([`decide`]), by routing's own rules, reused:
//!
//! - **Only Jev elevates.** The verdict's class picks its first usable
//!   profile under the place's cap ([`super::decide`]'s walk), as a
//!   conversation's switch does.
//! - **The task's base** is its brief's pick or its parent's. A move up keeps
//!   it (`Routed.from`), and the way back down goes to it after two agreeing
//!   verdicts ([`super::STEP_DOWN`]'s hold), never below it.
//! - **A cold switch up** waits for a second agreeing verdict when its
//!   estimate (the warm prefix taken off by the caller) is at or above
//!   `[routing] cold_switch_tokens`.
//! - **No detours**: `trivial` and `quick` move nothing, and count toward the
//!   step down when their pick is no dearer than the base.
//! - **The cap**: `max_switches` moves per task; past it nothing moves up,
//!   and the step back down to the base still does.

use super::{decide as route_decide, Ask, Decision, Hold, HoldNext, Profiles, Reason, Verdict};
use crate::config::routing::TaskCheckpoints;
use crate::config::RoutingConfig;

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
    text_within(brief, plan, loops, trigger, MAX_TOKENS * CHARS_PER_TOKEN)
}

/// [`text`] within `max` characters.
fn text_within(
    brief: &str,
    plan: &[(String, &str)],
    loops: &[LoopLine],
    trigger: &Trigger,
    max: usize,
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
    /// The task's base: its brief's pick or its parent's.
    pub base: &'a str,
    /// The profile the task runs on now.
    pub current: &'a str,
    /// The last compile's estimate, less the warm prefix on the target model.
    pub est_tokens: u64,
    /// The hold the last checkpoint left.
    pub hold: Option<&'a Hold>,
    pub images: bool,
    pub cap: Option<f64>,
    /// Moves the task has made, up or down.
    pub moves: u32,
}

/// The rule's answer for a checkpoint verdict. `switch` says the task moves
/// to `profile`; a stay names `current`.
pub fn decide(cfg: &RoutingConfig, profiles: &Profiles, a: &CheckpointAsk<'_>) -> Decision {
    let stay = |reason, hold| Decision {
        profile: a.current.to_string(),
        reason,
        detour: false,
        switch: false,
        hold,
    };
    if a.verdict.confidence < cfg.confidence_for(&a.verdict.mode) {
        return stay(Reason::Unsure, HoldNext::Clear);
    }
    // The class's pick against the task's base, by the conversation's own
    // walk with the cold hold left to this rule.
    let warm = RoutingConfig {
        cold_switch_tokens: u64::MAX,
        ..cfg.clone()
    };
    let d = route_decide(
        &warm,
        profiles,
        &Ask {
            verdict: a.verdict,
            base: a.base,
            est_tokens: a.est_tokens,
            hold: None,
            from: None,
            images: a.images,
            cap: a.cap,
        },
    );
    let target = match (d.switch || d.detour, d.reason) {
        (true, _) | (_, Reason::Verdict) => d.profile.clone(),
        // Nothing usable under the cap, or none usable: the task stays.
        _ => return stay(d.reason, HoldNext::Clear),
    };
    let cost = |p: &str| profiles.get(p).and_then(|p| p.short_cost);
    let (Some(target_cost), Some(base_cost)) = (cost(&target), cost(a.base)) else {
        return stay(Reason::Fallback, HoldNext::Clear);
    };
    if target_cost <= base_cost + 1e-12 {
        // The pick is the base or cheaper: it counts toward the way back down.
        if a.current == a.base {
            return stay(Reason::Verdict, HoldNext::Clear);
        }
        let held = a
            .hold
            .is_some_and(|h| h.mode == super::STEP_DOWN && h.profile == a.base);
        return match held {
            true => Decision {
                profile: a.base.to_string(),
                reason: Reason::StepDown,
                detour: false,
                switch: true,
                hold: HoldNext::Clear,
            },
            false => stay(
                Reason::CacheHold,
                HoldNext::Set(Hold {
                    mode: super::STEP_DOWN.into(),
                    profile: a.base.to_string(),
                    turn: a.verdict.turn.clone(),
                }),
            ),
        };
    }
    // A pick dearer than the base: only a verdict that is not a detour moves up.
    if d.detour || target == a.current {
        return stay(Reason::Verdict, HoldNext::Clear);
    }
    if a.moves >= cfg.task_checkpoints.max_switches {
        return stay(Reason::SwitchCap, HoldNext::Clear);
    }
    let agreed = a.hold.is_some_and(|h| h.profile == target);
    if a.est_tokens < cfg.cold_switch_tokens || agreed {
        return Decision {
            profile: target,
            reason: d.reason,
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

    fn go(
        mode: &str,
        (base, current): (&str, &str),
        est: u64,
        hold: Option<&Hold>,
        moves: u32,
    ) -> Decision {
        let (ps, v) = (five(), verdict(mode, 0.9));
        decide(
            &cfg(),
            &ps,
            &CheckpointAsk {
                verdict: &v,
                base,
                current,
                est_tokens: est,
                hold,
                images: false,
                cap: None,
                moves,
            },
        )
    }

    fn up(d: &Decision) -> (&str, Reason, bool) {
        (d.profile.as_str(), d.reason, d.switch)
    }

    /// A class that picks a dearer profile than the base moves the task up.
    #[test]
    fn a_dearer_pick_moves_a_task_up_from_its_base() {
        let d = go("sophisticated", ("glm", "glm"), 1_000, None, 0);
        assert_eq!(up(&d), ("opus", Reason::Verdict, true));
    }

    /// Two agreeing verdicts step back down to the base; one then a different
    /// one leaves the task up.
    #[test]
    fn it_steps_back_to_the_base_after_two_agreeing_verdicts() {
        let first = go("routine_coding", ("sonnet", "opus"), 1_000, None, 1);
        assert_eq!(up(&first), ("opus", Reason::CacheHold, false));
        let HoldNext::Set(h) = &first.hold else {
            panic!("a hold")
        };
        assert_eq!(
            (h.mode.as_str(), h.profile.as_str()),
            ("step_down", "sonnet")
        );
        let second = go("routine_coding", ("sonnet", "opus"), 1_000, Some(h), 1);
        assert_eq!(up(&second), ("sonnet", Reason::StepDown, true));
        // A verdict that picks another class between them breaks the row.
        let broken = go("sophisticated", ("sonnet", "opus"), 1_000, Some(h), 1);
        assert_eq!(broken.hold, HoldNext::Clear);
        assert!(!broken.switch);
    }

    /// Never below the base: a trivial verdict on a Sonnet task leaves Sonnet.
    #[test]
    fn a_cheap_pick_never_goes_below_the_base() {
        let d = go("trivial", ("sonnet", "sonnet"), 1_000, None, 0);
        assert_eq!(up(&d), ("sonnet", Reason::Verdict, false));
        // Above the base, a cheaper pick steps down to the base, no further.
        let first = go("trivial", ("sonnet", "opus"), 1_000, None, 1);
        let HoldNext::Set(h) = &first.hold else {
            panic!("a hold")
        };
        let second = go("trivial", ("sonnet", "opus"), 1_000, Some(h), 1);
        assert_eq!(up(&second), ("sonnet", Reason::StepDown, true));
    }

    /// A detour's class moves a task nowhere on its own: a quick verdict from a
    /// task on glm picks haiku-or-cheaper, never a dearer detour.
    #[test]
    fn a_detour_class_moves_nothing_up() {
        let d = go("quick", ("glm", "glm"), 1_000, None, 0);
        assert!(!d.switch && !d.detour, "{d:?}");
    }

    /// At an estimate over `cold_switch_tokens` the first agreeing verdict
    /// holds and the second switches.
    #[test]
    fn a_cold_switch_up_waits_for_a_second_verdict() {
        let first = go("sophisticated", ("glm", "glm"), 100_000, None, 0);
        assert_eq!(up(&first), ("glm", Reason::CacheHold, false));
        let HoldNext::Set(h) = &first.hold else {
            panic!("a hold")
        };
        assert_eq!(h.profile, "opus");
        let second = go("sophisticated", ("glm", "glm"), 100_000, Some(h), 0);
        assert_eq!(up(&second), ("opus", Reason::Verdict, true));
    }

    /// After `max_switches` moves a further elevating verdict moves nothing,
    /// and the step back down still works.
    #[test]
    fn the_cap_stops_moves_up_and_not_the_way_down() {
        let d = go("sophisticated", ("sonnet", "sonnet"), 1_000, None, 2);
        assert_eq!(up(&d), ("sonnet", Reason::SwitchCap, false));
        let down = go("routine_coding", ("sonnet", "opus"), 1_000, None, 2);
        let HoldNext::Set(h) = &down.hold else {
            panic!("a hold")
        };
        let down = go("routine_coding", ("sonnet", "opus"), 1_000, Some(h), 2);
        assert_eq!(up(&down), ("sonnet", Reason::StepDown, true));
    }

    #[test]
    fn a_verdict_under_its_bar_moves_nothing_and_breaks_the_row() {
        let (ps, v) = (five(), verdict("sophisticated", 0.1));
        let d = decide(
            &cfg(),
            &ps,
            &CheckpointAsk {
                verdict: &v,
                base: "glm",
                current: "glm",
                est_tokens: 0,
                hold: None,
                images: false,
                cap: None,
                moves: 0,
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
}
