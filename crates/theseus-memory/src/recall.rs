//! Recall's pipeline after the index (design M6 §2.4, step 30a): the
//! filters, each drop with its reason; the science's rank; and the pack under
//! the budget. Pure: the core reads the index and each candidate's place,
//! and this decides what would be admitted.
//!
//! 30b adds `labeled_wrong`: a node the operator labeled wrong or stale.
//! 31b added `arm`: a synthesis, which no arm admits (the `+synthesis` arm
//! that did, and the nightly writer, are gone; a stored synthesis stays
//! readable, and recall drops it).
//! 31a adds the memory pass's edges, read by a science that prefers the
//! newer node (`baseline`'s second version): of a `same_entity` group only
//! the newest is kept (`duplicate`), and the older side of a `supersedes`
//! is dropped (`superseded`), each when its newer node is a candidate the
//! filters kept, or already in the turn's context.
//!
//! Every turn draws on every session (the owner, 2026-11-22: anything
//! goes): no place rule, no seal and no thread distance drop a candidate. A
//! candidate still carries its session's place, which its heading names.

use std::collections::{BTreeMap, BTreeSet};

use crate::floor::Floor;
use crate::science::{MemoryScience, Scored};

/// Where a session speaks, as the core read it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// The CLI, the web UI, an owner's DM, a channel bound private.
    Private,
    /// A shared place: its target (`discord:channel:<id>`, `discord:dm:<user>`).
    Shared(String),
    /// Its place could not be read: shared, and no place's own.
    Unknown,
}

/// Why a candidate was not admitted (§2.4's table, as 30a builds it; 30b's
/// labels add `labeled_wrong`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// Already in the turn's context, or another chunk of a node admitted.
    InContext,
    /// External text, and the config admits none.
    Untrusted,
    /// The operator labeled it `wrong` or `stale` (30b's `memory.label`).
    LabeledWrong,
    /// A recall, or a line the harness wrote.
    Recursion,
    /// A synthesis: no arm admits one (31b).
    Arm,
    /// A newer node corrects it, and is a candidate or in context (31a).
    Superseded,
    /// A newer node of its `same_entity` group is a candidate or in context
    /// (31a).
    Duplicate,
    /// Below the science's least fused score, or the relevance floor
    /// (`Params::floor`).
    Threshold,
    /// About another subject of the same kind as the question's, and below
    /// the subject bar (`Params::floor`).
    Subject,
    /// It did not fit the budget, or the items were already at their most.
    Budget,
}

impl Reason {
    pub const ALL: [Reason; 10] = [
        Reason::InContext,
        Reason::Untrusted,
        Reason::LabeledWrong,
        Reason::Recursion,
        Reason::Arm,
        Reason::Superseded,
        Reason::Duplicate,
        Reason::Threshold,
        Reason::Subject,
        Reason::Budget,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::InContext => "in_context",
            Reason::Untrusted => "untrusted",
            Reason::LabeledWrong => "labeled_wrong",
            Reason::Recursion => "recursion",
            Reason::Arm => "arm",
            Reason::Superseded => "superseded",
            Reason::Duplicate => "duplicate",
            Reason::Threshold => "threshold",
            Reason::Subject => "subject",
            Reason::Budget => "budget",
        }
    }
}

/// One hit from the index, with its session's place.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub node_id: String,
    pub chunk: u64,
    pub session_id: String,
    pub position: u64,
    /// The node's kind (`user_message`, `tool_result`, …).
    pub kind: String,
    /// Who wrote it (`operator`, `agent`, `tool`, `harness`).
    pub origin: String,
    pub external: bool,
    pub text: String,
    /// The index's fused score.
    pub fused: f64,
    /// The index's rank, from 1.
    pub index_rank: usize,
    pub place: Place,
    /// When the node was written (ms since the epoch): a question about
    /// time orders by it (theseus-q1tbn).
    pub time_ms: u64,
    /// Every entity the chunk holds (`type:value`), as the index found them
    /// (theseus-55hlu): empty from an index that does not say.
    pub entities: Vec<String>,
    /// The question's entities this chunk holds.
    pub matched: Vec<String>,
    /// A look back's result (`context.history`, `look.work`): its words are other nodes', copied, so recall
    /// never serves them as its own (each original is indexed under its own id, with its own label). Read
    /// from the hit's tool when the candidate is made; never stored.
    pub copy: bool,
}

impl Candidate {
    /// `<node>#<chunk>`.
    pub fn key(&self) -> String {
        format!("{}#{}", self.node_id, self.chunk)
    }
}

/// The turn that recalls.
#[derive(Clone, Debug)]
pub struct Asker<'a> {
    pub session_id: &'a str,
    /// The nodes the turn's request already carries.
    pub in_context: &'a BTreeSet<String>,
    /// The nodes the operator labeled wrong or stale.
    pub labeled: &'a BTreeSet<String>,
    /// The memory pass's edges among the candidates (31a).
    pub links: &'a [Link],
}

/// What a memory pass's edge says of two nodes (31a), as recall reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LinkKind {
    /// The two say the same: `same_entity`.
    SameEntity,
    /// The newer corrects the older: `supersedes`.
    Supersedes,
}

/// An edge between two nodes, from the newer to the older (the pass gates a
/// node against nodes written before it).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Link {
    pub kind: LinkKind,
    pub newer: String,
    pub older: String,
}

/// The pack's limits (§2.4's defaults: 1,500 tokens, 6 items, 400 a item).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub budget_tokens: u64,
    pub max_items: usize,
    pub item_tokens: u64,
    pub include_external: bool,
    /// The relevance floor and the subject bar (theseus-55hlu); the default
    /// is neither.
    pub floor: Floor,
    /// The engaged persona's own memories rank higher (theseus-a8n7); none
    /// when no persona is engaged.
    pub persona: Option<PersonaBoost>,
    /// A note from a session that shares a category with the asker's ranks
    /// higher (theseus-jrf4c); none when the asker shares none, or in a
    /// shared place.
    pub category: Option<CategoryBoost>,
}

/// While a persona is engaged, its own agent's imported episodes are worth
/// `boost` times their score in the rank, and nothing else changes: a filter
/// still sees the index's score, so the boost never admits a note. The core
/// reads which agent each imported candidate's session came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersonaBoost {
    /// The persona's agent, lower case.
    pub agent: String,
    /// `[persona_import] own_boost`: 1.0 to 2.0.
    pub boost: f64,
    /// The agent of each candidate session that has one, lower case.
    pub of_session: BTreeMap<String, String>,
}

impl PersonaBoost {
    /// What a candidate of `session`'s score is multiplied by.
    pub fn factor(&self, session: &str) -> f64 {
        match self.of_session.get(session) {
            Some(a) if *a == self.agent => self.boost.clamp(1.0, 2.0),
            _ => 1.0,
        }
    }
}

/// A note from a session that shares a category with the asking session
/// gets a reciprocal-rank term, as activation's reached hits do (theseus-jrf4c):
/// the candidates that share one, in the order of their own score, are
/// worth `1 + gain * weight * (K + 1) / (K + rank)`, rank from 1, where
/// `weight` is the heaviest kind among the categories they share (project and
/// thread highest, topic next, the rest low). Only the rank moves: a filter
/// still sees the index's score, so the term admits nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CategoryBoost {
    /// `[ontology] recall_boost`: 0 (off) to 1.
    pub gain: f64,
    /// The weight (0 to 1) of the heaviest category each candidate session
    /// shares with the asker; a session sharing none is absent.
    pub shared: BTreeMap<String, f64>,
}

/// The reciprocal rank's constant: the one the fusion uses.
const RANK_K: f64 = 60.0;

impl CategoryBoost {
    /// What each candidate's score is multiplied by, by its key: 1 for a
    /// candidate whose session shares nothing. Rank among the sharers is by
    /// score, best first, ties by key, so the term does not depend on the
    /// candidates' order.
    pub fn factors<'a>(
        &self,
        candidates: impl IntoIterator<Item = (&'a str, &'a str, f64)>,
    ) -> BTreeMap<String, f64> {
        let gain = self.gain.clamp(0.0, 1.0);
        let mut sharers: Vec<(&str, &str, f64)> = candidates
            .into_iter()
            .filter(|(_, session, _)| self.shared.contains_key(*session))
            .collect();
        sharers.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(b.0)));
        sharers
            .into_iter()
            .enumerate()
            .map(|(i, (key, session, _))| {
                let weight = self.shared[session].clamp(0.0, 1.0);
                let rr = (RANK_K + 1.0) / (RANK_K + (i + 1) as f64);
                (key.to_string(), 1.0 + gain * weight * rr)
            })
            .collect()
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            budget_tokens: 1500,
            max_items: 6,
            item_tokens: 400,
            include_external: false,
            floor: Floor::default(),
            persona: None,
            category: None,
        }
    }
}

/// A candidate the pack would admit.
#[derive(Clone, Debug, PartialEq)]
pub struct Admitted {
    pub candidate: Candidate,
    /// Its place in the science's order, from 1.
    pub rank: usize,
    /// Its text, cut to the item's tokens.
    pub excerpt: String,
    pub tokens: u64,
}

/// A candidate dropped, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropped {
    pub candidate: Candidate,
    pub reason: Reason,
    /// Its excerpt's tokens, for a `budget` drop; else 0.
    pub tokens: u64,
}

/// What a recall would admit, and what it dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub admitted: Vec<Admitted>,
    pub dropped: Vec<Dropped>,
    pub tokens: u64,
}

impl Pack {
    /// How many were dropped for `reason`.
    pub fn dropped_for(&self, reason: Reason) -> usize {
        self.dropped.iter().filter(|d| d.reason == reason).count()
    }

    /// The distinct sessions the admitted items came from.
    pub fn sessions(&self) -> usize {
        self.admitted
            .iter()
            .map(|a| a.candidate.session_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }
}

/// The estimate a pack uses: a token for each 4 bytes, rounded up.
pub fn tokens_of(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

/// `text` cut to `tokens` (4 bytes each) on a character's edge, with `…`
/// when it was cut.
pub fn excerpt(text: &str, tokens: u64) -> String {
    let limit = usize::try_from(tokens.saturating_mul(4)).unwrap_or(usize::MAX);
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The filter that drops `c`, if one does.
fn filter(
    c: &Candidate,
    asker: &Asker<'_>,
    p: &Params,
    science: &dyn MemoryScience,
) -> Option<Reason> {
    if asker.in_context.contains(&c.node_id) {
        return Some(Reason::InContext);
    }
    if c.external && !p.include_external {
        return Some(Reason::Untrusted);
    }
    if asker.labeled.contains(&c.node_id) {
        return Some(Reason::LabeledWrong);
    }
    if c.origin == "harness" || c.kind == "recall" || c.copy {
        return Some(Reason::Recursion);
    }
    if c.kind == "synthesis" {
        return Some(Reason::Arm);
    }
    if c.fused < science.min_score() || c.fused.is_nan() {
        return Some(Reason::Threshold);
    }
    if c.fused < p.floor.min_fused {
        return Some(Reason::Threshold);
    }
    if c.fused < p.floor.subject_fused && p.floor.other_subject(&c.entities) {
        return Some(Reason::Subject);
    }
    None
}

/// The kept candidates less the older nodes of the pass's edges (31a): the
/// older side of a `supersedes`, and every node of a `same_entity` group but
/// its newest, when the newer node is kept or already in the turn's context.
/// Chains resolve: of `c → b → a`, only `c` stays.
fn fresher(kept: Vec<Candidate>, asker: &Asker<'_>, dropped: &mut Vec<Dropped>) -> Vec<Candidate> {
    let present: BTreeSet<&str> = kept
        .iter()
        .map(|c| c.node_id.as_str())
        .chain(asker.in_context.iter().map(String::as_str))
        .collect();
    let mut older: std::collections::BTreeMap<&str, Reason> = std::collections::BTreeMap::new();
    for l in asker.links {
        if l.newer == l.older || !present.contains(l.newer.as_str()) {
            continue;
        }
        let reason = match l.kind {
            LinkKind::Supersedes => Reason::Superseded,
            LinkKind::SameEntity => Reason::Duplicate,
        };
        // A correction outranks a likeness when a node is both.
        let e = older.entry(l.older.as_str()).or_insert(reason);
        if reason == Reason::Superseded {
            *e = reason;
        }
    }
    let mut out = Vec::new();
    for c in kept {
        match older.get(c.node_id.as_str()) {
            Some(reason) => dropped.push(Dropped {
                candidate: c,
                reason: *reason,
                tokens: 0,
            }),
            None => out.push(c),
        }
    }
    out
}

/// The pipeline after the index: filter, rank by the science, and pack
/// greedily under the budget. Every candidate is admitted or dropped, once.
pub fn recall(
    science: &dyn MemoryScience,
    asker: &Asker<'_>,
    candidates: Vec<Candidate>,
    p: &Params,
) -> Pack {
    let mut pack = Pack::default();
    let mut kept = Vec::new();
    for c in candidates {
        match filter(&c, asker, p, science) {
            Some(reason) => pack.dropped.push(Dropped {
                candidate: c,
                reason,
                tokens: 0,
            }),
            None => kept.push(c),
        }
    }
    if science.prefers_newer() {
        kept = fresher(kept, asker, &mut pack.dropped);
    }
    let shared = p.category.as_ref().map(|b| {
        b.factors(
            kept.iter()
                .map(|c| (c.node_id.as_str(), c.session_id.as_str(), c.fused)),
        )
    });
    let order = science.rank(
        kept.iter()
            .map(|c| Scored {
                key: c.key(),
                score: c.fused
                    * p.persona.as_ref().map_or(1.0, |b| b.factor(&c.session_id))
                    * shared
                        .as_ref()
                        .and_then(|f| f.get(c.node_id.as_str()))
                        .copied()
                        .unwrap_or(1.0),
            })
            .collect(),
    );
    let mut by_key: std::collections::BTreeMap<String, Candidate> =
        kept.into_iter().map(|c| (c.key(), c)).collect();
    let mut nodes = BTreeSet::new();
    for (i, s) in order.iter().enumerate() {
        let Some(c) = by_key.remove(&s.key) else {
            continue;
        };
        // Another chunk of a node already admitted: it is in the pack.
        if nodes.contains(&c.node_id) {
            pack.dropped.push(Dropped {
                candidate: c,
                reason: Reason::InContext,
                tokens: 0,
            });
            continue;
        }
        let text = excerpt(&c.text, p.item_tokens);
        let tokens = tokens_of(&text);
        if pack.admitted.len() >= p.max_items || pack.tokens + tokens > p.budget_tokens {
            pack.dropped.push(Dropped {
                candidate: c,
                reason: Reason::Budget,
                tokens,
            });
            continue;
        }
        nodes.insert(c.node_id.clone());
        pack.tokens += tokens;
        pack.admitted.push(Admitted {
            candidate: c,
            rank: i + 1,
            excerpt: text,
            tokens,
        });
    }
    // A science that left a candidate out of its order dropped it.
    for (_, c) in by_key {
        pack.dropped.push(Dropped {
            candidate: c,
            reason: Reason::Threshold,
            tokens: 0,
        });
    }
    pack
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::science::Baseline;

    fn cand(node: &str, session: &str, place: Place, fused: f64) -> Candidate {
        Candidate {
            node_id: node.into(),
            chunk: 0,
            session_id: session.into(),
            position: 1,
            kind: "user_message".into(),
            origin: "operator".into(),
            external: false,
            text: format!("the text of {node}"),
            fused,
            index_rank: 1,
            place,
            time_ms: 0,
            entities: Vec::new(),
            matched: Vec::new(),
            copy: false,
        }
    }

    fn run(in_context: &[&str], cands: Vec<Candidate>, p: &Params) -> Pack {
        run_with(in_context, cands, p, &[])
    }

    fn link(kind: LinkKind, newer: &str, older: &str) -> Link {
        Link {
            kind,
            newer: newer.into(),
            older: older.into(),
        }
    }

    fn run_with(in_context: &[&str], cands: Vec<Candidate>, p: &Params, links: &[Link]) -> Pack {
        let in_context = in_context.iter().map(|s| s.to_string()).collect();
        let labeled = BTreeSet::from(["wrong".to_string()]);
        let asker = Asker {
            session_id: "ses_here",
            in_context: &in_context,
            labeled: &labeled,
            links,
        };
        recall(
            &Baseline {
                min_score: 0.01,
                ..Baseline::default()
            },
            &asker,
            cands,
            p,
        )
    }

    fn reason_of(pack: &Pack, node: &str) -> Option<Reason> {
        pack.dropped
            .iter()
            .find(|d| d.candidate.node_id == node)
            .map(|d| d.reason)
    }

    /// A look back's result is never served: its words are copies, dropped as recursion as a recall
    /// note's are, however well it scores.
    #[test]
    fn a_copy_is_dropped_as_recursion() {
        let mut copy = cand("copy", "ses_b", Place::Private, 0.95);
        copy.copy = true;
        let cands = vec![copy, cand("ok", "ses_b", Place::Private, 0.9)];
        let pack = run(&[], cands, &Params::default());
        let admitted: Vec<_> = (pack.admitted.iter())
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(admitted, ["ok"]);
        assert_eq!(reason_of(&pack, "copy"), Some(Reason::Recursion));
    }

    /// Each filter's reason (§3.2's 30a tests), one candidate each, and one
    /// that passes them all.
    #[test]
    fn each_filter_drops_with_its_reason() {
        let mut external = cand("ext", "ses_b", Place::Private, 0.5);
        external.external = true;
        let mut harness = cand("harness", "ses_b", Place::Private, 0.5);
        harness.origin = "harness".into();
        let mut recalled = cand("recalled", "ses_b", Place::Private, 0.5);
        recalled.kind = "recall".into();
        let mut big = cand("big", "ses_c", Place::Private, 0.4);
        big.text = "x".repeat(4000);
        let mut second_chunk = cand("ok", "ses_b", Place::Private, 0.3);
        second_chunk.chunk = 1;
        let mut synthesis = cand("syn", "ses_mem", Place::Private, 0.95);
        synthesis.kind = "synthesis".into();
        synthesis.origin = "agent".into();
        let cands = vec![
            synthesis,
            cand("ok", "ses_b", Place::Private, 0.9),
            cand("seen", "ses_here", Place::Private, 0.9),
            external,
            harness,
            recalled,
            cand("faint", "ses_b", Place::Private, 0.001),
            big,
            second_chunk,
            cand("wrong", "ses_b", Place::Private, 0.95),
            cand("stale", "ses_b", Place::Private, 0.9),
            cand("twin", "ses_b", Place::Private, 0.9),
        ];
        let p = Params {
            budget_tokens: 300,
            ..Params::default()
        };
        let links = [
            link(LinkKind::Supersedes, "ok", "stale"),
            link(LinkKind::SameEntity, "seen", "twin"),
        ];
        let pack = run_with(&["seen"], cands, &p, &links);
        let admitted: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(admitted, ["ok"]);
        for (node, reason) in [
            ("syn", Reason::Arm),
            ("seen", Reason::InContext),
            ("ext", Reason::Untrusted),
            ("harness", Reason::Recursion),
            ("recalled", Reason::Recursion),
            ("faint", Reason::Threshold),
            ("big", Reason::Budget),
            ("wrong", Reason::LabeledWrong),
            ("stale", Reason::Superseded),
            ("twin", Reason::Duplicate),
        ] {
            assert_eq!(reason_of(&pack, node), Some(reason), "{node}");
        }
        // The second chunk of an admitted node is in the pack already.
        assert!(pack
            .dropped
            .iter()
            .any(|d| d.candidate.key() == "ok#1" && d.reason == Reason::InContext));
        assert_eq!(pack.admitted.len() + pack.dropped.len(), 12);
        let subject = subject_pack();
        // Every reason this step builds is met above.
        for r in Reason::ALL {
            assert!(
                pack.dropped_for(r) + subject.dropped_for(r) > 0,
                "{}",
                r.as_str()
            );
        }
    }

    /// A turn draws on any session: a private place's, a shared place's, and
    /// one whose place cannot be read (theseus-1is6; anything goes).
    #[test]
    fn a_private_place_draws_on_every_place() {
        let cands = vec![
            cand("cli", "ses_a", Place::Private, 0.9),
            cand(
                "lab",
                "ses_b",
                Place::Shared("discord:channel:7".into()),
                0.8,
            ),
            cand("dm", "ses_c", Place::Shared("discord:dm:3".into()), 0.7),
            cand("unknown", "ses_d", Place::Unknown, 0.6),
        ];
        let pack = run(&[], cands, &Params::default());
        assert_eq!(pack.admitted.len(), 4, "{:?}", pack.dropped);
    }

    /// External text is admitted only when the config says so.
    #[test]
    fn include_external_admits_it() {
        let mut c = cand("ext", "ses_b", Place::Private, 0.5);
        c.external = true;
        let p = Params {
            include_external: true,
            ..Params::default()
        };
        assert_eq!(run(&[], vec![c], &p).admitted.len(), 1);
    }

    /// The pack stops at its items and its tokens, in the science's order,
    /// and a smaller item later still fits.
    #[test]
    fn the_pack_keeps_to_its_budget() {
        let mut cands: Vec<_> = (0..10)
            .map(|i| {
                cand(
                    &format!("n{i}"),
                    "ses_b",
                    Place::Private,
                    1.0 - f64::from(i) / 100.0,
                )
            })
            .collect();
        cands[1].text = "y".repeat(2000);
        let p = Params {
            budget_tokens: 300,
            max_items: 3,
            item_tokens: 400,
            include_external: false,
            floor: Floor::default(),
            persona: None,
            category: None,
        };
        let pack = run(&[], cands, &p);
        let admitted: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| (a.candidate.node_id.as_str(), a.rank))
            .collect();
        assert_eq!(admitted, [("n0", 1), ("n2", 3), ("n3", 4)]);
        assert!(pack.tokens <= 300);
        assert_eq!(pack.dropped_for(Reason::Budget), 7);
        let n1 = pack
            .dropped
            .iter()
            .find(|d| d.candidate.node_id == "n1")
            .unwrap();
        assert_eq!(n1.tokens, 400, "an excerpt is cut to its item's tokens");
    }

    /// A floor of 0.4 over a ceiling of 0.05 admits what scores 0.02 or more
    /// and drops the rest as `threshold`; the best of them is not admitted
    /// for being the best, so a question whose every hit is faint gets an
    /// empty pack (theseus-55hlu).
    #[test]
    fn a_candidate_below_the_floor_is_not_admitted() {
        let floor = |share| Params {
            floor: Floor::new(share, 0.0, 0.05, Vec::new()),
            ..Params::default()
        };
        let cands = || {
            vec![
                cand("gold", "ses_a", Place::Private, 0.04),
                cand("edge", "ses_a", Place::Private, 0.0205),
                cand("near", "ses_b", Place::Private, 0.019),
            ]
        };
        let pack = run(&[], cands(), &floor(0.4));
        let kept: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(kept, ["gold", "edge"]);
        assert_eq!(reason_of(&pack, "near"), Some(Reason::Threshold));
        let faint: Vec<_> = cands()
            .into_iter()
            .map(|mut c| {
                c.fused /= 4.0;
                c
            })
            .collect();
        let none = run(&[], faint, &floor(0.4));
        assert!(none.admitted.is_empty(), "no candidate passes: no pack");
        assert_eq!(none.dropped_for(Reason::Threshold), 3);
        assert_eq!(run(&[], cands(), &floor(0.0)).admitted.len(), 3, "0 is off");
    }

    /// The pack of a question that names `db-prod`, over a note that names
    /// it, two about `db-stage` (one just under the subject bar, one over it)
    /// and one of another kind of thing.
    fn subject_pack() -> Pack {
        let p = Params {
            floor: Floor::new(0.2, 0.7, 0.05, vec!["host:db-prod.example".into()]),
            ..Params::default()
        };
        let with = |node: &str, fused, entities: &[&str]| {
            let mut c = cand(node, "ses_a", Place::Private, fused);
            c.entities = entities.iter().map(|e| e.to_string()).collect();
            c.matched = c
                .entities
                .iter()
                .filter(|e| p.floor.asked.contains(e))
                .cloned()
                .collect();
            c
        };
        run(
            &[],
            vec![
                with("prod", 0.031, &["host:db-prod.example"]),
                with("stage", 0.03, &["host:db-stage.example"]),
                with("stage_strong", 0.04, &["host:db-stage.example"]),
                with("file", 0.03, &["file:notes.md"]),
            ],
            &p,
        )
    }

    /// A distractor holding another host than the one the question names
    /// must clear the higher bar; the one that names the asked host, and one
    /// about another kind of thing, are held to the floor alone.
    #[test]
    fn a_same_kind_other_subject_must_clear_the_subject_bar() {
        let pack = subject_pack();
        let kept: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(kept, ["stage_strong", "prod", "file"]);
        assert_eq!(reason_of(&pack, "stage"), Some(Reason::Subject));
    }

    #[test]
    fn an_excerpt_is_cut_on_a_character_edge() {
        assert_eq!(excerpt("short", 400), "short");
        let cut = excerpt(&"é".repeat(100), 10);
        assert!(cut.len() <= 40 && cut.ends_with('…'), "{cut}");
        assert_eq!(tokens_of("abcde"), 2);
    }

    /// `baseline`'s second version (31a) prefers the newer side of a
    /// `supersedes` and keeps only the newest of a `same_entity` group, a
    /// chain included; a link whose newer node is not here (another place's,
    /// or below the threshold) drops nothing; and the first version reads no
    /// link at all.
    #[test]
    fn the_newer_node_is_preferred() {
        let p = Params {
            max_items: 10,
            ..Params::default()
        };
        let cands = || {
            vec![
                cand("old", "ses_a", Place::Private, 0.9),
                cand("new", "ses_b", Place::Private, 0.5),
                cand("a", "ses_c", Place::Private, 0.8),
                cand("b", "ses_c", Place::Private, 0.7),
                cand("c", "ses_c", Place::Private, 0.6),
                cand("lone", "ses_d", Place::Private, 0.4),
            ]
        };
        let links = [
            link(LinkKind::Supersedes, "new", "old"),
            link(LinkKind::SameEntity, "b", "a"),
            link(LinkKind::SameEntity, "c", "b"),
            link(LinkKind::Supersedes, "elsewhere", "lone"),
        ];
        let pack = run_with(&[], cands(), &p, &links);
        let admitted: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(admitted, ["c", "new", "lone"]);
        assert_eq!(reason_of(&pack, "old"), Some(Reason::Superseded));
        assert_eq!(reason_of(&pack, "a"), Some(Reason::Duplicate));
        assert_eq!(reason_of(&pack, "b"), Some(Reason::Duplicate));
    }
}
