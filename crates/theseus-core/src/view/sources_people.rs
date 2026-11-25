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
use super::text::literal::{literal, rewritten};
use super::text::tokens;
use super::who::NAME_BYTES;
use super::wire_text::TextRenderer;
use super::{
    Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Sizes, SourceRange, Space,
};
use crate::exchange::OWNER;
use crate::scrub::Cuts;
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
    /// The assembly's snapshot of the known values: where a stored name, description or guidance may be cut.
    pub cuts: Cuts,
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
        // The name under the handle met and every handle that names this same record, so a line found under
        // another of theirs carries it; a handle that names another record keeps that record's.
        let name = framing_name(&c.name, &self.cuts);
        let mut names = self.sessions.names.borrow_mut();
        for h in &all {
            if h == person || self.record(h).is_some_and(|r| r.id == c.id) {
                names.insert(h.clone(), name.clone());
            }
        }
        drop(names);
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
            &self.cuts,
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
        let text = format!("{SPEAKING}: {}", one_line(&c.name, NAME_BYTES, &self.cuts));
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

/// The end of a text that was cut.
const MORE: &str = "...";

/// `s` on one line, at most `most` bytes with its [`MORE`] when cut. A cut falls on a character's boundary and
/// never inside a known value (`cuts`); and it falls before any known value the one-line rewrite would change
/// (one holding a line break or another control character), so every value kept is whole and as written.
pub fn one_line(s: &str, most: usize, cuts: &Cuts) -> String {
    let s = s.trim();
    let flat = |t: &str| -> String {
        t.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let keep = cuts.unchanged(s, char::is_control);
    if keep == s.len() && s.len() <= most {
        return flat(s).trim().to_string();
    }
    let mut at = most.saturating_sub(MORE.len()).min(keep);
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}{MORE}", flat(&s[..cuts.before(s, at)]).trim_end())
}

/// A person's name as the wrapper of their words names its source: on one line ([`one_line`]), at most
/// [`NAME_IN_WRAPPER_BYTES`] once quoted and escaped (`text::literal`). The cut falls where `cuts` says, and before
/// any known value the escaping would change, so the name reaches both of its places in the wrapper already cut.
pub fn framing_name(name: &str, cuts: &Cuts) -> String {
    let name = one_line(name, usize::MAX, cuts);
    let keep = cuts.unchanged(&name, rewritten);
    let mut spent = 0;
    let mut end = name.len();
    for (at, c) in name.char_indices() {
        let quoted = literal(c.encode_utf8(&mut [0; 4]), 1).len() - 2;
        if spent + quoted > NAME_IN_WRAPPER_BYTES {
            end = at;
            break;
        }
        spent += quoted;
    }
    name[..cuts.before(&name, end.min(keep))]
        .trim_end()
        .to_string()
}

/// What one person's entry names.
#[derive(Debug, Clone, Copy)]
pub struct Person<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub guidance: &'a str,
}

/// One person's entry, under `most` tokens by today's estimate ([`tokens`]): a line with the name, the same
/// wording for every person ([`PRESENT`]), then the description and the guidance when there are any. The name
/// is always on its one line, so the entry's first line is a whole one. The rest as it is when it fits; else
/// each of the three is put on one line and cut ([`one_line`], where `cuts` lets it), every one a quarter
/// shorter while the whole is still over, down to the wording alone.
pub fn entry(p: &Person<'_>, most: u64, cuts: &Cuts) -> String {
    let whole = lay(p, None, cuts);
    if tokens(&whole) < most {
        return whole;
    }
    let mut bytes = [p.name, p.description, p.guidance]
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0);
    loop {
        let block = lay(p, Some(bytes), cuts);
        if tokens(&block) < most || bytes == 0 {
            return block;
        }
        bytes = bytes * 3 / 4;
    }
}

/// The entry's lines: the name on one line; the others whole (trimmed) or, with `most`, each on one line in at
/// most `most` bytes.
fn lay(p: &Person<'_>, most: Option<usize>, cuts: &Cuts) -> String {
    let text = |s: &str| match most {
        Some(m) => one_line(s, m, cuts),
        None => s.trim().to_string(),
    };
    let name = one_line(p.name, most.unwrap_or(usize::MAX), cuts);
    let mut lines = vec![format!("{PRESENT}: {name}")];
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
    use crate::scrub::Scrubber;
    use crate::tests_cut_sites::{whole_or_absent, V};
    use crate::view::text::labels::{outside_heading, wrap_outside};

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
        assert_eq!(entry(&ada(), 1_500, &Cuts::none()), whole);
        assert_eq!(entry(&ada(), 100, &Cuts::none()), whole);
        let bare = Person {
            description: " ",
            guidance: "",
            ..ada()
        };
        assert_eq!(
            entry(&bare, 100, &Cuts::none()),
            "In this conversation: Ada"
        );
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
                let e = entry(&p, most, &Cuts::none());
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
        let none = Cuts::none();
        assert_eq!(framing_name("Wren Hallam", &none), "Wren Hallam");
        assert_eq!(framing_name(" Wren\nHallam ", &none), "Wren Hallam");
        for c in ["[", "\u{200b}", "\"", "水"] {
            let n = framing_name(&c.repeat(400), &none);
            let quoted = super::super::text::literal::literal(&n, usize::MAX);
            assert!(quoted.len() - 2 <= NAME_IN_WRAPPER_BYTES, "{quoted}");
            assert!(
                !n.is_empty() && n.chars().all(|x| x.to_string() == c),
                "{n}"
            );
        }
    }

    /// KEY: a known value a cut would fall in, in a person's name, description or guidance (one holding a line
    /// break among them), is laid whole or not at all: in the entry and its first line, in the speaker's line,
    /// and in the name the wrapper of their words gives twice.
    #[test]
    fn a_known_value_in_a_persons_texts_is_laid_whole_or_not_at_all() {
        for v in [V, "kqzv-0417-wjxp\n9932-hmfb-ylno"] {
            let cuts = Scrubber::with_values(vec![(v.into(), "v".into())]).cuts();
            for before in [0, 60, 100, 110, 300, 310] {
                let text = format!("{}{v} and more", "x".repeat(before));
                let long = format!("{text} {}", "y ".repeat(2_000));
                for field in 0..3 {
                    let pick = |f: usize| if f == field { long.as_str() } else { "Ada" };
                    let p = Person {
                        name: pick(0),
                        description: pick(1),
                        guidance: pick(2),
                    };
                    for most in [100, 1_500, 100_000] {
                        let e = entry(&p, most, &cuts);
                        whole_or_absent("the entry", &e, v);
                        whole_or_absent("its first line", e.lines().next().unwrap(), v);
                    }
                }
                whole_or_absent("the speaker's line", &one_line(&text, NAME_BYTES, &cuts), v);
                let n = framing_name(&text, &cuts);
                let laid = format!(
                    "{}\n{}",
                    outside_heading(Some(&n)),
                    wrap_outside(Some(&n), "Morning.")
                );
                whole_or_absent("the wrapper's name", &laid, v);
            }
        }
    }
}
