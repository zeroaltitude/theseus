//! What a turn injects, sized to its prompt (theseus-d9je7, rows 2 and 4): one
//! [`InjectionBudget`] from `work.v1`'s answer, folded into two tiers.
//!
//! - **The fold** ([`tier_of`]): `work` = `coding`, `research_or_design` or
//!   `review` at its bar is deep; every other label is ordinary, and so is a
//!   verdict that is late, missing, under its bar, or not acting, and a turn
//!   routing does not decide (off, pinned, a continuation). There is no small tier: it was route.v8's `trivial` class,
//!   whose detour dropped the pack, and no pack says "short" any more.
//! - **Recall** packs at the tier's tokens and items (`[memory]
//!   recall_budget_tokens`/`recall_max_items` ordinary, `deep_recall_tokens`/
//!   `deep_recall_items` deep).
//! - **Tool admission** raises the place's budget to the tier's multiple
//!   (`[tools.defer] admit_ceiling`, `admit_ceiling_deep`; `admit_ceiling_small`
//!   is retired with the small tier).
//! - **Never a wait**: the pack reads the verdict if it has landed (a peek that
//!   leaves it for the compile's wait, [`peek`]) and is ordinary when it has
//!   not; the budget is then fixed for the turn and recorded on its
//!   `route.decided` row.
//! - **The tools settle twice when they must** (theseus-a2kh6): they settle once
//!   at the turn's start (`defer_tools`), before any verdict has landed, so a
//!   deep turn settles them at the ordinary ceiling. Once the verdict is read,
//!   [`TurnRunner::raise_tools`] gives a budget fixed ordinary for want of it the
//!   deep tier's ceiling, and the tools settle again where they are behind it
//!   ([`TurnRunner::tools_behind`]) — on the routed spec for a switch, and on
//!   the turn's own for a pick that stays on the session's model. The recall is
//!   packed by then, so the tier and the recall sizes stay what the pack read
//!   and only the ceiling moves; the row says `admit_raised` when it did.
//! - **The cache**: the tier sizes the tail (recall, after the message) and
//!   the families a recompile loads; an append keeps its compilation's tools
//!   (`tools_defer::forks`), so no cached prefix byte moves from turn to turn.
//!
//! A config whose tiers equal one another (deep's recall keys at ordinary's,
//! the three multiples alike) is the rollback.

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::{Turn, TurnRunner};
use crate::config::Config;
use crate::judge::inbound::RouteWait;
use crate::routing::Verdict;
use crate::toolrun::session_dir::read_window;

/// The size of what a prompt needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Ordinary,
    Deep,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Ordinary => "ordinary",
            Tier::Deep => "deep",
        }
    }
}

/// What the turn read of its verdict when the budget was fixed.
#[derive(Clone, Copy)]
pub enum Seen<'v> {
    /// A verdict that acts.
    Verdict(&'v Verdict),
    /// The wait has not ended: the pack does not wait for it.
    Late,
    /// Jev gave none, or could not be reached.
    Missing,
    /// Routing decides nothing for the turn: off, pinned, shadow, a steer.
    Off,
}

/// A turn's injection: its tier, why it fell back to ordinary when it did,
/// and the sizes the tier gives.
#[derive(Debug, Clone, PartialEq)]
pub struct InjectionBudget {
    pub tier: Tier,
    /// `verdict late`, `no verdict`, `unsure`, `routing off`: ordinary
    /// without a verdict's say.
    pub fallback: Option<&'static str>,
    /// What the request's recall may hold.
    pub recall_tokens: u64,
    pub recall_items: usize,
    /// The multiple of the place's first-request budget admitted families may
    /// raise a request to.
    pub admit_ceiling: f64,
    /// The verdict acts and says the work is code, `work = coding` (theseus-tdkcn; route.v8: `deep_coding`): the
    /// request's tail carries [`CODING_HINT`] and a bare `fs.read` is narrower.
    /// Not in the row.
    pub coding: bool,
    /// [`Self::admit_ceiling`] is the deep tier's, given after the budget was
    /// fixed: the verdict folded deep but landed after the pack, which had
    /// fixed an ordinary budget for want of it ([`TurnRunner::raise_tools`],
    /// theseus-a2kh6). The recall is packed by then, so [`Self::tier`] and the
    /// recall sizes stay the pack's and only the ceiling moved.
    pub admit_raised: bool,
    /// The verdict acts and says the reply needs `low` effort (`effort.v1` at
    /// its confirm bar): small talk. Read by the recall's shadow `would_drop`
    /// and, with `[memory] small_talk_bar` on, by the small-talk bar itself
    /// ([`TurnRunner::small_talk_bar`], theseus-fpm2); it sizes nothing. Not
    /// in the row.
    pub low_effort: bool,
}

/// The fallback of a budget fixed ordinary because no verdict had landed yet.
/// The one fallback a verdict can still overturn, since the verdict came.
pub const LATE: &str = "verdict late";

/// What a `deep_coding` turn's request carries in its tail (theseus-tdkcn).
pub const CODING_HINT: &str = "[The harness: for this work, map the code before reading it: list a file's or crate's functions first (fs_grep for function signatures). Read in slices of 100-300 lines with offset and limit, not whole files. After each slice, write a line of what you checked and what you found, so your findings survive when old results are cleared. Keep going until the areas you were asked about are covered.]";

/// The tier a verdict folds to, and why it is ordinary when it fell back.
pub fn tier_of(seen: Seen<'_>) -> (Tier, Option<&'static str>) {
    let v = match seen {
        Seen::Verdict(v) => v,
        Seen::Late => return (Tier::Ordinary, Some(LATE)),
        Seen::Missing => return (Tier::Ordinary, Some("no verdict")),
        Seen::Off => return (Tier::Ordinary, Some("routing off")),
    };
    // route.v10 (theseus-h66v4.6): the class names are gone, and `work.v1`'s label is what the
    // tier reads. The owner decided three separate classifications, with `work.v1` replacing the
    // class names for the hint readers and the injection tier (2026-10-30 16:01). `coding`,
    // `research_or_design` and `review` are Deep: under route.v8 `sophisticated` and `review` were,
    // and the owner approved folding them back in (2026-11-03 00:03, "your recommendation" to
    // "add `research_or_design` and `review` to the Deep injection tier alongside `coding`?";
    // the recommendation was yes). Only the tier widens: the coding hint (`is_coding`) stays
    // `coding` alone. `trivial` had no label to carry (Small was route.v8's "no recall for a
    // greeting"). A work answer under the question's confirm bar is "unsure", as a class under
    // its bar was.
    match v.work.as_deref() {
        None => (Tier::Ordinary, Some("no work answer")),
        Some(w) if !w.sure => (Tier::Ordinary, Some("unsure")),
        Some(w) => match w.choice.as_str() {
            "coding" | "research_or_design" | "review" => (Tier::Deep, None),
            _ => (Tier::Ordinary, None),
        },
    }
}

/// The verdict says the work is code: `work.v1`'s `coding` at its confirm bar.
pub fn is_coding(v: &Verdict) -> bool {
    v.work
        .as_deref()
        .is_some_and(|w| w.sure && w.choice == "coding")
}

/// The verdict says the reply needs the least effort: `effort.v1`'s `low` at
/// its confirm bar (a greeting, small talk).
///
/// UNPROVEN: no test shows that `effort.v1` calls a real greeting `low`. The
/// tests that read this (`tests_tier_manifest`) script the verdict with a
/// `FakeJev`, so a green one proves the plumbing from a `low` verdict to the
/// shadow `would_drop` and, with `[memory] small_talk_bar` on, to the
/// withholding itself ([`TurnRunner::small_talk_bar`]), not that the bar
/// would fire on real small talk. That unproven premise is why the key is
/// off by default: see its doc in `config::memory`.
///
/// **The bar reads this and deliberately does not read `classify`'s `social`
/// label.** That is the row's own decision (theseus-ectzg's B4 line: "Design
/// handed over: read `effort == low` from the already-peeked verdict in
/// tier.rs, record `would_drop` on RecallManifest in shadow, leave classify's
/// `social` until it is plumbed"), not an oversight. The effort answer is
/// peeked here already, for the tier, so the bar costs the turn no model call
/// and no wait, which is what `tier.rs`'s module doc means by "never a wait";
/// `social` reaches no turn at all. What would have to exist before the bar
/// could read it: a `social` question answered in the inbound batch, carried
/// on [`Verdict`] (which has no field for one) so a pack and a compile can
/// read it without waiting, and a rule for which answer wins when the two
/// disagree. Until all three exist, the effort answer is the whole of the bar.
pub fn is_low_effort(v: &Verdict) -> bool {
    v.effort
        .as_ref()
        .is_some_and(|e| e.sure && e.level == "low")
}

impl InjectionBudget {
    /// The small-talk bar's shadow mark for a turn whose verdict landed after
    /// the pack fixed a `Late` budget (theseus-fpm2): `injection_read` returns
    /// at once once a budget is fixed, so without this a `low` verdict that
    /// arrives late never sets `low_effort` and the turn is silently missing
    /// from the shadow's sample. It sizes nothing: only the mark moves, and
    /// only for a budget that fell back for want of a verdict.
    pub(super) fn raise_low_effort(&mut self, verdict: Option<&Verdict>) {
        if self.fallback == Some(LATE) && verdict.is_some_and(is_low_effort) {
            self.low_effort = true;
        }
    }

    pub fn of((tier, fallback): (Tier, Option<&'static str>), cfg: &Config) -> Self {
        let (m, d) = (&cfg.memory, &cfg.tools.defer);
        let (recall_tokens, recall_items, admit_ceiling) = match tier {
            Tier::Ordinary => (m.recall_budget_tokens, m.recall_max_items, d.admit_ceiling),
            Tier::Deep => (
                m.deep_recall_tokens,
                m.deep_recall_items,
                d.admit_ceiling_deep,
            ),
        };
        Self {
            tier,
            fallback,
            recall_tokens,
            recall_items,
            admit_ceiling,
            coding: false,
            admit_raised: false,
            low_effort: false,
        }
    }

    /// The budget a verdict (or its absence) gives: `of` its tier, and the
    /// coding flag when an acting verdict's mode is `deep_coding`.
    pub fn read(seen: Seen<'_>, cfg: &Config) -> Self {
        let coding = matches!(seen, Seen::Verdict(v) if is_coding(v));
        let mut b = Self::of(tier_of(seen), cfg);
        b.coding = coding && b.fallback.is_none();
        b.low_effort = matches!(seen, Seen::Verdict(v) if is_low_effort(v));
        b
    }

    /// The `route.decided` row's `injection`, which `theseus why` words.
    pub fn row(&self) -> Value {
        json!({
            "tier": self.tier.as_str(), "fallback": self.fallback,
            "recall_tokens": self.recall_tokens, "recall_items": self.recall_items,
            "admit_ceiling": self.admit_ceiling, "admit_raised": self.admit_raised,
        })
    }
}

/// What a peek at the verdict found.
enum Peeked {
    Landed(Option<Verdict>),
    Pending,
}

/// The verdict, if it has landed, left in `wait` for the compile's own wait to
/// take as it would have: a landed answer is put back in a fresh channel.
fn peek(wait: &mut Option<RouteWait>) -> Peeked {
    let Some(rx) = wait.as_mut() else {
        return Peeked::Landed(None);
    };
    match rx.try_recv() {
        Ok(a) => {
            let v = a.verdict.clone();
            let (tx, again) = oneshot::channel();
            let _ = tx.send(a);
            *wait = Some(again);
            Peeked::Landed(v)
        }
        Err(oneshot::error::TryRecvError::Closed) => Peeked::Landed(None),
        Err(oneshot::error::TryRecvError::Empty) => Peeked::Pending,
    }
}

impl TurnRunner {
    /// Whether the small-talk recall bar fires on this turn (theseus-fpm2):
    /// `[memory] small_talk_bar` is on and the budget the turn fixed read a
    /// verdict that called the reply `low` effort ([`is_low_effort`]).
    ///
    /// It reads the budget alone, so it asks for nothing: the verdict was
    /// peeked when the budget was fixed ([`peek`], the module doc's "never a
    /// wait"), and a turn whose peek found none is ordinary, so `low_effort`
    /// is false and the bar does not fire — the safe direction. A `Late`
    /// budget marked by [`InjectionBudget::raise_low_effort`] is marked after
    /// the first compile has read this, so such a turn carries its notes and
    /// still records its shadow count; that asymmetry is the `Late` hole's,
    /// not the bar's.
    ///
    /// Its one reader is the recall step's withholding
    /// (`recall_step::recall_carried` and `recall_view`).
    pub(super) fn small_talk_bar(&self, t: &Turn<'_>) -> bool {
        self.cfg.memory.small_talk_bar && t.route.injection.as_ref().is_some_and(|b| b.low_effort)
    }

    /// The budget the turn reads now: the one fixed, else one fixed from the
    /// verdict if it has landed (or from routing deciding nothing), else
    /// ordinary for now, fixed only when `fix` (the pack, which never waits).
    pub(super) fn injection(&self, t: &mut Turn<'_>, fix: bool) -> InjectionBudget {
        if let Some(b) = &t.route.injection {
            return b.clone();
        }
        let live = t.route.live();
        let deciding = t.route.wait.is_some() && live && t.route.steer.is_none();
        let peeked = match deciding {
            true => peek(&mut t.route.wait),
            false => Peeked::Landed(None),
        };
        let seen = match &peeked {
            Peeked::Pending => Seen::Late,
            Peeked::Landed(Some(v)) => Seen::Verdict(v),
            Peeked::Landed(None) if deciding => Seen::Missing,
            Peeked::Landed(None) => Seen::Off,
        };
        let b = InjectionBudget::read(seen, &self.cfg);
        if fix || !matches!(peeked, Peeked::Pending) {
            t.route.injection = Some(b.clone());
            Self::narrow_reads(t);
        } else {
            read_window::set(t.tc.session_id, 0);
        }
        b
    }

    /// A `deep_coding` turn's bare `fs.read` returns [`NARROW_LINES`] lines
    /// (theseus-tdkcn); any other turn's budget clears the session's value.
    fn narrow_reads(t: &Turn<'_>) {
        let coding = t.route.injection.as_ref().is_some_and(|b| b.coding);
        let lines = match coding {
            true => read_window::NARROW_LINES,
            false => 0,
        };
        read_window::set(t.tc.session_id, lines);
    }

    /// A `deep_coding` turn's hint, one tail block after the cached prefix,
    /// counted in the estimate; once per request.
    pub(super) fn coding_hint(t: &Turn<'_>, compiled: &mut crate::compiler::Compiled) {
        if t.route.injection.as_ref().is_some_and(|b| b.coding) {
            Self::attach_hint(compiled, CODING_HINT);
        }
    }

    /// `hint` as one tail block after the cached prefix, counted in the
    /// estimate; once per request.
    pub(super) fn attach_hint(compiled: &mut crate::compiler::Compiled, hint: &str) {
        let has = compiled
            .request
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|b| b.iter().any(|b| b["text"] == hint));
        if has {
            return;
        }
        crate::task_graph::view::attach_text(&mut compiled.request, hint);
        let n = (hint.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
        compiled.est_tokens += n;
        compiled.estimate.tokens += n;
        compiled.estimate.estimated += n;
        compiled.estimate.upper += n + n * crate::compiler::MARGIN_PERCENT / 100;
    }

    /// The budget `record_route` fixes when nothing fixed one before (no pack
    /// ran): from the verdict the turn read, or why it read none.
    pub(super) fn injection_read(
        &self,
        t: &mut Turn<'_>,
        verdict: Option<&Verdict>,
        reason: Option<crate::routing::Reason>,
    ) {
        if t.route.injection.is_some() {
            return;
        }
        use crate::routing::Reason;
        let seen = match (verdict, reason) {
            (Some(v), None) => Seen::Verdict(v),
            (_, Some(Reason::Late)) => Seen::Late,
            (_, Some(Reason::NoVerdict | Reason::Unreachable)) => Seen::Missing,
            _ => Seen::Off,
        };
        t.route.injection = Some(InjectionBudget::read(seen, &self.cfg));
        Self::narrow_reads(t);
    }

    /// The deep tier's ceiling for a turn whose verdict folds deep but landed
    /// after the pack had fixed an ordinary budget for want of it
    /// (theseus-a2kh6). Called once the verdict is read, after
    /// [`Self::injection_read`] has fixed a budget for a turn no pack fixed one
    /// for.
    ///
    /// Only the ceiling moves. The recall is packed and its manifest written by
    /// then, so the tier and the recall sizes stay what the pack read and the
    /// row goes on saying so; the ceiling is the one size the turn has not
    /// spent yet, since its later loops and a routed recompile still choose
    /// their tools by it. Only a budget that fell back for want of a verdict is
    /// reopened: a tier the pack read from a verdict of its own is the turn's,
    /// and a config whose two ceilings are equal still raises nothing.
    pub(super) fn raise_tools(&self, t: &mut Turn<'_>, verdict: Option<&Verdict>) {
        let deep = self.cfg.tools.defer.admit_ceiling_deep;
        let folds_deep = verdict.is_some_and(|v| tier_of(Seen::Verdict(v)).0 == Tier::Deep);
        let Some(b) = t.route.injection.as_mut() else {
            return;
        };
        if folds_deep && b.fallback == Some(LATE) && deep > b.admit_ceiling {
            b.admit_ceiling = deep;
            b.admit_raised = true;
        }
    }

    /// Whether the turn's tools settled at a lower ceiling than its budget now
    /// gives, so they must settle again (theseus-a2kh6). False for a turn whose
    /// verdict had landed before they settled, which settles them once, and for
    /// a turn with no deferral to settle.
    pub(super) fn tools_behind(&self, t: &Turn<'_>) -> bool {
        let now = t
            .route
            .injection
            .as_ref()
            .map_or(self.cfg.tools.defer.admit_ceiling, |b| b.admit_ceiling);
        t.route.tools_ceiling.is_some_and(|settled| now > settled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(mode: &str, confidence: f64) -> Verdict {
        Verdict {
            mode: mode.into(),
            confidence,
            effort: None,
            judgment: "jdg_x".into(),
            turn: "trn_x".into(),
            persona: None,
            seal: None,
            personal: None,
            model: None,
            work: None,
            split: None,
        }
    }

    /// The approved numbers (the owner, 2026-10-22: "far more generous").
    #[test]
    fn the_defaults_are_the_approved_numbers() {
        let cfg = Config::example();
        let at = |t| InjectionBudget::of((t, None), &cfg);
        let sizes = |b: InjectionBudget| (b.recall_tokens, b.recall_items, b.admit_ceiling);
        assert_eq!(sizes(at(Tier::Ordinary)), (3_000, 8, 2.0));
        assert_eq!(sizes(at(Tier::Deep)), (8_000, 16, 3.0));
        assert_eq!(cfg.memory.session_recall_cap_tokens, 64_000);
    }

    /// A route.v10 verdict: `model` answered (so `open()`), the class `mode` word is only the pick,
    /// and `work` is `work.v1`'s answer (`None`: not answered).
    fn open(work: Option<(&str, bool)>) -> Verdict {
        let mut x = v("sonnet", 0.9);
        x.model = Some(Box::new(crate::routing::Pick {
            choice: "sonnet".into(),
            confidence: 0.9,
            sure: true,
            no_match: Some("sonnet".into()),
        }));
        // The tier reads only a sure answer, so `work.v1`'s declared default
        // does not reach it: theseus-ig13o builds the mechanism and converts
        // `model`, `effort` and `course`, and leaves this site named rather
        // than quietly changed.
        x.work = work.map(|(c, sure)| {
            Box::new(crate::routing::Pick {
                choice: c.into(),
                confidence: 0.9,
                sure,
                no_match: None,
            })
        });
        x
    }

    /// Dark reader 1-3 (theseus-h66v4.6): under route.v10 the tier reads `work.v1`'s label, and the
    /// class names are never matched against a model pick (`mode` is "opus", "haiku": a retired key).
    #[test]
    fn under_route_v10_the_tier_and_the_coding_hint_read_the_work_label() {
        let cfg = Config::example();
        for (work, tier) in [
            // `coding`, `research_or_design` and `review` are Deep, as `sophisticated` and `review`
            // were under route.v8's class names; the owner approved the fold (2026-11-03 00:03).
            ("coding", Tier::Deep),
            ("research_or_design", Tier::Deep),
            ("review", Tier::Deep),
            ("writing", Tier::Ordinary),
            ("conversation", Tier::Ordinary),
            ("tool_run", Tier::Ordinary),
        ] {
            let x = open(Some((work, true)));
            assert_eq!(tier_of(Seen::Verdict(&x)), (tier, None), "{work}");
            let b = InjectionBudget::read(Seen::Verdict(&x), &cfg);
            assert_eq!(
                b.coding,
                work == "coding",
                "{work}: only `coding` is the hint"
            );
        }
        // The pick's word is no class: a model called `opus` is not the retired `sophisticated`,
        // and a pick that happens to spell a class name is no more one.
        for pick in ["opus", "haiku", "deep_coding", "review"] {
            let mut x = open(Some(("conversation", true)));
            x.mode = pick.into();
            assert_eq!(tier_of(Seen::Verdict(&x)).0, Tier::Ordinary, "{pick}");
            assert!(
                !InjectionBudget::read(Seen::Verdict(&x), &cfg).coding,
                "{pick}"
            );
        }
        // Under the bar, or unanswered: ordinary, with the reason said, and no hint.
        let x = open(Some(("coding", false)));
        assert_eq!(tier_of(Seen::Verdict(&x)), (Tier::Ordinary, Some("unsure")));
        assert!(!InjectionBudget::read(Seen::Verdict(&x), &cfg).coding);
        let x = open(None);
        assert_eq!(
            tier_of(Seen::Verdict(&x)),
            (Tier::Ordinary, Some("no work answer"))
        );
        assert!(!InjectionBudget::read(Seen::Verdict(&x), &cfg).coding);
    }

    #[tokio::test]
    async fn a_peek_leaves_the_verdict_for_the_wait() {
        let (tx, rx) = oneshot::channel();
        let mut wait = Some(rx);
        assert!(matches!(peek(&mut wait), Peeked::Pending));
        let at = tokio::time::Instant::now();
        tx.send(crate::judge::inbound::Answered {
            verdict: Some(v("review", 0.9)),
            at,
            cost_micros: 7,
        })
        .unwrap();
        assert!(matches!(peek(&mut wait), Peeked::Landed(Some(ref x)) if x.mode == "review"));
        let a = wait.take().unwrap().await.unwrap();
        assert_eq!(
            (a.verdict.unwrap().mode.as_str(), a.at, a.cost_micros),
            ("review", at, 7)
        );
    }
}
