//! Facts about the person speaking (slot B; design §5.1), from the ontology's person records: the held
//! person the speaker is (the owner by the place rule's owner handles, another person by the chat author's
//! handle), as its name, its role line and the owner's guidance for it. Nothing new is recorded.
//!
//! A person record holds role facts only, never an evaluation (`theseus_ontology::person`): a role line
//! the people pack judged evaluative or sensitive was dropped before it was proposed, and nothing here
//! reads anything else of a person. The facts are the owner's own filing, marked so (`FILING`): the gate
//! admits them to the owner alone.

use std::cell::RefCell;
use std::collections::BTreeMap;

use theseus_ontology::Ontology;

use super::select::sources::{People, Ref};
use super::sources_names::FILING;
use super::text::tokens;
use super::{Audience, Author, Form, Handle, Marks, Pos, Sizes, SourceRange, Space};
use crate::exchange::OWNER;

/// The view's [`People`] over the ontology.
pub struct StorePeople<'a> {
    pub ontology: std::sync::Arc<Ontology>,
    /// The place rule's owner handles (`[places] owner`).
    pub owner: Vec<String>,
    pub now_ms: u64,
    /// Each text read, by its space; the renderer lays it (`wire_text`).
    pub texts: &'a RefCell<BTreeMap<Space, String>>,
}

impl People for StorePeople<'_> {
    fn about(&self, person: &str) -> anyhow::Result<Vec<Ref>> {
        let handles: Vec<&str> = match person {
            OWNER => self.owner.iter().map(String::as_str).collect(),
            other => vec![other],
        };
        let Some(c) = (handles.iter())
            .find_map(|h| self.ontology.person_by_handle(h))
            .filter(|c| !c.is_gone())
        else {
            return Ok(Vec::new());
        };
        let mut lines = vec![format!("Who is speaking: {}", c.name)];
        if !c.description.trim().is_empty() {
            lines.push(format!("- {}", c.description.trim()));
        }
        if let Some(g) = self.ontology.guidance(&c.id) {
            if !g.text.trim().is_empty() {
                lines.push(format!("- Guidance: {}", g.text.trim()));
            }
        }
        let text = lines.join("\n");
        let (whole, line) = (tokens(&text), tokens(&lines[0]));
        let space = Space::Facts {
            thing: c.id.to_string(),
        };
        self.texts.borrow_mut().insert(space.clone(), text);
        let pos = Pos {
            at_ms: self.now_ms,
            id: c.id.to_string(),
        };
        Ok(vec![Ref {
            range: SourceRange {
                space,
                first: pos.clone(),
                last: pos,
            },
            marks: Marks {
                place: Some(FILING.into()),
                audience: Audience::Private,
                author: Author::Theseus,
                outside: false,
                sealed: false,
                veiled: false,
            },
            sizes: Sizes {
                whole,
                spoken: whole,
                line,
            },
            handle: Handle(c.id.to_string()),
            form: Form::Whole,
            unit: None,
        }])
    }
}
