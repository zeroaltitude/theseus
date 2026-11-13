//! The owner's grant of a repository's instruction files (theseus-wadu,
//! phase 2; `docs/design/project-context.md` §7). A grant and its revoke are
//! the owner's acts, and each is one frame: the META record
//! (`project_context::trust::RECORD`) and its `project.trust` row.
//!
//! **Who may grant** (theseus-me9fc). Narrower than the place rule, because
//! a grant lets a repository's own `AGENTS.md` and `CLAUDE.md` into the
//! system block with an *instruction's* authority: this is the act that
//! widens who can widen reach. So both writes are judged `Act::Trust`, which
//! asks `places::owner_in_person` rather than `places::owner_in_private`:
//! **no agent's process may grant**, whatever its surface, and on a
//! transport only a DM with an owner counts. Until theseus-me9fc both were
//! judged `Act::Standing`, which reads the place and not the process, so an
//! agent's `theseus project trust` was *recorded* as the grantor
//! (`by: agent:no terminal`) and allowed; the name that was evidence is now
//! the refusal. A refusal is one `approval.refused` row and the error, and
//! nothing is written. `places::owner_in_person` has the whole rule and the
//! reason the narrower of the owner's two readings was taken.
//!
//! The repository is named by any path inside it; its identity is the
//! canonical root's `repo_id`, so a clone elsewhere or a re-pointed remote
//! inherits nothing. A path outside every repository is refused.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::Value;
use theseus_protocol::projects::{
    ProjectGrant, ProjectTrustParams, ProjectTrustedParams, ProjectTrustedResult,
};
use theseus_protocol::{error_code, method};

use super::confirms::Act;
use super::projects::failure;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::fact::project::ProjectTrust;
use crate::flow::spec::{repo_id, repo_root};
use crate::project_context::trust::{Grant, Grants};

/// The grant's methods, routed by one dispatch arm (`rpc_project_trust`).
pub(super) const OWN: [&str; 3] = [
    method::PROJECT_TRUST,
    method::PROJECT_UNTRUST,
    method::PROJECT_TRUSTED,
];

/// One trusted repository, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trusted {
    pub id: String,
    pub grant: Grant,
}

impl Trusted {
    fn wire(self) -> ProjectGrant {
        ProjectGrant {
            id: self.id,
            root: self.grant.root,
            by: self.grant.by,
            via: self.grant.via,
            at_ms: self.grant.at_ms,
        }
    }
}

impl Core {
    /// The grant's methods, by name: the owner's acts, judged by the writes
    /// themselves (`Act::Trust`), and the list, a read. A relative path is
    /// refused first: `repo_root` would resolve it against the daemon's
    /// directory, not the caller's.
    pub(super) fn rpc_project_trust(
        self: &Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::PROJECT_TRUSTED {
            return route(params, |_: ProjectTrustedParams| {
                let trusted = self.project_trusted().map_err(RpcFailure::invalid)?;
                Ok(ProjectTrustedResult {
                    trusted: trusted.into_iter().map(Trusted::wire).collect(),
                })
            });
        }
        route(params, |p: ProjectTrustParams| {
            if !Path::new(&p.path).is_absolute() {
                return Err(RpcFailure::new(
                    error_code::INVALID_PARAMS,
                    format!("{} is not an absolute path", p.path),
                ));
            }
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            let path = Path::new(&p.path);
            let done = if name == method::PROJECT_TRUST {
                self.project_trust(path, who)
            } else {
                self.project_untrust(path, who)
            };
            done.map(Trusted::wire).map_err(failure)
        })
    }

    fn trust_root(path: &Path) -> Result<PathBuf> {
        repo_root(path).ok_or_else(|| {
            anyhow!(
                "{} is in no git repository: a trust is given to a repository",
                path.display()
            )
        })
    }

    /// Trust the repository holding `path`: its `AGENTS.md` and `CLAUDE.md`
    /// join the system block from the session's next recompile.
    pub fn project_trust(&self, path: &Path, who: impl Into<Answerer>) -> Result<Trusted> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Trust {
                method: method::PROJECT_TRUST,
                what: &format!("The trust of the repository holding {}", path.display()),
            },
        )?;
        let root = Self::trust_root(path)?;
        let id = repo_id(&root);
        let mut grants = Grants::read(&self.store)?;
        if let Some(g) = grants.repos.get(&id) {
            return Ok(Trusted {
                id,
                grant: g.clone(),
            });
        }
        let grant = Grant {
            root: root.display().to_string(),
            by: who.who(),
            via: who.via(),
            at_ms: theseus_protocol::now_unix_ms(),
        };
        grants.repos.insert(id.clone(), grant.clone());
        let fact = ProjectTrust {
            id: &id,
            root: &grant.root,
            granted: true,
            by: &grant.by,
            via: &grant.via,
        };
        let rec = self.rec(None);
        self.store.append(&[rec.row(&fact)?, grants.record()?])?;
        rec.announce(&fact);
        Ok(Trusted { id, grant })
    }

    /// Take the repository's trust back: its files are named, not read, from
    /// the next recompile. A repository that was not trusted is an error.
    pub fn project_untrust(&self, path: &Path, who: impl Into<Answerer>) -> Result<Trusted> {
        let who = who.into();
        self.judge_act(
            &who,
            Act::Trust {
                method: method::PROJECT_UNTRUST,
                what: &format!("The untrust of the repository holding {}", path.display()),
            },
        )?;
        let root = Self::trust_root(path)?;
        let id = repo_id(&root);
        let mut grants = Grants::read(&self.store)?;
        let grant = grants
            .repos
            .remove(&id)
            .ok_or_else(|| anyhow!("{} is not trusted", root.display()))?;
        let fact = ProjectTrust {
            id: &id,
            root: &grant.root,
            granted: false,
            by: &who.who(),
            via: &who.via(),
        };
        let rec = self.rec(None);
        self.store.append(&[rec.row(&fact)?, grants.record()?])?;
        rec.announce(&fact);
        Ok(Trusted { id, grant })
    }

    /// The trusted repositories, by id.
    pub fn project_trusted(&self) -> Result<Vec<Trusted>> {
        Ok(Grants::read(&self.store)?
            .repos
            .into_iter()
            .map(|(id, grant)| Trusted { id, grant })
            .collect())
    }
}
