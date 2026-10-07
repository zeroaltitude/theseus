//! The ontology's methods (theseus-8kk.1; M4 design §2.8's surfaces):
//! `ontology.list`, and the operator's three writes. Each write is an
//! operator's act, judged as an approval is (`judge_act(Act::Ontology)`: the
//! owner, from a private place), before it reads anything; then checked
//! against the snapshot, written in one frame with its ledger row, and held
//! in the snapshot (`ontology::Board::write`).
//!
//! Given kinds are the transport's: their categories are made at a place's
//! first bind ([`Core::bind_categories`]), their memberships are read from a
//! session's place at compile, and a write of either is invalid input.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use serde_json::json;
use theseus_ontology::{
    Category, CategoryId, Guidance, MemberList, Membership, Ontology, Origin, Record,
};
use theseus_protocol::{
    error_code, method, OntologyCategory, OntologyCategoryAddParams, OntologyGuidance,
    OntologyGuidanceSetParams, OntologyKind, OntologyListParams, OntologyListResult,
    OntologyMembership, OntologyMembershipResult, OntologyMembershipSetParams,
    OntologyProposalAcceptParams, OntologyProposalRejectParams,
};

use super::confirms::Act;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::fact;
use crate::places::BoundPlace;

impl Core {
    /// Build the snapshot now, off the start path: after serving, on the
    /// blocking pool, by one META prefix scan. A reader that comes sooner
    /// builds it itself, once.
    pub fn warm_ontology(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            let Some(core) = core.upgrade() else { return };
            if let Err(e) = core.runner.ontology.snapshot(&core.store) {
                tracing::warn!(error = %format!("{e:#}"), "the ontology's snapshot cannot be built");
            }
        });
    }

    /// `ontology.list`: the kinds, the category tree with its guidance and
    /// each one's count of sessions, and memberships: a session's (given,
    /// from its place; and interpreted), or every interpreted one stored,
    /// or none when `memberships` is false.
    pub fn ontology_list(&self, p: &OntologyListParams) -> Result<OntologyListResult> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let kinds = o.kinds().into_iter().map(kind_info).collect();
        let counts = o.member_counts();
        let mut categories = Vec::new();
        tree(&o, &counts, None, 1, &mut categories);
        let memberships = match &p.session_id {
            _ if p.memberships == Some(false) => Vec::new(),
            Some(s) => {
                let place = self.runner.target_of(s);
                let mut ms = crate::ontology::given(place.as_deref(), now());
                ms.extend(o.memberships(s));
                ms.iter().map(|m| membership_info(s, m)).collect()
            }
            None => o
                .records()
                .into_iter()
                .filter_map(|r| match r {
                    Record::Members(l) => Some(l),
                    _ => None,
                })
                .flat_map(|l| {
                    let s = l.session.clone();
                    l.members.into_iter().map(move |m| membership_info(&s, &m))
                })
                .collect(),
        };
        Ok(OntologyListResult {
            kinds,
            categories,
            memberships,
        })
    }

    /// `ontology.category.add`: a category of an interpreted kind, its id
    /// made from its name.
    pub fn ontology_category_add(
        &self,
        p: &OntologyCategoryAddParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyCategory> {
        let who = who.into();
        let kind = p.kind.as_deref().unwrap_or("topic");
        let what = format!("{kind} category {:?}", p.name);
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_CATEGORY_ADD,
                what: &what,
            },
        )?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let parent_kind = o
            .kind(kind)
            .and_then(|k| k.parent.clone())
            .unwrap_or_else(|| kind.to_string());
        let parent = p
            .parent
            .as_deref()
            .map(|s| resolve(&o, &parent_kind, s))
            .transpose()?;
        let c = Category {
            id: o.mint_id(kind, &p.name)?,
            name: p.name.trim().to_string(),
            parent,
            description: p.description.clone().unwrap_or_default().trim().to_string(),
            added_by: added_by(&who),
            retired_ms: None,
        };
        let (who_s, via) = (who.who(), who.via());
        let set = fact::ontology::CategorySet {
            category: &c,
            origin: Origin::Operator.name(),
            who: &who_s,
            via: &via,
        };
        let o = self.runner.ontology.write(
            &self.store,
            vec![Record::Category(c.clone())],
            Origin::Operator,
            |_| Ok(vec![fact::row(&set, None, None)?]),
        )?;
        self.rec(None).announce(&set);
        Ok(category_info(&o, &c, depth(&o, &c.id), 0))
    }

    /// `ontology.guidance.set`: a category's guidance, replaced whole; the
    /// same text again writes nothing.
    pub fn ontology_guidance_set(
        &self,
        p: &OntologyGuidanceSetParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyGuidance> {
        let who = who.into();
        let what = format!("guidance of {}", p.category);
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_GUIDANCE_SET,
                what: &what,
            },
        )?;
        let o = self.runner.ontology.snapshot(&self.store)?;
        let id = resolve(&o, "topic", &p.category)?;
        let old = o.guidance(&id);
        let g = Guidance::new(
            id.clone(),
            &p.text,
            old.map_or(1, |g| g.version + 1),
            &added_by(&who),
        );
        if let Some(old) = old.filter(|old| old.digest == g.digest) {
            return Ok(guidance_info(old));
        }
        let (who_s, via) = (who.who(), who.via());
        let set = fact::ontology::GuidanceSet {
            guidance: &g,
            who: &who_s,
            via: &via,
        };
        self.runner.ontology.write(
            &self.store,
            vec![Record::Guidance(g.clone())],
            Origin::Operator,
            |_| Ok(vec![fact::row(&set, None, None)?]),
        )?;
        self.rec(None).announce(&set);
        Ok(guidance_info(&g))
    }

    /// `ontology.membership.set`: a session's interpreted memberships, a
    /// list per kind it touches, all in one frame. They apply at the
    /// session's next recompile.
    pub fn ontology_membership_set(
        &self,
        p: &OntologyMembershipSetParams,
        who: impl Into<Answerer>,
    ) -> Result<OntologyMembershipResult> {
        let who = who.into();
        let what = format!("memberships of {}", p.session_id);
        self.judge_act(
            &who,
            Act::Ontology {
                method: method::ONTOLOGY_MEMBERSHIP_SET,
                what: &what,
            },
        )?;
        let sid = p.session_id.as_str();
        if self
            .store
            .get_session::<crate::session::SessionRecord>(sid)?
            .is_none()
        {
            bail!("no session is named {sid}: `theseus sessions` lists them");
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut lists: BTreeMap<String, Vec<Membership>> = BTreeMap::new();
        let list = |lists: &mut BTreeMap<String, Vec<Membership>>, kind: &str| {
            lists
                .entry(kind.to_string())
                .or_insert_with(|| {
                    o.member_list(sid, kind)
                        .map(|l| l.members.clone())
                        .unwrap_or_default()
                })
                .len()
        };
        for s in &p.add {
            let id = resolve(&o, "topic", s)?;
            list(&mut lists, id.kind());
            let l = lists.get_mut(id.kind()).unwrap();
            if !l.iter().any(|m| m.category == id) {
                l.push(Membership::operator(id, now()));
            }
        }
        for s in &p.remove {
            let id = resolve(&o, "topic", s)?;
            list(&mut lists, id.kind());
            lists
                .get_mut(id.kind())
                .unwrap()
                .retain(|m| m.category != id);
        }
        let changed: Vec<MemberList> = lists
            .into_iter()
            .filter(|(kind, members)| {
                o.member_list(sid, kind)
                    .map_or(!members.is_empty(), |l| l.members != *members)
            })
            .map(|(kind, members)| MemberList {
                session: sid.to_string(),
                kind,
                members,
            })
            .collect();
        let (who_s, via) = (who.who(), who.via());
        let sets: Vec<fact::ontology::MembershipSet<'_>> = changed
            .iter()
            .map(|list| fact::ontology::MembershipSet {
                list,
                who: &who_s,
                via: &via,
            })
            .collect();
        let o = self.runner.ontology.write(
            &self.store,
            changed.iter().cloned().map(Record::Members).collect(),
            Origin::Operator,
            |_| sets.iter().map(|f| fact::row(f, Some(sid), None)).collect(),
        )?;
        for f in &sets {
            self.rec(Some(sid)).announce(f);
        }
        Ok(OntologyMembershipResult {
            session_id: sid.to_string(),
            memberships: o
                .memberships(sid)
                .iter()
                .map(|m| membership_info(sid, m))
                .collect(),
            changed: changed.iter().map(|l| l.kind.clone()).collect(),
        })
    }

    /// The given categories of the places the binding binds, made at each
    /// one's first bind (the transport's facts): a guild channel's channel,
    /// a DM's person, named as the binding names it. One frame for every new
    /// or renamed one; nothing when all are as they were.
    pub(crate) fn bind_categories(&self, places: &[BoundPlace]) -> Result<()> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let made: Vec<Category> = places
            .iter()
            .filter_map(|p| crate::ontology::bound_category(&p.target, &p.name))
            .filter(|c| o.category(&c.id).is_none_or(|old| old.name != c.name))
            .collect();
        if made.is_empty() {
            return Ok(());
        }
        let sets: Vec<fact::ontology::CategorySet<'_>> = made
            .iter()
            .map(|category| fact::ontology::CategorySet {
                category,
                origin: Origin::Transport.name(),
                who: "the binding",
                via: "discord",
            })
            .collect();
        self.runner.ontology.write(
            &self.store,
            made.iter().cloned().map(Record::Category).collect(),
            Origin::Transport,
            |_| sets.iter().map(|f| fact::row(f, None, None)).collect(),
        )?;
        for f in &sets {
            self.rec(None).announce(f);
        }
        Ok(())
    }

    /// The ontology's methods (its proposals' too, 28b), from one arm of `dispatch`, which stays
    /// within clippy's length that way (the shape budget).
    pub(super) fn rpc_ontology(
        &self,
        name: &str,
        params: serde_json::Value,
        conn: Conn<'_>,
    ) -> Result<serde_json::Value, RpcFailure> {
        match name {
            method::ONTOLOGY_LIST => route(params, |p| self.rpc_ontology_list(p)),
            method::ONTOLOGY_CATEGORY_ADD => {
                route(params, |p| self.rpc_ontology_category_add(p, conn))
            }
            method::ONTOLOGY_GUIDANCE_SET => {
                route(params, |p| self.rpc_ontology_guidance_set(p, conn))
            }
            method::ONTOLOGY_PROPOSALS => route(params, |p| {
                self.ontology_proposals(&p).map_err(RpcFailure::invalid)
            }),
            method::ONTOLOGY_PROPOSAL_ACCEPT => route(params, |p: OntologyProposalAcceptParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.ontology_proposal_accept(&p, who).map_err(failure)
            }),
            method::ONTOLOGY_PROPOSAL_REJECT => route(params, |p: OntologyProposalRejectParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.ontology_proposal_reject(&p, who).map_err(failure)
            }),
            _ => route(params, |p| self.rpc_ontology_membership_set(p, conn)),
        }
    }

    pub(super) fn rpc_ontology_list(
        &self,
        p: OntologyListParams,
    ) -> Result<OntologyListResult, RpcFailure> {
        self.ontology_list(&p).map_err(RpcFailure::invalid)
    }

    pub(super) fn rpc_ontology_category_add(
        &self,
        p: OntologyCategoryAddParams,
        conn: Conn<'_>,
    ) -> Result<OntologyCategory, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.ontology_category_add(&p, who).map_err(failure)
    }

    pub(super) fn rpc_ontology_guidance_set(
        &self,
        p: OntologyGuidanceSetParams,
        conn: Conn<'_>,
    ) -> Result<OntologyGuidance, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.ontology_guidance_set(&p, who).map_err(failure)
    }

    pub(super) fn rpc_ontology_membership_set(
        &self,
        p: OntologyMembershipSetParams,
        conn: Conn<'_>,
    ) -> Result<OntologyMembershipResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.ontology_membership_set(&p, who).map_err(failure)
    }
}

/// A write's error on the wire: a judged refusal is `REFUSED`, with who,
/// through what, and why; anything else (the ontology's own refusal, a name
/// that resolves to nothing) is invalid input.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "an ontology write from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

fn now() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// Who wrote a record, as its `added_by` holds it: one line of at most 100
/// characters.
pub(super) fn added_by(who: &Answerer) -> String {
    let w: String = who
        .who()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect();
    match w.trim() {
        "" => "the operator".into(),
        t => t.to_string(),
    }
}

/// A category named by its id (`topic:theseus`), or by its name, or its id's
/// last part, among the categories of `kind`.
pub(super) fn resolve(o: &Ontology, kind: &str, s: &str) -> Result<CategoryId> {
    let s = s.trim();
    if s.contains(':') {
        return Ok(CategoryId::parse(s)?);
    }
    let found: Vec<&Category> = o
        .categories()
        .filter(|c| c.kind() == kind && (c.name.eq_ignore_ascii_case(s) || c.id.local() == s))
        .collect();
    match found.as_slice() {
        [c] => Ok(c.id.clone()),
        [] => Err(anyhow!(
            "no {kind} is named {s:?}: `theseus ontology categories` lists them"
        )),
        many => Err(anyhow!(
            "{} {kind} categories are named {s:?} ({}): name one by its id",
            many.len(),
            many.iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The category tree below `parent`, depth first, each with its count.
fn tree(
    o: &Ontology,
    counts: &BTreeMap<CategoryId, u64>,
    parent: Option<&CategoryId>,
    depth: u32,
    out: &mut Vec<OntologyCategory>,
) {
    for c in o.children(parent) {
        let n = counts.get(&c.id).copied().unwrap_or(0);
        out.push(category_info(o, c, depth, n));
        if depth < theseus_ontology::MAX_DEPTH as u32 {
            tree(o, counts, Some(&c.id), depth + 1, out);
        }
    }
}

fn depth(o: &Ontology, id: &CategoryId) -> u32 {
    o.path(id).len() as u32
}

fn kind_info(k: &theseus_ontology::Kind) -> OntologyKind {
    OntologyKind {
        name: k.name.clone(),
        basis: match k.basis {
            theseus_ontology::Basis::Given => "given".into(),
            theseus_ontology::Basis::Interpreted => "interpreted".into(),
        },
        assigned_by: k.assigned_by.iter().map(|o| o.name().to_string()).collect(),
        per_session: k.per_session.to_string(),
        parent: k.parent.clone(),
        precedence: k.precedence,
        rule: k.rule.name().to_string(),
        description: k.description.clone(),
        version: k.version,
        added_by: k.added_by.clone(),
    }
}

fn category_info(o: &Ontology, c: &Category, depth: u32, members: u64) -> OntologyCategory {
    OntologyCategory {
        id: c.id.to_string(),
        kind: c.kind().to_string(),
        name: c.name.clone(),
        parent: c.parent.as_ref().map(ToString::to_string),
        depth,
        description: c.description.clone(),
        added_by: c.added_by.clone(),
        guidance: o
            .guidance(&c.id)
            .filter(|g| !g.is_empty())
            .map(guidance_info),
        members,
    }
}

fn guidance_info(g: &Guidance) -> OntologyGuidance {
    OntologyGuidance {
        category: g.category.to_string(),
        text: g.text.clone(),
        version: g.version,
        digest: g.digest.clone(),
        added_by: g.added_by.clone(),
    }
}

pub(super) fn membership_info(session: &str, m: &Membership) -> OntologyMembership {
    OntologyMembership {
        session_id: session.to_string(),
        kind: m.kind().to_string(),
        category: m.category.to_string(),
        origin: m.origin.name().to_string(),
        confidence: m.confidence,
        as_of_ms: m.as_of_ms,
    }
}
