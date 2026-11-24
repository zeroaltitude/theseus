//! Who am I talking to, its stable part (slot B; design §5.1), from the ontology's person records: for each
//! person present (the selector chooses them, `super::select::people`), the held person's name, its
//! description, and the owner's own guidance for it, the same whoever speaks; and beside the message the one
//! line that names the speaker. Laid in the front, in shared places too (the owner's word of 2026-11-24). The
//! facts are the owner's own filing, marked so (`FILING`), and headed with no time
//! (`text::labels::filing_block`). Nothing new is recorded.

use std::collections::BTreeSet;

use theseus_ontology::person::is_name;
use theseus_ontology::{handles_of, Category, CategoryId, Ontology};

use super::select::sources::{Held, People, Ref};
use super::sources_names::FILING;
use super::sources_store::Sessions;
use super::text::tokens;
use super::who::{cut, NAME_BYTES};
use super::wire_text::TextRenderer;
use super::{
    Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Sizes, SourceRange, Space,
};
use crate::exchange::OWNER;
use crate::store::Store;

/// The most bytes a person's name takes in the wrapper of their words, quoted and escaped as framing
/// (`text::literal`): twice in each recent line, so a name of brackets cannot spend the lines' bound.
pub const NAME_IN_WRAPPER_BYTES: usize = 120;

/// How every person's entry in the front begins: one wording for all, whoever speaks.
pub const PRESENT: &str = "In this conversation";
/// The line beside the message that names the speaker.
pub const SPEAKING: &str = "Who is speaking";

/// The view's [`People`] over the ontology.
pub struct StorePeople<'a> {
    pub ontology: std::sync::Arc<Ontology>,
    /// The place rule's effective owner handles (`PlaceRule::owners`: `[places] owner`, else the person of
    /// each DM the bindings file binds).
    pub owner: Vec<String>,
    pub now_ms: u64,
    pub store: &'a Store,
    /// The assembly's reads: each text read, by its space (the renderer lays it, `wire_text`), and each held
    /// person's name by the ledger's name for them.
    pub sessions: &'a Sessions,
}

impl StorePeople<'_> {
    /// The held record `handle` names, if a live one does.
    fn record(&self, handle: &str) -> Option<&Category> {
        (self.ontology.person_by_handle(handle)).filter(|c| !c.is_gone())
    }

    /// The records the effective owner handles name: the owner's, by identity.
    fn owners(&self) -> BTreeSet<CategoryId> {
        (self.owner.iter())
            .filter_map(|h| self.record(h))
            .map(|c| c.id.clone())
            .collect()
    }

    fn category(&self, id: &str) -> Option<&Category> {
        (CategoryId::parse(id).ok())
            .and_then(|id| self.ontology.category(&id))
            .filter(|c| !c.is_gone())
    }

    /// One piece of the owner's filing about `c`, its text `text`, kept under `space`.
    fn filed(&self, c: &Category, space: Space, text: String) -> Ref {
        let line = tokens(text.lines().next().unwrap_or_default());
        let whole = tokens(&text);
        self.sessions.texts.borrow_mut().insert(space.clone(), text);
        let pos = Pos {
            at_ms: self.now_ms,
            id: c.id.to_string(),
        };
        Ref {
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
        }
    }
}

impl People for StorePeople<'_> {
    fn held(&self, person: &str) -> anyhow::Result<Option<Held>> {
        let handles: Vec<&str> = match person {
            OWNER => self.owner.iter().map(String::as_str).collect(),
            other => vec![other],
        };
        let Some(c) = handles.iter().find_map(|h| self.record(h)) else {
            return Ok(None);
        };
        let mut all: Vec<String> = (handles_of(c).into_iter())
            .filter(|h| !is_name(h))
            .collect();
        if !all.iter().any(|h| h == person) {
            all.push(person.to_string());
        }
        (self.sessions.names.borrow_mut()).insert(person.to_string(), framing_name(&c.name));
        Ok(Some(Held {
            id: c.id.to_string(),
            owner: self.owner(person) || self.owners().contains(&c.id),
            handles: all,
        }))
    }

    fn owner(&self, person: &str) -> bool {
        person == OWNER || self.owner.iter().any(|h| h == person)
    }

    fn about(&self, id: &str, most: u64) -> anyhow::Result<Vec<Ref>> {
        let Some(c) = self.category(id) else {
            return Ok(Vec::new());
        };
        let guidance = (self.ontology.guidance(&c.id)).map_or("", |g| g.text.as_str());
        let text = entry(
            &Person {
                name: &c.name,
                description: &c.description,
                guidance,
            },
            most,
        );
        let space = Space::Facts {
            thing: c.id.to_string(),
        };
        Ok(vec![self.filed(c, space, text)])
    }

    fn speaking(&self, id: &str) -> anyhow::Result<Vec<Ref>> {
        let Some(c) = self.category(id) else {
            return Ok(Vec::new());
        };
        let space = Space::Facts {
            thing: format!("{}#speaking", c.id),
        };
        let text = format!("{SPEAKING}: {}", cut(&c.name, NAME_BYTES));
        Ok(vec![self.filed(c, space, text)])
    }

    fn laid(&self, piece: &Piece, pov: &PointOfView) -> anyhow::Result<u64> {
        let renderer = TextRenderer {
            store: self.store,
            kernel: None,
            sessions: Some(self.sessions),
            zone: &crate::recall::stamp::system,
        };
        renderer.recent_tokens(piece, pov)
    }
}

/// A person's name as the wrapper of their words names its source: on one line, at most
/// [`NAME_IN_WRAPPER_BYTES`] once quoted and escaped (`text::literal`), cut on a character's boundary.
pub fn framing_name(name: &str) -> String {
    let name = cut(name, usize::MAX);
    let mut spent = 0;
    let mut end = name.len();
    for (at, c) in name.char_indices() {
        let quoted = super::text::literal::literal(c.encode_utf8(&mut [0; 4]), 1).len() - 2;
        if spent + quoted > NAME_IN_WRAPPER_BYTES {
            end = at;
            break;
        }
        spent += quoted;
    }
    name[..end].trim_end().to_string()
}

/// What one person's entry names.
#[derive(Debug, Clone, Copy)]
pub struct Person<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub guidance: &'a str,
}

/// One person's entry, under `most` tokens by today's estimate ([`tokens`]): a line with the name, the same
/// wording for every person ([`PRESENT`]), then the description and the guidance when there are any. As it
/// is when it fits; else each of the three is put on one line and cut (`who::cut`), every one a quarter
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
    let mut lines = vec![format!("{PRESENT}: {}", text(p.name))];
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

    fn ada() -> Person<'static> {
        Person {
            name: "Ada",
            description: "keeps the harbour's books",
            guidance: "Answer in short lines.\nNo jokes.",
        }
    }

    /// A person's description is laid with the name and the guidance, as they are when they fit, in one
    /// wording whoever speaks.
    #[test]
    fn an_entry_lays_the_description_and_the_guidance_as_they_are() {
        let whole = "In this conversation: Ada\n- Description: keeps the harbour's books\n\
                     - Guidance: Answer in short lines.\nNo jokes.";
        assert_eq!(entry(&ada(), 1_500), whole);
        assert_eq!(entry(&ada(), 100), whole);
        let bare = Person {
            description: " ",
            guidance: "",
            ..ada()
        };
        assert_eq!(entry(&bare, 100), "In this conversation: Ada");
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
            };
            for most in [100, 1_500] {
                let e = entry(&p, most);
                assert!(tokens(&e) < most, "{} tokens: {e}", tokens(&e));
                assert!(e.starts_with(&format!("{PRESENT}: {c}")), "{e}");
                assert!(e.contains("...\n- Description: "), "{e}");
                assert_eq!(e.lines().count(), 3, "{e}");
            }
        }
    }

    /// A name in a wrapper is bounded as it is laid: an ordinary one whole, one of brackets or of hidden
    /// characters cut where its escaped form would pass the bound, never inside a character.
    #[test]
    fn a_name_in_framing_is_bounded_after_escaping() {
        assert_eq!(framing_name("Wren Hallam"), "Wren Hallam");
        assert_eq!(framing_name(" Wren\nHallam "), "Wren Hallam");
        for c in ["[", "\u{200b}", "\"", "水"] {
            let n = framing_name(&c.repeat(400));
            let quoted = super::super::text::literal::literal(&n, usize::MAX);
            assert!(quoted.len() - 2 <= NAME_IN_WRAPPER_BYTES, "{quoted}");
            assert!(
                !n.is_empty() && n.chars().all(|x| x.to_string() == c),
                "{n}"
            );
        }
    }
}
