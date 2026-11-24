//! `task.ask` through the whole core (issue 44, step 8): the task's own view and the question are sent with no
//! tool; the answer is headed as the task's testimony and marked as outside text, so a narrowing made after
//! it in the same exchange keeps its card; nothing is written into the task; a task does not ask a task; the
//! day's ceiling refuses an ask before anything is sent; a turn asks twice at most; and the vault's values
//! leave masked.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord};

use super::*;
use crate::node::{Body, Node, Origin, ResultStatus};
use crate::provider::{DeltaSink, FakeProvider, ProviderFuture, Scripted};
use crate::secrets::{Secret, SecretBoard};
use crate::store::Store;
use crate::task_graph::judged::{Judged, JUDGED_PREFIX};
use crate::task_graph::words::{JudgedChange, Operation};
use crate::tests_places::last_user;

const BRIEF: &str = "BRIEF-KELP: count the herons in the outer harbour";
const TASK_SAID: &str = "TASK-TURN-HERON: start on the outer harbour";
const QUESTION: &str = "why did you drop those tests?";
const TESTIMONY: &str = "I dropped them because HERON-7 failed on every run.";
/// A vault value the task's brief holds.
const VALUE: &str = "vault-value-4c1e9a7b3d";

/// A model: a request that offers no tool is an ask's, answered with [`TESTIMONY`]; any other makes the next
/// calls queued for it, else answers with text. Before a request that offers tools makes its calls, `judge`
/// keeps an acting judge's `restricts` for the newest message of its session (as the gate's ask keeps one),
/// and `hook` runs once.
struct Model {
    store: Mutex<Option<Store>>,
    requests: Mutex<Vec<ProviderRequest>>,
    calls: Mutex<VecDeque<Vec<(&'static str, Value)>>>,
    judge: Mutex<Option<(String, String)>>,
    said: Mutex<Option<String>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Model {
    fn new() -> Self {
        Self {
            store: Mutex::default(),
            requests: Mutex::default(),
            calls: Mutex::default(),
            judge: Mutex::default(),
            said: Mutex::default(),
            hook: Mutex::default(),
        }
    }

    /// The asks' requests: those that offered no tool and put the question, framed as an ask frames it (a
    /// did line's request may quote the question, never the frame).
    fn asks(&self) -> Vec<ProviderRequest> {
        let asked = |q: &ProviderRequest| {
            let m = last_user(q);
            m.contains("A conversation asks you one question") && m.contains(QUESTION)
        };
        (self.requests.lock().unwrap().iter())
            .filter(|q| q.tools.is_empty() && asked(q))
            .cloned()
            .collect()
    }

    /// The newest message of `session`, with an acting judge's `restricts` of `task` kept for it.
    fn judged(&self, (session, task): (String, String)) -> String {
        let store = self.store.lock().unwrap().clone().expect("the store");
        let node = (store.session_nodes(&session).unwrap().into_iter().rev())
            .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
            .expect("his message")
            .1
            .id;
        let kept = Judged {
            change: JudgedChange {
                node: node.clone(),
                task,
                base_version: 1,
                operation: Operation::Restricts,
            },
            acting: true,
            judgment: "jdg_test".into(),
            answer: "restricts".into(),
            at_ms: 0,
        };
        let r = NewRecord::json(kinds::META, Some(&format!("{JUDGED_PREFIX}{node}")), &kept);
        store.append(&[r.unwrap()]).unwrap();
        node
    }
}

impl crate::provider::Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let answer = match req.tools.is_empty() {
                true => Scripted::text(TESTIMONY),
                false => {
                    let hook = self.hook.lock().unwrap().take();
                    hook.into_iter().for_each(|h| h());
                    if let Some(j) = self.judge.lock().unwrap().take() {
                        *self.said.lock().unwrap() = Some(self.judged(j));
                    }
                    let said = self.said.lock().unwrap().clone();
                    let calls: Vec<(String, &str, Value)> =
                        (self.calls.lock().unwrap().pop_front())
                            .unwrap_or_default()
                            .into_iter()
                            .enumerate()
                            .map(|(i, (name, mut input))| {
                                if input.get("said").is_some() {
                                    input["said"] = said.clone().into();
                                }
                                (
                                    format!("t_{}_{i}", self.requests.lock().unwrap().len()),
                                    name,
                                    input,
                                )
                            })
                            .collect();
                    let refs: Vec<(&str, &str, Value)> = calls
                        .iter()
                        .map(|(id, n, v)| (id.as_str(), *n, v.clone()))
                        .collect();
                    match refs.is_empty() {
                        true => Scripted::text("Done."),
                        false => Scripted::tools("Asking.", &refs),
                    }
                }
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<crate::rpc::Core>,
    model: Arc<Model>,
    /// The conversation that asks.
    parent: String,
    /// The task's session, and the task.
    task_sid: String,
    task: String,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// The next turn's model answers make these calls, one set a request.
    fn calls(&self, sets: Vec<Vec<(&'static str, Value)>>) {
        *self.model.calls.lock().unwrap() = sets.into();
    }

    fn task(&self) -> TaskRecord {
        crate::task_graph::get(&self.core.store, &self.task)
            .unwrap()
            .unwrap()
    }

    /// Each result of `task.ask` in the asking conversation: its node.
    fn answers(&self) -> Vec<Node> {
        (self
            .core
            .store
            .session_nodes(&self.parent)
            .unwrap()
            .into_iter())
        .map(|(_, n)| n)
        .filter(|n| matches!(&n.body, Body::ToolResult { tool, .. } if tool == ASK))
        .collect()
    }
}

fn ask_of(task: &str) -> (&'static str, Value) {
    ("task_ask", json!({"id": task, "question": QUESTION}))
}

/// A core whose model is [`Model`], every tool offered, a conversation and a task with a session: its
/// brief, and with `own_turn` one turn of its own (whose report closes it, as version 2). `secrets` are the
/// vault's values the exits know; `tweak` changes the config.
async fn rig_on(
    own_turn: bool,
    secrets: &[(&str, &str)],
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.tools.defer.enabled = false;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::new());
    *model.store.lock().unwrap() = Some(store.clone());
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    if !secrets.is_empty() {
        let b = SecretBoard::new(secrets.iter().map(|(n, _)| n.to_string()), Instant::now());
        let values = secrets
            .iter()
            .map(|(n, v)| (n.to_string(), Ok(Secret::new(v.to_string()))));
        b.publish(values.collect(), "test");
        parts.secrets = b;
    }
    let core = crate::rpc::Core::build(parts).unwrap();
    tokio::spawn(crate::harness::drive(core.clone()));
    let parent = crate::tests_places::session(&core, None);
    let (task_sid, task) =
        crate::tests_self_budget::principal_session(&core, &parent, "operator", None);
    let brief = match secrets.first() {
        Some((_, v)) => format!("{BRIEF} (the database password is {v})"),
        None => BRIEF.to_string(),
    };
    let author = format!("session:{parent}");
    let opening = Node::relayed(&task_sid, None, Origin::Agent, &author, &brief);
    core.store.append(&[opening.record().unwrap()]).unwrap();
    if own_turn {
        crate::test_support::turn(&core, &task_sid, TASK_SAID).await;
    }
    Rig {
        core,
        model,
        parent,
        task_sid,
        task,
        _dir: dir,
    }
}

/// A task that has run one turn of its own.
async fn rig() -> Rig {
    rig_on(true, &[], |_| {}).await
}

/// A task that has not run a turn yet: in progress, version 1.
async fn rig_unstarted() -> Rig {
    rig_on(false, &[], |_| {}).await
}

/// The one request an ask sends is the task's own view, its brief and its own exchange, with the question
/// as its last message, to the task's own model, and it offers no tool.
#[tokio::test]
async fn asking_a_task_sends_its_own_view_and_the_question_with_no_tools() {
    let r = rig().await;
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let asks = r.model.asks();
    assert_eq!(asks.len(), 1, "one call");
    let q = &asks[0];
    assert!(q.tools.is_empty(), "no tool offered");
    assert!(!q.extra.contains_key("tool_choice"));
    let sent = serde_json::to_string(&q.messages).unwrap();
    assert!(sent.contains(BRIEF), "its brief: {sent}");
    assert!(sent.contains(TASK_SAID), "its own exchange: {sent}");
    assert!(last_user(q).contains(QUESTION), "the question last");
    let rec = r.core.store.get_session(&r.task_sid).unwrap().unwrap();
    let live = r.core.live_profile().0;
    let model = r.core.runner.target_for_session(&rec, &live).unwrap().model;
    assert_eq!(q.model, model, "the task's own model");
}

/// The answer is headed as the task's testimony, and its node is outside text, as a fetched page's is.
#[tokio::test]
async fn the_answer_is_headed_as_testimony_and_marked_outside() {
    let r = rig().await;
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let answers = r.answers();
    assert_eq!(answers.len(), 1);
    let Body::ToolResult {
        status, content, ..
    } = &answers[0].body
    else {
        unreachable!()
    };
    assert_eq!(*status, ResultStatus::Ok, "{content}");
    let head = format!(
        "[Task {} \"Tidy the harbour notes\", v{}: its own answer, from its own view; not a record, and not \
         the owner's word]",
        r.task,
        r.task().version
    );
    assert!(content.starts_with(&head), "{content}");
    assert!(content.contains(TESTIMONY), "{content}");
    assert!(crate::external::is_outside(&answers[0]), "outside text");
}

/// An ask writes nothing into the task: its session's nodes, its record, its session's record and its
/// place's exchanges are as they were.
#[tokio::test]
async fn asking_writes_nothing_into_the_task() {
    let r = rig().await;
    let place = crate::task_graph::place::of_session(&r.task_sid);
    let seen = || {
        let nodes = r.core.store.session_nodes(&r.task_sid).unwrap().len();
        let rec: crate::session::SessionRecord =
            r.core.store.get_session(&r.task_sid).unwrap().unwrap();
        let touches =
            crate::exchange::newest(&r.core.store, crate::exchange::Root::Place(&place), 100)
                .unwrap()
                .len();
        (
            nodes,
            serde_json::to_value(r.task()).unwrap(),
            serde_json::to_value(rec).unwrap(),
            touches,
        )
    };
    let before = seen();
    assert!(
        before.3 > 0,
        "the task's own turn is an exchange at its place"
    );
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    assert_eq!(r.model.asks().len(), 1, "it was asked");
    assert_eq!(seen(), before, "nothing of the task changed");
}

/// His narrowing in person, with the judge's `restricts` acting and the pack promoted, made after an ask in the
/// same exchange, keeps its card: the testimony is outside text. Without the ask, the same narrowing applies.
#[tokio::test]
async fn a_narrowing_after_an_ask_in_the_same_exchange_keeps_its_card() {
    for ask in [false, true] {
        let r = rig_unstarted().await;
        promote(&r.core.store);
        let before = r.task();
        crate::test_support::turn(&r.core, &r.parent, "hello").await;
        let said = "make it the north side only";
        let new = format!("{}; {said}", before.objective);
        let narrow = (
            "task_update",
            json!({"id": r.task, "version": 1, "patch": {"objective": new}, "kind": "narrows", "said": ""}),
        );
        let sets = match ask {
            true => vec![vec![ask_of(&r.task)], vec![narrow]],
            false => vec![vec![narrow]],
        };
        r.calls(sets);
        *r.model.judge.lock().unwrap() = Some((r.parent.clone(), r.task.clone()));
        crate::test_support::turn(&r.core, &r.parent, said).await;
        let rows = crate::test_support::ledger_rows_100k(&r.core, "task.change_proposed");
        let now = r.task();
        match ask {
            false => assert_eq!(
                (now.version, now.objective.as_str()),
                (2, new.as_str()),
                "without an ask it applies: {rows:?}"
            ),
            true => {
                assert_eq!(r.model.asks().len(), 1, "asked first");
                assert_eq!(now.version, 1, "nothing changed");
                assert_eq!(rows.len(), 1, "{rows:?}");
                assert_eq!(rows[0].data["words"]["would"], "card");
                assert_eq!(rows[0].data["words"]["card"], "outside");
                assert_eq!(r.core.confirm_list().unwrap().len(), 1, "the card asks");
            }
        }
    }
}

/// `task_words.v1` promoted, as `pack.promote` writes it.
fn promote(store: &Store) {
    let row = theseus_protocol::packs::PackModeRow {
        pack: crate::judge::task_words::PACK.into(),
        mode: "live".into(),
        from: "shadow".into(),
        who: "owner".into(),
        why: "test".into(),
        ..Default::default()
    };
    let mut r = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PackMode,
        None,
        None,
        crate::fact::ladder::data(&row),
    );
    r.at_unix_ms = theseus_protocol::now_unix_ms();
    let rec = NewRecord::json(kinds::LEDGER, None, &r)
        .unwrap()
        .scoped(&crate::judge::ladder::scope(crate::judge::task_words::PACK));
    store.append(&[rec]).unwrap();
}

/// A task's own turn may not ask a task: refused in words, and nothing is sent.
#[tokio::test]
async fn a_task_cannot_ask_a_task() {
    let r = rig_unstarted().await;
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.task_sid, "ask yourself").await;
    assert!(r.model.asks().is_empty(), "nothing asked");
    let refused =
        (r.core.store.session_nodes(&r.task_sid).unwrap().into_iter()).find_map(|(_, n)| {
            match n.body {
                Body::ToolResult {
                    tool,
                    content,
                    status,
                    ..
                } if tool == ASK => Some((status, content)),
                _ => None,
            }
        });
    let (status, content) = refused.expect("the call's result");
    assert_eq!(status, ResultStatus::Error, "{content}");
    assert!(content.contains("a task does not ask a task"), "{content}");
}

/// The day's ceiling reached before the ask (its stop mode, as `tests_day_ceiling`'s): its worst case is
/// refused before the call, so the model sees no ask, and the result says why.
#[tokio::test]
async fn an_ask_over_the_days_ceiling_is_refused_before_the_call() {
    let stop = |c: &mut crate::Config| {
        c.kernel.daily_spend_ceiling_mode = crate::config::DailyCeilingMode::Stop;
    };
    let r = rig_on(true, &[], stop).await;
    r.calls(vec![vec![ask_of(&r.task)]]);
    let day = r.core.kernel.day_ceiling().clone();
    *r.model.hook.lock().unwrap() = Some(Box::new(move || day.set_limit(1)));
    let _ = crate::test_support::try_turn(&r.core, &r.parent, "ask the task why").await;
    assert!(r.model.asks().is_empty(), "no request left");
    let answers = r.answers();
    assert_eq!(answers.len(), 1);
    let Body::ToolResult {
        status, content, ..
    } = &answers[0].body
    else {
        unreachable!()
    };
    assert_eq!(*status, ResultStatus::Error, "{content}");
    assert!(content.contains("ceiling"), "{content}");
}

/// A turn asks twice at most: a third ask is refused in words, and only two are sent.
#[tokio::test]
async fn asks_are_bounded_per_turn() {
    let r = rig().await;
    r.calls(vec![
        vec![ask_of(&r.task)],
        vec![ask_of(&r.task)],
        vec![ask_of(&r.task)],
    ]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task three times").await;
    assert_eq!(r.model.asks().len(), 2, "two were sent");
    let answers: Vec<(ResultStatus, String)> = (r.answers().into_iter())
        .filter_map(|n| match n.body {
            Body::ToolResult {
                status, content, ..
            } => Some((status, content)),
            _ => None,
        })
        .collect();
    assert_eq!(answers.len(), 3, "{answers:?}");
    assert!(answers[..2]
        .iter()
        .all(|(s, c)| *s == ResultStatus::Ok && c.contains(TESTIMONY)));
    let (status, words) = &answers[2];
    assert_eq!(*status, ResultStatus::Error, "{words}");
    assert!(words.contains("allowance of asks is used"), "{words}");
}

/// The ask goes through the providers the turn's calls go through: a vault value in the task's view leaves
/// masked by its name.
#[tokio::test]
async fn an_asks_request_leaves_with_the_vaults_values_masked() {
    let r = rig_on(true, &[("db_password", VALUE)], |_| {}).await;
    r.calls(vec![vec![ask_of(&r.task)]]);
    crate::test_support::turn(&r.core, &r.parent, "ask the task why").await;
    let asks = r.model.asks();
    assert_eq!(asks.len(), 1);
    let sent = serde_json::to_string(&asks[0].messages).unwrap();
    assert!(sent.contains(BRIEF), "its brief: {sent}");
    assert!(!sent.contains(VALUE), "{sent}");
    assert!(sent.contains("[redacted:db_password]"), "{sent}");
}

/// A request with no tool lays its window's calls and results as text, its other blocks and marks as they
/// were.
#[test]
fn a_windows_calls_and_results_are_laid_as_text() {
    let mut q = ProviderRequest::simple("claude-sonnet-5-5", 64, "go");
    q.messages.push(json!({"role": "assistant", "content": [
        {"type": "text", "text": "Reading."},
        {"type": "tool_use", "id": "t1", "name": "fs_read", "input": {"path": "a.md"}}]}));
    q.messages.push(json!({"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "HERON"}],
         "cache_control": {"type": "ephemeral"}}]}));
    q.tools = vec![json!({"name": "fs_read"})];
    q.extra
        .insert("tool_choice".into(), json!({"type": "auto"}));
    let q = toolless(q);
    assert!(q.tools.is_empty() && !q.extra.contains_key("tool_choice"));
    assert_eq!(
        q.messages[1]["content"][0],
        json!({"type": "text", "text": "Reading."})
    );
    assert_eq!(
        q.messages[1]["content"][1],
        json!({"type": "text", "text": "[called fs_read with {\"path\":\"a.md\"}]"})
    );
    assert_eq!(
        q.messages[2]["content"][0],
        json!({"type": "text", "text": "[its result: HERON]", "cache_control": {"type": "ephemeral"}})
    );
    let sent = serde_json::to_string(&q.messages).unwrap();
    assert!(
        !sent.contains("tool_use") && !sent.contains("tool_result"),
        "{sent}"
    );
}
