//! A session's persona (theseus-slvl.2): `session.open { persona }`. The rest
//! of the persona's methods are in `persona_view.rs`.

use anyhow::{bail, Result};

use super::Core;

impl Core {
    /// `persona`, checked against the ontology: a persona that can answer.
    pub(crate) fn chosen_persona(&self, persona: Option<&str>) -> Result<Option<String>> {
        let Some(name) = persona.map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        let book = self.personas();
        if !book.exists(name) {
            bail!(
                "{name:?} is not a persona that can answer: {}",
                match book.names() {
                    known if known.is_empty() => "none is defined (`theseus persona add`)".into(),
                    known => format!("the personas are {known}"),
                }
            );
        }
        Ok(Some(name.to_string()))
    }

    /// Keep a session's own persona: a META record, and the board's memory.
    pub(crate) fn set_session_persona(&self, session_id: &str, persona: &str) -> Result<()> {
        self.store.put_meta(
            &format!("{}{session_id}", crate::persona::SESSION_PREFIX),
            &persona,
        )?;
        self.runner.persona.choose(&self.store, session_id, persona);
        Ok(())
    }

    /// Read what personas need from the store now, off the start path.
    pub fn warm_persona(self: &std::sync::Arc<Self>) {
        let core = std::sync::Arc::downgrade(self);
        tokio::task::spawn_blocking(move || {
            if let Some(core) = core.upgrade() {
                core.runner.persona.warm(&core.store);
            }
        });
    }
}
