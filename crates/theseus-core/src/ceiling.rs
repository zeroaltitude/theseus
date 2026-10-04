//! A place's ceiling (step 38a, theseus-ext.3; M7's design, §2.3): what the
//! operator's bindings file lets one place have, beneath what the place rule
//! allows it. It narrows and never widens: whatever a ceiling says, a shared
//! place is offered no tool the place rule withholds.
//!
//! - **The floor** (`posture_floor`): a call's posture is the strictest of
//!   the config's, a tightening, the floor, and T1's hold. It is never a
//!   refusal: a floor of `approve` makes every call wait.
//! - **The tools** (`tools`): the families offered (`fs`, `git`, `web`, …,
//!   the name before a tool's first dot) and MCP servers (`mcp:<server>`,
//!   offering every tool named `mcp:<server>/…`). Absent: no narrowing. The
//!   model is offered only those, and the gate refuses any other call, as the
//!   place rule refuses one in a shared place, in words naming the ceiling.
//! - **The spend limit** (`spend_limit_usd`): the place's session has the
//!   lower of this and `[kernel] spend_limit_usd` (the kernel's
//!   `place_limit`), told at each binding start.
//! - **The profile**: the place's model, unless a turn names one.
//!
//! It is read where the place rule reads a session's class (`TurnRunner::view_of`):
//! where its words go, so a task, which speaks in its parent's place, has
//! its parent's ceiling.

use theseus_protocol::PlaceCeiling;

use crate::places::PlaceClass;
use crate::policy::{Decision, Posture};

/// A place's ceiling, as the gate reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ceiling {
    /// The place's name (`#pier`), for the words that cite it.
    pub place: String,
    pub floor: Option<Posture>,
    pub tools: Option<Vec<String>>,
    pub spend_limit_micros: Option<u64>,
    pub profile: Option<String>,
    /// As the bindings file says it, for health.
    pub wire: PlaceCeiling,
}

impl Ceiling {
    /// The ceiling of the place named `place`, or none when it narrows
    /// nothing. A floor that names no posture is the strictest (the binding
    /// refuses such a file; this fails safe if one arrives).
    pub fn new(place: &str, c: &PlaceCeiling) -> Option<Self> {
        if c.is_empty() {
            return None;
        }
        Some(Self {
            place: place.into(),
            floor: c
                .posture_floor
                .as_deref()
                .map(|f| Posture::parse(f).unwrap_or(Posture::Approve)),
            tools: c.tools.clone(),
            spend_limit_micros: c.spend_limit_usd.map(theseus_kernel::usd_to_micros),
            profile: c.profile.clone(),
            wire: c.clone(),
        })
    }

    /// Whether this ceiling offers the tool `name`: its family, or its MCP
    /// server, is listed, or nothing is.
    pub fn offers(&self, name: &str) -> bool {
        let Some(tools) = &self.tools else {
            return true;
        };
        let family = family(name);
        tools.iter().any(|t| t == family)
    }

    /// Why the gate refuses `name` here, or None: a tool this ceiling does
    /// not offer.
    pub fn refusal(&self, name: &str) -> Option<String> {
        if self.offers(name) {
            return None;
        }
        let listed = self.tools.as_deref().unwrap_or_default();
        let offered = match listed.is_empty() {
            true => "no tools".to_string(),
            false => format!("only {}", listed.join(", ")),
        };
        Some(format!(
            "{name} is not offered in {}: its ceiling in the bindings file offers {offered}",
            self.place
        ))
    }

    /// The tools paragraph's line for this ceiling, after its others.
    pub fn note(&self) -> String {
        let mut said = Vec::new();
        if let Some(t) = &self.tools {
            said.push(match t.is_empty() {
                true => "you are offered no tools here".to_string(),
                false => format!(
                    "you are offered only these tool families here: {}",
                    t.join(", ")
                ),
            });
        }
        if let Some(f) = self.floor.filter(|f| *f != Posture::Open) {
            said.push(format!("no call here runs looser than {}", f.as_str()));
        }
        if said.is_empty() {
            return String::new();
        }
        format!(
            "\n- This place has a ceiling, the operator's word in the bindings file: {}.",
            said.join("; ")
        )
    }

    /// `d` at no looser a posture than this ceiling's floor.
    pub fn floor(&self, d: Decision, tool: &str, summary: &str) -> Decision {
        let Some(floor) = self.floor else {
            return d;
        };
        let why = format!(
            "{}'s ceiling sets a floor of {}",
            self.place,
            floor.as_str()
        );
        let setting = format!("{}'s posture_floor", self.place);
        d.at_least(floor, &why, &setting, tool, summary)
    }
}

/// A tool's family, as a ceiling's `tools` names it: `mcp:<server>` for an
/// MCP tool (`mcp:<server>/<tool>`), else the name before its first dot.
pub fn family(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix(crate::policy::MCP_PREFIX) {
        let server = rest.split('/').next().unwrap_or(rest);
        return &name[..crate::policy::MCP_PREFIX.len() + server.len()];
    }
    name.split('.').next().unwrap_or(name)
}

/// Whether `entry` is shaped as a ceiling's `tools` entry: a family's name
/// (lower-case letters, digits, `_`), or `mcp:<server>`. `Err` says why not.
/// Which families exist is the daemon's (`Core::bind_places` warns of one
/// that names no tool there), so a file stays valid as tools are added.
pub fn check_entry(entry: &str) -> Result<(), String> {
    let word = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    };
    let ok = match entry.strip_prefix(crate::policy::MCP_PREFIX) {
        Some(server) => word(server),
        None => word(entry),
    };
    match ok {
        true => Ok(()),
        false => Err(format!(
            "{entry:?} is not a tool family (`fs`, `git`, `web`, `proc`, …) or an MCP server (`mcp:<server>`)"
        )),
    }
}

/// Where a turn's words go, as the gate reads it: its class (the place rule)
/// and its ceiling, if the bindings file gives one.
#[derive(Debug, Clone, Copy)]
pub struct PlaceView {
    pub class: PlaceClass,
    pub ceiling: Option<&'static Ceiling>,
}

impl From<PlaceClass> for PlaceView {
    fn from(class: PlaceClass) -> Self {
        Self {
            class,
            ceiling: None,
        }
    }
}

impl PlaceView {
    /// Whether the model here is offered the tool `name`: the place rule's
    /// set, narrowed by the ceiling (never widened).
    pub fn offered(&self, name: &str, public: &[std::path::PathBuf]) -> bool {
        crate::places::offered(self.class, name, public)
            && self.ceiling.is_none_or(|c| c.offers(name))
    }
}

impl crate::turn::TurnRunner {
    /// Where a session's turn speaks (the place rule, theseus-nbsh) and that
    /// place's ceiling: the session's place (a task's is its parent's,
    /// `outbox.target`), or, for a session no place runs on any more
    /// (`/new`), the place its wakes and reports answer in (theseus-4lx).
    /// With neither, the CLI's or the web UI's: private, with no ceiling.
    /// Shared when where it goes cannot be read.
    pub fn view_of(&self, session_id: &str) -> PlaceView {
        let place = self.outbox.try_target(session_id).and_then(|t| match t {
            Some(t) => Ok(Some(t)),
            None => self.outbox.try_wake_target(session_id),
        });
        match place {
            Ok(p) => self.place_rule.place(&self.cfg, p.as_deref()),
            Err(e) => {
                tracing::warn!(session_id, error = %format!("{e:#}"),
                    "where a session speaks cannot be read: its turn is a shared place's");
                PlaceClass::Shared.into()
            }
        }
    }
}

impl crate::rpc::Core {
    /// The spend limit of a place's session (step 38a): the lower of
    /// `[kernel] spend_limit_usd` and the place's ceiling's, told by the
    /// binding at each start, after it finds or opens the session, so the
    /// limit follows either when it changes. With no cap the session's limit
    /// is the config's again. What changed is said as the start's follow is.
    pub fn place_spend(&self, session_id: &str, place: &str, cap_usd: Option<f64>) {
        let exec = match self
            .store
            .get_session::<crate::session::SessionRecord>(session_id)
        {
            Ok(Some(s)) => s.execution_id,
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(session_id, error = %format!("{e:#}"), "a place's session was not read");
                None
            }
        };
        let Some(exec) = exec else {
            return;
        };
        let cap = cap_usd.map(theseus_kernel::usd_to_micros);
        match self.kernel.place_limit(&exec, cap) {
            Ok(Some(f)) => self.said_limits_followed_for(&[f], Some(place)),
            Ok(None) => {}
            Err(e) => tracing::warn!(session_id, place, error = %format!("{e:#}"),
                "a place's spend limit was not set"),
        }
    }

    /// The profile a turn in `session_id` runs under when it names none:
    /// its place's (step 38a), else the live one.
    pub fn place_profile(&self, session_id: &str, live: String) -> String {
        let place = self.runner.view_of(session_id);
        match place.ceiling.and_then(|c| c.profile.clone()) {
            Some(p) => p,
            None => live,
        }
    }

    /// Each ceiling's `tools` entry that names no tool this daemon has, as
    /// `(place, entry)`: a family the binding names that offers nothing
    /// here, which `bind_places` warns of.
    pub(crate) fn unknown_families(
        &self,
        places: &[crate::places::BoundPlace],
    ) -> Vec<(String, String)> {
        let have: std::collections::BTreeSet<&str> = self
            .runner
            .tools
            .registry
            .all()
            .map(|t| family(t.name()))
            .collect();
        let mut unknown = Vec::new();
        for p in places {
            let listed = p.ceiling.as_ref().and_then(|c| c.tools.as_ref());
            for t in listed.into_iter().flatten() {
                if !t.starts_with(crate::policy::MCP_PREFIX) && !have.contains(t.as_str()) {
                    unknown.push((p.name.clone(), t.clone()));
                }
            }
        }
        unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pier(tools: &[&str]) -> Ceiling {
        let wire = PlaceCeiling {
            tools: Some(tools.iter().map(|t| t.to_string()).collect()),
            ..Default::default()
        };
        Ceiling::new("#pier", &wire).unwrap()
    }

    #[test]
    fn a_family_is_the_name_before_its_dot_or_an_mcp_server() {
        assert_eq!(family("fs.read"), "fs");
        assert_eq!(family("aws.s3.get"), "aws");
        assert_eq!(family("mcp:github/create_issue"), "mcp:github");
        assert_eq!(family("mcp:github"), "mcp:github");
        assert_eq!(family("proc"), "proc");
        for ok in ["fs", "web", "mcp:github", "mcp:my_server"] {
            assert_eq!(check_entry(ok), Ok(()), "{ok}");
        }
        for bad in ["", "fs.read", "Web", "mcp:", "mcp:a/b", "web search"] {
            assert!(check_entry(bad).is_err(), "{bad:?}");
        }
    }

    /// A ceiling offers its families and MCP servers alone, and a call naming
    /// anything else is refused in words naming the ceiling.
    #[test]
    fn a_ceiling_offers_its_families_and_refuses_the_rest() {
        let c = pier(&["web", "mcp:github"]);
        assert!(c.offers("web.search") && c.offers("mcp:github/list_issues"));
        for name in ["http.fetch", "wake.at", "proc.run", "mcp:other/x"] {
            assert!(!c.offers(name), "{name}");
            let why = c.refusal(name).unwrap();
            assert!(
                why.contains("#pier") && why.contains("ceiling") && why.contains("web, mcp:github"),
                "{why}"
            );
        }
        let none = pier(&[]);
        assert!(none
            .refusal("web.search")
            .unwrap()
            .contains("offers no tools"));
        let all = Ceiling::new(
            "#lab",
            &PlaceCeiling {
                profile: Some("glm".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(all.offers("proc.run"), "no tools key: no narrowing");
        assert!(Ceiling::new("#lab", &PlaceCeiling::default()).is_none());
    }

    /// A ceiling never offers a shared place a private tool: it narrows the
    /// place rule's set, whatever it lists.
    #[test]
    fn a_ceiling_never_offers_a_shared_place_a_private_tool() {
        let c: &'static Ceiling = Box::leak(Box::new(pier(&["proc", "aws", "fs", "web"])));
        let shared = PlaceView {
            class: PlaceClass::Shared,
            ceiling: Some(c),
        };
        for name in ["proc.run", "aws.call", "fs.read"] {
            assert!(!shared.offered(name, &[]), "{name}");
        }
        assert!(shared.offered("web.search", &[]));
        assert!(!shared.offered("wake.at", &[]), "public, but not listed");
        let private = PlaceView {
            class: PlaceClass::Private,
            ceiling: Some(c),
        };
        assert!(private.offered("proc.run", &[]));
        assert!(!private.offered("git.diff", &[]), "private, but not listed");
    }

    /// The floor: the stricter of it and the decision, never looser.
    #[test]
    fn the_floor_is_the_stricter_of_the_two() {
        let wire = |f: &str| PlaceCeiling {
            posture_floor: Some(f.into()),
            ..Default::default()
        };
        let notify = Ceiling::new("#lab", &wire("notify")).unwrap();
        let open = Decision {
            posture: Posture::Open,
            reason: "fs.read — open".into(),
            notify: None,
            floor: false,
            granted: None,
            external: None,
        };
        let d = notify.floor(open.clone(), "fs.read", "read a file");
        assert_eq!(d.posture, Posture::Notify);
        assert!(
            d.reason.contains("#lab's ceiling sets a floor of notify"),
            "{}",
            d.reason
        );
        let approve = Ceiling::new("#lab", &wire("approve")).unwrap();
        let waits = approve.floor(open, "fs.read", "read a file");
        assert_eq!(waits.posture, Posture::Approve);
        let asked = Decision {
            posture: Posture::Approve,
            ..waits
        };
        assert_eq!(
            notify.floor(asked, "fs.read", "s").posture,
            Posture::Approve
        );
        let odd = Ceiling::new("#lab", &wire("loose")).unwrap();
        assert_eq!(
            odd.floor,
            Some(Posture::Approve),
            "an unknown floor fails safe"
        );
    }
}
