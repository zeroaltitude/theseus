//! Whether a repository's instruction text may enter the prompt
//! (`docs/design/project-context.md` §7, §8). The default is closed: a
//! repository is trusted only by a record in the store, written by the
//! owner's own act.
//!
//! **Where the grant lives.** One META record, [`RECORD`], a map from
//! `flow::spec::repo_id(root)` (`r:<hex>` of the canonical root) to who
//! granted it, how, and when. It is the store, not the configuration file:
//! the tool floor floors the store, and does not floor the live config, so a
//! grant in `theseus.toml` would be writable by a tool call. Keyed on the
//! canonical root, so a re-pointed remote or a clone elsewhere inherits
//! nothing. A grant and its `project.trust` row are one frame.
//!
//! **What a grant is not.** It names no file and no digest: it trusts the
//! repository's instruction files as they change, the design's "notify
//! rather than ask" (§8). The block's header records each file's digest.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

/// The META key of the grants.
pub const RECORD: &str = "project.trust";

/// One grant, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The canonical root it was given for, for the owner reading the list.
    pub root: String,
    pub by: String,
    pub via: String,
    pub at_ms: u64,
}

/// The META record: every trusted repository, by `repo_id`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grants {
    pub repos: BTreeMap<String, Grant>,
}

impl Grants {
    pub fn read(store: &crate::store::Store) -> Result<Self> {
        Ok(store.get_meta::<Self>(RECORD)?.unwrap_or_default())
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(RECORD), self)
    }
}

/// Has the owner granted this repository's instruction text the authority
/// of the system block? One keyed read; a store that fails to answer is no.
pub fn granted(store: &crate::store::Store, root: &Path) -> bool {
    Grants::read(store).is_ok_and(|g| g.repos.contains_key(&crate::flow::spec::repo_id(root)))
}
