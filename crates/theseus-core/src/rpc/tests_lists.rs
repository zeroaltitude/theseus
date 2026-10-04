//! The polled lists through the store's index (theseus-96w2): each answers
//! what the read of every record it replaced answered.

use serde_json::json;
use theseus_protocol::SessionListParams;
use theseus_protocol::{ActionListParams, NodeListParams, SessionHistoryParams};
use theseus_store::{kinds, NewRecord};

use super::tests::test_core;
use super::Core;
use crate::node::Node;
use crate::session::SessionRecord;

/// A small deterministic generator: the test needs no crate for it.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

/// An action as the kernel stores it, by its fields alone.
fn action(id: &str, execution: &str, state: &str, planned_at_ms: u64) -> NewRecord {
    let a = json!({
        "correlation_id": id, "schema": 4, "execution_id": execution,
        "session_id": "ses_lists", "tool": "fs.read", "args_digest": "",
        "retry_class": {"class": "non_repeatable"}, "state": state,
        "deadline_at_ms": 0, "planned_at_ms": planned_at_ms, "completions_seen": 0,
    });
    NewRecord::json(kinds::ACTION, Some(id), &a)
        .unwrap()
        .scoped("ses_lists")
}

/// `action.list` (theseus-96w2): the newest `n` by birth and an
/// execution's by its tag give what every action read, sorted, and cut
/// gave: the same actions, in the same order, each in its latest state, and
/// the same total. Actions change state after they are planned, many to a
/// frame, and a randomized check asks for every `n` and execution.
#[tokio::test]
async fn action_list_through_the_index_answers_as_every_action_read_did() {
    let core = test_core("ok");
    let mut g = Lcg(9);
    let states = ["authorized", "dispatched", "succeeded", "failed"];
    let mut planned = 0u64;
    let mut execution_of: Vec<String> = Vec::new();
    for _ in 0..300u64 {
        let mut frame = Vec::new();
        for _ in 0..1 + g.below(3) {
            planned += 1;
            let x = format!("exe_{}", g.below(6));
            frame.push(action(
                &format!("cor_{planned:05}"),
                &x,
                "planned",
                1_000 + planned,
            ));
            execution_of.push(x);
        }
        // An older action moves on.
        if planned > 3 {
            let old = 1 + g.below(planned - 1);
            let x = &execution_of[(old - 1) as usize];
            let state = states[g.below(4) as usize];
            frame.push(action(&format!("cor_{old:05}"), x, state, 1_000 + old));
        }
        core.store.append(&frame).unwrap();
    }
    for q in 0..60 {
        let n = (1 + g.below(80)) as usize;
        let execution = (q % 3 == 0).then(|| format!("exe_{}", g.below(7)));
        let got = core
            .action_list(ActionListParams {
                execution_id: execution.clone(),
                n: Some(n),
            })
            .unwrap();
        let mut all = core.kernel.actions().unwrap();
        let total = all.len() as u64;
        if let Some(x) = &execution {
            all.retain(|a| &a.execution_id == x);
        }
        all.sort_by_key(|a| std::cmp::Reverse(a.planned_at_ms));
        all.truncate(n);
        let ids = |v: &[(String, String)]| v.to_vec();
        let want: Vec<(String, String)> = all
            .iter()
            .map(|a| (a.correlation_id.clone(), a.state.as_str().to_string()))
            .collect();
        let have: Vec<(String, String)> = got
            .actions
            .iter()
            .map(|a| (a.correlation_id.clone(), a.state.clone()))
            .collect();
        assert_eq!(ids(&have), want, "query {q}: n {n}, {execution:?}");
        assert_eq!(got.total, total, "query {q}");
    }
}

/// A node as the turn writes it, of `kind` in `session`.
fn node(session: &str, kind: u64, i: u64) -> Node {
    match kind {
        0 => Node::user(session, None, "operator", &format!("message {i}")),
        _ => {
            let body: crate::node::Body = serde_json::from_value(json!({
                "kind": "tool_result", "tool_use_id": format!("call_{i}"),
                "tool": "fs.read", "status": "ok", "is_error": false,
                "content": format!("result {i}"),
            }))
            .unwrap();
            let mut n = Node::user(session, None, "operator", "x");
            n.body = body;
            n
        }
    }
}

/// `node.list` with a kind, a session, or both, and `session.history`
/// with `n` (theseus-96w2): the tag's newest `n` give what reading the
/// session (or ten times `n` of every session's) then filtering gave.
#[tokio::test]
async fn node_list_and_history_through_the_index_answer_as_the_scan_did() {
    let core = test_core("ok");
    let mut g = Lcg(4);
    let sessions: Vec<String> = (0..3)
        .map(|_| {
            let rec = SessionRecord::new(theseus_kernel::SessionKind::Conversation, None);
            core.store.put_session(&rec.session_id, &rec).unwrap();
            rec.session_id
        })
        .collect();
    for i in 0..400u64 {
        let s = &sessions[g.below(3) as usize];
        let n = node(s, g.below(2), i);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
    for q in 0..60 {
        let n = (1 + g.below(50)) as usize;
        let kind = (q % 3 != 0).then(|| ["user_message", "tool_result"][g.below(2) as usize]);
        let session = (q % 2 == 0).then(|| sessions[g.below(3) as usize].clone());
        let got = core
            .node_list(NodeListParams {
                session_id: session.clone(),
                kind: kind.map(str::to_string),
                n: Some(n),
            })
            .unwrap();
        let mut want: Vec<(u64, Node)> = match &session {
            Some(s) => core.store.session_nodes(s).unwrap(),
            None => core.store.recent_nodes(usize::MAX).unwrap(),
        };
        if let Some(k) = kind {
            want.retain(|(_, nd)| nd.kind_str() == k);
        }
        let want: Vec<u64> = want.iter().rev().take(n).map(|(p, _)| *p).collect();
        let have: Vec<u64> = got.nodes.iter().map(|x| x.position).collect();
        assert_eq!(have, want, "query {q}: n {n}, {kind:?}, {session:?}");
        // session.history with `n`: the session's newest `n`, oldest first.
        if let Some(s) = &session {
            let h = core
                .session_history(SessionHistoryParams {
                    session_id: s.clone(),
                    n: Some(n),
                })
                .unwrap();
            let all = core.store.session_nodes(s).unwrap();
            let want: Vec<u64> = all[all.len().saturating_sub(n)..]
                .iter()
                .map(|(p, _)| *p)
                .collect();
            let have: Vec<u64> = h.nodes.iter().map(|x| x.position).collect();
            assert_eq!(have, want, "history {q}: n {n}");
        }
    }
}

/// `session.list { n, before }` (theseus-96w2): the newest sessions by when
/// each was opened, a page at a time by `older`, each once, while sessions
/// are opened between the pages; and without `n`, every session, as
/// before.
#[tokio::test]
async fn session_list_pages_back_from_the_newest_by_when_each_was_opened() {
    let core: std::sync::Arc<Core> = test_core("ok");
    let open = |core: &Core| {
        let rec = SessionRecord::new(theseus_kernel::SessionKind::Conversation, None);
        core.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let mut opened: Vec<String> = (0..23).map(|_| open(&core)).collect();
    // A session written again later keeps its place: it was opened then.
    let first = core
        .store
        .get_session::<SessionRecord>(&opened[0])
        .unwrap()
        .unwrap();
    core.store.put_session(&first.session_id, &first).unwrap();
    let mut seen: Vec<String> = Vec::new();
    let mut before = None;
    loop {
        let r = core
            .session_list_of(SessionListParams {
                n: Some(5),
                before,
                ..Default::default()
            })
            .unwrap();
        assert!(r.sessions.len() <= 5);
        seen.extend(r.sessions.iter().map(|s| s.session_id.clone()));
        // Opened between pages: newer than the cursor, so never in a page back.
        open(&core);
        match r.older {
            Some(b) => before = Some(b),
            None => break,
        }
    }
    opened.reverse();
    assert_eq!(seen, opened, "each session once, the newest first");
    let all = core.session_list_of(SessionListParams::default()).unwrap();
    assert_eq!(all.sessions.len(), 23 + 5);
    assert_eq!(all.older, None);
}
