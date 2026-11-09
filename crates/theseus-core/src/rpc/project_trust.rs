//! The owner's grant of a repository's instruction files (theseus-wadu,
//! phase 2; `docs/design/project-context.md` §7). A grant and its revoke are
//! the owner's acts, judged as a standing record's are (`Act::Standing`: the
//! owner, from a private place), and each is one frame: the META record
//! (`project_context::trust::RECORD`) and its `project.trust` row.
//!
//! The repository is named by any path inside it; its identity is the
//! canonical root's `repo_id`, so a clone elsewhere or a re-pointed remote
//! inherits nothing. A path outside every repository is refused.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use super::confirms::Act;
use super::Core;
use crate::approval::Answerer;
use crate::fact::project::ProjectTrust;
use crate::flow::spec::{repo_id, repo_root};
use crate::project_context::trust::{Grant, Grants};

/// The label the judge's refusals and rows carry.
const WHAT: &str = "project.trust";

/// One trusted repository, as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trusted {
    pub id: String,
    pub grant: Grant,
}

impl Core {
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
            Act::Standing {
                method: WHAT,
                what: "a repository's trust",
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
            Act::Standing {
                method: WHAT,
                what: "a repository's untrust",
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
