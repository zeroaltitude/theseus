//! The place rule (theseus-nbsh; it replaces 19a's labels on nodes): every
//! place a session speaks in is private or shared, and a shared place never
//! receives the owner's material.
//!
//! - **Private**: the CLI and the web UI (a session with no place), a DM with
//!   an owner, and a guild channel the bindings file binds with
//!   `private = true` (the operator's word, trusted by default). It gets
//!   everything.
//! - **A trusted guild** (theseus-rdqg): `private = true` in a guild's
//!   `[[guild]]` (beside `guild_id` in a format-1 bindings file) is the
//!   operator's word that the whole guild is theirs; each guild has its own
//!   (step 38a). Every channel bound in it is private unless it says
//!   `private = false` (the binding resolves each place's class before it
//!   tells them), none has its viewers read, and health says "in a trusted
//!   guild" where it would warn.
//! - **Shared**: every other place. It gets its own conversation; the tools
//!   whose results are public by nature (`web.search`, `http.fetch`,
//!   `wake.*`, `task.*`); `fs.*`, `git.*`, and `text.*` only under
//!   `public_paths`; no `proc.run` and no `aws.*`; and only the context
//!   files marked `readers = "public"`. Its model is offered only those
//!   tools (`offered`), and the gate refuses any other call (`refusal`).
//! - **A task takes its parent's class**: it speaks where its parent does,
//!   since `outbox.target` gives a task its parent's place (`task.rs`).
//! - A guild place the binding has not named (it has not started yet, or
//!   Discord is off) is shared until it does. The class is fixed for a turn,
//!   as its request's spec is.
//! - **Approvals follow it** (theseus-zmgb): an answer to a waiting call, the
//!   undo of a tightening, a trust, and a publish count only from a private
//!   place, by the owner (`owner_in_private`). A shared place's cards go to
//!   the owner's DM.
//! - **Gliding follows it** (38b, the owner 2026-10-04): words that move between
//!   places, a `channel.post` or a `channel.read`, take `glide_rule`. Into a
//!   private place they always may, and what comes from a shared place is
//!   outside text there; out of a private place, or between two shared
//!   places, they ask the owner first, as `/publish` does. Every place is
//!   offered the two tools, since the rule asks wherever it must.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use theseus_protocol::{PlaceInfo, PlacesHealth, Plan};
use theseus_tools::paths;

use crate::ceiling::{Ceiling, PlaceView};
use crate::policy::{Decision, Posture};

pub use theseus_protocol::PlaceClass;

/// `[places]` (`[labels]` before the place rule, still read): who the owner
/// is on Discord, and the trees a shared place's file tools may reach.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacesConfig {
    /// The owner on Discord, as `discord:<user id>`, beside the local
    /// surfaces (the CLI and the web UI). Absent: the person of each DM the
    /// bindings file binds (theseus-zmgb).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Vec<String>>,
    /// Trees anyone may read (a public repository), `~/` or absolute: a
    /// shared place's `fs.*`, `git.*`, and `text.*` reach these alone.
    /// Default none: a shared place gets no file tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_paths: Vec<String>,
}

impl PlacesConfig {
    pub fn is_empty(&self) -> bool {
        self.owner.is_none() && self.public_paths.is_empty()
    }
}

impl crate::config::Config {
    /// `[places]`: the owner's ids as `discord:<user id>`, and the public
    /// trees as paths.
    pub(crate) fn validate_places(&self) -> anyhow::Result<()> {
        for o in self.places.owner.iter().flatten() {
            if crate::approval::parse_user(o).is_err() {
                anyhow::bail!(
                    "places.owner entry {o:?} is not a surface-qualified id: write \"discord:<user id>\""
                );
            }
        }
        if let Some(p) = self
            .places
            .public_paths
            .iter()
            .find(|p| !(p.starts_with('/') || p.starts_with("~/")))
        {
            anyhow::bail!(
                "places.public_paths entry {p:?} must be an absolute path or start with ~/"
            );
        }
        Ok(())
    }
}

/// A place the Discord binding binds, as its bindings file names it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoundPlace {
    /// `discord:channel:<id>` or `discord:dm:<user id>` (`outbox.target`).
    pub target: String,
    /// `#openclaw`, `DM @zeroaltitude`.
    pub name: String,
    /// A guild channel bound private: by its own `private = true`, or by its
    /// trusted guild's when it says nothing (theseus-rdqg). A DM's class is
    /// its person's: private with an owner.
    pub private: bool,
    /// A guild channel's guild id (step 38a); none for a DM.
    pub guild: Option<String>,
    /// What the bindings file narrows here (step 38a): it narrows what the
    /// class allows, never widens it (`ceiling.rs`).
    pub ceiling: Option<theseus_protocol::PlaceCeiling>,
}

/// What a read of who can view a guild channel bound `private = true`, outside
/// a trusted guild, found at the binding's start: the people besides the
/// owner, by name, or why it could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Viewed {
    Others(Vec<String>),
    Unread(String),
}

#[derive(Debug, Clone)]
struct Bound {
    place: BoundPlace,
    viewed: Option<Viewed>,
    /// The place's ceiling as the gate reads it. Made once a bind and never
    /// freed, so a turn's context can hold it by reference: a bind is once a
    /// binding start, so this is a few small records a run.
    ceiling: Option<&'static Ceiling>,
}

impl Bound {
    fn new(place: BoundPlace) -> Self {
        let ceiling = place
            .ceiling
            .as_ref()
            .and_then(|c| Ceiling::new(&place.name, c))
            .map(|c| &*Box::leak(Box::new(c)));
        Self {
            place,
            viewed: None,
            ceiling,
        }
    }
}

/// The places the binding binds, in memory: what it told the core when it
/// started. Nothing is kept in the store, so a place's class is always the
/// bindings file's of this run.
#[derive(Default)]
pub struct PlaceRule {
    bound: RwLock<Vec<Bound>>,
    /// The guilds the bindings file trusts whole (theseus-rdqg), by id.
    trusted_guilds: RwLock<BTreeSet<String>>,
    /// What the binding's start found wrong with its places (theseus-ext.11).
    warnings: RwLock<Vec<theseus_protocol::PlaceWarning>>,
}

impl PlaceRule {
    /// The owner on Discord, as `discord:<user id>` (theseus-zmgb): `[places]
    /// owner`, else the person of each DM the bindings file binds, whom the
    /// binding lets nobody else reach. The local surfaces are the owner's
    /// too, and need no entry.
    pub fn owners(&self, cfg: &crate::Config) -> BTreeSet<String> {
        if let Some(o) = &cfg.places.owner {
            return o.iter().cloned().collect();
        }
        self.bound
            .read()
            .unwrap()
            .iter()
            .filter_map(|b| b.place.target.strip_prefix("discord:dm:"))
            .map(|u| format!("discord:{u}"))
            .collect()
    }

    /// One more bound place, as the binding's start would name it: tests
    /// that bind a session to a place by hand bind it here too.
    #[cfg(test)]
    pub(crate) fn bind_one(&self, place: BoundPlace) {
        self.bound.write().unwrap().push(Bound::new(place));
    }

    /// The guilds the bindings file trusts whole (`private = true` in a
    /// guild's word, theseus-rdqg; one each, step 38a), by id. Their
    /// channels' classes come resolved in their places; this says why the
    /// private ones are never read.
    pub fn trust_guilds(&self, trusted: BTreeSet<String>) {
        *self.trusted_guilds.write().unwrap() = trusted;
    }

    /// The binding's places, from its bindings file: they replace any told
    /// before.
    pub fn bind(&self, places: Vec<BoundPlace>) {
        *self.bound.write().unwrap() = places.into_iter().map(Bound::new).collect();
    }

    /// The bound place `to` names: its name (`#openclaw`, `openclaw`, `DM
    /// @zeroaltitude`), its target (`discord:channel:<id>`), its key
    /// (`channel:<id>`), or its id.
    pub fn find(&self, to: &str) -> Option<BoundPlace> {
        let to = to.trim();
        self.bound
            .read()
            .unwrap()
            .iter()
            .map(|b| &b.place)
            .find(|p| {
                p.target == to
                    || p.target.strip_prefix("discord:") == Some(to)
                    || p.name.trim_start_matches('#') == to.trim_start_matches('#')
                    || p.target.rsplit(':').next() == Some(to)
            })
            .cloned()
    }

    /// What the binding's start found wrong with its places, for health:
    /// this start's alone (`Core::place_warnings`, theseus-ext.11).
    pub fn warn(&self, warnings: Vec<theseus_protocol::PlaceWarning>) {
        *self.warnings.write().unwrap() = warnings;
    }

    /// Who can view a guild channel bound `private = true`, as the binding
    /// read it at its start, for health.
    pub fn viewed(&self, target: &str, viewed: Viewed) {
        if let Some(b) = self
            .bound
            .write()
            .unwrap()
            .iter_mut()
            .find(|b| b.place.target == target)
        {
            b.viewed = Some(viewed);
        }
    }

    /// The class of the place `target` (`outbox.target`): none is the CLI or
    /// the web UI, private; a DM is private with an owner; a guild channel
    /// is private only when the binding bound it private (its own word, or
    /// its trusted guild's); anything else is shared.
    pub fn class(&self, cfg: &crate::Config, target: Option<&str>) -> PlaceClass {
        self.class_alone(cfg, target)
    }

    /// The class of the place `target`, as `class` says, and its ceiling
    /// (step 38a), which a place the binding has not named has none of.
    pub fn place(&self, cfg: &crate::Config, target: Option<&str>) -> PlaceView {
        let ceiling = target.and_then(|t| {
            let bound = self.bound.read().unwrap();
            bound.iter().find(|b| b.place.target == t)?.ceiling
        });
        PlaceView {
            class: self.class_alone(cfg, target),
            ceiling,
        }
    }

    fn class_alone(&self, cfg: &crate::Config, target: Option<&str>) -> PlaceClass {
        let Some(t) = target else {
            return PlaceClass::Private;
        };
        if let Some(u) = t.strip_prefix("discord:dm:") {
            return match self.owners(cfg).contains(&format!("discord:{u}")) {
                true => PlaceClass::Private,
                false => PlaceClass::Shared,
            };
        }
        let private = t.starts_with("discord:channel:")
            && self
                .bound
                .read()
                .unwrap()
                .iter()
                .any(|b| b.place.target == t && b.place.private);
        match private {
            true => PlaceClass::Private,
            false => PlaceClass::Shared,
        }
    }

    /// Health's `places` block: the CLI and the web UI, then each bound
    /// place with its class, and a private channel's viewers as read, or
    /// that its guild is trusted.
    pub fn health(&self, cfg: &crate::Config) -> PlacesHealth {
        let local = |place: &str, name: &str| PlaceInfo {
            place: place.into(),
            name: name.into(),
            class: PlaceClass::Private,
            others: None,
            unchecked: None,
            trusted_guild: false,
            guild: None,
            ceiling: None,
        };
        let mut places = vec![local("cli", "CLI"), local("web", "web")];
        let trusted = self.trusted_guilds.read().unwrap().clone();
        for b in self.bound.read().unwrap().iter() {
            let (others, unchecked) = match &b.viewed {
                Some(Viewed::Others(o)) => (Some(o.clone()), None),
                Some(Viewed::Unread(why)) => (None, Some(why.clone())),
                None => (None, None),
            };
            let class = self.class(cfg, Some(&b.place.target));
            places.push(PlaceInfo {
                place: b.place.target.clone(),
                name: b.place.name.clone(),
                class,
                others,
                unchecked,
                trusted_guild: b.place.guild.as_ref().is_some_and(|g| trusted.contains(g))
                    && class == PlaceClass::Private
                    && b.place.target.starts_with("discord:channel:"),
                guild: b.place.guild.clone(),
                ceiling: b.place.ceiling.clone().filter(|c| !c.is_empty()),
            });
        }
        PlacesHealth {
            places,
            public_paths: cfg.places.public_paths.clone(),
            warnings: self.warnings.read().unwrap().clone(),
        }
    }
}

/// Whether `who` may make an owner's act (theseus-zmgb): answer a waiting
/// call or a budget question, undo a tightening, trust a session again, or
/// publish into a place. The owner, from a private place: the CLI and the
/// web UI are the owner's own surfaces; through Discord, an owner, in a DM
/// with them or a channel bound private. `Err` says why not.
pub fn owner_in_private(
    who: &crate::approval::Answerer,
    rule: &PlaceRule,
    cfg: &crate::Config,
) -> Result<(), String> {
    use crate::approval::Surface;
    if let Some(why) = who.unknown() {
        return Err(why);
    }
    let Some(d) = who
        .discord
        .as_ref()
        .filter(|_| who.surface == Surface::Discord)
    else {
        // The CLI and the web UI, which `unknown` has let through.
        return Ok(());
    };
    let user = format!("discord:{}", d.user_id);
    let from = match &d.guild_id {
        None => format!("discord:dm:{}", d.user_id),
        Some(_) => format!("discord:channel:{}", d.channel_id),
    };
    if !rule.owners(cfg).contains(&user) {
        return Err(format!("{user} is not an owner"));
    }
    match rule.class(cfg, Some(&from)) {
        PlaceClass::Private => Ok(()),
        PlaceClass::Shared => Err(
            "it came from a shared place, and only a private one counts \
                                   (the CLI, the web UI, a DM with you, or a channel bound \
                                   private)"
                .into(),
        ),
    }
}

/// The tools whose results are public by nature, which a shared place is
/// offered whatever its config.
pub fn public_tool(name: &str) -> bool {
    matches!(name, "web.search" | "http.fetch")
        || name.starts_with("wake.")
        || name.starts_with("task.")
}

/// The tools that read or write files: a shared place gets them only under
/// `public_paths`.
pub fn files_tool(name: &str) -> bool {
    ["fs.", "git.", "text."].iter().any(|p| name.starts_with(p))
}

/// The gliding tools (38b), `channel.post` and `channel.read`: offered in a
/// shared place too, since every way out of a private place, and every way
/// between two shared ones, asks the owner first (`glide_rule`).
pub fn glide_tool(name: &str) -> bool {
    name.starts_with("channel.")
}

/// Whether the model in a place of `class` is offered the tool `name`.
/// `public` is `public_paths`, canonical.
pub fn offered(class: PlaceClass, name: &str, public: &[PathBuf]) -> bool {
    match class {
        PlaceClass::Private => true,
        PlaceClass::Shared => {
            public_tool(name) || glide_tool(name) || (files_tool(name) && !public.is_empty())
        }
    }
}

/// One end of a glide (38b): where its words were said, or where they go.
/// `target` is the place's (`discord:channel:<id>`), none for the CLI or
/// the web UI; `name` is how the rule's words say it (`#deploys`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End<'a> {
    pub target: Option<&'a str>,
    pub name: &'a str,
    pub class: PlaceClass,
}

/// What the place rule says of a glide (38b): it runs at the call's own
/// posture, or it asks the owner first, for the reason given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Glide {
    Allow,
    AskFirst(String),
}

/// The place rule for words that move between places (38b, approved by
/// the owner on 2026-10-04), the one rule every glide takes: `from` is where
/// the words were said, `to` where they go. A post goes from its session's
/// place to the place it names; a read goes from the place it names into
/// its session's.
/// - **The same place**: allowed, since its audience is the same.
/// - **Into a private place**: allowed. What a read brings there from a
///   shared place is outside text (`glide::outside`).
/// - **Out of a private place into a shared one**: asks first, as
///   `/publish` does. Only the owner puts the owner's material where others
///   read it, and the answer counts only from a private place.
/// - **Between two shared places**: asks first: they are different
///   audiences.
pub fn glide_rule(from: &End<'_>, to: &End<'_>) -> Glide {
    if from.target.is_some() && from.target == to.target {
        return Glide::Allow;
    }
    match (from.class, to.class) {
        (_, PlaceClass::Private) => Glide::Allow,
        (PlaceClass::Private, PlaceClass::Shared) => Glide::AskFirst(format!(
            "out of a private place: {} is shared, so this puts words from {} where others \
             read them, and only the owner does that, as with /publish",
            to.name, from.name
        )),
        (PlaceClass::Shared, PlaceClass::Shared) => Glide::AskFirst(format!(
            "between two shared places: {} and {} are different audiences",
            from.name, to.name
        )),
    }
}

/// The place rule with the config its classes read (the owners): what a
/// turn's tools resolve a named place by (38b).
#[derive(Clone, Copy)]
pub struct Places<'a> {
    pub rule: &'a PlaceRule,
    pub cfg: &'a crate::Config,
}

impl Places<'_> {
    /// The bound place `name` names, as `PlaceRule::find` reads it.
    pub fn find(&self, name: &str) -> Option<BoundPlace> {
        self.rule.find(name)
    }

    /// The class and ceiling of the place `target`, as the gate reads them.
    pub fn view(&self, target: Option<&str>) -> PlaceView {
        self.rule.place(self.cfg, target)
    }

    /// The name of the place `target`: its bound name, else the target
    /// itself; none is the CLI or the web UI.
    pub fn name_of(&self, target: Option<&str>) -> String {
        match target {
            None => "the CLI or the web UI".into(),
            Some(t) => self
                .rule
                .bound
                .read()
                .unwrap()
                .iter()
                .find(|b| b.place.target == t)
                .map_or_else(|| t.to_string(), |b| b.place.name.clone()),
        }
    }
}

/// Why the gate refuses a call in a place of `class`, or None: in a shared
/// place, a tool it is not offered, or a file tool's path outside every
/// public tree (each path canonical, so a link out of one is outside it).
pub fn refusal(class: PlaceClass, name: &str, plan: &Plan, public: &[PathBuf]) -> Option<String> {
    if class == PlaceClass::Private {
        return None;
    }
    if !offered(class, name, public) {
        return Some(format!(
            "{name} is not offered in a shared place: one others can read gets only the public tools"
        ));
    }
    if !files_tool(name) {
        return None;
    }
    let outside = plan
        .resources
        .iter()
        .map(|r| paths::canonical_best_effort(&r.path))
        .find(|p| !public.iter().any(|root| paths::within(p, root)))?;
    Some(format!(
        "{} is outside the public paths, and this place is shared: here {name} reaches only {}",
        outside.display(),
        shown(public)
    ))
}

/// The card of a fetch that waits on a private address, in a shared place
/// (theseus-94a6). DD5 asks before `http.fetch` reaches a private address,
/// and a shared place is offered the tool, so an approved fetch brings that
/// page into a conversation others can read. The approver decides (the owner,
/// 2026-10-03: "Leave it to the approver"), and the reason tells them, in its
/// parenthesis: "… — approve (127.0.0.1 is a loopback address, and a private
/// address waits for approval; this is a shared place, so the page joins a
/// conversation others can read)". Any other decision is returned as it is.
pub fn private_fetch(class: PlaceClass, plan: &Plan, mut d: Decision) -> Decision {
    let private = plan
        .url
        .as_deref()
        .and_then(crate::web::net::private_url)
        .is_some();
    if class == PlaceClass::Private || d.posture != Posture::Approve || !private {
        return d;
    }
    let note = "this is a shared place, so the page joins a conversation others can read";
    d.reason = match d.reason.strip_suffix(')') {
        Some(head) => format!("{head}; {note})"),
        None => format!("{}; {note}", d.reason),
    };
    d
}

/// The public trees, for the model.
pub fn shown(public: &[PathBuf]) -> String {
    public
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `public_paths`, expanded and canonical.
pub fn public_roots(cfg: &crate::Config) -> Vec<PathBuf> {
    cfg.places
        .public_paths
        .iter()
        .map(|p| paths::canonical_best_effort(&crate::config::expand(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use std::path::Path;
    use theseus_protocol::{Access, Resource};

    const OWNER: &str = "271828182845904523";
    const ALICE: &str = "222222222222222222";

    fn cfg() -> Config {
        let mut c = Config::example();
        c.places.owner = Some(vec![format!("discord:{OWNER}")]);
        c
    }

    /// The template's `[places]` block and its persona's files, uncommented,
    /// are real: the owner, the public tree, and a file's table with its
    /// readers parse and validate.
    #[test]
    fn the_templates_places_lines_are_real() {
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
            block("# [places]"),
            block("# [personas.theseus]")
        );
        let (cfg, _) = Config::parse(&doc).unwrap();
        assert_eq!(
            PlaceRule::default()
                .owners(&cfg)
                .into_iter()
                .collect::<Vec<_>>(),
            ["discord:271828182845904523"]
        );
        assert_eq!(cfg.places.public_paths, ["~/projects/some-public-repo"]);
        let f = &cfg.personas["theseus"].files;
        assert_eq!(
            (f[0].readers(), f[1].readers()),
            (ContextReaders::Owner, ContextReaders::Public)
        );
        assert_eq!(f[1].path(), "~/projects/some-public-repo/README.md");
    }

    /// `[places]`: the owner is the person of each DM the bindings file binds
    /// unless the section names one (theseus-zmgb); `[labels]`, its name
    /// before, still reads; a context file may be a table with its readers;
    /// and a malformed owner, a relative public tree, or an unknown key in a
    /// file's table is refused with its key.
    #[test]
    fn places_name_the_owner_and_the_public_trees() {
        let base = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n\
                    [context]\nfiles = [\"~/w/USER.md\", { path = \"/w/open/README.md\", readers = \"public\" }]\n";
        let (cfg, _) = Config::parse(base).unwrap();
        let rule = PlaceRule::default();
        assert!(rule.owners(&cfg).is_empty(), "nobody, before a DM is bound");
        rule.bind(vec![BoundPlace {
            target: "discord:dm:271828182845904523".into(),
            name: "DM @zeroaltitude".into(),
            private: false,
            ..Default::default()
        }]);
        assert_eq!(
            rule.owners(&cfg).into_iter().collect::<Vec<_>>(),
            ["discord:271828182845904523"],
            "the bound DM's person by default"
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
        for section in ["places", "labels"] {
            let named = format!(
                "{base}\n[{section}]\nowner = [\"discord:314159265358979323\"]\npublic_paths = [\"~/w/open\"]\n"
            );
            let (cfg, _) = Config::parse(&named).unwrap();
            assert_eq!(
                rule.owners(&cfg).into_iter().collect::<Vec<_>>(),
                ["discord:314159265358979323"],
                "[{section}]"
            );
            assert_eq!(cfg.places.public_paths, ["~/w/open"]);
        }
        for (bad, says) in [
            ("[places]\nowner = [\"zeroaltitude\"]", "places.owner"),
            (
                "[places]\npublic_paths = [\"w/open\"]",
                "places.public_paths",
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

    /// The rule, place by place: the local surfaces and an owner's DM are
    /// private; anyone else's DM is shared; a guild channel is private only
    /// when the binding bound it so, and shared before the binding says.
    #[test]
    fn each_place_has_its_class() {
        let (rule, cfg) = (PlaceRule::default(), cfg());
        let class = |t: Option<&str>| rule.class(&cfg, t);
        assert_eq!(class(None), PlaceClass::Private, "the CLI and the web UI");
        assert_eq!(
            class(Some(&format!("discord:dm:{OWNER}"))),
            PlaceClass::Private
        );
        assert_eq!(
            class(Some(&format!("discord:dm:{ALICE}"))),
            PlaceClass::Shared
        );
        let lab = "discord:channel:314159265358979323";
        let open = "discord:channel:161803398874989484";
        assert_eq!(
            class(Some(lab)),
            PlaceClass::Shared,
            "before the binding says"
        );
        rule.bind(vec![
            BoundPlace {
                target: lab.into(),
                name: "#lab".into(),
                private: true,
                ..Default::default()
            },
            BoundPlace {
                target: open.into(),
                name: "#open".into(),
                private: false,
                ..Default::default()
            },
        ]);
        assert_eq!(class(Some(lab)), PlaceClass::Private);
        assert_eq!(class(Some(open)), PlaceClass::Shared);
        assert_eq!(
            class(Some("slack:channel:1")),
            PlaceClass::Shared,
            "a place nobody binds"
        );
        let h = rule.health(&cfg);
        let named: Vec<(&str, PlaceClass)> = h
            .places
            .iter()
            .map(|p| (p.name.as_str(), p.class))
            .collect();
        assert_eq!(
            named,
            [
                ("CLI", PlaceClass::Private),
                ("web", PlaceClass::Private),
                ("#lab", PlaceClass::Private),
                ("#open", PlaceClass::Shared)
            ]
        );
    }

    /// The glide rule's whole matrix (38b): each way between a private place
    /// and a shared one, both ways, two shared places, and the same place of
    /// each class. Into a private place it allows; out of a private place
    /// into a shared one, and between two shared ones, it asks first, naming
    /// both places; within one place it allows.
    #[test]
    fn the_glide_rule_allows_into_private_and_asks_out_of_it_and_between_shared() {
        use PlaceClass::{Private, Shared};
        let end = |target, name, class| End {
            target: Some(target),
            name,
            class,
        };
        let dm = end("discord:dm:1", "DM @owner", Private);
        let lab = end("discord:channel:2", "#lab", Private);
        let hall = end("discord:channel:3", "#hall", Shared);
        let pier = end("discord:channel:4", "#pier", Shared);
        let cli = End {
            target: None,
            name: "the CLI or the web UI",
            class: Private,
        };
        let out = Some("out of a private place");
        let between = Some("between two shared places");
        let table = [
            (cli, dm, None),
            (dm, lab, None),
            (hall, dm, None),
            (hall, cli, None),
            (dm, hall, out),
            (cli, hall, out),
            (lab, pier, out),
            (hall, pier, between),
            (pier, hall, between),
            (hall, hall, None),
            (dm, dm, None),
            (cli, cli, None),
        ];
        for (from, to, asks) in table {
            match (glide_rule(&from, &to), asks) {
                (Glide::Allow, None) => {}
                (Glide::AskFirst(why), Some(says)) => {
                    assert!(why.starts_with(says), "{} to {}: {why}", from.name, to.name);
                    assert!(
                        why.contains(from.name) && why.contains(to.name),
                        "it names both places: {why}"
                    );
                }
                (got, want) => panic!(
                    "{} to {}: the rule said {got:?}, and the table {want:?}",
                    from.name, to.name
                ),
            }
        }
    }

    /// A shared place is offered the public tools, and the file tools only
    /// with a public tree; a private one everything.
    #[test]
    fn a_shared_place_is_offered_the_public_tools() {
        let public = [PathBuf::from("/w/open")];
        for name in [
            "web.search",
            "http.fetch",
            "wake.at",
            "task.create",
            "channel.post",
            "channel.read",
        ] {
            assert!(offered(PlaceClass::Shared, name, &[]), "{name}");
        }
        for name in [
            "proc.run",
            "aws.call",
            "aws.whoami",
            "fs.read",
            "git.diff",
            "text.diff",
        ] {
            assert!(!offered(PlaceClass::Shared, name, &[]), "{name}");
            assert!(offered(PlaceClass::Private, name, &[]), "{name}");
        }
        assert!(offered(PlaceClass::Shared, "fs.read", &public));
        assert!(!offered(PlaceClass::Shared, "proc.run", &public));
    }

    /// In a shared place the gate refuses a tool it is not offered, and a
    /// file tool's path outside the public trees, a link out of one included.
    #[test]
    fn the_gate_refuses_what_a_shared_place_may_not_reach() {
        let dir = tempfile::tempdir().unwrap();
        let open = dir.path().join("open");
        std::fs::create_dir_all(&open).unwrap();
        std::fs::write(dir.path().join("notes.md"), "private").unwrap();
        std::os::unix::fs::symlink(dir.path().join("notes.md"), open.join("link.md")).unwrap();
        let public = [paths::canonical_best_effort(&open)];
        let plan = |p: &Path| Plan {
            resources: vec![Resource {
                path: p.to_path_buf(),
                access: Access::Read,
            }],
            summary: "read".into(),
            ..Default::default()
        };
        let readme = open.join("README.md");
        assert_eq!(
            refusal(PlaceClass::Shared, "fs.read", &plan(&readme), &public),
            None
        );
        for p in [
            dir.path().join("notes.md"),
            open.join("link.md"),
            open.join("../notes.md"),
        ] {
            let why = refusal(PlaceClass::Shared, "fs.read", &plan(&p), &public).unwrap();
            assert!(why.contains("outside the public paths"), "{why}");
        }
        let why = refusal(PlaceClass::Shared, "proc.run", &plan(&readme), &public).unwrap();
        assert!(why.contains("not offered in a shared place"), "{why}");
        assert!(refusal(PlaceClass::Shared, "fs.read", &plan(&readme), &[]).is_some());
        assert_eq!(
            refusal(PlaceClass::Private, "proc.run", &plan(&readme), &[]),
            None
        );
    }
}
