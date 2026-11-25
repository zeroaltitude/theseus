//! A fork made before forks were given their source's role (issue 53) has no role record of its own. The one
//! reader of a role (`Board::role_in`) reads its source's, so its scope, its tools and the builder's verb all
//! treat it as its source: an old fork of a reviewer or an approver is a judge, and so is a fork of that fork.
//! A role that cannot be read fails closed: the local reads alone, and the verb refused. A builder's role is
//! never inferred from none.

use serde_json::json;

use crate::rsi::roles::{key, Role};
use crate::view_gate::Verified;

fn rig() -> crate::tests_places::Rig {
    crate::tests_places::rig_with(|c| c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act)
}

/// A session the self loop opened, for `role`'s job (none: a mint writer's).
fn self_session(c: &crate::Core, role: Option<Role>) -> String {
    let id = c
        .open_session_as(
            theseus_protocol::SessionOpenParams {
                label: Some("self: kelp".into()),
                ..Default::default()
            },
            theseus_kernel::Authority {
                principal: crate::rsi::SELF_PRINCIPAL.into(),
                ..Default::default()
            },
            None,
        )
        .unwrap()
        .session_id;
    if let Some(role) = role {
        c.tools.self_pr.set_role(&id, role).unwrap();
    }
    id
}

/// A fork of `source` as one made before forks were given a role: its source's principal, its origin, and no
/// role record of its own.
fn old_fork(c: &crate::Core, source: &str) -> String {
    let id = self_session(c, None);
    let mut rec: crate::session::SessionRecord = c.store.get_session(&id).unwrap().unwrap();
    rec.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: source.into(),
        node: "msg_point".into(),
        position: 0,
        compilation: None,
        at_ms: 0,
    }));
    c.store.put_session(&id, &rec).unwrap();
    id
}

/// What the builder's verb says of `session` before anything else.
fn verb(c: &crate::Core, session: &str) -> Result<(), String> {
    let input = json!({"item": "kelp", "issue": "theseus-kelp1", "why": "Mend the net."});
    super::fixing::builder_input(&c.tools.self_pr, session, &input).map(|_| ())
}

#[tokio::test]
async fn an_old_fork_of_a_judge_has_its_judges_tools_scope_and_refusal() {
    let r = rig();
    let c = &r.core;
    for role in [Role::Reviewer, Role::Approver] {
        let source = self_session(c, Some(role));
        let once = old_fork(c, &source);
        let twice = old_fork(c, &once);
        c.tools.self_pr.forget_roles();
        for (f, who) in [(&once, "an old fork"), (&twice, "a fork of an old fork")] {
            let who = format!("{role:?}, {who}");
            assert_eq!(
                c.store.get_meta::<Role>(&key(f)).unwrap(),
                None,
                "{who}: no record"
            );
            assert_eq!(
                c.tools.self_pr.role_in(&c.store, f).unwrap(),
                Some(role),
                "{who}: its source's role"
            );
            let view = c.runner.view_of(f);
            assert!(!view.offered("fs.write", &[]), "{who}: fs.write");
            assert!(
                !view.offered("proc.run", &[]) || role == Role::Reviewer,
                "{who}"
            );
            assert!(!view.offered("self.propose_change", &[]), "{who}: the verb");
            assert!(!view.offered("context.history", &[]), "{who}: history");
            assert_eq!(
                view.offered("fs.read", &[]),
                role == Role::Reviewer,
                "{who}"
            );
            assert!(
                Verified::of_session(&c.runner, f).own_place_only(),
                "{who}: its own place"
            );
            let refused = verb(c, f).unwrap_err();
            assert!(refused.contains(role.words()), "{who}: {refused}");
        }
    }
}

/// A role record that cannot be read: the session, and an old fork of it, are offered the local reads and
/// nothing that writes, runs or leaves; the builder's verb refuses them; their scope is unread.
#[tokio::test]
async fn a_role_that_cannot_be_read_fails_closed_in_the_tools_and_the_verb() {
    let r = rig();
    let c = &r.core;
    let s = self_session(c, None);
    let fork = old_fork(c, &s);
    crate::tests_self_wide::unreadable_role(c, &s);
    for (f, who) in [(&s, "the session"), (&fork, "its old fork")] {
        assert!(c.tools.self_pr.role_in(&c.store, f).is_err(), "{who}");
        let view = c.runner.view_of(f);
        assert!(view.offered("fs.read", &[]), "{who}: the local reads");
        for t in [
            "fs.write",
            "proc.run",
            "context.history",
            "self.propose_change",
            "web.fetch",
        ] {
            assert!(!view.offered(t, &[]), "{who}: {t}");
        }
        let refused = verb(c, f).unwrap_err();
        assert!(refused.contains("cannot be read"), "{who}: {refused}");
        assert!(Verified::of_session(&c.runner, f).scope_unread(), "{who}");
    }
}

/// An old fork of a session with no role (a mint writer's) has none: never a builder's, so it keeps its own
/// place, and nothing narrows its tools that did not before.
#[tokio::test]
async fn an_old_fork_of_a_role_less_session_is_given_no_role() {
    let r = rig();
    let c = &r.core;
    let fork = old_fork(c, &self_session(c, None));
    c.tools.self_pr.forget_roles();
    assert_eq!(c.tools.self_pr.role_in(&c.store, &fork).unwrap(), None);
    assert!(Verified::of_session(&c.runner, &fork).own_place_only());
    assert!(c.runner.view_of(&fork).offered("fs.write", &[]));
    assert_eq!(verb(c, &fork), Ok(()));
}
