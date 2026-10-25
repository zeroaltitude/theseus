//! `persona.soul_read` (theseus-lky63): a soul file as it stands, or one of
//! the backups beside it, so an editor can open the whole file the way the
//! CLI's `persona soul` does, and write it back with `expect_digest`.
//!
//! Judged as the writes are (`judge_persona`: the owner, from a private
//! place): a soul can hold personal text, so it is read only where it may be
//! written. A caller never names a path: `bak_ms` picks one of the listed
//! backups of the persona's own file.

use std::path::Path;

use anyhow::{bail, Context, Result};
use theseus_protocol::{
    method, PersonaSoulBackup, PersonaSoulReadParams, PersonaSoulReadResult, SOUL_BACKUPS_SHOWN,
};

use crate::approval::Answerer;
use crate::rpc::Core;

use super::soul::tokens;

/// The backups of `path` (`<file name>.<ms>.bak` in its directory), newest
/// first, at most [`SOUL_BACKUPS_SHOWN`].
pub(crate) fn backups_of(path: &Path) -> Vec<PersonaSoulBackup> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut found: Vec<PersonaSoulBackup> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.file_name();
            let ms = file
                .to_str()?
                .strip_prefix(name)?
                .strip_prefix('.')?
                .strip_suffix(".bak")?;
            if ms.is_empty() || !ms.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            Some(PersonaSoulBackup {
                ms: ms.parse().ok()?,
                bytes: meta.len(),
                tokens: tokens_of_len(meta.len()),
            })
        })
        .collect();
    found.sort_by_key(|b| std::cmp::Reverse(b.ms));
    found.truncate(SOUL_BACKUPS_SHOWN);
    found
}

/// `soul::tokens` of a text of `len` bytes.
fn tokens_of_len(len: u64) -> u64 {
    len.div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

impl Core {
    /// `persona.soul_read`.
    pub fn persona_soul_read(
        &self,
        p: &PersonaSoulReadParams,
        who: &Answerer,
    ) -> Result<PersonaSoulReadResult> {
        self.judge_persona(
            method::PERSONA_SOUL_READ,
            &format!("the soul of persona {}", p.key),
            who,
        )?;
        let key = p.key.trim();
        let c = self.persona_category(key)?;
        let cap = self.cfg.context.persona_cap();
        let mut out = PersonaSoulReadResult {
            key: key.to_string(),
            cap,
            ..Default::default()
        };
        let Some(spelled) = c.persona.as_ref().and_then(|c| c.soul.clone()) else {
            if p.bak_ms.is_some() {
                bail!("persona {key:?} awaits its soul: it has no backups");
            }
            return Ok(out);
        };
        let path = crate::config::expand(&spelled);
        out.backups = backups_of(&path);
        out.path = Some(spelled);
        out.live_digest = std::fs::read_to_string(&path)
            .ok()
            .map(|t| crate::persona::digest(&t));
        let read = match p.bak_ms {
            None => path,
            Some(ms) => {
                if !out.backups.iter().any(|b| b.ms == ms) {
                    bail!(
                        "persona {key:?} has no backup {ms}: `persona.soul_read` lists the backups it can read"
                    );
                }
                std::path::PathBuf::from(format!("{}.{ms}.bak", path.display()))
            }
        };
        let text = std::fs::read_to_string(&read)
            .with_context(|| format!("reading the soul of {key:?}"))?;
        out.bytes = text.len() as u64;
        out.tokens = tokens(&text);
        out.digest = Some(crate::persona::digest(&text));
        out.text = Some(text);
        Ok(out)
    }
}
