//! The compile walk's composition (M4 design §2.8, steps 2 to 4).
//!
//! The categories in play are the session's memberships and, under `chain`,
//! every ancestor of each. Each is admitted once, by its own kind's rule, in
//! one order:
//! 1. by its kind's precedence, lowest first, so a higher kind's guidance
//!    comes later and governs;
//! 2. within a kind, by its path of names from the root, so an ancestor
//!    comes before its descendants (the farthest first, the nearest last);
//! 3. then by id, so the order is total.
//!
//! A kind's parent kind always has a lower precedence (the table refuses
//! anything else), so the two orders never disagree: an ancestor of another
//! kind sorts first by precedence, and one of the same kind by its path.
//!
//! The bytes depend only on which categories are in play, the table, the
//! tree, and the guidance: never on the memberships' order, origin,
//! confidence, or as-of. So a membership list saved again in another order
//! renders the same system block, and the prompt cache survives it.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::category::{CategoryId, Membership};
use crate::kind::{Kind, Origin, Rule};
use crate::refusal::Refusal;
use crate::Ontology;

/// The first section, when any guidance is admitted: it tells the model the
/// rule the order carries.
pub const PREAMBLE: &str = "# Guidance\n\nThe sections below are guidance for the categories \
                            this session belongs to. Where two disagree, the later one governs.";

/// Between the names of a category's path, in a header.
pub const PATH_SEP: &str = " › ";

/// What a compile admits: the system block's sections and what the manifest
/// records of them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composition {
    /// The sections, in order, each a header and its text. The core puts
    /// them in the system block after the persona's files, a blank line
    /// between each, as a context file's sections are.
    pub sections: Vec<String>,
    /// Every membership the compile used, in the order it was taken: the
    /// manifest's `memberships`.
    pub memberships: Vec<MembershipUsed>,
    /// Each category's guidance the sections carry, in order: the
    /// manifest's record of each block.
    pub guidance: Vec<GuidanceUsed>,
    /// Memberships the compile could not use, and why. A valid snapshot's
    /// own never land here; a stale one, or a place's that names a category
    /// not yet made, may.
    pub skipped: Vec<Skipped>,
}

impl Composition {
    /// The sections as the system block carries them: a blank line between
    /// each, and an empty string when there are none.
    pub fn render(&self) -> String {
        self.sections.join("\n\n")
    }
}

/// A membership a compile used, for the manifest: "why did it know that?"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MembershipUsed {
    pub kind: String,
    pub category: CategoryId,
    pub origin: Origin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub as_of_ms: u64,
}

/// A category's guidance a compile admitted, for the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidanceUsed {
    pub category: CategoryId,
    pub version: u32,
    /// The guidance's own digest (the first 16 hex digits of the SHA-256 of
    /// its text). The system block's digest covers the rendered section.
    pub digest: String,
}

/// A membership a compile could not use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skipped {
    pub membership: Membership,
    pub why: String,
}

impl Ontology {
    /// Compose the guidance for a session's memberships: given ones from its
    /// place (origin `transport`), and interpreted ones (at a recompile, the
    /// snapshot's current ones; on an append, the ones its manifest
    /// recorded). Reads memory only, and never fails: a membership it cannot
    /// use is skipped, with the reason.
    pub fn compose(&self, memberships: &[Membership]) -> Composition {
        let mut out = Composition::default();

        // 1. The memberships it can use, in the composition's order, each
        //    category once, and no kind past its count.
        let mut usable: Vec<(Key<'_>, &Membership)> = Vec::new();
        for m in memberships {
            match self.usable(m) {
                Ok(kind) => usable.push((self.key(kind, &m.category), m)),
                Err(why) => out.skipped.push(Skipped {
                    membership: m.clone(),
                    why: why.to_string(),
                }),
            }
        }
        usable.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| tie(a, b)));
        let mut kept: Vec<&Membership> = Vec::new();
        for (_, m) in usable {
            let kind = &self.kinds[m.kind()];
            let why = if kept.iter().any(|k| k.category == m.category) {
                Some(format!("`{}` is listed more than once", m.category))
            } else if !kind
                .per_session
                .allows(kept.iter().filter(|k| k.kind() == m.kind()).count() + 1)
            {
                Some(format!(
                    "a session holds at most {} `{}` categories",
                    kind.per_session, kind.name
                ))
            } else {
                None
            };
            match why {
                Some(why) => out.skipped.push(Skipped {
                    membership: m.clone(),
                    why,
                }),
                None => kept.push(m),
            }
        }

        // 2. The categories in play: each membership's, and under `chain`
        //    every ancestor of it.
        let mut in_play: BTreeSet<&CategoryId> = BTreeSet::new();
        for m in &kept {
            match self.kinds[m.kind()].rule {
                Rule::Chain => in_play.extend(self.path(&m.category).into_iter().map(|c| &c.id)),
                Rule::IntentLine => {
                    in_play.insert(&m.category);
                }
                // Refused by the table, so never in a valid snapshot.
                Rule::Ranked | Rule::RecallOnly => {}
            }
        }
        let mut order: Vec<(Key<'_>, &CategoryId)> = in_play
            .into_iter()
            .filter_map(|id| Some((self.key(self.kinds.get(id.kind())?, id), id)))
            .collect();
        order.sort();

        // 3. The sections: a block per category under `chain`, and one per
        //    kind under `intent_line`, with a line per category.
        let mut lines: Vec<String> = Vec::new();
        for (i, (key, id)) in order.iter().enumerate() {
            let kind = &self.kinds[id.kind()];
            if let Some(g) = self.guidance.get(*id).filter(|g| !g.is_empty()) {
                let path = key.names.join(PATH_SEP);
                match kind.rule {
                    Rule::Chain => out
                        .sections
                        .push(format!("# Guidance ({} {path})\n\n{}", kind.name, g.text)),
                    Rule::IntentLine => lines.push(format!("- {path}: {}", g.text)),
                    Rule::Ranked | Rule::RecallOnly => continue,
                }
                out.guidance.push(GuidanceUsed {
                    category: g.category.clone(),
                    version: g.version,
                    digest: g.digest.clone(),
                });
            }
            let last_of_kind = order
                .get(i + 1)
                .is_none_or(|(_, next)| next.kind() != id.kind());
            if last_of_kind && !lines.is_empty() {
                out.sections.push(format!(
                    "# Guidance ({})\n\n{}",
                    kind.name,
                    lines.join("\n")
                ));
                lines.clear();
            }
        }
        if !out.sections.is_empty() {
            out.sections.insert(0, PREAMBLE.to_string());
        }

        out.memberships = kept
            .into_iter()
            .map(|m| MembershipUsed {
                kind: m.kind().to_string(),
                category: m.category.clone(),
                origin: m.origin,
                confidence: m.confidence,
                as_of_ms: m.as_of_ms,
            })
            .collect();
        out
    }

    /// Whether a compile can use a membership: its kind and its category are
    /// held, a given kind's comes from the transport, and an interpreted
    /// kind's from an origin the kind lets assign it.
    fn usable(&self, m: &Membership) -> Result<&Kind, Refusal> {
        let kind = self.kinds.get(m.kind()).ok_or_else(|| Refusal::Missing {
            what: "kind",
            id: m.kind().to_string(),
        })?;
        if !self.categories.contains_key(&m.category) {
            return Err(Refusal::Missing {
                what: "category",
                id: m.category.to_string(),
            });
        }
        if kind.is_given() != (m.origin == Origin::Transport) {
            return Err(Refusal::Given {
                kind: kind.name.clone(),
                why: if kind.is_given() {
                    format!(
                        "`{}` memberships come from the session's place, not from `{}`",
                        kind.name, m.origin
                    )
                } else {
                    format!(
                        "the transport gives only guild, channel, and person, not `{}`",
                        kind.name
                    )
                },
            });
        }
        if !kind.assigned_by.contains(&m.origin) {
            return Err(Refusal::Writer {
                why: format!("`{}` may not assign `{}` memberships", m.origin, kind.name),
            });
        }
        m.check_confidence()?;
        Ok(kind)
    }

    /// A category's place in the order: its kind's precedence, then its
    /// path of names from the root, then its id.
    fn key<'a>(&'a self, kind: &Kind, id: &'a CategoryId) -> Key<'a> {
        Key {
            precedence: kind.precedence,
            names: self.path(id).into_iter().map(|c| c.name.as_str()).collect(),
            id,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key<'a> {
    precedence: u32,
    names: Vec<&'a str>,
    id: &'a CategoryId,
}

/// Between two memberships of one category: the first kept is the
/// transport's over the operator's, then the earliest, then the surest.
fn tie(a: &Membership, b: &Membership) -> Ordering {
    a.origin
        .cmp(&b.origin)
        .then(a.as_of_ms.cmp(&b.as_of_ms))
        .then_with(|| {
            let c = |m: &Membership| m.confidence.unwrap_or(1.0);
            c(b).total_cmp(&c(a))
        })
}
