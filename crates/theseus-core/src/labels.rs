//! Confidentiality labels (M4 19a; design m4-boundaries §2.5 and §2.7).
//!
//! - **At write.** Each node written from 19a on carries a label, set by
//!   whoever writes it, in the frame that writes it, and never rewritten
//!   (`for_input`, `for_result`, `for_harness`, and the model's own, which
//!   takes the meet of what its compile admitted). A node written before 19a
//!   has none: it reads as its origin says, and is disclosable only within
//!   its own session, so every old session compiles as it did.
//! - **A session's audience** comes from its place (`outbox.target`): none
//!   (the CLI, the web UI, a task that reports nowhere) is the owner alone; a
//!   Discord DM with `u` is `u`; a guild channel is whoever can view it, which
//!   the binding reads and pushes here (`Places`). Without the Server Members
//!   intent nobody's view can be read, and the channel counts as public.
//! - **At compile** (`Judge`), a node is admitted only when its readers cover
//!   the audience: every member of the audience is a reader or the owner. A
//!   node labeled for a place always covers that place's own audience, since
//!   what was said there may be said there again. A withheld node keeps its
//!   place in the request as a placeholder (the compiler's), so every
//!   `tool_use` keeps its result.
//! - **The audience is part of the compilation.** Its manifest records the
//!   audience it was compiled for; a compile for another audience recompiles
//!   (`audience`), so a request never changes what it carries under the
//!   model's thinking. Each place's viewers are kept in a META record, so a
//!   restart keeps every session's audience until the binding reads again.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use theseus_protocol::{Audience, InPlay, Integrity, Label, Readers};

use crate::node::{Body, Node};

/// The META key prefix of a place's viewers: `labels.place.discord:<id>`.
pub const PLACE_PREFIX: &str = "labels.place.";

/// `[labels]` (M4 19a, design m4-boundaries §2.5): every node from 19a on
/// carries a label, and a compile admits a node only when its readers cover
/// the session's audience (`labels.rs`). The owner may read everything.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelsConfig {
    /// The owner on Discord, as `discord:<user id>`, beside the local
    /// surfaces (the CLI and the web UI). Absent: `[approval] trusted_users`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Vec<String>>,
    /// Trees anyone may read (a public repository), `~/` or absolute: an
    /// `fs.*`, `git.*`, or `text.*` result whose every path is inside one is
    /// public. Default none: such results are the owner's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_paths: Vec<String>,
}

impl LabelsConfig {
    pub fn is_empty(&self) -> bool {
        self.owner.is_none() && self.public_paths.is_empty()
    }
}

impl crate::config::Config {
    /// The owner on Discord (M4 19a): `[labels] owner`, else `[approval]
    /// trusted_users`, as `discord:<user id>`. The local surfaces are the
    /// owner's too, and need no entry.
    pub fn owners(&self) -> std::collections::BTreeSet<String> {
        match &self.labels.owner {
            Some(o) => o.iter().cloned().collect(),
            None => self
                .approval
                .as_ref()
                .map(|a| a.trusted_users.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// The owner, for a session posting to `place` (`outbox.target`): the
    /// owners, and, with neither `[labels] owner` nor `[approval]`, the
    /// person of a DM the bindings file binds, whom approval takes for the
    /// owner then too (review 2's consideration 2): the binding lets nobody
    /// else reach it.
    pub fn owners_for(&self, place: Option<&str>) -> std::collections::BTreeSet<String> {
        let mut owners = self.owners();
        if self.labels.owner.is_none() && self.approval.is_none() {
            if let Some(u) = place.and_then(|p| p.strip_prefix("discord:dm:")) {
                owners.insert(format!("discord:{u}"));
            }
        }
        owners
    }

    /// `[labels]`: the owner's ids as `[approval] trusted_users` writes them,
    /// and the public trees as paths.
    pub(crate) fn validate_labels(&self) -> anyhow::Result<()> {
        for o in self.labels.owner.iter().flatten() {
            if crate::approval::parse_user(o).is_err() {
                anyhow::bail!(
                    "labels.owner entry {o:?} is not a surface-qualified id: write \"discord:<user id>\""
                );
            }
        }
        if let Some(p) = self
            .labels
            .public_paths
            .iter()
            .find(|p| !(p.starts_with('/') || p.starts_with("~/")))
        {
            anyhow::bail!(
                "labels.public_paths entry {p:?} must be an absolute path or start with ~/"
            );
        }
        Ok(())
    }
}

/// Who can view a guild channel, as the binding found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceViewers {
    /// The channel's name, as the binding knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Every member who can view it, the bot aside, as `discord:<user id>`.
    /// None: they cannot be read (the Server Members intent is off), and the
    /// channel counts as public.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewers: Option<BTreeSet<String>>,
}

/// The places' viewers (M4 19a): what the bindings last read, kept in META
/// so a restart keeps each session's audience. Read lazily, a place at a
/// time, so a start reads nothing (FAST).
#[derive(Default)]
pub struct Places {
    seen: Mutex<HashMap<String, Option<PlaceViewers>>>,
}

impl Places {
    /// A place's viewers: from memory, else its META record, read once.
    pub fn get(&self, store: &crate::store::Store, place: &str) -> Option<PlaceViewers> {
        if let Some(v) = self.seen.lock().unwrap().get(place) {
            return v.clone();
        }
        let read = store
            .get_meta::<PlaceViewers>(&format!("{PLACE_PREFIX}{place}"))
            .unwrap_or_else(|e| {
                tracing::warn!(place, error = %format!("{e:#}"), "a place's viewers are unreadable: it counts as public");
                None
            });
        self.seen
            .lock()
            .unwrap()
            .entry(place.to_string())
            .or_insert(read)
            .clone()
    }

    /// The binding read who can view `place`. Kept, and written to META,
    /// only when it changed; true then.
    pub fn set(
        &self,
        store: &crate::store::Store,
        place: &str,
        now: PlaceViewers,
    ) -> anyhow::Result<bool> {
        if self.get(store, place).as_ref() == Some(&now) {
            return Ok(false);
        }
        store.put_meta(&format!("{PLACE_PREFIX}{place}"), &now)?;
        self.seen
            .lock()
            .unwrap()
            .insert(place.to_string(), Some(now));
        Ok(true)
    }

    /// Health's block: the owner, counted, and each place read so far, with
    /// how many of its viewers are not the owner. Reads nothing from the
    /// store: a place no compile or binding has read yet is not listed.
    pub fn health(&self, owners: &BTreeSet<String>) -> theseus_protocol::LabelsHealth {
        let mut places: Vec<theseus_protocol::PlaceAudience> = self
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(place, read)| {
                let read = read.as_ref()?;
                let viewers = read.viewers.as_ref();
                Some(theseus_protocol::PlaceAudience {
                    place: place.clone(),
                    name: read.name.clone(),
                    viewers: viewers.map(|v| v.len() as u32),
                    others: viewers
                        .map(|v| v.iter().filter(|u| !owners.contains(*u)).count() as u32),
                })
            })
            .collect();
        places.sort_by(|a, b| a.place.cmp(&b.place));
        theseus_protocol::LabelsHealth {
            owners: owners.len() as u32,
            places,
            held: None,
        }
    }

    /// Every place read so far whose viewers are known, for nodes labeled
    /// for another place than the session's.
    fn known(&self) -> HashMap<String, BTreeSet<String>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(p, v)| Some((p.clone(), v.as_ref()?.viewers.clone()?)))
            .collect()
    }
}

/// The place of a session's target, as readers name it: `discord:<channel
/// id>` for `discord:channel:<id>`.
pub fn place_of(target: &str) -> Option<String> {
    target
        .strip_prefix("discord:channel:")
        .map(|c| format!("discord:{c}"))
}

/// The readers of what is said in a session's place: the owner's with no
/// place, a DM's person's, a channel's.
pub fn readers_of(target: Option<&str>) -> Readers {
    match target {
        None => Readers::Owner,
        Some(t) => {
            if let Some(u) = t.strip_prefix("discord:dm:") {
                Readers::People(BTreeSet::from([format!("discord:{u}")]))
            } else if let Some(p) = place_of(t) {
                Readers::Place(p)
            } else {
                Readers::Owner
            }
        }
    }
}

/// The first 16 hex digits of the SHA-256 of a viewer set, sorted.
fn digest(viewers: &BTreeSet<String>) -> String {
    use sha2::{Digest, Sha256};
    let joined: Vec<&str> = viewers.iter().map(String::as_str).collect();
    hex::encode(Sha256::digest(joined.join("\n").as_bytes()))[..16].to_string()
}

/// Who the audience is, when that can be said.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Members {
    /// The owner alone: everything covers it.
    Owner,
    These(BTreeSet<String>),
    /// Anyone (a channel whose viewers cannot be read): only public covers it.
    Anyone,
}

/// Whether a compile admits a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Admitted, with the readers it adds to the meet.
    Admit(&'a Readers),
    /// Left out, with its readers in words (`owner-only`).
    Withhold(String),
}

/// A compile's view of who may see what (M4 19a): the session's audience,
/// the owner, and the viewers of the places it knows.
#[derive(Debug, Clone)]
pub struct Judge {
    pub audience: Audience,
    members: Members,
    owner: BTreeSet<String>,
    /// The session's own place, whose words always cover its audience.
    place: Option<String>,
    /// The viewers of every place read so far.
    places: HashMap<String, BTreeSet<String>>,
    /// The session holds external text (T1's latch).
    pub latched: bool,
    /// The audience as readers (`own_readers`), kept: a node from before
    /// labels adds it to the meet, a thousand times in a long session.
    own: Readers,
}

impl Judge {
    /// The judge for a session posting to `target` (`outbox.target`).
    pub fn new(
        target: Option<&str>,
        owner: BTreeSet<String>,
        places: &Places,
        store: &crate::store::Store,
        latched: bool,
    ) -> Self {
        let place = target.and_then(place_of);
        let (audience, members) = match (target, &place) {
            (None, _) => (Audience::Owner, Members::Owner),
            (Some(_), Some(p)) => {
                let read = places.get(store, p);
                let name = read.as_ref().and_then(|r| r.name.clone());
                match read.and_then(|r| r.viewers) {
                    Some(v) => (
                        Audience::Place {
                            place: p.clone(),
                            name,
                            viewers: Some(v.len() as u32),
                            digest: Some(digest(&v)),
                        },
                        Members::These(v),
                    ),
                    None => (
                        Audience::Place {
                            place: p.clone(),
                            name,
                            viewers: None,
                            digest: None,
                        },
                        Members::Anyone,
                    ),
                }
            }
            (Some(t), None) => match readers_of(Some(t)) {
                Readers::People(people) => (
                    Audience::People {
                        people: people.clone(),
                    },
                    Members::These(people),
                ),
                // A place no binding names: nobody can say who sees it.
                _ => (
                    Audience::Place {
                        place: t.to_string(),
                        name: None,
                        viewers: None,
                        digest: None,
                    },
                    Members::Anyone,
                ),
            },
        };
        let mut j = Self {
            audience,
            members,
            owner,
            place,
            places: places.known(),
            latched,
            own: Readers::Owner,
        };
        j.own = j.own_readers();
        j
    }

    /// A judge for the owner alone, whom everything covers (tests, and a
    /// compile with no place).
    pub fn owner_only() -> Self {
        Self {
            audience: Audience::Owner,
            members: Members::Owner,
            owner: BTreeSet::new(),
            place: None,
            places: HashMap::new(),
            latched: false,
            own: Readers::Owner,
        }
    }

    /// The same judge, with its place's audience counted as public: who views
    /// it could not be read just now (M4 19c; §5, question 6).
    pub fn counted_public(mut self) -> Self {
        if let Audience::Place {
            viewers, digest, ..
        } = &mut self.audience
        {
            *viewers = None;
            *digest = None;
            self.members = Members::Anyone;
        }
        self
    }

    /// The audience as readers: what the harness says in the session, and
    /// what a node from before labels may be read by.
    pub fn own_readers(&self) -> Readers {
        match &self.audience {
            Audience::Owner => Readers::Owner,
            Audience::People { people } => Readers::People(people.clone()),
            Audience::Place { place, .. } => Readers::Place(place.clone()),
        }
    }

    /// `covers(readers, audience)` (§2.5): every member of the audience is a
    /// reader or the owner. A node labeled for the session's own place always
    /// covers it.
    pub fn covers(&self, r: &Readers) -> bool {
        match (r, &self.members) {
            (Readers::Public, _) | (_, Members::Owner) => true,
            (Readers::Place(p), _) if self.place.as_ref() == Some(p) => true,
            (_, Members::Anyone) => false,
            (r, Members::These(audience)) => {
                let empty = BTreeSet::new();
                let readers = match r {
                    Readers::People(s) => s,
                    Readers::Place(p) => self.places.get(p).unwrap_or(&empty),
                    _ => &empty,
                };
                audience
                    .iter()
                    .all(|m| self.owner.contains(m) || readers.contains(m))
            }
        }
    }

    /// Whether a node of this session's goes into its request. A node with
    /// no label is from before 19a, and is read only in its own session,
    /// where it was said to this audience.
    pub fn verdict<'a>(&'a self, n: &'a Node) -> Verdict<'a> {
        match &n.label {
            None => Verdict::Admit(&self.own),
            Some(l) if self.covers(&l.readers) => Verdict::Admit(&l.readers),
            Some(l) => Verdict::Withhold(l.readers.describe()),
        }
    }

    /// The meet of two readers (§2.5): who may read what draws on both.
    /// Conservative wherever it cannot be computed.
    pub fn meet(&self, a: &Readers, b: &Readers) -> Readers {
        fn people(s: BTreeSet<String>) -> Readers {
            if s.is_empty() {
                Readers::Owner
            } else {
                Readers::People(s)
            }
        }
        match (a, b) {
            (Readers::Public, x) | (x, Readers::Public) => x.clone(),
            (Readers::Owner, _) | (_, Readers::Owner) => Readers::Owner,
            (Readers::People(s), Readers::People(t)) => {
                people(s.intersection(t).cloned().collect())
            }
            (Readers::Place(x), Readers::Place(y)) if x == y => Readers::Place(x.clone()),
            (Readers::Place(_), Readers::Place(_)) => Readers::Owner,
            (Readers::Place(p), Readers::People(s)) | (Readers::People(s), Readers::Place(p)) => {
                match self.places.get(p) {
                    Some(v) => people(s.intersection(v).cloned().collect()),
                    None => Readers::Owner,
                }
            }
        }
    }
}

/// What a compile admitted and left out, for its manifest and for the label
/// of what the model writes next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    /// The meet of the admitted nodes' readers, and of the context files the
    /// request carries whole (theseus-42ub); `Public` when none.
    pub readers: Readers,
    pub in_play: InPlay,
    /// Each node left out, in order, with why.
    pub withheld: Vec<theseus_protocol::Withheld>,
}

impl Admitted {
    pub fn new(latched: bool) -> Self {
        Self {
            readers: Readers::Public,
            in_play: InPlay {
                latched,
                untrusted: 0,
            },
            withheld: Vec::new(),
        }
    }

    /// Take in a node's verdict.
    pub fn add(&mut self, judge: &Judge, n: &Node, v: &Verdict) {
        match v {
            Verdict::Admit(r) => {
                self.meet(judge, r);
                if untrusted(n) {
                    self.in_play.untrusted += 1;
                }
            }
            Verdict::Withhold(why) => self.withheld.push(theseus_protocol::Withheld {
                node_id: n.id.clone(),
                reason: why.clone(),
            }),
        }
    }

    /// Take in the readers of something admitted beside the nodes: a context
    /// file the system block carries whole (theseus-42ub).
    pub fn meet(&mut self, judge: &Judge, r: &Readers) {
        // The meet of equal readers is themselves: most of a session's nodes
        // share them, so nothing is built for those.
        if self.readers != *r {
            self.readers = judge.meet(&self.readers, r);
        }
    }
}

/// The readers of each context file a request's system block carries whole
/// (M4 19a, theseus-42ub): the owner's unless its entry says it is public. A
/// file withheld from the audience, or missing, carries none of its text.
pub fn carried_files(
    files: &[theseus_protocol::ContextFileRef],
) -> impl Iterator<Item = Readers> + '_ {
    files
        .iter()
        .filter(|f| f.withheld.is_none() && f.missing.is_none())
        .map(|f| f.readers.clone().unwrap_or(Readers::Owner))
}

/// A node is untrusted by its label, or, written before labels, by DD5's
/// `external` marker on a result (§2.5).
pub fn untrusted(n: &Node) -> bool {
    match &n.label {
        Some(l) => l.integrity == Integrity::Untrusted,
        None => matches!(
            &n.body,
            Body::ToolResult {
                external: Some(_),
                ..
            }
        ),
    }
}

/// An operator's message (§2.5's table): from the CLI or the web UI, the
/// owner's; through Discord, its place's (a DM's person, a guild channel).
pub fn for_input(discord: bool, target: Option<&str>) -> Label {
    Label::trusted(if discord {
        readers_of(target)
    } else {
        Readers::Owner
    })
}

/// A tool's result (§2.5's table): a fetched page or a search is untrusted
/// and public; a file, a diff, or a text tool's result is the owner's unless
/// every path it read is inside a public tree; a `proc.run` whose job
/// connected out of L1 (18c) is untrusted, `via: egress`, and the owner's;
/// anything else (`proc.run` at L0, and in L1 with no egress used, the
/// harness's tools, AWS) is the owner's.
pub fn for_result(
    tool: &str,
    node_id: &str,
    external: Option<&theseus_tools::External>,
    query: Option<&str>,
    paths: &[PathBuf],
    public: &[PathBuf],
) -> Label {
    if let Some(e) = external {
        let source = crate::external::read(
            node_id,
            tool,
            &e.url,
            query,
            theseus_protocol::now_unix_ms(),
        );
        let readers = match tool {
            crate::sandbox::PROC_RUN => Readers::Owner,
            _ => Readers::Public,
        };
        return Label::untrusted(source, readers);
    }
    let inside = |p: &Path| public.iter().any(|root| p.starts_with(root));
    if reads_files(tool) && !paths.is_empty() && paths.iter().all(|p| inside(p)) {
        Label::trusted(Readers::Public)
    } else {
        Label::trusted(Readers::Owner)
    }
}

/// The tools whose results are the files they read (§2.5's table).
fn reads_files(tool: &str) -> bool {
    ["fs.", "git.", "text."].iter().any(|p| tool.starts_with(p))
}

/// The paths a files tool's call reads, by its own plan, for `[labels]
/// public_paths`: none for any other tool, or with no public tree, so no
/// call is planned twice for nothing.
pub fn paths_of(
    tool: &dyn theseus_tools::Tool,
    input: &serde_json::Value,
    ctx: &theseus_tools::ToolCtx,
    public: &[PathBuf],
) -> Vec<PathBuf> {
    if public.is_empty() || !reads_files(tool.name()) {
        return Vec::new();
    }
    tool.plan(input, ctx)
        .map(|p| p.resources.into_iter().map(|r| r.path).collect())
        .unwrap_or_default()
}

/// The harness's own lines in a session (a wake, a notice): what its
/// audience may read.
pub fn for_harness(target: Option<&str>) -> Label {
    Label::trusted(readers_of(target))
}

/// A node relayed from another session (§2.5's table: a task's brief in the
/// task, a task's report in its parent): untrusted when the session it came
/// from held external text (T1's transmission, `held`), and read by
/// `readers`, the meet of the context it came from.
pub fn relayed(held: Option<theseus_protocol::ExternalText>, readers: Readers) -> Label {
    match held {
        Some(h) => Label::untrusted(h, readers),
        None => Label::trusted(readers),
    }
}

/// What the model writes (§2.5): trusted, since its origin is the agent and
/// never by exposure, read by the meet of what its compile admitted.
pub fn for_agent(readers: Readers) -> Label {
    Label::trusted(readers)
}

/// `label.graduate`'s `to`, parsed (M4 19c): `public`, `place` (whoever can
/// view the session's place: a guild channel's viewers, or a DM's person), or
/// `people:<id>[,<id>…]`, each id `discord:<user id>` or the bare number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraduateTo {
    Public,
    Place,
    People(BTreeSet<String>),
}

pub fn parse_graduate_to(to: &str) -> anyhow::Result<GraduateTo> {
    match to.trim() {
        "public" => Ok(GraduateTo::Public),
        "place" => Ok(GraduateTo::Place),
        t => {
            let Some(ids) = t.strip_prefix("people:") else {
                anyhow::bail!("`--to {t}` is none of public, place, or people:<ids>");
            };
            let mut people = BTreeSet::new();
            for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let n = id.strip_prefix("discord:").unwrap_or(id);
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    anyhow::bail!(
                        "`{id}` is not a Discord user id: write discord:<user id>, or the number"
                    );
                }
                people.insert(format!("discord:{n}"));
            }
            if people.is_empty() {
                anyhow::bail!("`people:` names nobody: write people:<id>[,<id>…]");
            }
            Ok(GraduateTo::People(people))
        }
    }
}

/// The label of a node graduated from `source` (M4 19c, §2.5's table): the
/// source's integrity, never touched, the wider readers, and the warrant.
/// `held` is the source's external text when it has no label of its own but
/// is untrusted by DD5's marker.
pub fn graduated(
    source: Option<&Label>,
    held: Option<theseus_protocol::ExternalText>,
    readers: Readers,
    warrant: theseus_protocol::Warrant,
) -> Label {
    let mut l = match (source, held) {
        (Some(s), _) => Label {
            readers,
            warrant: None,
            ..s.clone()
        },
        (None, Some(h)) => Label::untrusted(h, readers),
        (None, None) => Label::trusted(readers),
    };
    l.warrant = Some(warrant);
    l
}

/// Whether what `readers` may read may leave for `now`'s audience, as it is
/// when it leaves (M4 19c, §2.7; decision 14: what may not waits for the
/// owner, and is never refused). The outbox asks it before a post; MCP
/// responses and posts to another place will ask it the same way.
pub fn may_leave(readers: &Readers, now: &Judge) -> bool {
    now.covers(readers)
}

/// Readers that may leave for any audience their place can have: the public,
/// and what was said in the place itself. A post with these needs no read of
/// who views its place.
pub fn fits_any_audience(readers: &Readers, place: Option<&str>) -> bool {
    match readers {
        Readers::Public => true,
        Readers::Place(p) => place.is_some_and(|own| own == p),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    /// The template's `[labels]` block and its persona's files, uncommented,
    /// are real: the owner, the public tree, and a file's table with its
    /// readers parse and validate.
    #[test]
    fn the_templates_labels_lines_are_real() {
        use crate::context_files::ContextReaders;
        let t = Config::EXAMPLE_TOML;
        let block = |head: &str| -> String {
            let from = t.find(head).unwrap();
            t[from..]
                .lines()
                .take_while(|l| l.starts_with("# "))
                .map(|l| format!("{}\n", &l[2..]))
                .collect()
        };
        let doc = format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{}\n{}",
            block("# [labels]"),
            block("# [personas.theseus]")
        );
        let (cfg, _) = Config::parse(&doc).unwrap();
        assert_eq!(
            cfg.owners().into_iter().collect::<Vec<_>>(),
            ["discord:271828182845904523"]
        );
        assert_eq!(cfg.labels.public_paths, ["~/projects/some-public-repo"]);
        let f = &cfg.personas["theseus"].files;
        assert_eq!(
            (f[0].readers(), f[1].readers()),
            (ContextReaders::Owner, ContextReaders::Public)
        );
        assert_eq!(f[1].path(), "~/projects/some-public-repo/README.md");
    }

    /// `[labels]` (M4 19a): the owner is `[approval] trusted_users` unless
    /// the section names one, a context file may be a table with its
    /// readers, and a malformed owner, a relative public tree, or an unknown
    /// key in a file's table is refused with its key.
    #[test]
    fn labels_name_the_owner_and_the_public_trees() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [approval]\ntrusted_users = [\"discord:271828182845904523\"]\n\n\
                    [context]\nfiles = [\"~/w/USER.md\", { path = \"/w/open/README.md\", readers = \"public\" }]\n";
        let (cfg, _) = Config::parse(base).unwrap();
        assert_eq!(
            cfg.owners().into_iter().collect::<Vec<_>>(),
            ["discord:271828182845904523"],
            "the trusted users by default"
        );
        let public: Vec<(String, bool)> = cfg
            .context_paths(None)
            .into_iter()
            .map(|p| (p.path, p.public))
            .collect();
        assert_eq!(
            public,
            [
                ("~/w/USER.md".to_string(), false),
                ("/w/open/README.md".to_string(), true)
            ]
        );
        let named = format!(
            "{base}\n[labels]\nowner = [\"discord:314159265358979323\"]\npublic_paths = [\"~/w/open\"]\n"
        );
        let (cfg, _) = Config::parse(&named).unwrap();
        assert_eq!(
            cfg.owners().into_iter().collect::<Vec<_>>(),
            ["discord:314159265358979323"]
        );
        assert_eq!(cfg.labels.public_paths, ["~/w/open"]);
        for (bad, says) in [
            ("[labels]\nowner = [\"eddie\"]", "labels.owner"),
            (
                "[labels]\npublic_paths = [\"w/open\"]",
                "labels.public_paths",
            ),
        ] {
            let doc = format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{bad}\n");
            let e = format!("{:#}", Config::parse(&doc).unwrap_err());
            assert!(e.contains(says), "{e}");
        }
        let doc = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                   [context]\nfiles = [{ path = \"/w/a.md\", readers = \"public\", who = 1 }]\n";
        assert!(
            Config::parse(doc).is_err(),
            "an unknown key in a file's table"
        );
    }

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn judge(members: Members, place: Option<&str>) -> Judge {
        Judge {
            audience: Audience::Owner,
            members,
            owner: set(&["discord:1"]),
            place: place.map(str::to_string),
            places: HashMap::from([("discord:9".to_string(), set(&["discord:1", "discord:2"]))]),
            latched: false,
            own: Readers::Owner,
        }
    }

    /// §2.5's rules, case by case: the owner alone is covered by anything; a
    /// DM by its person's readers; a channel of two by readers naming both,
    /// or by its own words; a channel nobody can read only by public text.
    #[test]
    fn readers_cover_an_audience_only_when_every_member_may_read() {
        let owner = judge(Members::Owner, None);
        assert!(owner.covers(&Readers::Owner));
        let dm = judge(Members::These(set(&["discord:2"])), None);
        assert!(dm.covers(&Readers::People(set(&["discord:2"]))));
        assert!(!dm.covers(&Readers::Owner));
        assert!(dm.covers(&Readers::Public));
        let of_owner = judge(Members::These(set(&["discord:1"])), None);
        assert!(of_owner.covers(&Readers::Owner), "the owner's own DM");
        let two = judge(
            Members::These(set(&["discord:1", "discord:2"])),
            Some("discord:9"),
        );
        assert!(!two.covers(&Readers::Owner));
        assert!(!two.covers(&Readers::People(set(&["discord:3"]))));
        assert!(
            two.covers(&Readers::People(set(&["discord:2"]))),
            "the owner and 2"
        );
        assert!(two.covers(&Readers::Place("discord:9".into())));
        let anyone = judge(Members::Anyone, Some("discord:9"));
        assert!(
            anyone.covers(&Readers::Place("discord:9".into())),
            "its own words"
        );
        assert!(!anyone.covers(&Readers::Place("discord:8".into())));
        assert!(!anyone.covers(&Readers::People(set(&["discord:1"]))));
        assert!(anyone.covers(&Readers::Public));
    }

    /// The bench of 19c's `may_leave`: a channel of 1,000 viewers, all of
    /// them owners (the worst case: every viewer is looked up), by the
    /// readers a post can have. Run with `--run-ignored only --no-capture`.
    #[test]
    #[ignore = "a bench: run it by name"]
    fn bench_may_leave() {
        let viewers: BTreeSet<String> = (0..1000).map(|i| format!("discord:{i}")).collect();
        let mut j = judge(Members::These(viewers.clone()), Some("discord:9"));
        j.owner = viewers;
        for (what, r) in [
            ("owner-only", Readers::Owner),
            (
                "for 2 people",
                Readers::People(set(&["discord:1", "discord:2"])),
            ),
            ("the channel's own", Readers::Place("discord:9".into())),
            ("public", Readers::Public),
        ] {
            let n = 20_000u32;
            let t0 = std::time::Instant::now();
            let fits = (0..n)
                .filter(|_| may_leave(std::hint::black_box(&r), &j))
                .count();
            eprintln!(
                "may_leave({what}) over 1,000 viewers: {:.2} µs a call ({fits} of {n} fit)",
                t0.elapsed().as_secs_f64() * 1e6 / f64::from(n)
            );
        }
    }

    #[test]
    fn readers_meet_by_intersection_and_conservatively() {
        let j = judge(Members::Owner, None);
        let p = |ids: &[&str]| Readers::People(set(ids));
        let place = |s: &str| Readers::Place(s.into());
        assert_eq!(
            j.meet(&Readers::Public, &p(&["discord:2"])),
            p(&["discord:2"])
        );
        assert_eq!(j.meet(&Readers::Owner, &Readers::Public), Readers::Owner);
        assert_eq!(
            j.meet(&p(&["discord:2", "discord:3"]), &p(&["discord:3"])),
            p(&["discord:3"])
        );
        assert_eq!(
            j.meet(&p(&["discord:2"]), &p(&["discord:3"])),
            Readers::Owner
        );
        assert_eq!(
            j.meet(&place("discord:9"), &place("discord:9")),
            place("discord:9")
        );
        assert_eq!(
            j.meet(&place("discord:9"), &place("discord:8")),
            Readers::Owner
        );
        assert_eq!(
            j.meet(&place("discord:9"), &p(&["discord:2", "discord:4"])),
            p(&["discord:2"])
        );
        assert_eq!(
            j.meet(&place("discord:8"), &p(&["discord:2"])),
            Readers::Owner,
            "viewers unknown"
        );
    }

    #[test]
    fn a_result_is_labeled_by_its_tool_and_its_paths() {
        let public = [PathBuf::from("/w/open")];
        let page = theseus_tools::External {
            url: "https://example.invalid/tides".into(),
        };
        let l = for_result("http.fetch", "trs_1", Some(&page), None, &[], &public);
        assert_eq!(
            (l.integrity, &l.readers),
            (Integrity::Untrusted, &Readers::Public)
        );
        assert_eq!(l.source.as_ref().unwrap().node_id, "trs_1");
        let read = |paths: &[&str]| {
            let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            for_result("fs.read", "trs_2", None, None, &paths, &public).readers
        };
        assert_eq!(read(&["/w/open/README.md"]), Readers::Public);
        assert_eq!(read(&["/w/open/a", "/w/notes"]), Readers::Owner);
        assert_eq!(read(&[]), Readers::Owner);
        let run = for_result(
            "proc.run",
            "trs_3",
            None,
            None,
            &[PathBuf::from("/w/open")],
            &public,
        );
        assert_eq!(run.readers, Readers::Owner);
    }

    #[test]
    fn an_operators_message_is_its_places() {
        assert_eq!(
            for_input(false, Some("discord:dm:42")).readers,
            Readers::Owner
        );
        assert_eq!(
            for_input(true, Some("discord:dm:42")).readers,
            Readers::People(set(&["discord:42"]))
        );
        assert_eq!(
            for_input(true, Some("discord:channel:7")).readers,
            Readers::Place("discord:7".into())
        );
        assert_eq!(for_input(true, None).readers, Readers::Owner);
    }
}
