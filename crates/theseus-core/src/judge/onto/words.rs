//! What the owner reads when Theseus keeps track of something new, or asks
//! whether to (theseus-nzwa5): written for someone who is not a model
//! expert. Theseus is the one acting, the conversation is named by its
//! title, the kind of thing is said in plain words, and the whole name is
//! shown. The wording is the owner's, agreed 2026-11-14; it never says a
//! label was applied ("tagged", "filed", "categorized") and never names the
//! component that judged.

use crate::owner_note::{note, Kind};
use crate::scrub::safe;

/// A title as the notice quotes it: one line, cut at a word with an
/// ellipsis when long.
const TITLE_CHARS: usize = 80;

/// The conversation's place in the sentence: its title, or the plain
/// words when it has none yet.
pub(crate) fn in_conversation(title: Option<&str>) -> String {
    let title = title.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "));
    match title.filter(|t| !t.is_empty()) {
        Some(t) if t.chars().count() > TITLE_CHARS => {
            let cuts = crate::scrub::Cuts::live();
            let (cut, _) = safe::head_with(&cuts, &t, TITLE_CHARS);
            // At the last whole word: a word is never cut in two, unless the
            // title's first word is itself longer than the limit. That word
            // end is checked again on the whole title, as a known value may
            // hold the space.
            let whole = match (t[cut.len()..].chars().next(), cut.rsplit_once(' ')) {
                (Some(next), _) if next.is_whitespace() => cut,
                (_, Some((head, _))) if !head.is_empty() => head,
                _ => cut,
            };
            let whole = &t[..cuts.before(&t, whole.len())];
            format!("In \"{}…\"", whole.trim_end())
        }
        Some(t) => format!("In \"{t}\""),
        None => "In a conversation with no title yet".into(),
    }
}

/// What a person calls an entry of `kind` called `name`, in a word.
pub(super) fn kind_word(kind: &str, name: &str) -> &'static str {
    match kind {
        "topic" => "topic",
        "project" => "project",
        "event" => "event",
        "person" => "person",
        "capability" => "skill",
        "expertise" => "area of expertise",
        "resource" => theseus_judge::mine::resource_word(name),
        _ => "thing",
    }
}

fn sure(confidence: f64) -> String {
    format!("How sure it was: {:.0}%", confidence * 100.0)
}

/// The notice that an entry was added: `path` is the entry's whole name,
/// its parents first.
pub(crate) fn minted_text(
    title: Option<&str>,
    kind: &str,
    path: &[String],
    confidence: f64,
    write: &str,
) -> String {
    let name = path.join(" › ");
    let word = kind_word(kind, path.last().map_or("", String::as_str));
    note(
        Kind::Fyi,
        "Theseus started keeping track of something new",
        &format!(
            "{}, this {word} came up and Theseus had no record of it:",
            in_conversation(title)
        ),
        &format!(
            "{name}\nIt added the {word} to the things it knows about. Conversations where it \
             comes up will now share memory more readily.\n{}\nUndo: `theseus ontology undo {write}`",
            sure(confidence)
        ),
    )
}

/// What a question is about.
pub(crate) struct Asked<'a> {
    pub title: Option<&'a str>,
    pub kind: &'a str,
    /// The whole name; none while a new entry has none.
    pub name: Option<&'a str>,
    /// Whether the entry is one Theseus already has.
    pub known: bool,
    /// The command that answers yes, when there is one.
    pub how: Option<&'a str>,
    pub confidence: f64,
}

/// The question Theseus asks when it is not sure enough to act: the same
/// words as the notice, about the same thing, and nothing changed.
pub(crate) fn ask_text(a: &Asked<'_>) -> String {
    let (here, name) = (in_conversation(a.title), a.name.unwrap_or_default());
    let word = kind_word(
        a.kind,
        a.name
            .unwrap_or_default()
            .rsplit(" › ")
            .next()
            .unwrap_or(""),
    );
    let (title, trigger, body) = match (a.kind, a.name, a.known) {
        ("project", Some(_), _) => (
            "Should this conversation be part of a project?",
            format!("{here}, Theseus thinks this conversation belongs to the project:"),
            format!("{name}\nTheseus was not sure enough to attach it, so nothing has changed."),
        ),
        (_, Some(_), true) => (
            "Does this conversation belong with something Theseus knows?",
            format!("{here}, this {word} came up and Theseus already knows it:"),
            format!(
                "{name}\nTheseus was not sure enough to link this conversation to it, so nothing \
                 has changed. Linked, conversations where it comes up share memory more readily."
            ),
        ),
        (_, Some(_), false) => (
            "Should Theseus keep track of something new?",
            format!("{here}, this {word} came up and Theseus has no record of it:"),
            format!(
                "{name}\nTheseus was not sure enough to add it to the things it knows about, so it \
                 has not."
            ),
        ),
        (_, None, _) => (
            "Should Theseus keep track of something new?",
            format!(
                "{here}, something came up that Theseus has no record of, and no name for yet."
            ),
            "Theseus was not sure enough to add it to the things it knows about, so it has not."
                .to_string(),
        ),
    };
    let answer = a.how.map_or_else(String::new, |h| {
        let verb = if a.kind == "project" {
            "attaches"
        } else {
            "links"
        };
        format!("\n`{h}` {verb} it.")
    });
    note(
        Kind::Decision,
        title,
        &trigger,
        &format!("{body}\n{}{answer}", sure(a.confidence)),
    )
}
