//! What a turn injects, sized to its prompt (theseus-d9je7, rows 2 and 4): one
//! [`InjectionBudget`] from the route pack's mode, folded into three tiers.
//!
//! - **The fold** ([`tier_of`]): `trivial` is small; `sophisticated`,
//!   `deep_coding` and `review` are deep; every other mode is ordinary, and so
//!   is a verdict that is late, missing, under its mode's bar, or not acting,
//!   and a turn routing does not decide (off, pinned, a continuation).
//! - **Recall** packs at the tier's tokens and items (`[memory]
//!   recall_budget_tokens`/`recall_max_items` ordinary, `deep_recall_tokens`/
//!   `deep_recall_items` deep). Small's request carries none: its trivial
//!   detour drops the pack (`recall_routed`, theseus-n7nc), so it packs as
//!   ordinary, and a trivial verdict that does not detour keeps today's recall.
//! - **Tool admission** raises the place's budget to the tier's multiple
//!   (`[tools.defer] admit_ceiling_small`, `admit_ceiling`, `admit_ceiling_deep`).
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
    Small,
    Ordinary,
    Deep,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Small => "small",
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
    /// What the request's recall may hold; 0 for small, whose detour sends none.
    pub recall_tokens: u64,
    pub recall_items: usize,
    /// The multiple of the place's first-request budget admitted families may
    /// raise a request to.
    pub admit_ceiling: f64,
    /// The verdict acts and its mode is `deep_coding` (theseus-tdkcn): the
    /// request's tail carries [`CODING_HINT`] and a bare `fs.read` is narrower.
    /// Not in the row.
    pub coding: bool,
}

/// What a `deep_coding` turn's request carries in its tail (theseus-tdkcn).
pub const CODING_HINT: &str = "[The harness: for this work, map the code before reading it: list a file's or crate's functions first (fs_grep for function signatures). Read in slices of 100-300 lines with offset and limit, not whole files. After each slice, write a line of what you checked and what you found, so your findings survive when old results are cleared. Keep going until the areas you were asked about are covered.]";

/// The tier a verdict folds to, and why it is ordinary when it fell back.
pub fn tier_of(seen: Seen<'_>, cfg: &Config) -> (Tier, Option<&'static str>) {
    let v = match seen {
        Seen::Verdict(v) => v,
        Seen::Late => return (Tier::Ordinary, Some("verdict late")),
        Seen::Missing => return (Tier::Ordinary, Some("no verdict")),
        Seen::Off => return (Tier::Ordinary, Some("routing off")),
    };
    if v.confidence < cfg.routing.confidence_for(&v.mode) {
        return (Tier::Ordinary, Some("unsure"));
    }
    let tier = match v.mode.as_str() {
        "trivial" => Tier::Small,
        "sophisticated" | "deep_coding" | "review" => Tier::Deep,
        _ => Tier::Ordinary,
    };
    (tier, None)
}

impl InjectionBudget {
    pub fn of((tier, fallback): (Tier, Option<&'static str>), cfg: &Config) -> Self {
        let (m, d) = (&cfg.memory, &cfg.tools.defer);
        let (recall_tokens, recall_items, admit_ceiling) = match tier {
            Tier::Small => (0, 0, d.admit_ceiling_small),
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
        let coding = matches!(seen, Seen::Verdict(v) if v.mode == "deep_coding");
        let mut b = Self::of(tier_of(seen, cfg), cfg);
        b.coding = coding && b.fallback.is_none();
        b
    }

    /// The pack's tokens and items: small packs as ordinary, and its detour
    /// drops the pack (theseus-n7nc).
    pub fn pack(&self, cfg: &Config) -> (u64, usize) {
        match self.tier {
            Tier::Small => (cfg.memory.recall_budget_tokens, cfg.memory.recall_max_items),
            _ => (self.recall_tokens, self.recall_items),
        }
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
            context: None,
        }
    }

    #[test]
    fn each_mode_folds_to_its_tier_and_the_rest_to_ordinary() {
        let cfg = Config::example();
        for (mode, tier) in [
            ("trivial", Tier::Small),
            ("quick", Tier::Ordinary),
            ("chat", Tier::Ordinary),
            ("routine_coding", Tier::Ordinary),
            ("other", Tier::Ordinary),
            ("sophisticated", Tier::Deep),
            ("deep_coding", Tier::Deep),
            ("review", Tier::Deep),
        ] {
            assert_eq!(
                tier_of(Seen::Verdict(&v(mode, 0.95)), &cfg),
                (tier, None),
                "{mode}"
            );
        }
        let unsure = v("deep_coding", 0.1);
        assert_eq!(
            tier_of(Seen::Verdict(&unsure), &cfg),
            (Tier::Ordinary, Some("unsure"))
        );
        assert_eq!(tier_of(Seen::Late, &cfg).1, Some("verdict late"));
        assert_eq!(tier_of(Seen::Missing, &cfg).1, Some("no verdict"));
        assert_eq!(tier_of(Seen::Off, &cfg).1, Some("routing off"));
    }

    /// The approved numbers (the owner, 2026-10-22: "far more generous").
    #[test]
    fn the_defaults_are_the_approved_numbers() {
        let cfg = Config::example();
        let at = |t| InjectionBudget::of((t, None), &cfg);
        let sizes = |b: InjectionBudget| (b.recall_tokens, b.recall_items, b.admit_ceiling);
        assert_eq!(sizes(at(Tier::Small)), (0, 0, 1.0));
        assert_eq!(sizes(at(Tier::Ordinary)), (3_000, 8, 2.0));
        assert_eq!(sizes(at(Tier::Deep)), (8_000, 16, 3.0));
        assert_eq!(
            at(Tier::Small).pack(&cfg),
            (3_000, 8),
            "its detour drops it"
        );
        assert_eq!(cfg.memory.session_recall_cap_tokens, 64_000);
    }

    /// Only an acting `deep_coding` verdict is a coding turn (theseus-tdkcn):
    /// not `review` or `sophisticated`, not an unsure verdict, not a turn
    /// routing does not decide.
    #[test]
    fn only_an_acting_deep_coding_verdict_is_a_coding_turn() {
        let cfg = Config::example();
        fn coding(s: Seen<'_>, cfg: &Config) -> bool {
            InjectionBudget::read(s, cfg).coding
        }
        let coding = |s: Seen<'_>| coding(s, &cfg);
        assert!(coding(Seen::Verdict(&v("deep_coding", 0.95))));
        for mode in [
            "review",
            "sophisticated",
            "routine_coding",
            "chat",
            "trivial",
        ] {
            assert!(!coding(Seen::Verdict(&v(mode, 0.95))), "{mode}");
        }
        assert!(!coding(Seen::Verdict(&v("deep_coding", 0.1))), "unsure");
        for s in [Seen::Late, Seen::Missing, Seen::Off] {
            assert!(!coding(s));
        }
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
