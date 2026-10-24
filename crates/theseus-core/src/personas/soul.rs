//! A persona's soul file (theseus-utfnl): the cap on its size, and the two
//! steps that write it: [`Core::plan_soul`] checks the text and says what
//! will be written, [`Plan::write`] writes it.
//!
//! A soul is small: `[context] persona_max_tokens` (2,000) bounds it, because
//! every persona's soul sits in the DM's cached persona layer and a runbook
//! that got in cost a request 26,000 tokens. A write over the cap is refused
//! with the count (never cut). A file already over it, by a hand edit or a
//! migration, is carried whole and flagged.
//!
//! **Only the owner writes a soul.** [`Plan::write`] is reached only
//! from the `persona.*` methods, each judged as an ontology write is: the
//! owner, from a private place, and the CLI refuses them inside a job. No
//! model tool writes a persona or its file. The write is atomic, the
//! replaced file is kept beside it as `<file>.<ms>.bak`, and a
//! `persona.soul` row records the path and both digests.

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::rpc::Core;

/// A text's tokens at the harness's one estimate.
pub fn tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN)
}

/// `Err` naming the count and the cap when `text` is over it.
pub fn within(key: &str, text: &str, cap: u64) -> Result<u64> {
    let n = tokens(text);
    if n > cap {
        bail!(
            "persona {key}'s soul is {n} tokens, over the cap of {cap} ([context] persona_max_tokens): \
             trim it to {cap} tokens. A persona is one small file; a runbook belongs in a project's \
             context, not in every DM's persona layer."
        );
    }
    Ok(n)
}

/// A soul as it is written and what the write replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The path as the category holds it.
    pub path: String,
    pub digest: String,
    /// The digest of the file it replaced, when there was one.
    pub old: Option<String>,
    pub tokens: u64,
}

impl Core {
    /// Where a persona's soul lives when it has none yet:
    /// `<state>/personas/<key>/PERSONA.md`.
    pub(crate) fn soul_home(&self, key: &str) -> PathBuf {
        self.runner
            .state_root()
            .join("personas")
            .join(key)
            .join("PERSONA.md")
    }

    /// Check `text` as `key`'s soul and work out what writing it would do:
    /// the file at `current` (the path the category holds) or at its home,
    /// the text as it will be written, its digest and its tokens. It writes
    /// nothing, so a caller can check the category it will set before any
    /// byte is on the disk.
    pub(crate) fn plan_soul(&self, key: &str, text: &str, current: Option<&str>) -> Result<Plan> {
        let text = match text.trim() {
            "" => bail!(
                "persona {key}'s soul is empty: write its voice, temperament and what it is for"
            ),
            t => format!("{t}\n"),
        };
        let tokens = within(key, &text, self.cfg.context.persona_cap())?;
        let (spelled, path) = match current {
            Some(c) => (c.to_string(), crate::config::expand(c)),
            None => {
                let p = self.soul_home(key);
                (p.display().to_string(), p)
            }
        };
        Ok(Plan {
            spelled,
            path,
            digest: crate::persona::digest(&text),
            text,
            tokens,
        })
    }
}

/// A soul checked and not yet written: [`Core::plan_soul`]'s answer.
#[derive(Debug)]
pub(crate) struct Plan {
    /// The path as the category holds it.
    pub spelled: String,
    pub digest: String,
    path: PathBuf,
    text: String,
    tokens: u64,
}

impl Plan {
    /// Write it: the directory is 0700 and the file 0600; the file is
    /// replaced atomically and the old one kept as `.<ms>.bak`. It touches
    /// the disk: the callers run on the blocking pool.
    pub(crate) fn write(self) -> Result<Written> {
        let old = replace(&self.path, &self.text)
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(Written {
            path: self.spelled,
            digest: self.digest,
            old,
            tokens: self.tokens,
        })
    }
}

/// Atomically replace `path` with `text`, keeping what was there; its old
/// digest.
fn replace(path: &Path, text: &str) -> Result<Option<String>> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let old = std::fs::read_to_string(path).ok();
    if old.is_some() {
        let bak = format!("{}.{}.bak", path.display(), theseus_protocol::now_unix_ms());
        std::fs::copy(path, bak)?;
    }
    let tmp = path.with_extension("md.new");
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(old.map(|o| crate::persona::digest(&o)))
}
