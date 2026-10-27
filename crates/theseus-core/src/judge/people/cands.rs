//! Pairs of people that may be one person, found without a model and put to
//! the owner, never merged on their own (theseus-jrffh, step 3). The name key
//! (`same.rs`) already joins people whose names differ only in spacing; this
//! pass reads what it leaves: a first name that heads one full name, a handle
//! two people hold, a role line two share. A pair the owner recorded as not
//! the same, or whose two names one message says together, is no candidate.
//! The pass is pure: `rpc/people_cands.rs` reads the ontology and the
//! messages and calls it.

use std::collections::{BTreeSet, HashSet};

use super::name_words;
use super::same::{name_key, pair_key};

/// What the pass reads of a person.
#[derive(Debug, Clone, Default)]
pub struct Who {
    pub id: String,
    pub name: String,
    /// Handles other than `name:` ones (`discord:`, `slack:`, `email:`).
    pub handles: Vec<String>,
    /// The role line (the person's description).
    pub role: String,
    /// The sessions whose person lists hold them.
    pub sessions: BTreeSet<String>,
}

/// One pair put to the owner.
#[derive(Debug, Clone, PartialEq)]
pub struct Cand {
    pub a: String,
    pub b: String,
    /// `handle`, `first_name`, `ambiguous` or `role`.
    pub kind: &'static str,
    pub score: f64,
    pub why: String,
    /// For `ambiguous`: every full name the short one could be.
    pub could_be: Vec<String>,
}

/// A shared handle is nearly certain; a first name that heads one full name is
/// likely; a role line and a shared session is weak; a first name that heads
/// several is a question, with its names.
const HANDLE: f64 = 0.95;
const FIRST_NAME: f64 = 0.6;
const ROLE: f64 = 0.35;
const AMBIGUOUS: f64 = 0.3;

fn words(name: &str) -> Vec<String> {
    name_words(name).map(str::to_lowercase).collect()
}

/// How many times `phrase` stands among `text`'s words.
fn occurrences(text: &[String], phrase: &[String]) -> usize {
    match phrase.is_empty() || phrase.len() > text.len() {
        true => 0,
        false => text.windows(phrase.len()).filter(|w| *w == phrase).count(),
    }
}

/// Whether one message names both people: each name stands in the text, and
/// where one name heads the other (`Juno`, `Juno Pike`) the short one stands
/// apart from the long one at least once. Two names in one message are
/// evidence of two people.
pub fn named_together(text: &str, x: &str, y: &str) -> bool {
    let (t, x, y) = (words(text), words(x), words(y));
    if x.is_empty() || y.is_empty() || x == y {
        return false;
    }
    let (short, long) = match x.len() <= y.len() {
        true => (&x, &y),
        false => (&y, &x),
    };
    let (ns, nl) = (occurrences(&t, short), occurrences(&t, long));
    match long.starts_with(short) {
        true => nl > 0 && ns > nl,
        false => ns > 0 && nl > 0,
    }
}

/// The candidate pairs among `people`, best first, and how many were left out
/// as forbidden (`forbidden`, `same::pair_key`s) or apart (`apart`: one
/// message names both).
pub fn candidates(
    people: &[Who],
    forbidden: &HashSet<String>,
    apart: &dyn Fn(&Who, &Who) -> bool,
) -> (Vec<Cand>, usize) {
    let full: Vec<&Who> = people.iter().filter(|p| words(&p.name).len() > 1).collect();
    let mut out = Vec::new();
    let mut left_out = 0;
    for (i, a) in people.iter().enumerate() {
        for b in &people[i + 1..] {
            let Some(c) = pair(a, b, &full) else { continue };
            // An equal key is the automatic merge's, not a question.
            if name_key(&a.name) == name_key(&b.name) {
                continue;
            }
            if forbidden.contains(&pair_key(&a.id, &b.id)) || apart(a, b) {
                left_out += 1;
                continue;
            }
            out.push(c);
        }
    }
    out.sort_by(|x, y| {
        y.score
            .total_cmp(&x.score)
            .then_with(|| (&x.a, &x.b).cmp(&(&y.a, &y.b)))
    });
    (out, left_out)
}

/// The best reason `a` and `b` may be one person, if any.
fn pair(a: &Who, b: &Who, full: &[&Who]) -> Option<Cand> {
    let mk = |kind, score, why: String, could_be: Vec<String>| Cand {
        a: a.id.clone(),
        b: b.id.clone(),
        kind,
        score,
        why,
        could_be,
    };
    if let Some(h) = a.handles.iter().find(|h| b.handles.contains(h)) {
        return Some(mk("handle", HANDLE, format!("both hold {h}"), Vec::new()));
    }
    let (wa, wb) = (words(&a.name), words(&b.name));
    let (short, long) = match (wa.len(), wb.len()) {
        (1, n) if n > 1 => (Some((a, &wa)), Some((b, &wb))),
        (n, 1) if n > 1 => (Some((b, &wb)), Some((a, &wa))),
        _ => (None, None),
    };
    if let (Some((s, sw)), Some((l, lw))) = (short, long) {
        if lw.first() == sw.first() {
            // The distinct full names this first name heads.
            let heads: BTreeSet<String> = full
                .iter()
                .filter(|p| words(&p.name).first() == sw.first())
                .map(|p| name_key(&p.name))
                .collect();
            return Some(match heads.len() {
                1 => mk(
                    "first_name",
                    FIRST_NAME,
                    format!("\"{}\" is the first name of \"{}\"", s.name, l.name),
                    Vec::new(),
                ),
                _ => {
                    let mut names: Vec<String> = full
                        .iter()
                        .filter(|p| words(&p.name).first() == sw.first())
                        .map(|p| p.name.clone())
                        .collect();
                    names.sort();
                    names.dedup();
                    mk(
                        "ambiguous",
                        AMBIGUOUS,
                        format!(
                            "\"{}\" heads {} full names: ask, never merge",
                            s.name,
                            names.len()
                        ),
                        names,
                    )
                }
            });
        }
    }
    let role = |p: &Who| {
        p.role
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let r = role(a);
    if !r.is_empty() && r == role(b) && a.sessions.intersection(&b.sessions).next().is_some() {
        return Some(mk(
            "role",
            ROLE,
            format!(
                "the same role line (\"{}\") and a shared session",
                a.role.trim()
            ),
            Vec::new(),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(id: &str, name: &str) -> Who {
        Who {
            id: format!("person:{id}"),
            name: name.into(),
            ..Default::default()
        }
    }

    fn none(_: &Who, _: &Who) -> bool {
        false
    }

    #[test]
    fn a_first_name_heading_one_full_name_is_a_candidate() {
        let p = [
            who("j", "Juno"),
            who("jp", "Juno Pike"),
            who("q", "Quill Marsh"),
        ];
        let (c, out) = candidates(&p, &HashSet::new(), &none);
        assert_eq!((c.len(), out), (1, 0), "{c:?}");
        assert_eq!((c[0].kind, c[0].a.as_str()), ("first_name", "person:j"));
        assert!(c[0].could_be.is_empty());
    }

    /// Two full names under one first name: asked, with both names, and each
    /// pair is `ambiguous`, never `first_name`.
    #[test]
    fn a_first_name_heading_two_full_names_asks_and_names_them() {
        let p = [
            who("b", "Bram"),
            who("bt", "Bram Teller"),
            who("bb", "Bram Bell"),
        ];
        let (c, _) = candidates(&p, &HashSet::new(), &none);
        assert_eq!(c.len(), 2, "{c:?}");
        for x in &c {
            assert_eq!(x.kind, "ambiguous");
            assert_eq!(x.could_be, ["Bram Bell", "Bram Teller"]);
            assert!(x.score < FIRST_NAME);
        }
        // Two full names of one first name are not a pair to each other.
        assert!(c.iter().all(|x| x.a == "person:b"));
    }

    #[test]
    fn a_shared_handle_outranks_the_rest() {
        let (mut a, mut b) = (who("a", "Wren Hale"), who("b", "W. Hale"));
        a.handles = vec!["slack:U1".into()];
        b.handles = vec!["slack:U1".into()];
        let (c, _) = candidates(&[a, b], &HashSet::new(), &none);
        assert_eq!((c[0].kind, c[0].score), ("handle", HANDLE));
    }

    #[test]
    fn a_role_line_needs_a_shared_session() {
        let (mut a, mut b) = (who("a", "Ivo Marsh"), who("b", "I. Marsh"));
        a.role = "Runs the gull  survey".into();
        b.role = "runs the gull survey".into();
        assert!(candidates(&[a.clone(), b.clone()], &HashSet::new(), &none)
            .0
            .is_empty());
        a.sessions.insert("ses_1".into());
        b.sessions.insert("ses_1".into());
        let (c, _) = candidates(&[a, b], &HashSet::new(), &none);
        assert_eq!((c.len(), c[0].kind), (1, "role"));
    }

    #[test]
    fn equal_keys_are_the_automatic_merges_and_a_recorded_pair_is_left_out() {
        let p = [who("a", "Odo Finch"), who("b", "odo-finch")];
        assert!(candidates(&p, &HashSet::new(), &none).0.is_empty());
        let p = [who("j", "Juno"), who("jp", "Juno Pike")];
        let forbidden = HashSet::from([pair_key("person:jp", "person:j")]);
        let (c, out) = candidates(&p, &forbidden, &none);
        assert_eq!((c.len(), out), (0, 1));
    }

    /// Two names in one message are two people, and suppress the candidate.
    #[test]
    fn two_names_in_one_message_suppress_the_candidate() {
        assert!(named_together(
            "Juno and Juno Pike met",
            "Juno",
            "Juno Pike"
        ));
        assert!(!named_together("Juno Pike met", "Juno", "Juno Pike"));
        assert!(!named_together("juno said hi", "Juno", "Juno Pike"));
        assert!(named_together(
            "Ask Wren Hale and Ivo Marsh",
            "wren hale",
            "Ivo Marsh"
        ));
        assert!(!named_together("Ask Wren Hale", "wren hale", "Ivo Marsh"));
        let p = [who("j", "Juno"), who("jp", "Juno Pike")];
        let said = |a: &Who, b: &Who| named_together("Juno and Juno Pike", &a.name, &b.name);
        let (c, out) = candidates(&p, &HashSet::new(), &said);
        assert_eq!((c.len(), out), (0, 1));
    }
}
