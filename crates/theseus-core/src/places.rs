//! The place rule (theseus-nbsh; it replaces 19a's labels on nodes): every
//! place a session speaks in is private or shared, and a shared place never
//! receives the owner's material.
//!
//! - **Private**: the CLI and the web UI (a session with no place), a DM with
//!   an owner, and a guild channel the bindings file binds with
//!   `private = true` (the operator's word, trusted by default). It gets
//!   everything.
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

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use theseus_protocol::{PlaceInfo, PlacesHealth, Plan};
use theseus_tools::paths;

pub use theseus_protocol::PlaceClass;

/// `[places]` (`[labels]` before the place rule, still read): who the owner
/// is on Discord, and the trees a shared place's file tools may reach.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacesConfig {
    /// The owner on Discord, as `discord:<user id>`, beside the local
    /// surfaces (the CLI and the web UI). Absent: `[approval] trusted_users`.
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
    /// The owner on Discord: `[places] owner`, else `[approval]
    /// trusted_users`, as `discord:<user id>`. The local surfaces are the
    /// owner's too, and need no entry.
    pub fn owners(&self) -> BTreeSet<String> {
        match &self.places.owner {
            Some(o) => o.iter().cloned().collect(),
            None => self
                .approval
                .as_ref()
                .map(|a| a.trusted_users.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// The owner, for a session posting to `place` (`outbox.target`): the
    /// owners, and, with neither `[places] owner` nor `[approval]`, the
    /// person of a DM the bindings file binds, whom approval takes for the
    /// owner then too (review 2's consideration 2): the binding lets nobody
    /// else reach it.
    pub fn owners_for(&self, place: Option<&str>) -> BTreeSet<String> {
        let mut owners = self.owners();
        if self.places.owner.is_none() && self.approval.is_none() {
            if let Some(u) = place.and_then(|p| p.strip_prefix("discord:dm:")) {
                owners.insert(format!("discord:{u}"));
            }
        }
        owners
    }

    /// `[places]`: the owner's ids as `[approval] trusted_users` writes them,
    /// and the public trees as paths.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundPlace {
    /// `discord:channel:<id>` or `discord:dm:<user id>` (`outbox.target`).
    pub target: String,
    /// `#openclaw`, `DM @eddie`.
    pub name: String,
    /// A guild channel bound `private = true`. A DM's class is its person's:
    /// private with an owner.
    pub private: bool,
}

/// What a read of who can view a guild channel bound `private = true` found,
/// at the binding's start: the people besides the owner, by name, or why it
/// could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Viewed {
    Others(Vec<String>),
    Unread(String),
}

#[derive(Debug, Clone)]
struct Bound {
    place: BoundPlace,
    viewed: Option<Viewed>,
}

/// The places the binding binds, in memory: what it told the core when it
/// started. Nothing is kept in the store, so a place's class is always the
/// bindings file's of this run.
#[derive(Default)]
pub struct PlaceRule {
    bound: RwLock<Vec<Bound>>,
}

impl PlaceRule {
    /// The binding's places, from its bindings file: they replace any told
    /// before.
    pub fn bind(&self, places: Vec<BoundPlace>) {
        *self.bound.write().unwrap() = places
            .into_iter()
            .map(|place| Bound {
                place,
                viewed: None,
            })
            .collect();
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
    /// is private only when the binding bound it `private = true`; anything
    /// else is shared.
    pub fn class(&self, cfg: &crate::Config, target: Option<&str>) -> PlaceClass {
        let Some(t) = target else {
            return PlaceClass::Private;
        };
        if let Some(u) = t.strip_prefix("discord:dm:") {
            return match cfg.owners_for(target).contains(&format!("discord:{u}")) {
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
    /// place with its class, and a private channel's viewers as read.
    pub fn health(&self, cfg: &crate::Config) -> PlacesHealth {
        let local = |place: &str, name: &str| PlaceInfo {
            place: place.into(),
            name: name.into(),
            class: PlaceClass::Private,
            others: None,
            unchecked: None,
        };
        let mut places = vec![local("cli", "CLI"), local("web", "web")];
        for b in self.bound.read().unwrap().iter() {
            let (others, unchecked) = match &b.viewed {
                Some(Viewed::Others(o)) => (Some(o.clone()), None),
                Some(Viewed::Unread(why)) => (None, Some(why.clone())),
                None => (None, None),
            };
            places.push(PlaceInfo {
                place: b.place.target.clone(),
                name: b.place.name.clone(),
                class: self.class(cfg, Some(&b.place.target)),
                others,
                unchecked,
            });
        }
        PlacesHealth {
            places,
            public_paths: cfg.places.public_paths.clone(),
        }
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

/// Whether the model in a place of `class` is offered the tool `name`.
/// `public` is `public_paths`, canonical.
pub fn offered(class: PlaceClass, name: &str, public: &[PathBuf]) -> bool {
    match class {
        PlaceClass::Private => true,
        PlaceClass::Shared => public_tool(name) || (files_tool(name) && !public.is_empty()),
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
            cfg.owners().into_iter().collect::<Vec<_>>(),
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

    /// `[places]`: the owner is `[approval] trusted_users` unless the section
    /// names one; `[labels]`, its name before, still reads; a context file
    /// may be a table with its readers; and a malformed owner, a relative
    /// public tree, or an unknown key in a file's table is refused with its
    /// key.
    #[test]
    fn places_name_the_owner_and_the_public_trees() {
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
        for section in ["places", "labels"] {
            let named = format!(
                "{base}\n[{section}]\nowner = [\"discord:314159265358979323\"]\npublic_paths = [\"~/w/open\"]\n"
            );
            let (cfg, _) = Config::parse(&named).unwrap();
            assert_eq!(
                cfg.owners().into_iter().collect::<Vec<_>>(),
                ["discord:314159265358979323"],
                "[{section}]"
            );
            assert_eq!(cfg.places.public_paths, ["~/w/open"]);
        }
        for (bad, says) in [
            ("[places]\nowner = [\"eddie\"]", "places.owner"),
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
            },
            BoundPlace {
                target: open.into(),
                name: "#open".into(),
                private: false,
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

    /// A shared place is offered the public tools, and the file tools only
    /// with a public tree; a private one everything.
    #[test]
    fn a_shared_place_is_offered_the_public_tools() {
        let public = [PathBuf::from("/w/open")];
        for name in ["web.search", "http.fetch", "wake.at", "task.create"] {
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
