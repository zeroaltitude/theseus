//! The snapshot: every `onto:*` record, held in memory (M4 design §2.8).
//! The core builds it lazily after serving, by one META scan, and keeps it
//! current on every write; a compile reads it and nothing else.
//!
//! A snapshot is always valid: [`Ontology::put`] checks a record against the
//! whole before it applies it, and [`Ontology::load`] keeps what checks and
//! says what it dropped. So a compile over a snapshot never meets a loop, a
//! missing parent, or a rule nothing reads.

use std::collections::{BTreeMap, BTreeSet};

use crate::category::{Category, CategoryId, Guidance, MemberList, Membership};
use crate::kind::{seeds, Basis, Kind, Origin, PerSession, Rule};
use crate::refusal::Refusal;
use crate::text;
use crate::{keys, Record};

/// The deepest a category nests: a root is 1 deep.
pub const MAX_DEPTH: usize = 8;

/// A category's place among its siblings: its parent, its kind, and its
/// name in lowercase.
type NameKey = (Option<CategoryId>, String, String);

#[derive(Debug, Clone, PartialEq)]
pub struct Ontology {
    pub(crate) kinds: BTreeMap<String, Kind>,
    pub(crate) categories: BTreeMap<CategoryId, Category>,
    pub(crate) guidance: BTreeMap<CategoryId, Guidance>,
    /// By session, then kind.
    pub(crate) members: BTreeMap<(String, String), MemberList>,
    /// Derived from `categories`: each one's children, and the roots under
    /// `None`.
    children: BTreeMap<Option<CategoryId>, BTreeSet<CategoryId>>,
    /// Derived from `categories`: the categories by their place among their
    /// siblings (given ones may share a name; interpreted ones may not).
    names: BTreeMap<NameKey, BTreeSet<CategoryId>>,
}

impl Ontology {
    /// The seed rows alone: the ontology of a store that has no `onto:*`
    /// records. A stored row of the same name supersedes a seed.
    pub fn seeded() -> Self {
        Ontology {
            kinds: seeds().into_iter().map(|k| (k.name.clone(), k)).collect(),
            categories: BTreeMap::new(),
            guidance: BTreeMap::new(),
            members: BTreeMap::new(),
            children: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }

    /// The kinds, by precedence.
    pub fn kinds(&self) -> Vec<&Kind> {
        let mut v: Vec<&Kind> = self.kinds.values().collect();
        v.sort_by_key(|k| k.precedence);
        v
    }

    pub fn kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.get(name)
    }

    /// Every category, by id.
    pub fn categories(&self) -> impl Iterator<Item = &Category> {
        self.categories.values()
    }

    pub fn category(&self, id: &CategoryId) -> Option<&Category> {
        self.categories.get(id)
    }

    /// The category tree, one level: the children of `parent`, or the roots
    /// when it is `None`, by name and then id.
    pub fn children(&self, parent: Option<&CategoryId>) -> Vec<&Category> {
        let mut v: Vec<&Category> = self
            .children
            .get(&parent.cloned())
            .into_iter()
            .flatten()
            .filter_map(|id| self.categories.get(id))
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        v
    }

    /// A category and its ancestors, the root first; empty when it is not
    /// held.
    pub fn path(&self, id: &CategoryId) -> Vec<&Category> {
        let mut out = Vec::new();
        let mut at = self.categories.get(id);
        while let Some(c) = at {
            out.push(c);
            // A valid snapshot has no loop; the bound keeps a bad one finite.
            if out.len() > MAX_DEPTH {
                break;
            }
            at = c.parent.as_ref().and_then(|p| self.categories.get(p));
        }
        out.reverse();
        out
    }

    pub fn guidance(&self, id: &CategoryId) -> Option<&Guidance> {
        self.guidance.get(id)
    }

    pub fn member_list(&self, session: &str, kind: &str) -> Option<&MemberList> {
        self.members.get(&(session.to_string(), kind.to_string()))
    }

    /// A session's interpreted memberships, every kind's: what a recompile
    /// takes, beside the given ones from its place.
    pub fn memberships(&self, session: &str) -> Vec<Membership> {
        self.members
            .range((session.to_string(), String::new())..)
            .take_while(|((s, _), _)| s == session)
            .flat_map(|(_, l)| l.members.iter().cloned())
            .collect()
    }

    /// Every record the snapshot holds, the seed rows included: the input
    /// [`Ontology::load`] rebuilds it from.
    pub fn records(&self) -> Vec<Record> {
        let kinds = self.kinds.values().cloned().map(Record::Kind);
        let cats = self.categories.values().cloned().map(Record::Category);
        let guides = self.guidance.values().cloned().map(Record::Guidance);
        let lists = self.members.values().cloned().map(Record::Members);
        kinds.chain(cats).chain(guides).chain(lists).collect()
    }

    /// Check a write by `by`, then apply it: the snapshot changes only when
    /// the record checks. The core writes the record to the store between
    /// [`Ontology::check`] and this, under one lock, so the two agree.
    pub fn put(&mut self, record: Record, by: Origin) -> Result<(), Refusal> {
        self.check(&record, by)?;
        self.apply(record);
        Ok(())
    }

    /// Whether `by` may write `record`, and whether the ontology would still
    /// be valid with it.
    ///
    /// - Kinds are the operator's. A change is one more version than the
    ///   row it supersedes, and every category, guidance, and membership
    ///   list must still check under it.
    /// - A given kind's categories come from the transport; an interpreted
    ///   kind's, from the operator. Either may supersede one (a new name, or
    ///   a new parent), never into a loop.
    /// - Guidance is the operator's, for any category, one more version than
    ///   the last.
    /// - Membership lists are the operator's, of interpreted kinds only:
    ///   given memberships are never stored.
    pub fn check(&self, record: &Record, by: Origin) -> Result<(), Refusal> {
        by.built()?;
        match record {
            Record::Kind(k) => {
                if by != Origin::Operator {
                    return Err(Refusal::Writer {
                        why: format!("the kinds table is the operator's to write, not `{by}`'s"),
                    });
                }
                k.check_row()?;
                let want = self.kinds.get(&k.name).map_or(1, |old| old.version + 1);
                if k.version != want {
                    return Err(Refusal::Version {
                        what: format!("kind `{}`", k.name),
                        want,
                        got: k.version,
                    });
                }
                let mut next = self.clone();
                next.kinds.insert(k.name.clone(), k.clone());
                next.check_all()
            }
            Record::Category(c) => {
                let kind = self.kind_of(c.kind())?;
                match (kind.basis, by) {
                    (Basis::Given, Origin::Transport) | (Basis::Interpreted, Origin::Operator) => {}
                    (Basis::Given, _) => {
                        return Err(Refusal::Given {
                            kind: kind.name.clone(),
                            why: format!(
                                "`{}` categories come from the transport, at a place's first \
                                 bind: they are not written through the API",
                                kind.name
                            ),
                        })
                    }
                    (Basis::Interpreted, _) => {
                        return Err(Refusal::Writer {
                            why: format!(
                                "`{}` categories are the operator's to declare: the transport \
                                 writes only given kinds' categories",
                                kind.name
                            ),
                        })
                    }
                }
                self.check_category(c)
            }
            Record::Guidance(g) => {
                if by != Origin::Operator {
                    return Err(Refusal::Writer {
                        why: format!("guidance is the operator's to write, not `{by}`'s"),
                    });
                }
                self.check_guidance(g)?;
                let want = self
                    .guidance
                    .get(&g.category)
                    .map_or(1, |old| old.version + 1);
                if g.version != want {
                    return Err(Refusal::Version {
                        what: format!("the guidance of `{}`", g.category),
                        want,
                        got: g.version,
                    });
                }
                Ok(())
            }
            Record::Members(l) => {
                let kind = self.kind_of(&l.kind)?;
                if kind.is_given() {
                    return Err(given_memberships(kind));
                }
                if by != Origin::Operator {
                    return Err(Refusal::Writer {
                        why: format!(
                            "`{}` memberships are the operator's to set, not `{by}`'s",
                            kind.name
                        ),
                    });
                }
                self.check_members(l)
            }
        }
    }

    /// Rebuild a snapshot from stored records, in any order: the seed rows,
    /// then every record that checks against what is held so far, in passes
    /// until a pass adds nothing (so a parent stored after its child still
    /// comes first). A record that never checks is dropped, with its key and
    /// why: the core warns of each, and the rest serves. A membership list
    /// keeps the entries that check.
    pub fn load(records: impl IntoIterator<Item = Record>) -> (Ontology, Vec<(String, Refusal)>) {
        let mut onto = Ontology::seeded();
        let mut dropped = Vec::new();
        let (mut kinds, mut cats, mut guides, mut lists) = (vec![], vec![], vec![], vec![]);
        for r in records {
            match r {
                Record::Kind(k) => kinds.push(k),
                Record::Category(c) => cats.push(c),
                Record::Guidance(g) => guides.push(g),
                Record::Members(l) => lists.push(l),
            }
        }

        kinds.sort_by(|a, b| a.name.cmp(&b.name));
        let mut pending: Vec<Kind> = Vec::new();
        for k in kinds {
            match k.check_row() {
                Ok(()) => pending.push(k),
                Err(e) => dropped.push((keys::kind(&k.name), e)),
            }
        }
        passes(&mut pending, |k| {
            let mut next = onto.kinds.clone();
            next.insert(k.name.clone(), k.clone());
            check_table(&next)?;
            onto.kinds = next;
            Ok(())
        });
        for k in pending {
            let mut next = onto.kinds.clone();
            next.insert(k.name.clone(), k.clone());
            if let Err(e) = check_table(&next) {
                dropped.push((keys::kind(&k.name), e));
            }
        }

        cats.sort_by(|a, b| a.id.cmp(&b.id));
        let mut pending = cats;
        passes(&mut pending, |c| {
            onto.check_category(c)?;
            onto.insert_category(c.clone());
            Ok(())
        });
        let stuck: BTreeMap<CategoryId, Category> =
            pending.iter().map(|c| (c.id.clone(), c.clone())).collect();
        for c in pending {
            if let Some(e) = stuck_loop(&stuck, &c.id)
                .map(|path| Refusal::Cycle { path })
                .or_else(|| onto.check_category(&c).err())
            {
                dropped.push((keys::category(&c.id), e));
            }
        }

        for g in guides {
            match onto.check_guidance(&g) {
                Ok(()) => {
                    onto.guidance.insert(g.category.clone(), g);
                }
                Err(e) => dropped.push((keys::guidance(&g.category), e)),
            }
        }

        for mut l in lists {
            let key = keys::members(&l.session, &l.kind);
            let kind = match onto.list_kind(&l) {
                Ok(kind) => kind.clone(),
                Err(e) => {
                    dropped.push((key, e));
                    continue;
                }
            };
            let mut kept: Vec<Membership> = Vec::new();
            for m in l.members {
                let fits = onto
                    .check_member(&kind, &m)
                    .and_then(|()| unique(&kept, &m))
                    .and_then(|()| match kind.per_session {
                        PerSession::AtMost(max) if kept.len() >= max as usize => {
                            Err(Refusal::TooMany {
                                kind: kind.name.clone(),
                                max,
                                got: kept.len() + 1,
                            })
                        }
                        _ => Ok(()),
                    });
                match fits {
                    Ok(()) => kept.push(m),
                    Err(e) => dropped.push((format!("{key} ({})", m.category), e)),
                }
            }
            l.members = kept;
            onto.members.insert((l.session.clone(), l.kind.clone()), l);
        }
        debug_assert_eq!(onto.check_all(), Ok(()));
        (onto, dropped)
    }

    /// A fresh id for a category of `kind` named `name`: the name as a slug
    /// (lowercase ASCII letters and digits, other runs as `-`, at most 48),
    /// then `-2`, `-3`, … while one is taken. A name with no ASCII letter or
    /// digit is `category`.
    pub fn mint_id(&self, kind: &str, name: &str) -> Result<CategoryId, Refusal> {
        let kind = self.kind_of(kind)?;
        let mut slug = String::new();
        for ch in name.chars().flat_map(char::to_lowercase) {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        slug.truncate(48);
        let base = match slug.trim_matches('-') {
            "" => "category",
            s => s,
        };
        let mut id = CategoryId::new(&kind.name, base)?;
        let mut n = 2;
        while self.categories.contains_key(&id) {
            id = CategoryId::new(&kind.name, &format!("{base}-{n}"))?;
            n += 1;
        }
        Ok(id)
    }

    fn apply(&mut self, record: Record) {
        match record {
            Record::Kind(k) => {
                self.kinds.insert(k.name.clone(), k);
            }
            Record::Category(c) => self.insert_category(c),
            Record::Guidance(g) => {
                self.guidance.insert(g.category.clone(), g);
            }
            Record::Members(l) => {
                self.members.insert((l.session.clone(), l.kind.clone()), l);
            }
        }
    }

    /// Hold `c`, in place of the one it supersedes, and keep the indexes.
    fn insert_category(&mut self, c: Category) {
        if let Some(old) = self.categories.remove(&c.id) {
            unindex(&mut self.children, &old.parent, &old.id);
            unindex(&mut self.names, &name_key(&old), &old.id);
        }
        self.children
            .entry(c.parent.clone())
            .or_default()
            .insert(c.id.clone());
        self.names
            .entry(name_key(&c))
            .or_default()
            .insert(c.id.clone());
        self.categories.insert(c.id.clone(), c);
    }

    /// Every rule over the whole snapshot.
    pub(crate) fn check_all(&self) -> Result<(), Refusal> {
        for k in self.kinds.values() {
            k.check_row()?;
        }
        check_table(&self.kinds)?;
        for c in self.categories.values() {
            self.check_category(c)?;
        }
        for g in self.guidance.values() {
            self.check_guidance(g)?;
        }
        for l in self.members.values() {
            self.check_members(l)?;
        }
        Ok(())
    }

    fn kind_of(&self, name: &str) -> Result<&Kind, Refusal> {
        self.kinds.get(name).ok_or_else(|| Refusal::Missing {
            what: "kind",
            id: name.to_string(),
        })
    }

    /// A category against the snapshot as it would be with `c` in it, in
    /// place of the one it supersedes: its kind is held, its parent is held
    /// and of the kind's parent kind, its parents do not lead back to it,
    /// it and what hangs below it nest at most `MAX_DEPTH` deep, and an
    /// interpreted one's name is its own among its siblings.
    fn check_category(&self, c: &Category) -> Result<(), Refusal> {
        c.check_fields()?;
        let kind = self.kind_of(c.kind())?;
        if let Some(p) = &c.parent {
            let Some(parent_kind) = kind.parent.as_deref() else {
                return Err(Refusal::WrongKind {
                    why: format!(
                        "`{}` categories do not nest: the kind has no parent kind, so `{}` \
                         cannot have a parent",
                        kind.name, c.id
                    ),
                });
            };
            if !self.categories.contains_key(p) {
                return Err(Refusal::Missing {
                    what: "category",
                    id: p.to_string(),
                });
            }
            if p.kind() != parent_kind {
                return Err(Refusal::WrongKind {
                    why: format!(
                        "`{}`'s parent must be a {parent_kind} category, and `{p}` is a {}",
                        c.id,
                        p.kind()
                    ),
                });
            }
        }
        // Up from the parent: the rest of the tree has no loop, so a loop
        // `c` makes must lead back to `c`.
        let mut up = vec![&c.id];
        let mut next = c.parent.as_ref();
        while let Some(id) = next {
            up.push(id);
            if *id == c.id {
                let path: Vec<&str> = up.iter().rev().map(|i| i.as_str()).collect();
                return Err(Refusal::Cycle {
                    path: path.join(" › "),
                });
            }
            if up.len() > MAX_DEPTH {
                break;
            }
            next = self.categories.get(id).and_then(|p| p.parent.as_ref());
        }
        let depth = up.len() + self.height_below(&c.id);
        if depth > MAX_DEPTH {
            return Err(Refusal::TooDeep {
                id: c.id.to_string(),
                depth,
                max: MAX_DEPTH,
            });
        }
        if kind.basis == Basis::Interpreted {
            let twin = self
                .names
                .get(&name_key(c))
                .into_iter()
                .flatten()
                .find(|id| **id != c.id);
            if let Some(twin) = twin {
                return Err(Refusal::Duplicate {
                    why: format!(
                        "`{twin}` is already named {:?}{}: two {} categories under one parent \
                         need two names",
                        self.categories[twin].name,
                        match &c.parent {
                            Some(p) => format!(" under `{p}`"),
                            None => String::new(),
                        },
                        kind.name
                    ),
                });
            }
        }
        Ok(())
    }

    /// How many levels of categories hang below `id` (0 for a leaf).
    fn height_below(&self, id: &CategoryId) -> usize {
        let mut level: Vec<&CategoryId> = vec![id];
        let mut height = 0;
        while height <= MAX_DEPTH {
            let next: Vec<&CategoryId> = level
                .iter()
                .flat_map(|i| self.children.get(&Some((*i).clone())).into_iter().flatten())
                .collect();
            if next.is_empty() {
                break;
            }
            height += 1;
            level = next;
        }
        height
    }

    /// Guidance against its category's kind: prose under `chain` (at most
    /// 16 KiB), one line under `intent_line` (at most 200 characters).
    fn check_guidance(&self, g: &Guidance) -> Result<(), Refusal> {
        g.check_fields()?;
        let c = self
            .categories
            .get(&g.category)
            .ok_or_else(|| Refusal::Missing {
                what: "category",
                id: g.category.to_string(),
            })?;
        if g.is_empty() {
            return Ok(());
        }
        let kind = self.kind_of(c.kind())?;
        let what = format!("the guidance of `{}`", g.category);
        match kind.rule {
            Rule::Chain => {
                text::prose(&what, &g.text, usize::MAX)?;
                if g.text.len() > text::GUIDANCE_MAX {
                    return Err(Refusal::invalid(
                        what,
                        format!(
                            "it is {} bytes, and a category's guidance is at most {} (16 KiB): \
                             longer reference belongs in a context file",
                            g.text.len(),
                            text::GUIDANCE_MAX
                        ),
                    ));
                }
                Ok(())
            }
            Rule::IntentLine => text::line(
                &format!(
                    "{what} (`{}` is `intent_line`: one line per category)",
                    kind.name
                ),
                &g.text,
                text::INTENT_LINE_MAX,
            ),
            Rule::Ranked | Rule::RecallOnly => Err(Refusal::UnbuiltRule {
                kind: kind.name.clone(),
                rule: kind.rule,
                comes_with: kind.rule.comes_with().unwrap_or("a later milestone"),
            }),
        }
    }

    /// A membership list's kind: held, and interpreted.
    fn list_kind(&self, l: &MemberList) -> Result<&Kind, Refusal> {
        l.check_fields()?;
        let kind = self.kind_of(&l.kind)?;
        if kind.is_given() {
            return Err(given_memberships(kind));
        }
        Ok(kind)
    }

    /// One entry of a list: of the list's kind, its category held, and from
    /// an origin the kind lets assign it.
    fn check_member(&self, kind: &Kind, m: &Membership) -> Result<(), Refusal> {
        if m.kind() != kind.name {
            return Err(Refusal::WrongKind {
                why: format!(
                    "`{}` is a {} category, and this is a {} list",
                    m.category,
                    m.kind(),
                    kind.name
                ),
            });
        }
        if !self.categories.contains_key(&m.category) {
            return Err(Refusal::Missing {
                what: "category",
                id: m.category.to_string(),
            });
        }
        m.origin.built()?;
        if !kind.assigned_by.contains(&m.origin) {
            let names: Vec<&str> = kind.assigned_by.iter().map(|o| o.name()).collect();
            return Err(Refusal::Writer {
                why: format!(
                    "`{}` may not assign `{}` memberships: the kind's assigned_by is {}",
                    m.origin,
                    kind.name,
                    names.join(", ")
                ),
            });
        }
        m.check_confidence()
    }

    fn check_members(&self, l: &MemberList) -> Result<(), Refusal> {
        let kind = self.list_kind(l)?;
        for (i, m) in l.members.iter().enumerate() {
            self.check_member(kind, m)?;
            unique(&l.members[..i], m)?;
        }
        if let PerSession::AtMost(max) = kind.per_session {
            if l.members.len() > max as usize {
                return Err(Refusal::TooMany {
                    kind: kind.name.clone(),
                    max,
                    got: l.members.len(),
                });
            }
        }
        Ok(())
    }
}

/// The table's rules across rows: a parent kind is held (or is the kind
/// itself, for nesting), the parents never loop, a parent kind comes first
/// (a lower precedence), and no two kinds share a precedence.
pub(crate) fn check_table(kinds: &BTreeMap<String, Kind>) -> Result<(), Refusal> {
    for k in kinds.values() {
        let Some(p) = k.parent.as_deref().filter(|p| *p != k.name) else {
            continue;
        };
        if !kinds.contains_key(p) {
            return Err(Refusal::Missing {
                what: "parent kind",
                id: p.to_string(),
            });
        }
        let mut path = vec![k.name.as_str()];
        let mut at = Some(p);
        while let Some(name) = at {
            path.push(name);
            if name == k.name {
                return Err(Refusal::Cycle {
                    path: path.join(" › "),
                });
            }
            if path.len() > kinds.len() + 1 {
                break;
            }
            at = kinds
                .get(name)
                .and_then(|n| n.parent.as_deref().filter(|q| *q != name));
        }
    }
    for k in kinds.values() {
        if let Some(other) = kinds
            .values()
            .find(|o| o.name != k.name && o.precedence == k.precedence)
        {
            let (a, b) = if k.name < other.name {
                (&k.name, &other.name)
            } else {
                (&other.name, &k.name)
            };
            return Err(Refusal::Precedence {
                why: format!(
                    "`{a}` and `{b}` both have precedence {}: each kind needs its own, so that \
                     clashes resolve the same way every time",
                    k.precedence
                ),
            });
        }
        if let Some(p) = k.parent.as_deref().filter(|p| *p != k.name) {
            let parent = &kinds[p];
            if parent.precedence >= k.precedence {
                return Err(Refusal::Precedence {
                    why: format!(
                        "`{}`'s parent kind `{p}` must come first: its precedence ({}) must be \
                         below `{}`'s ({})",
                        k.name, parent.precedence, k.name, k.precedence
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Take `id` out of an index, and the entry with it when it empties, so a
/// snapshot's indexes are the same however its categories got there.
fn unindex<K: Ord>(index: &mut BTreeMap<K, BTreeSet<CategoryId>>, key: &K, id: &CategoryId) {
    if let Some(set) = index.get_mut(key) {
        set.remove(id);
        if set.is_empty() {
            index.remove(key);
        }
    }
}

fn name_key(c: &Category) -> NameKey {
    (
        c.parent.clone(),
        c.kind().to_string(),
        c.name.to_lowercase(),
    )
}

fn given_memberships(kind: &Kind) -> Refusal {
    Refusal::Given {
        kind: kind.name.clone(),
        why: format!(
            "`{}` memberships are given: they come from the session's place at compile, are \
             never stored, and cannot be set",
            kind.name
        ),
    }
}

/// `m`'s category is not already among `before`.
fn unique(before: &[Membership], m: &Membership) -> Result<(), Refusal> {
    if before.iter().any(|o| o.category == m.category) {
        return Err(Refusal::Duplicate {
            why: format!("the list names `{}` twice", m.category),
        });
    }
    Ok(())
}

/// Try each pending item; keep the ones that fail, and go again while a pass
/// takes one.
fn passes<T>(pending: &mut Vec<T>, mut take: impl FnMut(&T) -> Result<(), Refusal>) {
    loop {
        let before = pending.len();
        pending.retain(|item| take(item).is_err());
        if pending.len() == before {
            return;
        }
    }
}

/// The loop `id` is on, among the categories that could not load, written
/// as a path is (the parent before the child).
fn stuck_loop(stuck: &BTreeMap<CategoryId, Category>, id: &CategoryId) -> Option<String> {
    let mut path = vec![id];
    let mut at = stuck.get(id)?.parent.as_ref();
    while let Some(p) = at {
        path.push(p);
        if p == id {
            let names: Vec<&str> = path.iter().rev().map(|i| i.as_str()).collect();
            return Some(names.join(" › "));
        }
        if path.len() > stuck.len() + 1 {
            return None;
        }
        at = stuck.get(p).and_then(|c| c.parent.as_ref());
    }
    None
}
