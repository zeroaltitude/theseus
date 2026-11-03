//! What a turn injects, sized to its prompt (theseus-d9je7, rows 2 and 4): one
//! [`InjectionBudget`] from `work.v1`'s answer, folded into two tiers.
//!
//! - **The fold** ([`tier_of`]): `work = coding` at its bar is deep; every other
//!   label is ordinary, and so is a verdict that is late, missing, under its
//!   bar, or not acting, and a turn routing does not decide (off, pinned, a
//!   continuation). There is no small tier: it was route.v8's `trivial` class,
//!   whose detour dropped the pack, and no pack says "short" any more.
//! - **Recall** packs at the tier's tokens and items (`[memory]
//!   recall_budget_tokens`/`recall_max_items` ordinary, `deep_recall_tokens`/
//!   `deep_recall_items` deep).
//! - **Tool admission** raises the place's budget to the tier's multiple
//!   (`[tools.defer] admit_ceiling`, `admit_ceiling_deep`; `admit_ceiling_small`
//!   is retired with the small tier).
//! - **Never a wait**: the pack reads the verdict if it has landed (a peek that
//!   leaves it for the compile's wait, [`peek`]) and is ordinary when it has
//!   not; the budget is then fixed for the turn, recorded on its
//!   `route.decided` row, and a routed recompile's admission takes it too.
//! - **The cache**: the tier sizes the tail (recall, after the message) and
//!   the families a recompile loads; an append keeps its compilation's tools
//!   (`tools_defer::forks`), so no cached prefix byte moves from turn to turn.
//!
//! A config whose tiers equal one another (deep's recall keys at ordinary's,
//! the three multiples alike) is the rollback.

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::{Turn, TurnRunner};
use crate::config::{Config, PackMode};
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
}

/// What a `deep_coding` turn's request carries in its tail (theseus-tdkcn).
pub const CODING_HINT: &str = "[The harness: for this work, map the code before reading it: list a file's or crate's functions first (fs_grep for function signatures). Read in slices of 100-300 lines with offset and limit, not whole files. After each slice, write a line of what you checked and what you found, so your findings survive when old results are cleared. Keep going until the areas you were asked about are covered.]";

/// The tier a verdict folds to, and why it is ordinary when it fell back.
pub fn tier_of(seen: Seen<'_>) -> (Tier, Option<&'static str>) {
    let v = match seen {
        Seen::Verdict(v) => v,
        Seen::Late => return (Tier::Ordinary, Some("verdict late")),
        Seen::Missing => return (Tier::Ordinary, Some("no verdict")),
        Seen::Off => return (Tier::Ordinary, Some("routing off")),
    };
    // route.v10 (theseus-h66v4.6): the class names are gone, and `work.v1`'s label is what the
    // tier reads. The owner decided three separate classifications, with `work.v1` replacing the
    // class names for the hint readers and the injection tier (2026-10-30 16:01). The narrowing to
    // `work = coding` alone, so that code is Deep and every other label Ordinary, is the
    // conductor's reading (16:1x) of that answer, not the owner's words, and it is an open
    // question with him (decision 5 of the Phase 2 report). It is narrower than the names were:
    // research, design and review were Deep under route.v8's `sophisticated` and `review`, and are
    // Ordinary here until the owner says otherwise (the wider fold is one more arm, and his to ask
    // for; theseus-h66v4.6 fix pass, F7). `trivial` had no label to carry (Small was
    // route.v8's "no recall for a greeting"). A work answer under the question's confirm bar is
    // "unsure", as a class under its bar was.
    match v.work.as_deref() {
        None => (Tier::Ordinary, Some("no work answer")),
        Some(w) if !w.sure => (Tier::Ordinary, Some("unsure")),
        Some(w) => match w.choice.as_str() {
            "coding" => (Tier::Deep, None),
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

impl InjectionBudget {
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
        }
    }

    /// The budget a verdict (or its absence) gives: `of` its tier, and the
    /// coding flag when an acting verdict's mode is `deep_coding`.
    pub fn read(seen: Seen<'_>, cfg: &Config) -> Self {
        let coding = matches!(seen, Seen::Verdict(v) if is_coding(v));
        let mut b = Self::of(tier_of(seen), cfg);
        b.coding = coding && b.fallback.is_none();
        b
    }

    /// The `route.decided` row's `injection`, which `theseus why` words.
    pub fn row(&self) -> Value {
        json!({
            "tier": self.tier.as_str(), "fallback": self.fallback,
            "recall_tokens": self.recall_tokens, "recall_items": self.recall_items,
            "admit_ceiling": self.admit_ceiling,
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
    /// The budget the turn reads now: the one fixed, else one fixed from the
    /// verdict if it has landed (or from routing deciding nothing), else
    /// ordinary for now, fixed only when `fix` (the pack, which never waits).
    pub(super) fn injection(&self, t: &mut Turn<'_>, fix: bool) -> InjectionBudget {
        if let Some(b) = &t.route.injection {
            return b.clone();
        }
        let live = t.route.mode.unwrap_or(PackMode::Shadow) >= PackMode::Canary
            && !self.judge.breaker_open();
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
        }));
        x.work = work.map(|(c, sure)| {
            Box::new(crate::routing::Pick {
                choice: c.into(),
                confidence: 0.9,
                sure,
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
            // The owner decided that `work.v1` replaces the class names for the injection tier
            // (2026-10-30 16:01); `work = coding` alone being Deep is the conductor's reading
            // of that (16:1x), and an open question with him (decision 5). Research, design and
            // review were Deep under route.v8's class names; folding them here is a decision the
            // owner has not made (theseus-h66v4.6 fix pass, F7).
            ("coding", Tier::Deep),
            ("research_or_design", Tier::Ordinary),
            ("review", Tier::Ordinary),
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
