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

use std::path::PathBuf;
use std::sync::RwLock;

use theseus_protocol::{PlaceInfo, PlacesHealth, Plan};
use theseus_tools::paths;

pub use theseus_protocol::PlaceClass;

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
            public_paths: cfg.labels.public_paths.clone(),
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
    cfg.labels
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
        c.labels.owner = Some(vec![format!("discord:{OWNER}")]);
        c
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
