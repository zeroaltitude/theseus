//! Numbered items: "issue 33", "#33", "PR 12" (the index's extractor names
//! them, `index.items`, as `item:` terms). A bare number never makes an
//! identity by itself: issue 33 of two projects is two things. So an item is
//! scoped by the project of the place it was said in, when that is known
//! (the session serves one), and is otherwise kept as an unscoped candidate.
//! An item that names its repository (`octo/lantern#33`) is qualified
//! already and is kept as it was said.
//!
//! | Said, in a session that serves project `prj_1` | Term |
//! |---|---|
//! | `issue 33` | `item:project:prj_1#issue-33` |
//! | `octo/lantern#33` | `item:octo/lantern#33` |
//!
//! With no project, `issue 33` stays `item:issue-33`: a candidate that a
//! reader may show, and never treats as one thing.

use crate::session::SessionRecord;
use crate::store::Store;

/// The kind of an item term.
pub const ITEM: &str = "item:";

/// An item's scope from the place it was said in: the project its session
/// serves, as `project:<id>`, or none.
pub fn scope_of(store: &Store, session: &str) -> Option<String> {
    let r: SessionRecord = store.get_session(session).ok().flatten()?;
    r.project.map(|p| format!("project:{p}"))
}

/// How long the tender is given to name a text's items: past it they are
/// given up, and the things named beside them are kept.
pub const WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The scope of a message's items: the project of the session it arrived
/// at, by its owner's mark (a thread may have taken it to a session that
/// serves another project); the running session's only for an input the
/// mark does not name an arrival for.
pub fn scope_for(
    store: &Store,
    mark: Option<&crate::exchange::OwnerMark>,
    running: &str,
) -> Option<String> {
    let arrived = mark
        .map(|m| m.arrived_session.as_str())
        .filter(|s| !s.is_empty());
    scope_of(store, arrived.unwrap_or(running))
}

/// Whether an item term names its own scope (`item:<scope>#<number>`).
pub fn is_qualified(term: &str) -> bool {
    term.strip_prefix(ITEM).is_some_and(|v| v.contains('#'))
}

/// `terms` with each unqualified item scoped by `scope`, when there is one;
/// a term that is no item, or is qualified, is kept as it is.
pub fn scoped(terms: Vec<String>, scope: Option<&str>) -> Vec<String> {
    let Some(scope) = scope else {
        return terms;
    };
    terms
        .into_iter()
        .map(|t| match t.strip_prefix(ITEM) {
            Some(local) if !is_qualified(&t) => format!("{ITEM}{scope}#{local}"),
            _ => t,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::{OwnerMark, Peer};

    fn serving(store: &Store, project: &str) -> String {
        let mut r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        r.project = Some(project.into());
        store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    /// KEY: a message that arrived in a session of project A and ran in a
    /// thread's session of project B is scoped by A.
    #[test]
    fn an_items_scope_is_the_project_of_the_session_it_arrived_at() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let (a, b) = (serving(&store, "prj_a"), serving(&store, "prj_b"));
        let mark = OwnerMark {
            node: "nod_1".into(),
            in_person: true,
            owner_handle: None,
            peer: Peer::Person,
            agent: None,
            surface: "cli".into(),
            place: "local".into(),
            arrived_session: a,
            ran_session: b.clone(),
            author: None,
            at_ms: 1,
        };
        assert_eq!(
            scope_for(&store, Some(&mark), &b).as_deref(),
            Some("project:prj_a")
        );
        assert_eq!(
            scope_for(&store, None, &b).as_deref(),
            Some("project:prj_b")
        );
    }

    fn v(terms: &[&str]) -> Vec<String> {
        terms.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn an_unqualified_item_takes_the_places_project_and_a_qualified_one_stays() {
        let terms = v(&[
            "item:issue-33",
            "item:4",
            "item:octo/lantern#33",
            "file:a.rs",
        ]);
        assert_eq!(
            scoped(terms.clone(), Some("project:prj_1")),
            v(&[
                "item:project:prj_1#issue-33",
                "item:project:prj_1#4",
                "item:octo/lantern#33",
                "file:a.rs"
            ])
        );
        assert_eq!(scoped(terms.clone(), None), terms, "unscoped candidates");
    }
}
