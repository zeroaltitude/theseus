//! `[persona]` (theseus-a8n7): how the owner's agents come over as personas
//! and how an engaged persona's own memories rank. (`[personas.<name>]` is a
//! map of personas, so these keys cannot sit in it.) Every key has a default.
//!
//! - `import_files`: what `theseus persona import` carries from an agent's
//!   workspace, in the order a persona's files compile: a name ending in
//!   `.md` is a file at the workspace's top, any other name is a folder of
//!   curated notes. The files are copied byte for byte.
//! - `own_boost`: while a persona is engaged, the share an imported episode of
//!   its own agent is multiplied by at equal relevance in recall (1.2; 1.0 is
//!   off). Bounded to 1.0 to 2.0.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaSettings {
    #[serde(default = "default_import_files")]
    pub import_files: Vec<String>,
    #[serde(default = "default_own_boost")]
    pub own_boost: f64,
}

fn default_import_files() -> Vec<String> {
    [
        "SOUL.md",
        "IDENTITY.md",
        "USER.md",
        "AGENTS.md",
        "MEMORY.md",
        "memory",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_own_boost() -> f64 {
    1.2
}

impl Default for PersonaSettings {
    fn default() -> Self {
        Self {
            import_files: default_import_files(),
            own_boost: default_own_boost(),
        }
    }
}

impl PersonaSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        if !(1.0..=2.0).contains(&self.own_boost) {
            bail!("persona.own_boost must be between 1.0 (off) and 2.0");
        }
        for f in &self.import_files {
            if f.trim().is_empty() || f.contains('/') || f.contains("..") || f.starts_with('.') {
                bail!("persona.import_files entry {f:?} must be a plain name in the workspace's top folder");
            }
        }
        Ok(())
    }
}
