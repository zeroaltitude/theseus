//! The subject beside a same-kind value (theseus-96cyh): a recalled note
//! whose chunk is about another subject of the kind the question asked
//! about (another host, path or crate than the one it names) says whose it
//! is in its header, so the model cannot read the neighbour's value as the
//! asked one. Abstention probes of the recall bench were answered with a
//! near note's value in 28 of 60 runs.
//!
//! The rule is the subject bar's (`theseus_memory::Floor::other_subject`):
//! the question's entities that some hit holds (`asked`), a chunk holding
//! none of them and one of the type of an asked one. It is computed from the
//! candidates alone, so it holds with the floor and the bar off, and it is
//! frozen with the item's header (the same bytes at every later request,
//! compaction's recall included: a note is rendered from its frozen header
//! whenever it is read).

use std::collections::{BTreeMap, BTreeSet};

use theseus_memory::Candidate;
use theseus_protocol::memory::RecallItem;

/// The type of an entity (`host` of `host:db.example`).
fn kind(e: &str) -> &str {
    e.split_once(':').map_or("", |(t, _)| t)
}

/// The words for a chunk holding `entities` when the question asked after
/// `asked`: none unless it is about another subject of an asked one's type.
pub fn note(asked: &[String], entities: &[String]) -> Option<String> {
    if asked.is_empty() || entities.iter().any(|e| asked.contains(e)) {
        return None;
    }
    entities.iter().find_map(|e| {
        let a = asked
            .iter()
            .find(|a| kind(a) == kind(e) && !kind(e).is_empty())?;
        let (t, v) = e.split_once(':')?;
        let (_, av) = a.split_once(':')?;
        Some(format!("about {t} {v}, not the asked {t} {av}"))
    })
}

/// Each candidate's note, by `<node>#<chunk>`, from the entities the
/// question named that some candidate holds.
pub fn marks(candidates: &[Candidate]) -> BTreeMap<String, String> {
    let asked: Vec<String> = candidates
        .iter()
        .flat_map(|c| c.matched.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    candidates
        .iter()
        .filter_map(|c| Some((c.key(), note(&asked, &c.entities)?)))
        .collect()
}

/// Put each admitted item's note (in `marks`) on it.
pub fn apply(items: &mut [RecallItem], marks: &BTreeMap<String, String>) {
    for i in items {
        i.neighbour = marks.get(&format!("{}#{}", i.node_id, i.chunk)).cloned();
    }
}

/// `header` as it is frozen for `item`: its note, if it has one, after the
/// evidence line; the header itself, byte for byte, if not.
pub fn with_note(header: String, item: &RecallItem) -> String {
    match &item.neighbour {
        Some(note) => format!("{header}, {note}"),
        None => header,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|e| e.to_string()).collect()
    }

    /// Another host than the asked one is named beside its value; the asked
    /// host's own note, another type, and a question that named nothing get
    /// none.
    #[test]
    fn a_same_kind_other_subject_is_named() {
        let asked = s(&["host:db-prod.example.test"]);
        assert_eq!(
            note(&asked, &s(&["host:db-stage.example.test"])).as_deref(),
            Some("about host db-stage.example.test, not the asked host db-prod.example.test")
        );
        assert_eq!(
            note(
                &asked,
                &s(&["host:db-prod.example.test", "host:db-stage.example.test"])
            ),
            None
        );
        assert_eq!(note(&asked, &s(&["file:notes.md"])), None);
        assert_eq!(note(&[], &s(&["host:db-stage.example.test"])), None);
        assert_eq!(note(&asked, &[]), None);
    }

    fn cand(id: &str, entities: &[&str], matched: &[&str]) -> Candidate {
        Candidate {
            node_id: id.into(),
            chunk: 0,
            session_id: "ses_a".into(),
            position: 1,
            kind: "user_message".into(),
            origin: "operator".into(),
            external: false,
            text: String::new(),
            fused: 0.03,
            index_rank: 1,
            place: theseus_memory::recall::Place::Private,
            time_ms: 0,
            entities: s(entities),
            matched: s(matched),
        }
    }

    /// A pack with a same-kind neighbour frames the neighbour's header with
    /// its own subject; the note of the asked subject is unchanged.
    #[test]
    fn the_neighbours_header_names_its_subject_and_the_askeds_is_unchanged() {
        let cands = [
            cand(
                "nod_asked",
                &["host:db-prod.example.test"],
                &["host:db-prod.example.test"],
            ),
            cand("nod_near", &["host:db-stage.example.test"], &[]),
        ];
        let mut items: Vec<RecallItem> = cands
            .iter()
            .map(|c| RecallItem {
                node_id: c.node_id.clone(),
                ..RecallItem::default()
            })
            .collect();
        apply(&mut items, &marks(&cands));
        let h = "a message in the CLI, 2026-10-01 (as of @7)".to_string();
        assert_eq!(with_note(h.clone(), &items[0]), h);
        let near = with_note(h.clone(), &items[1]);
        assert!(
            near.starts_with(&h) && near.contains("db-stage.example.test"),
            "{near}"
        );
        assert!(
            near.contains("not the asked host db-prod.example.test"),
            "{near}"
        );
    }
}
