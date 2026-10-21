//! The words of what Theseus says first when no model writes them: one table,
//! so they follow the persona. A template names no character: it reads the
//! persona's `name` and `emoji` and nothing else of it, so an owner's
//! persona changes every line by changing those two fields. A pure status
//! line (a budget, a ceiling) stays its own words; these are the lines that
//! stand in for a voiced message when `[initiative] day_usd` is spent, the
//! model is unreachable, or the owner turned the model off with 0.

use crate::config::PersonaView;

/// What the line says something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The day's note: `n` things happened since the last one.
    Morning,
    /// The same, on a day a draft waits for the owner's send.
    MorningWaiting,
    /// A consolidation pass wrote `n` notes.
    Consolidation,
}

/// The table: a line's kind, its words with `{who}` (the emoji, then the name)
/// and `{n}` (how many) in them, and its plural form.
const TABLE: &[(Line, &str, &str)] = &[
    (
        Line::Morning,
        "{who}: one thing happened since yesterday. Nothing needs you.",
        "{who}: {n} things happened since yesterday. Nothing needs you.",
    ),
    (
        Line::MorningWaiting,
        "{who}: one thing today, and a draft waits for your send.",
        "{who}: {n} things today, and drafts wait for your send.",
    ),
    (
        Line::Consolidation,
        "{who}: tonight's memory pass wrote one new note from what we talked about.",
        "{who}: tonight's memory pass wrote {n} new notes from what we talked about.",
    ),
];

/// `🧭 Name` with an emoji, `Name` without.
pub fn who(p: &PersonaView) -> String {
    match p.emoji.as_deref() {
        Some(e) => format!("{e} {}", p.name),
        None => p.name.clone(),
    }
}

/// The words of `line` for `n` of its thing, in `p`'s name.
pub fn say(line: Line, n: u64, p: &PersonaView) -> String {
    let (_, one, many) = TABLE
        .iter()
        .find(|(l, _, _)| *l == line)
        .expect("every line is in the table");
    let template = if n == 1 { one } else { many };
    template
        .replace("{who}", &who(p))
        .replace("{n}", &n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(name: &str, emoji: Option<&str>) -> PersonaView {
        PersonaView {
            name: name.into(),
            emoji: emoji.map(str::to_string),
            ..PersonaView::built_in()
        }
    }

    #[test]
    fn every_line_is_in_the_table_and_names_the_persona() {
        for line in [Line::Morning, Line::MorningWaiting, Line::Consolidation] {
            for n in [1, 4] {
                let said = say(line, n, &persona("Ondine", Some("🌊")));
                assert!(said.starts_with("🌊 Ondine: "), "{said}");
                assert!(!said.contains('{'), "{said}");
            }
        }
    }

    #[test]
    fn a_persona_without_an_emoji_is_its_name_alone() {
        assert_eq!(
            say(Line::Morning, 3, &persona("Marlow", None)),
            "Marlow: 3 things happened since yesterday. Nothing needs you."
        );
        assert_eq!(
            say(Line::Consolidation, 1, &PersonaView::built_in()),
            "Theseus: tonight's memory pass wrote one new note from what we talked about."
        );
    }
}
