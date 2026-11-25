//! The self loop's building sessions read history at the widest (issue 53): a builder (a self session with no
//! role) and a fixer page every place, open another place's exchange, and search the imported conversations,
//! as any session does. The judging sessions keep to their own place: an approver's call reads nothing of
//! another place and no import. A self session whose role cannot be read reads nothing.

use serde_json::{json, Value};

use super::{xs2_as, xs2_ask, DM};
use crate::rsi::roles::Role;

/// What a call answered: its page, or the words of its refusal.
fn text(r: Result<String, String>) -> String {
    r.unwrap_or_else(|e| e)
}

/// A builder and a fixer: another place by page, by words, by handle and by every place; another session at
/// its own place; and the imports, by words.
#[tokio::test]
async fn a_builders_and_a_fixers_history_reads_every_place_and_the_imports() {
    use crate::tests_imports::{episode, import, index_over};
    for role in [None, Some(Role::Fixer)] {
        let x = xs2_as(role).await;
        let c = &x.r.core;
        let ask = |a: Value| xs2_ask(c, &x.me, a);
        let page = text(ask(json!({})).await);
        assert!(page.contains("OWN-HERON"), "{role:?}: its own: {page}");
        assert!(
            page.contains("SAMEPLACE-KELP"),
            "{role:?}: another session at its place: {page}"
        );
        let there = text(ask(json!({"place": DM})).await);
        assert!(
            there.contains("FOREIGN-LAMP"),
            "{role:?}: another place's page: {there}"
        );
        let every = text(ask(json!({"place": "*"})).await);
        assert!(
            every.contains("FOREIGN-LAMP"),
            "{role:?}: every place: {every}"
        );
        let sought = text(ask(json!({"place": DM, "words": "FOREIGN-LAMP"})).await);
        assert!(
            sought.contains("FOREIGN-LAMP"),
            "{role:?}: another place's words: {sought}"
        );
        let opened = text(ask(json!({"open": "turn_0003"})).await);
        assert!(
            opened.contains("FOREIGN-LAMP"),
            "{role:?}: another place's exchange opens: {opened}"
        );
        let ids = import(
            c,
            &[episode(
                63,
                "personal",
                "2026-03-03T18:00:00Z",
                "the FOREIGN-OWL import",
            )],
        );
        c.runner.memory.set_ask(index_over(c, ids.clone()));
        let found = ask(json!({"place": "import", "words": "FOREIGN-OWL"})).await;
        let said = text(found.clone());
        assert!(found.is_ok(), "{role:?}: the import search runs: {said}");
        assert!(
            said.contains("FOREIGN-OWL") && said.contains(&ids[0]),
            "{role:?}: the imported words: {said}"
        );
    }
}

/// An approver, the other judging role: no other place, by page or by handle, and no import search.
#[tokio::test]
async fn an_approvers_history_keeps_to_its_own_place_and_searches_no_import() {
    let x = xs2_as(Some(Role::Approver)).await;
    let c = &x.r.core;
    let ask = |a: Value| xs2_ask(c, &x.me, a);
    let own = text(ask(json!({})).await);
    assert!(own.contains("OWN-HERON"), "its own: {own}");
    assert!(!own.contains("SAMEPLACE-KELP"), "another session: {own}");
    let there = text(ask(json!({"place": DM})).await);
    assert!(!there.contains("FOREIGN-LAMP"), "another place: {there}");
    let opened = text(ask(json!({"open": "turn_0003"})).await);
    assert!(!opened.contains("FOREIGN-LAMP"), "by handle: {opened}");
    let refused = ask(json!({"place": "import", "words": "FOREIGN-OWL"})).await;
    assert!(refused.is_err(), "the import search: {refused:?}");
}

/// A self session whose role record cannot be read is never read at the widest: its history call fails, and
/// reads nothing.
#[tokio::test]
async fn a_self_session_whose_role_cannot_be_read_reads_no_history() {
    let x = xs2_as(None).await;
    let c = &x.r.core;
    crate::tests_self_wide::unreadable_role(c, &x.me);
    let v = crate::view_gate::Verified::of_session(&c.runner, &x.me);
    assert!(v.scope_unread(), "an unreadable role is an unread scope");
    let failed = xs2_ask(c, &x.me, json!({"place": DM})).await;
    assert!(failed.is_err(), "{failed:?}");
    assert!(!text(failed).contains("FOREIGN-LAMP"));
}
