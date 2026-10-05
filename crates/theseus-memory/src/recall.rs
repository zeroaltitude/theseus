//! Recall's pipeline after the index (design M6 §2.4, step 30a): the
//! filters, each drop with its reason; the science's rank; and the pack under
//! the budget. Pure: the core reads the index and each candidate's place,
//! and this decides what would be admitted.
//!
//! 30b adds `labeled_wrong`: a node the operator labeled wrong or stale.
//! 31a adds the memory pass's edges, read by a science that prefers the
//! newer node (`baseline`'s second version): of a `same_entity` group only
//! the newest is kept (`duplicate`), and the older side of a `supersedes`
//! is dropped (`superseded`), each when its newer node is a candidate the
//! filters kept, or already in the turn's context.
//!
//! The place rule (theseus-nbsh) is the first filter: a turn in a shared
//! place draws only on that place's own sessions, and a turn in a private
//! place only on private places' sessions. A candidate whose place cannot be
//! read is dropped by it too.

use std::collections::{BTreeMap, BTreeSet};

use crate::fsrs::Retention;
use crate::science::{MemoryScience, RankCtx, Scored};

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

impl Place {
    /// Whether a turn speaking here may draw on a session speaking at `from`.
    pub fn may_draw_on(&self, from: &Place) -> bool {
        match (self, from) {
            (Place::Private, Place::Private) => true,
            (Place::Shared(here), Place::Shared(there)) => here == there,
            _ => false,
        }
    }
}

/// Why a candidate was not admitted (§2.4's table, as 30a builds it; 30b's
/// labels add `labeled_wrong`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// Its session speaks in a place this turn may not draw on.
    Place,
    /// Already in the turn's context, or another chunk of a node admitted.
    InContext,
    /// External text, and the config admits none.
    Untrusted,
    /// The operator labeled it `wrong` or `stale` (30b's `memory.label`).
    LabeledWrong,
    /// A recall, or a line the harness wrote.
    Recursion,
    /// A newer node corrects it, and is a candidate or in context (31a).
    Superseded,
    /// A newer node of its `same_entity` group is a candidate or in context
    /// (31a).
    Duplicate,
    /// Below the science's least fused score.
    Threshold,
    /// It did not fit the budget, or the items were already at their most.
    Budget,
}

impl Reason {
    pub const ALL: [Reason; 9] = [
        Reason::Place,
        Reason::InContext,
        Reason::Untrusted,
        Reason::LabeledWrong,
        Reason::Recursion,
        Reason::Superseded,
        Reason::Duplicate,
        Reason::Threshold,
        Reason::Budget,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Place => "place",
            Reason::InContext => "in_context",
            Reason::Untrusted => "untrusted",
            Reason::LabeledWrong => "labeled_wrong",
            Reason::Recursion => "recursion",
            Reason::Superseded => "superseded",
            Reason::Duplicate => "duplicate",
            Reason::Threshold => "threshold",
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
    pub place: &'a Place,
    /// The nodes the turn's request already carries.
    pub in_context: &'a BTreeSet<String>,
    /// The nodes the operator labeled wrong or stale.
    pub labeled: &'a BTreeSet<String>,
    /// The memory pass's edges among the candidates (31a).
    pub links: &'a [Link],
    pub now_ms: u64,
    /// The candidates' retention by node (32a), for a science that reads
    /// it; empty for one that does not, or before the projection is built.
    pub retention: &'a BTreeMap<String, Retention>,
}

impl Asker<'_> {
    /// The rank's context for `kept`: the turn's time, and their retention.
    pub fn rank_ctx(&self, kept: &[Candidate]) -> RankCtx {
        RankCtx {
            now_ms: self.now_ms,
            retention: kept
                .iter()
                .filter_map(|c| Some((c.node_id.clone(), *self.retention.get(&c.node_id)?)))
                .collect(),
        }
    }
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
}

impl Default for Params {
    fn default() -> Self {
        Self {
            budget_tokens: 1500,
            max_items: 6,
            item_tokens: 400,
            include_external: false,
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

/// The filter that drops `c`, if one does, in the order the place rule
/// first.
fn filter(c: &Candidate, asker: &Asker<'_>, p: &Params, min_score: f64) -> Option<Reason> {
    if !asker.place.may_draw_on(&c.place) {
        return Some(Reason::Place);
    }
    if asker.in_context.contains(&c.node_id) {
        return Some(Reason::InContext);
    }
    if c.external && !p.include_external {
        return Some(Reason::Untrusted);
    }
    if asker.labeled.contains(&c.node_id) {
        return Some(Reason::LabeledWrong);
    }
    if c.origin == "harness" || c.kind == "recall" {
        return Some(Reason::Recursion);
    }
    if c.fused < min_score || c.fused.is_nan() {
        return Some(Reason::Threshold);
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
        match filter(&c, asker, p, science.min_score()) {
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
    let order = science.rank(
        kept.iter()
            .map(|c| Scored {
                key: c.key(),
                score: c.fused,
            })
            .collect(),
        &asker.rank_ctx(&kept),
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
    use proptest::prelude::*;

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
        }
    }

    fn run(place: &Place, in_context: &[&str], cands: Vec<Candidate>, p: &Params) -> Pack {
        run_with(place, in_context, cands, p, &[])
    }

    fn link(kind: LinkKind, newer: &str, older: &str) -> Link {
        Link {
            kind,
            newer: newer.into(),
            older: older.into(),
        }
    }

    fn run_with(
        place: &Place,
        in_context: &[&str],
        cands: Vec<Candidate>,
        p: &Params,
        links: &[Link],
    ) -> Pack {
        let in_context = in_context.iter().map(|s| s.to_string()).collect();
        let labeled = BTreeSet::from(["wrong".to_string()]);
        let asker = Asker {
            session_id: "ses_here",
            place,
            in_context: &in_context,
            labeled: &labeled,
            links,
            now_ms: 0,
            retention: &BTreeMap::new(),
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

    /// Each filter's reason (§3.2's 30a tests), one candidate each, and one
    /// that passes them all.
    #[test]
    fn each_filter_drops_with_its_reason() {
        let lab = Place::Shared("discord:channel:7".into());
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
        let cands = vec![
            cand("ok", "ses_b", Place::Private, 0.9),
            cand("shared", "ses_lab", lab, 0.9),
            cand("unknown", "ses_x", Place::Unknown, 0.9),
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
        let pack = run_with(&Place::Private, &["seen"], cands, &p, &links);
        let admitted: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(admitted, ["ok"]);
        for (node, reason) in [
            ("shared", Reason::Place),
            ("unknown", Reason::Place),
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
        assert_eq!(pack.admitted.len() + pack.dropped.len(), 13);
        // Every reason this step builds is met above.
        for r in Reason::ALL {
            assert!(pack.dropped_for(r) > 0, "{}", r.as_str());
        }
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
        assert_eq!(run(&Place::Private, &[], vec![c], &p).admitted.len(), 1);
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
        };
        let pack = run(&Place::Private, &[], cands, &p);
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
        let pack = run_with(&Place::Private, &[], cands(), &p, &links);
        let admitted: Vec<_> = pack
            .admitted
            .iter()
            .map(|a| a.candidate.node_id.as_str())
            .collect();
        assert_eq!(admitted, ["c", "new", "lone"]);
        assert_eq!(reason_of(&pack, "old"), Some(Reason::Superseded));
        assert_eq!(reason_of(&pack, "a"), Some(Reason::Duplicate));
        assert_eq!(reason_of(&pack, "b"), Some(Reason::Duplicate));
        // The first version reads no link: the older node outranks.
        let v1 = Baseline {
            version: 1,
            min_score: 0.01,
            ..Baseline::default()
        };
        let in_context = BTreeSet::new();
        let labeled = BTreeSet::new();
        let asker = Asker {
            session_id: "ses_here",
            place: &Place::Private,
            in_context: &in_context,
            labeled: &labeled,
            links: &links,
            now_ms: 0,
            retention: &BTreeMap::new(),
        };
        let pack = recall(&v1, &asker, cands(), &p);
        assert_eq!(pack.admitted[0].candidate.node_id, "old");
        assert_eq!(pack.dropped.len(), 0);
    }

    fn place_strategy() -> impl Strategy<Value = Place> {
        prop_oneof![
            Just(Place::Private),
            Just(Place::Unknown),
            (0u8..4).prop_map(|i| Place::Shared(format!("discord:channel:{i}"))),
            (0u8..2).prop_map(|i| Place::Shared(format!("discord:dm:{i}"))),
        ]
    }

    proptest! {
        /// The place rule, over generated candidates and places: nothing from
        /// a private place's session is admitted, or dropped for any reason
        /// but the place, in a shared place; nothing from one shared place
        /// in another; and nothing of a place that cannot be read anywhere.
        #[test]
        fn the_place_rule_holds_for_every_pack(
            here in place_strategy(),
            places in proptest::collection::vec(place_strategy(), 1..30),
            scores in proptest::collection::vec(0.0f64..1.0, 30),
        ) {
            let cands: Vec<_> = places
                .iter()
                .enumerate()
                .map(|(i, pl)| cand(&format!("n{i}"), &format!("ses_{i}"), pl.clone(), scores[i]))
                .collect();
            let pack = run(&here, &[], cands, &Params { max_items: 40, budget_tokens: 100_000, ..Params::default() });
            for a in &pack.admitted {
                prop_assert!(here.may_draw_on(&a.candidate.place), "{here:?} admitted {:?}", a.candidate.place);
            }
            for d in &pack.dropped {
                if !here.may_draw_on(&d.candidate.place) {
                    prop_assert_eq!(d.reason, Reason::Place);
                }
            }
            for c in &pack.admitted {
                match (&here, &c.candidate.place) {
                    (Place::Shared(a), Place::Shared(b)) => prop_assert_eq!(a, b),
                    (Place::Shared(_), other) => prop_assert!(false, "a shared place drew on {other:?}"),
                    (Place::Private, other) => prop_assert_eq!(other, &Place::Private),
                    (Place::Unknown, other) => prop_assert!(false, "an unknown place drew on {other:?}"),
                }
            }
        }
    }
}
