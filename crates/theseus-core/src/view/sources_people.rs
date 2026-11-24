//! Who am I talking to, its stable part (slot B; design §5.1), from the ontology's person records: for each
//! person present (the selector chooses them, `super::select::people`), the held person's name, its
//! description, and the owner's own guidance for it. Laid in the front, in shared places too (the owner's
//! word of 2026-11-24). The facts are the owner's own filing, marked so (`FILING`). Nothing new is recorded.

use std::cell::RefCell;
use std::collections::BTreeMap;

use theseus_ontology::{CategoryId, Ontology};

use super::select::sources::{People, Ref};
use super::sources_names::FILING;
use super::text::tokens;
use super::who::cut;
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
    /// Each held person's name, by the ledger's name for them (`super::sources_store::Sessions::names`).
    pub names: &'a RefCell<BTreeMap<String, String>>,
}

impl People for StorePeople<'_> {
    fn held(&self, person: &str) -> anyhow::Result<Option<String>> {
        let handles: Vec<&str> = match person {
            OWNER => self.owner.iter().map(String::as_str).collect(),
            other => vec![other],
        };
        let Some(c) = (handles.iter())
            .find_map(|h| self.ontology.person_by_handle(h))
            .filter(|c| !c.is_gone())
        else {
            return Ok(None);
        };
        (self.names.borrow_mut()).insert(person.to_string(), c.name.clone());
        Ok(Some(c.id.to_string()))
    }

    fn about(&self, id: &str, speaking: bool, most: u64) -> anyhow::Result<Vec<Ref>> {
        let Some(c) = (CategoryId::parse(id).ok())
            .and_then(|id| self.ontology.category(&id))
            .filter(|c| !c.is_gone())
        else {
            return Ok(Vec::new());
        };
        let guidance = (self.ontology.guidance(&c.id)).map_or("", |g| g.text.as_str());
        let text = entry(
            &Person {
                name: &c.name,
                description: &c.description,
                guidance,
                speaking,
            },
            most,
        );
        let line = tokens(text.lines().next().unwrap_or_default());
        let whole = tokens(&text);
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

/// What one person's entry names.
#[derive(Debug, Clone, Copy)]
pub struct Person<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub guidance: &'a str,
    /// The turn's speaker.
    pub speaking: bool,
}

/// One person's entry, under `most` tokens by today's estimate ([`tokens`]): a line with the name (who is
/// speaking, or who else is in this conversation), then the description and the guidance when there are any.
/// As it is when it fits; else each of the three is put on one line and cut (`who::cut`), every one a quarter
/// shorter while the whole is still over, down to the wording alone.
pub fn entry(p: &Person<'_>, most: u64) -> String {
    let whole = lay(p, None);
    if tokens(&whole) < most {
        return whole;
    }
    let mut bytes = [p.name, p.description, p.guidance]
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    loop {
        let block = lay(p, Some(bytes));
        if tokens(&block) < most || bytes == 0 {
            return block;
        }
        bytes = bytes * 3 / 4;
    }
}

/// The entry's lines, each text whole (trimmed) or, with `most`, on one line in at most `most` bytes.
fn lay(p: &Person<'_>, most: Option<usize>) -> String {
    let text = |s: &str| match most {
        Some(m) => cut(s, m),
        None => s.trim().to_string(),
    };
    let head = match p.speaking {
        true => "Who is speaking",
        false => "Also in this conversation",
    };
    let mut lines = vec![format!("{head}: {}", text(p.name))];
    if !p.description.trim().is_empty() {
        lines.push(format!("- Description: {}", text(p.description)));
    }
    if !p.guidance.trim().is_empty() {
        lines.push(format!("- Guidance: {}", text(p.guidance)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ada(speaking: bool) -> Person<'static> {
        Person {
            name: "Ada",
            description: "keeps the harbour's books",
            guidance: "Answer in short lines.\nNo jokes.",
            speaking,
        }
    }

    /// A person's description is laid with the name and the guidance, as they are when they fit.
    #[test]
    fn an_entry_lays_the_description_and_the_guidance_as_they_are() {
        assert_eq!(
            entry(&ada(true), 1_500),
            "Who is speaking: Ada\n- Description: keeps the harbour's books\n\
             - Guidance: Answer in short lines.\nNo jokes."
        );
        assert_eq!(
            entry(&ada(false), 100),
            "Also in this conversation: Ada\n- Description: keeps the harbour's books\n\
             - Guidance: Answer in short lines.\nNo jokes."
        );
        let bare = Person {
            description: " ",
            guidance: "",
            ..ada(false)
        };
        assert_eq!(entry(&bare, 100), "Also in this conversation: Ada");
    }

    /// Long texts are cut to the bound, every cut on a character's boundary and marked, the name first.
    #[test]
    fn an_entry_is_under_its_bound_whatever_its_texts() {
        for c in ["a", "é", "水", "🌊"] {
            let long = c.repeat(4_000);
            let p = Person {
                name: &long,
                description: &long,
                guidance: &long,
                speaking: false,
            };
            for most in [100, 1_500] {
                let e = entry(&p, most);
                assert!(tokens(&e) < most, "{} tokens: {e}", tokens(&e));
                assert!(
                    e.starts_with(&format!("Also in this conversation: {c}")),
                    "{e}"
                );
                assert!(e.contains("...\n- Description: "), "{e}");
                assert_eq!(e.lines().count(), 3, "{e}");
            }
        }
    }
}
