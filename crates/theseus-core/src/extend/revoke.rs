//! `extension.revoke { name }` (M7 43b, design §2.7): the operator's act,
//! judged as an answer is (`judge_act(Act::Revoke)`: the owner, from a
//! private place; the CLI refuses it in a job's shell). Reached by `theseus
//! extend revoke <name>` and the Revoke button on Discord's `/extensions`.
//!
//! One frame: the `extensions` record without it, its manifest `revoked`,
//! and `extend.revoked`. Then the board stops its server (SIGTERM to its
//! group: in L1, its whole namespace) and drops its tools from the catalog,
//! so the next turn is not offered them; a turn already running keeps its
//! spec, and a call of it there fails as its server is gone. The frozen
//! copy stays on disk.

use anyhow::Result;
use theseus_protocol::extend::{ExtensionRevokeParams, ExtensionRevokeResult};

use super::load::LoadedSet;
use super::{manifest_key, Manifest};
use crate::approval::Answerer;
use crate::fact::extend::ExtendRevoked;
use crate::rpc::{Act, Core};

impl Core {
    /// Revoke the loaded extension `p.name`, by `who`.
    pub fn extension_revoke(
        &self,
        p: &ExtensionRevokeParams,
        who: impl Into<Answerer>,
    ) -> Result<ExtensionRevokeResult> {
        let who = who.into();
        let _writes = self
            .tools
            .extend
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut set = LoadedSet::read(&self.store)?;
        let Some(l) = set.loaded.remove(&p.name) else {
            anyhow::bail!(
                "no extension named {:?} is loaded (`theseus extend list` lists them)",
                p.name
            );
        };
        self.judge_act(&who, Act::Revoke { name: &p.name })?;
        let (by, via) = (who.who(), who.via());
        let manifest = self
            .store
            .get_meta::<Manifest>(&manifest_key(&l.name, &l.digest))?
            .map(|mut m| {
                m.state = "revoked".into();
                m.note = Some(format!("revoked by {by}"));
                m
            });
        let fact = ExtendRevoked {
            name: &l.name,
            digest: &l.digest,
            tools: &l.tools,
            by: &by,
            via: &via,
        };
        let session = l.proposed_by.session_id.as_str();
        let mut rows = vec![crate::fact::row(&fact, Some(session), None)?, set.record()?];
        if let Some(m) = &manifest {
            rows.push(m.record()?);
        }
        self.store.append(&rows)?;
        let server = l.server();
        self.tools.extend.floors.clear(&server);
        self.mcp.unload_extension(&server);
        self.session_rec(session).announce(&fact);
        Ok(ExtensionRevokeResult {
            name: l.name.clone(),
            digest: l.digest.clone(),
            tools: l.canonical(),
            frozen: l.frozen.clone(),
        })
    }
}
