//! The ontology wired in (row 26, step 21b; M4 design §2.8; theseus-8kk.1):
//! its records in the store, the snapshot in memory, the given kinds from
//! the transport, and what a turn's compile walk reads.
//!
//! - **Records** are META keys under `onto:` (`theseus_ontology::keys`), each
//!   change in one frame with its ledger row (`fact/ontology.rs`).
//! - **The snapshot** ([`Board`]) is built by one META prefix scan after
//!   serving (`Core::warm_ontology`), or by its first reader if that comes
//!   sooner, and kept current in memory on every write: a write checks its
//!   records against the snapshot, writes them, and swaps the snapshot under
//!   one lock. Nothing on the start path reads it.
//! - **Given kinds** are the transport's: a bound place's category, and its
//!   guild's, are made at its first bind (`Core::bind_places`), and a
//!   session's given memberships are read from its place (and its channel's
//!   parent) at compile, never stored ([`given`]).
//! - **The compile walk** ([`Walk`]) is fixed for a turn, as its request's
//!   spec is: the snapshot as the turn began and the session's memberships
//!   then. The compiler composes the memberships its current manifest
//!   recorded while it appends, and the current ones when it recompiles, so
//!   a membership change waits for the next recompile while a guidance edit
//!   in play changes the system block, which is one `system_changed`.
//! - **The place rule comes first.** What a place is admitted (its tools, its
//!   context files) is decided by its class before the walk, and never reads
//!   a membership. A shared place's walk takes only its own place's given
//!   memberships: what the operator assigned a session is the owner's
//!   material, and a shared place never receives it.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use theseus_ontology::{
    keys, Category, CategoryId, Composition, Membership, MembershipUsed, Ontology, Origin, Record,
    Refusal,
};
use theseus_store::{kinds, NewRecord};

use crate::compiler::RequestSpec;
use crate::places::PlaceClass;
use crate::store::Store;

/// The snapshot, in memory, and the lock its writers take.
#[derive(Default)]
pub struct Board {
    snapshot: RwLock<Option<Arc<Ontology>>>,
    /// Held from a write's check to its swap, so two writes never check
    /// against the same snapshot; and by the one load.
    writes: Mutex<()>,
}

impl Board {
    /// The snapshot: memory once built; built now, by one META prefix scan,
    /// when nothing has built it yet.
    pub fn snapshot(&self, store: &Store) -> Result<Arc<Ontology>> {
        if let Some(s) = self.held() {
            return Ok(s);
        }
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            if let Some(s) = self.held() {
                return Ok(s);
            }
            let s = Arc::new(load(store)?);
            *self.snapshot.write().unwrap() = Some(s.clone());
            Ok(s)
        })
    }

    /// The snapshot if it is built: memory only.
    pub fn held(&self) -> Option<Arc<Ontology>> {
        self.snapshot.read().unwrap().clone()
    }

    /// Check `records` against the snapshot as `by`'s, each against the
    /// snapshot with the ones before it applied; then write them in one
    /// frame with `rows`, which `rows` makes from the snapshot as it was
    /// before; then hold the new snapshot. A refusal writes nothing. No
    /// records writes nothing either.
    pub fn write(
        &self,
        store: &Store,
        records: Vec<Record>,
        by: Origin,
        rows: impl FnOnce(&Ontology) -> Result<Vec<NewRecord>>,
    ) -> Result<Arc<Ontology>> {
        let now = self.snapshot(store)?;
        if records.is_empty() {
            return Ok(now);
        }
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            let now = self.held().unwrap_or(now);
            let mut next = (*now).clone();
            let mut frame = Vec::with_capacity(records.len() * 2);
            for r in &records {
                next.put(r.clone(), by).map_err(anyhow::Error::new)?;
                frame.push(NewRecord::json(kinds::META, Some(&r.key()), &r.to_value())?);
            }
            frame.extend(rows(&now)?);
            store.append(&frame)?;
            let next = Arc::new(next);
            *self.snapshot.write().unwrap() = Some(next.clone());
            Ok(next)
        })
    }

    /// A write checked and framed, not yet appended: the caller's own frame
    /// carries [`Staged::frame`] beside its other records (the sink writes a
    /// judgment's filing in the judgment's frame), then [`Staged::commit`]s
    /// the snapshot once the frame is written; dropped, nothing changes.
    /// `make` gives the records and their rows from the snapshot as it
    /// stands under the writers' lock (`None` when it gives neither), and a
    /// record that does not check is a refusal that stages nothing. The lock
    /// is held until the [`Staged`] is gone, so no other write checks against
    /// the snapshot meanwhile.
    pub fn stage<'a>(
        &'a self,
        store: &Store,
        by: Origin,
        make: impl FnOnce(&Ontology) -> Result<(Vec<Record>, Vec<NewRecord>)>,
    ) -> Result<Option<Staged<'a>>> {
        let now = self.snapshot(store)?;
        let guard = self.writes.lock().unwrap();
        let now = self.held().unwrap_or(now);
        let (records, rows) = make(&now)?;
        if records.is_empty() && rows.is_empty() {
            return Ok(None);
        }
        let mut next = (*now).clone();
        let mut frame = Vec::with_capacity(records.len() * 2 + rows.len());
        for r in &records {
            next.put(r.clone(), by).map_err(anyhow::Error::new)?;
            frame.push(NewRecord::json(kinds::META, Some(&r.key()), &r.to_value())?);
        }
        frame.extend(rows);
        Ok(Some(Staged {
            _guard: guard,
            board: self,
            next,
            frame,
        }))
    }

    /// Forget the snapshot, so the next reader builds it from the store
    /// again (tests: what a restart would build).
    #[cfg(test)]
    pub(crate) fn forget(&self) {
        *self.snapshot.write().unwrap() = None;
    }
}

/// A write [`Board::stage`] made: its frame, and the snapshot it commits.
pub struct Staged<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    board: &'a Board,
    next: Ontology,
    frame: Vec<NewRecord>,
}

impl Staged<'_> {
    /// The records and rows to append.
    pub fn frame(&self) -> &[NewRecord] {
        &self.frame
    }

    /// The frame was written: hold the new snapshot.
    pub fn commit(self) {
        *self.board.snapshot.write().unwrap() = Some(Arc::new(self.next));
    }
}

/// The snapshot the store's `onto:*` records make: the seed rows, and every
/// record that checks. One that does not is left out and warned of, and the
/// rest serves.
pub fn load(store: &Store) -> Result<Ontology> {
    let mut records = Vec::new();
    use theseus_store::Store as _;
    for r in store
        .inner()
        .latest_with_prefix(kinds::META, keys::PREFIX)?
    {
        let key = r.key.clone().unwrap_or_default();
        let decoded = r
            .decode::<serde_json::Value>()
            .map_err(|e| Refusal::Invalid {
                what: format!("`{key}`"),
                why: e.to_string(),
            })
            .and_then(|v| Record::decode(&key, v));
        match decoded {
            Ok(rec) => records.push(rec),
            Err(e) => tracing::warn!(key, error = %e, "an ontology record does not read: left out"),
        }
    }
    let (onto, dropped) = Ontology::load(records);
    for (key, why) in dropped {
        tracing::warn!(key, error = %why, "an ontology record does not check: left out");
    }
    Ok(onto)
}

/// The given category a place is (the transport's fact): a guild channel's
/// channel, a DM's person. `None` for the CLI and the web UI, and for a
/// place no given kind reads.
pub fn place_category(place: &str) -> Option<CategoryId> {
    if let Some(id) = place.strip_prefix("discord:channel:") {
        return CategoryId::new("channel", id).ok();
    }
    if let Some(user) = place.strip_prefix("discord:dm:") {
        return CategoryId::new("person", user).ok();
    }
    None
}

/// A session's given memberships, read from its place now (origin
/// `transport`): never stored. A guild channel gives its guild too, read
/// through the channel category's parent in `snapshot`.
pub fn given(place: Option<&str>, snapshot: &Ontology, now_ms: u64) -> Vec<Membership> {
    let Some(channel) = place.and_then(place_category) else {
        return Vec::new();
    };
    let guild = snapshot.category(&channel).and_then(|c| c.parent.clone());
    guild
        .into_iter()
        .chain([channel])
        .map(|c| Membership::given(c, now_ms))
        .collect()
}

/// The category a bound guild makes at its first bind: named by its id, as
/// the binding does not carry the guild's name to the core.
pub fn guild_category(guild: &str) -> Option<Category> {
    let id = CategoryId::new("guild", guild).ok()?;
    Some(Category::new(
        id.clone(),
        id.local(),
        Origin::Transport.name(),
    ))
}

/// The category a bound place makes at its first bind, named as the
/// binding names it: `#lab` is `lab`, `DM @wren` is `@wren`. A guild
/// channel's parent is its guild (`guild`, the guild's id).
pub fn bound_category(target: &str, name: &str, guild: Option<&str>) -> Option<Category> {
    let id = place_category(target)?;
    let parent = guild
        .filter(|_| id.kind() == "channel")
        .and_then(|g| CategoryId::new("guild", g).ok());
    let name = name
        .strip_prefix("DM ")
        .unwrap_or(name)
        .trim_start_matches('#')
        .trim();
    let name: String = name.chars().filter(|c| !c.is_control()).take(100).collect();
    Some(Category {
        name: if name.is_empty() {
            id.local().to_string()
        } else {
            name
        },
        id,
        parent,
        description: String::new(),
        added_by: Origin::Transport.name().to_string(),
        retired_ms: None,
        handles: Vec::new(),
        merged_into: None,
    })
}

/// What a turn's compile walk reads, fixed for the turn.
#[derive(Debug)]
pub struct Walk {
    pub snapshot: Arc<Ontology>,
    /// The memberships a recompile takes: the given ones from the place, and
    /// in a private place the interpreted ones the snapshot holds.
    pub current: Vec<Membership>,
    /// The place's class: a shared place composes its given memberships
    /// alone.
    pub class: PlaceClass,
}

impl Walk {
    /// A thread's topics as the operator's memberships of its session, for
    /// the topics the ontology holds (theseus-h4a7): the guidance of the
    /// third layer is the thread's own.
    pub fn with_topics(mut self, topics: &[String], now_ms: u64) -> Self {
        for topic in topics {
            let Ok(id) = CategoryId::parse(topic).or_else(|_| CategoryId::new("topic", topic))
            else {
                continue;
            };
            if self.snapshot.category(&id).is_some()
                && !self.current.iter().any(|m| m.category == id)
            {
                self.current.push(Membership {
                    category: id,
                    origin: Origin::Operator,
                    confidence: None,
                    as_of_ms: now_ms,
                });
            }
        }
        self
    }

    /// The walk for a session in `place`, of `class`, over `snapshot`; none
    /// while the ontology holds no category, so a store without one renders
    /// no differently than before it.
    pub fn of(
        snapshot: Arc<Ontology>,
        session: &str,
        place: Option<&str>,
        class: PlaceClass,
        now_ms: u64,
    ) -> Option<Walk> {
        snapshot.categories().next()?;
        let mut current = given(place, &snapshot, now_ms);
        if class == PlaceClass::Private {
            // A stored person the place gives too is taken once, as given.
            for m in snapshot.memberships(session) {
                if !current.iter().any(|g| g.category == m.category) {
                    current.push(m);
                }
            }
        }
        Some(Walk {
            snapshot,
            current,
            class,
        })
    }

    /// Compose `memberships`: the place rule's filter first (a shared place
    /// takes only the transport's), then the snapshot's composition.
    pub fn compose(&self, memberships: &[Membership]) -> Composition {
        match self.class {
            PlaceClass::Private => self.snapshot.compose(memberships),
            PlaceClass::Shared => {
                let given: Vec<Membership> = memberships
                    .iter()
                    .filter(|m| m.origin == Origin::Transport)
                    .cloned()
                    .collect();
                self.snapshot.compose(&given)
            }
        }
    }
}

/// The memberships a manifest recorded, as the walk takes them again.
pub fn recorded(used: &[MembershipUsed]) -> Vec<Membership> {
    used.iter()
        .map(|m| Membership {
            category: m.category.clone(),
            origin: m.origin,
            confidence: m.confidence,
            as_of_ms: m.as_of_ms,
        })
        .collect()
}

/// `spec` with a composition: its guidance after the context files in the
/// system's second block, a blank line between, and what it used for the
/// manifest. The same spec when the composition used nothing.
pub fn guided(spec: &RequestSpec, c: Composition) -> Cow<'_, RequestSpec> {
    if c.memberships.is_empty() && c.sections.is_empty() {
        return Cow::Borrowed(spec);
    }
    let mut s = spec.clone();
    let text = c.render();
    if !text.is_empty() {
        s.context_text = match s.context_text.is_empty() {
            true => text,
            false => format!("{}\n\n{text}", s.context_text),
        };
    }
    s.memberships = c.memberships;
    s.guidance = c.guidance;
    Cow::Owned(s)
}

impl crate::turn::TurnRunner {
    /// A turn's compile walk: the snapshot now, and the session's
    /// memberships now, from its place and the snapshot; none while the
    /// ontology holds no category, or when the snapshot cannot be built
    /// (warned: the turn compiles without guidance).
    pub fn walk(&self, session: &str, class: PlaceClass) -> Option<Arc<Walk>> {
        let snapshot = match self.ontology.snapshot(&self.store) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(session, error = %format!("{e:#}"),
                    "the ontology cannot be read: this turn compiles without guidance");
                return None;
            }
        };
        let place = self.target_of(session);
        Walk::of(
            snapshot,
            session,
            place.as_deref(),
            class,
            theseus_protocol::now_unix_ms(),
        )
        .map(|w| match self.thread_topics(session) {
            Some(topics) => w.with_topics(&topics, theseus_protocol::now_unix_ms()),
            None => w,
        })
        .map(Arc::new)
    }

    /// Where a session speaks, as `class_of` reads it: its place's target,
    /// or where its wakes and reports answer; none for the CLI and the web
    /// UI, or when it cannot be read. (Recall's `place_of` is the same
    /// target with the place rule's class.)
    pub fn target_of(&self, session: &str) -> Option<String> {
        match self.outbox.try_target(session) {
            Ok(Some(t)) => Some(t),
            Ok(None) => self.outbox.try_wake_target(session).ok().flatten(),
            Err(_) => None,
        }
    }
}
