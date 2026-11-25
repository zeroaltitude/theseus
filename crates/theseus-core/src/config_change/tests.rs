//! `config.propose` and the owner's yes, through the whole core, on a scratch
//! settings file and a scratch state directory (issue 22, line 1).
//!
//! - a proposal builds its candidate from the file as it is, comments kept,
//!   checks it loads, and puts one card to the owner whose text comes from
//!   the two parsed configs;
//! - a candidate that does not load makes no card, and the loader's words are
//!   the result;
//! - a yes writes the candidate in one step and keeps the previous copy; a
//!   file changed under the card, a decline and an expiry write nothing, and
//!   each is recorded;
//! - a shared place, a task's session, a sensitive setting: refused, with the
//!   words, before a card is made.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

pub(super) const OWNER: u64 = 4_100_000_000_000_000_001;
const LAB: u64 = 4_100_000_000_000_000_777;

/// The settings file the owner keeps: comments, an order of its own.
pub(super) const FILE: &str = r#"# Theseus, on this machine.
narrative = false

[secrets]
anthropic_api_key = "op://v/i/f"

# The notes import.
[import.notes]
enabled = true   # turn it off to stop the import
"#;

/// What `Rig::answers_plain` is given for a model that cannot answer.
pub(super) const FAILS: &str = "<the provider fails>";

/// The text of the tool result a request answers, when it answers one.
fn tool_result_text(req: &ProviderRequest) -> Option<String> {
    let blocks = req.messages.last()?["content"].as_array()?.clone();
    blocks.iter().find(|b| b["type"] == "tool_result").map(|b| {
        let c = &b["content"];
        c.as_str().map(str::to_string).unwrap_or_else(|| {
            c.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["text"].as_str())
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
    })
}

/// Answers each turn's first request with the call its input names.
struct Model {
    calls: Mutex<Vec<(String, Value)>>,
    /// A turn of several steps (`Rig::steps`): each answer to a result is the
    /// next call, named `p1`, `p2`, ..., then the text.
    steps: Mutex<std::collections::VecDeque<(String, Value)>>,
    made: Mutex<usize>,
    /// Run once, when the model first reads a result of a step's call (`Rig::after_the_first_step`).
    after_a_step: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// Run once, when the turn first asks the model: the turn has written its
    /// input by then, as the judge that reads it has had it written.
    first_request: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// What a model with no tool offered answers (a profile asked one question,
    /// issue 28), and the requests that asked it; the turn's own answer to such a
    /// result quotes it.
    plain: Mutex<Option<String>>,
    asked: Mutex<Vec<ProviderRequest>>,
    /// Run once, when a model with no tool offered is asked (a stop that lands during the call).
    on_asked: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The turn's own model call fails once it has read a tool's result (a turn that faults after
    /// a successful ask).
    fails_after_a_result: std::sync::atomic::AtomicBool,
    /// Every request's model and how many tools it offered (`profile.run`: an agent has tools).
    seen: Mutex<Vec<(String, usize)>>,
    /// The requests the turn itself made, whole (an agent's and a tool-less ask are not these).
    sent: Mutex<Vec<ProviderRequest>>,
    /// What an agent (a request whose first message holds `AGENT:`) reports, and the gate that holds
    /// it until a test lets it go.
    agent: Mutex<Option<String>>,
    gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    /// A call the agent makes before it reports (`Rig::agents_call`).
    agent_call: Mutex<Option<(String, Value)>>,
    /// The tool names each agent request offered, by wire name.
    agent_tools: Mutex<Vec<Vec<String>>>,
    /// Every request the turn's own model was asked, whole, in order.
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Model {
    /// What an agent (a request whose first message holds `AGENT:`) answers: the call it was told to
    /// make before it reports (`Rig::agents_call`), then its report. None for any other request.
    fn agent_answer(&self, req: &ProviderRequest) -> Option<Scripted> {
        let agent = req
            .messages
            .first()
            .is_some_and(|m| m.to_string().contains("AGENT:"));
        if !agent {
            return None;
        }
        self.agent_tools.lock().unwrap().push(
            req.tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect(),
        );
        let said = self.agent.lock().unwrap().clone()?;
        let answers_a_call = req
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
        let call = self.agent_call.lock().unwrap().clone();
        Some(match call {
            Some((tool, input)) if !answers_a_call => {
                Scripted::tools("", &[("ag1", tool.as_str(), input)])
            }
            _ => Scripted::text(&said),
        })
    }
}

/// A call the provider cannot take: it is busy.
fn busy<'a>() -> ProviderFuture<'a> {
    Box::pin(async {
        Err(anyhow::Error::new(
            crate::provider::ProviderError::Overloaded {
                message: "the lighthouse is busy".into(),
            },
        ))
    })
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.seen
            .lock()
            .unwrap()
            .push((req.model.clone(), req.tools.len()));
        if let Some(answer) = self.agent_answer(req) {
            let gate = self.gate.lock().unwrap().clone();
            return Box::pin(async move {
                if let Some(g) = gate {
                    g.acquire().await.unwrap().forget();
                }
                FakeProvider::scripted(vec![answer])
                    .stream_message(req, on_delta)
                    .await
            });
        }
        self.requests.lock().unwrap().push(req.clone());
        if let Some(f) = self.first_request.lock().unwrap().take() {
            f();
        }
        if !req.tools.is_empty() {
            self.sent.lock().unwrap().push(req.clone());
        }
        if self
            .fails_after_a_result
            .load(std::sync::atomic::Ordering::SeqCst)
            && !req.tools.is_empty()
            && tool_result_text(req).is_some()
        {
            return busy();
        }
        if let Some(said) = self.plain.lock().unwrap().clone() {
            if req.tools.is_empty() {
                self.asked.lock().unwrap().push(req.clone());
                if let Some(f) = self.on_asked.lock().unwrap().take() {
                    f();
                }
                if said == FAILS {
                    return busy();
                }
                let answer = Scripted::text(&said);
                return Box::pin(async move {
                    FakeProvider::scripted(vec![answer])
                        .stream_message(req, on_delta)
                        .await
                });
            }
            if let (true, Some(result)) =
                (self.steps.lock().unwrap().is_empty(), tool_result_text(req))
            {
                let answer = Scripted::text(&format!("It said: {result}"));
                return Box::pin(async move {
                    FakeProvider::scripted(vec![answer])
                        .stream_message(req, on_delta)
                        .await
                });
            }
        }
        let answers_a_call = crate::tests_places::answers_a_call(req);
        let after = answers_a_call.then(|| self.after_a_step.lock().unwrap().take());
        if let Some(f) = after.flatten() {
            f();
        }
        if let Some((tool, input)) = self.steps.lock().unwrap().pop_front() {
            let id = format!("p{}", {
                let mut n = self.made.lock().unwrap();
                *n += 1;
                *n
            });
            let answer = Scripted::tools("", &[(id.as_str(), tool.as_str(), input)]);
            return Box::pin(async move {
                FakeProvider::scripted(vec![answer])
                    .stream_message(req, on_delta)
                    .await
            });
        }
        let answer = match self.calls.lock().unwrap().pop() {
            Some((tool, input)) if !answers_a_call => {
                Scripted::tools("", &[("p1", tool.as_str(), input)])
            }
            _ => Scripted::text("Done."),
        };
        Box::pin(async move {
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

pub(super) struct Rig {
    pub(super) core: Arc<Core>,
    model: Arc<Model>,
    pub(super) file: PathBuf,
    pub(super) state: PathBuf,
    _dir: tempfile::TempDir,
    /// The driver `Rig::drive` started: it takes a task's turns, as the daemon's does.
    driver: Mutex<Option<tokio::task::JoinHandle<()>>>,
    said: Mutex<Option<String>>,
    /// The owner's turns carry `turn.submit`'s in-person mark (`Rig::in_person`); off, they carry none,
    /// as the rig's turns always did.
    in_person: std::sync::atomic::AtomicBool,
}

pub(super) fn rig() -> Rig {
    rig_with(|_, _| {})
}

/// [`rig`], with `tweak` to say about the settings last: it is given them and
/// the state directory.
pub(super) fn rig_with(tweak: impl FnOnce(&mut Config, &std::path::Path)) -> Rig {
    rig_in(tempfile::tempdir().unwrap(), tweak)
}

/// [`rig_with`] over the state directory `dir`: a daemon started again on a store that is there.
fn rig_in(dir: tempfile::TempDir, tweak: impl FnOnce(&mut Config, &std::path::Path)) -> Rig {
    let state = dir.path().to_path_buf();
    let file = dir.path().join("theseus.toml");
    std::fs::write(&file, FILE).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.policy.enforcement = Posture::Notify;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.config_file = Some(file.clone());
    // No index tender runs in a test, so a daemon that waits for it to answer
    // would never be healthy.
    cfg.index.enabled = false;
    tweak(&mut cfg, &state);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        calls: Mutex::default(),
        steps: Mutex::default(),
        made: Mutex::default(),
        after_a_step: Mutex::default(),
        first_request: Mutex::default(),
        plain: Mutex::default(),
        asked: Mutex::default(),
        on_asked: Mutex::default(),
        fails_after_a_result: Default::default(),
        seen: Mutex::default(),
        sent: Mutex::default(),
        agent: Mutex::default(),
        gate: Mutex::default(),
        agent_call: Mutex::default(),
        agent_tools: Mutex::default(),
        requests: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    Rig {
        core,
        model,
        file,
        state,
        _dir: dir,
        driver: Mutex::default(),
        said: Mutex::default(),
        in_person: Default::default(),
    }
}

/// A settings question carries the daemon's words wherever it is read: what
/// `confirm.list` returns and what the push and the ledger row carry both have
/// the card whole as `plain.detail` and the change in words as `plain.what`.
/// Planting `plain: None` back into `confirm_request` fails every use of this.
pub(crate) fn assert_worded(
    core: &Arc<Core>,
    q: &theseus_protocol::ConfirmRequest,
    what_starts: &str,
) {
    let p = q
        .plain
        .as_ref()
        .expect("a settings question has plain words");
    assert_eq!(p.detail, q.reason, "the card whole, not a cut of it");
    assert!(
        p.what.starts_with(what_starts),
        "{} / {what_starts}",
        p.what
    );
    assert!(
        p.why.is_empty(),
        "the model's reason is not in the words: {p:?}"
    );
    let listed = core.confirm_list().unwrap();
    let listed = listed
        .iter()
        .find(|c| c.correlation_id == q.correlation_id)
        .expect("listed");
    assert_eq!(listed.plain.as_ref(), Some(p), "confirm.list carries it");
    let pushed: Vec<Value> = core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "tool.confirm_requested")
        .map(|(_, r)| r.data)
        .filter(|d| d["correlation_id"] == json!(q.correlation_id))
        .collect();
    assert_eq!(pushed.len(), 1, "one push: {pushed:?}");
    assert_eq!(
        pushed[0]["plain"]["detail"],
        json!(p.detail),
        "the push carries it"
    );
    assert_eq!(pushed[0]["plain"]["what"], json!(p.what));
}

pub(super) fn change(path: &str, value: Value) -> Value {
    json!({"changes": [{"path": path, "value": value}], "reason": "the owner asked for it"})
}

impl Rig {
    /// A model with no tool offered answers `said`, and the turn's own answer to a tool's
    /// result quotes the result.
    pub(super) fn answers_plain(&self, said: &str) {
        *self.model.plain.lock().unwrap() = Some(said.into());
    }

    /// What the turn's own model was given in each request it was asked, as the provider's messages.
    pub(super) fn requests(&self) -> Vec<Vec<Value>> {
        let all = self.model.requests.lock().unwrap();
        all.iter().map(|r| r.messages.clone()).collect()
    }

    /// The requests a model with no tool offered was asked.
    pub(super) fn asked_plain(&self) -> Vec<ProviderRequest> {
        self.model.asked.lock().unwrap().clone()
    }

    /// Have `f` run when the model with no tool offered is next asked.
    pub(super) fn when_the_profile_is_asked(&self, f: impl FnOnce() + Send + 'static) {
        *self.model.on_asked.lock().unwrap() = Some(Box::new(f));
    }

    /// An agent (a request whose first message holds `AGENT:`) reports `said`; its call is held until
    /// `hold` is given a permit when it is.
    pub(super) fn agents_say(&self, said: &str, hold: Option<Arc<tokio::sync::Semaphore>>) {
        *self.model.agent.lock().unwrap() = Some(said.into());
        *self.model.gate.lock().unwrap() = hold;
    }

    /// Task turns run, as the daemon's driver runs them.
    pub(super) fn drive(&self) {
        let h = tokio::spawn(crate::harness::drive(self.core.clone()));
        *self.driver.lock().unwrap() = Some(h);
    }

    /// [`agents_say`](Self::agents_say), and the driver runs.
    pub(super) fn agents_report(&self, said: &str, hold: Option<Arc<tokio::sync::Semaphore>>) {
        self.agents_say(said, hold);
        self.drive();
    }

    /// Before its first request an agent calls `tool` with `input`, and reports after the result.
    pub(super) fn agents_call(&self, tool: &str, input: Value) {
        *self.model.agent_call.lock().unwrap() = Some((tool.into(), input));
    }

    /// The daemon is stopped and started again on the same store: the driver is gone, the model's
    /// script stays, and `tweak` says the settings again.
    pub(super) fn restart(self, tweak: impl FnOnce(&mut Config, &std::path::Path)) -> Rig {
        let Rig {
            core,
            model,
            _dir,
            driver,
            ..
        } = self;
        if let Some(h) = driver.lock().unwrap().take() {
            h.abort();
        }
        let said = model.agent.lock().unwrap().clone();
        drop((core, model));
        let r = rig_in(_dir, tweak);
        *r.model.agent.lock().unwrap() = said;
        r
    }

    /// The tool names each agent request was offered, in the order the requests came.
    pub(super) fn agent_tools(&self) -> Vec<Vec<String>> {
        self.model.agent_tools.lock().unwrap().clone()
    }

    /// The owner's turns from now carry the mark `turn.submit` gives a message typed in person (issue 44):
    /// an unmarked message's exchange has no audience, and no later view turn lays it.
    pub(super) fn in_person(&self) {
        self.in_person
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// The owner's words the next turns start from (`change a setting` when none are set).
    pub(super) fn says(&self, words: &str) {
        *self.said.lock().unwrap() = Some(words.into());
    }

    /// The requests the turn itself made, whole.
    pub(super) fn sent(&self) -> Vec<ProviderRequest> {
        self.model.sent.lock().unwrap().clone()
    }

    /// The model of every request made, and the tools it offered.
    pub(super) fn seen(&self) -> Vec<(String, usize)> {
        self.model.seen.lock().unwrap().clone()
    }

    /// A session posting to `place` (`dm:<user>`), as a binding's place is.
    pub(super) fn session_at(&self, place: &str) -> String {
        let sid = self.session();
        self.core.outbox.bind_place(place, &sid).unwrap();
        sid
    }

    pub(super) fn session(&self) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    /// Have `f` run when the next turn first asks the model.
    pub(super) fn when_the_turn_asks_the_model(&self, f: impl FnOnce() + Send + 'static) {
        *self.model.first_request.lock().unwrap() = Some(Box::new(f));
    }

    /// Have `f` run when the model has the first result of a turn's calls, before it makes the next.
    pub(super) fn after_the_first_step(&self, f: impl FnOnce() + Send + 'static) {
        *self.model.after_a_step.lock().unwrap() = Some(Box::new(f));
    }

    pub(super) async fn turn(&self, sid: &str, call: Value) {
        self.turn_with("config_propose", sid, call).await;
    }

    pub(super) async fn turn_with(&self, tool: &str, sid: &str, call: Value) {
        self.model.calls.lock().unwrap().push((tool.into(), call));
        self.run_turn(sid).await;
    }

    /// One turn of several calls, in order, each made after the result of the
    /// one before: `p1`, `p2`, ...; the owner's words are the turn's input.
    pub(super) async fn steps(&self, sid: &str, steps: Vec<(&str, Value)>) {
        *self.model.made.lock().unwrap() = 0;
        *self.model.steps.lock().unwrap() = steps.into_iter().map(|(t, v)| (t.into(), v)).collect();
        self.run_turn(sid).await;
    }

    /// The turn's own model call fails once it has read a tool's result.
    pub(super) fn the_turn_fails_after_a_result(&self) {
        self.model
            .fails_after_a_result
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// [`steps`](Self::steps), for a turn that may fail (a day's ceiling reached): its result.
    pub(super) async fn try_steps(
        &self,
        sid: &str,
        steps: Vec<(&str, Value)>,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        *self.model.made.lock().unwrap() = 0;
        *self.model.steps.lock().unwrap() = steps.into_iter().map(|(t, v)| (t.into(), v)).collect();
        self.try_turn(sid).await
    }

    pub(super) async fn run_turn(&self, sid: &str) -> theseus_protocol::TurnSubmitResult {
        self.try_turn(sid).await.unwrap()
    }

    /// The call the next turn's model makes.
    pub(super) fn call_next(&self, tool: &str, call: Value) {
        self.model.calls.lock().unwrap().push((tool.into(), call));
    }

    /// [`Rig::turn_with`], for a turn that may end in an error.
    pub(super) async fn try_turn_with(
        &self,
        tool: &str,
        sid: &str,
        call: Value,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        self.model.calls.lock().unwrap().push((tool.into(), call));
        self.try_turn(sid).await
    }

    /// The turn the driver runs after an answer: no input, the call that was waiting.
    pub(super) async fn try_continue(
        &self,
        sid: &str,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        self.run(sid, None).await
    }

    /// One turn, as `run_turn` runs it, and what it ended with.
    pub(super) async fn try_turn(
        &self,
        sid: &str,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        self.run(sid, Some(self.owner_words())).await
    }

    /// The owner's words a turn starts from: `says`, else `change a setting`.
    fn owner_words(&self) -> String {
        self.said
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| "change a setting".into())
    }

    /// [`Rig::try_turn`], for a turn that answers in another place (`answer_at`).
    pub(super) async fn try_turn_answering_at(
        &self,
        sid: &str,
        answer_at: &str,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        self.run_at(sid, Some(self.owner_words()), Some(answer_at.into()))
            .await
    }

    async fn run(
        &self,
        sid: &str,
        input: Option<String>,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        self.run_at(sid, input, None).await
    }

    async fn run_at(
        &self,
        sid: &str,
        input: Option<String>,
        answer_at: Option<String>,
    ) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
        let rec: SessionRecord = self.core.store.get_session(sid).unwrap().unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let marked = input.is_some() && self.in_person.load(std::sync::atomic::Ordering::SeqCst);
        let mark = marked.then(|| crate::view_gate::test_mark(&self.core.runner, sid, true));
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input,
                target,
                sink: EventSink::new(self.core.bus.clone(), sid, None).with_arrival(mark),
                author: "test".into(),
                attachments: vec![],
                arrived: self.core.runner.tools.chat_acts.heard.test_arrival(sid),
                reply_to: None,
                answer_at,
            })
            .await
    }

    pub(super) fn result(&self, sid: &str) -> (ResultStatus, String) {
        self.result_of(sid, "p1")
    }

    pub(super) fn result_of(&self, sid: &str, call: &str) -> (ResultStatus, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    ..
                } if tool_use_id == call => Some((*status, content.clone())),
                _ => None,
            })
            .expect("the proposal's result")
    }

    pub(super) fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    pub(super) fn asked(&self) -> Vec<theseus_protocol::ConfirmRequest> {
        self.core.confirm_list().unwrap()
    }

    pub(super) fn file_text(&self) -> String {
        std::fs::read_to_string(&self.file).unwrap()
    }

    /// A change made earlier, as the daemon leaves one: the text before it kept
    /// as the next copy, the file left as `after`, and its row.
    pub(super) fn seed(&self, before: &str, after: &str, paths: &[&str]) {
        let (n, _) = super::write::keep_previous(&self.state, before).unwrap();
        std::fs::write(&self.file, after).unwrap();
        let row = crate::ledger::LedgerRow::new(
            theseus_protocol::LedgerKind::ConfigChanged,
            None,
            None,
            json!({
                "source": "owner", "question": format!("seed{n}"), "paths": paths,
                "asked_by": {"session_id": "ses_old", "place": "discord:dm:42"},
                "approved_by": "cli", "via": "cli", "restart": true, "previous_copy": n,
                "digest_before": crate::config_copy::sha256(before),
                "digest_after": crate::config_copy::sha256(after),
            }),
        );
        self.core.store.append_ledger(&row).unwrap();
    }
}

/// The card is built from the file and the candidate, and says the model's
/// reason as the model's; the candidate keeps the file's comments.
#[tokio::test]
async fn a_proposal_puts_one_card_built_from_the_two_configs() {
    let r = rig();
    let sid = r.session();
    let mut call = change("import.notes.enabled", json!(false));
    call["reason"] = json!("before it was 999, after it is 1000\nand more");
    r.turn(&sid, call).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    let q = &asks[0];
    assert_eq!(q.tool, super::ACK);
    assert_worded(
        &r.core,
        q,
        "change `import.notes.enabled` from on to off; Theseus then restarts",
    );
    assert!(q.reason.starts_with("Change 1 setting?\n"), "{}", q.reason);
    assert!(
        q.reason
            .contains("- import.notes.enabled: on → off (after a restart)"),
        "the values come from the parsed files: {}",
        q.reason
    );
    assert!(
        q.reason
            .contains("in its own words: \"before it was 999, after it is 1000 and more\""),
        "{}",
        q.reason
    );
    assert!(!q.reason.contains("999 →"), "{}", q.reason);
    // Nothing is written by a proposal.
    assert_eq!(r.file_text(), FILE);
    // The candidate kept for the card has the comments and the one change.
    let kept: Vec<_> = std::fs::read_dir(super::write::dir(&r.state))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(kept.len(), 1);
    let cand = std::fs::read_to_string(kept[0].path()).unwrap();
    assert!(
        cand.contains("enabled = false   # turn it off to stop the import"),
        "{cand}"
    );
    assert!(
        cand.starts_with("# Theseus, on this machine.\nnarrative = false\n"),
        "{cand}"
    );
}

/// A proposal can name settings and values, and nothing else: there is no
/// field in which the model could write a card's "before", or text for the file.
#[tokio::test]
async fn the_model_has_no_field_for_a_before_or_for_text_to_write() {
    let r = rig();
    let sid = r.session();
    let mut call = change("import.notes.enabled", json!(false));
    call["changes"][0]["before"] = json!("true");
    r.turn(&sid, call).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(r.asked().is_empty());
    let sid = r.session();
    r.turn(&sid, json!({"toml": "narrative = true", "reason": "x"}))
        .await;
    assert_eq!(r.result(&sid).0, ResultStatus::Error);
    assert!(r.asked().is_empty());
}

/// A candidate that does not load is refused with the loader's words, and no
/// card is made; neither is a change that changes nothing.
#[tokio::test]
async fn a_candidate_that_does_not_load_makes_no_card() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.nope", json!(true))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("do not load") && text.contains("nope"),
        "{text}"
    );
    assert!(r.asked().is_empty());
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(true)))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("change nothing"), "{text}");
    assert!(r.asked().is_empty());
    assert_eq!(r.file_text(), FILE);
}

/// The owner's yes writes the candidate the card was built from, in one step,
/// and keeps the text it replaced as a numbered copy; the ledger says what
/// changed, who asked and who approved, and the two digests.
#[tokio::test]
async fn a_yes_writes_the_candidate_and_keeps_the_previous_copy() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let done = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(done.approved && !done.resumes);
    let now = r.file_text();
    assert!(
        now.contains("enabled = false   # turn it off to stop the import"),
        "{now}"
    );
    assert!(Config::parse(&now).is_ok());
    let prev = super::write::previous(&r.state);
    assert_eq!(prev.len(), 1);
    assert_eq!(std::fs::read_to_string(&prev[0].1).unwrap(), FILE);
    let row = &r.rows("config.changed")[0];
    assert_eq!(row["paths"], json!(["import.notes.enabled"]));
    assert_eq!(
        (row["approved_by"].as_str(), row["source"].as_str()),
        (Some("cli"), Some("owner"))
    );
    assert_eq!(row["asked_by"]["session_id"], json!(sid));
    assert_eq!(
        row["digest_before"],
        json!(crate::config_copy::sha256(FILE))
    );
    assert_eq!(row["digest_after"], json!(crate::config_copy::sha256(&now)));
    assert!(r.asked().is_empty());
    // The candidate is not left lying about: the copy and the restart's marker remain.
    let left: Vec<String> = std::fs::read_dir(super::write::dir(&r.state))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        left.iter()
            .all(|n| n.starts_with("previous-") || n == "pending.json"),
        "{left:?}"
    );
}

/// A file that changed after the card was made is not overwritten: nothing is
/// written, the question settles declined with the reason, and it is recorded.
#[tokio::test]
async fn a_file_changed_under_the_card_is_refused_and_nothing_is_written() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let edited = FILE.replace("narrative = false", "narrative = true");
    std::fs::write(&r.file, &edited).unwrap();
    let done = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(!done.approved);
    assert_eq!(r.file_text(), edited, "the owner's own edit is untouched");
    assert!(super::write::previous(&r.state).is_empty());
    let row = &r.rows("config.declined")[0];
    assert_eq!(row["outcome"], "refused");
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("changed after this card was made"),
        "{row}"
    );
    assert!(r.rows("config.changed").is_empty());
}

/// A decline, and a question nobody answered, write nothing and are recorded.
#[tokio::test]
async fn a_decline_and_an_expiry_write_nothing_and_are_recorded() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let done = r
        .core
        .confirm_action(&q.correlation_id, false, Some("not today"), "cli")
        .unwrap();
    assert!(!done.approved);
    assert_eq!(r.file_text(), FILE);
    let d = &r.rows("config.declined")[0];
    assert_eq!(
        (d["outcome"].as_str(), d["why"].as_str()),
        (Some("declined"), Some("not today"))
    );
    // A second, which nobody answers.
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(r.core.expire_questions(q.requested_at_ms + ttl + 1), 1);
    assert_eq!(r.file_text(), FILE);
    let d = r.rows("config.declined");
    assert_eq!(
        (
            d.len(),
            d[1]["outcome"].as_str(),
            d[1]["answered_by"].as_str()
        ),
        (2, Some("expired"), Some("expiry"))
    );
    assert!(r.rows("config.changed").is_empty());
}

/// A yes from a shared place counts for nothing: the question keeps waiting,
/// the file is as it was; the owner's own DM is private.
#[tokio::test]
async fn a_yes_from_a_shared_place_is_refused() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let from = |channel: Option<u64>| crate::approval::Answerer {
        label: format!("discord:{OWNER}"),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: channel.unwrap_or(OWNER + 1).to_string(),
            guild_id: channel.map(|_| "900000000000000001".to_string()),
        }),
        agent: None,
    };
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, from(Some(LAB)))
        .unwrap_err();
    assert!(e.to_string().contains("shared"), "{e:#}");
    assert_eq!(r.file_text(), FILE);
    assert_eq!(r.asked().len(), 1, "it still waits");
    r.core
        .confirm_action(&q.correlation_id, true, None, from(None))
        .unwrap();
    assert!(r.file_text().contains("enabled = false"));
}

/// A task's session is refused, and so is a place that is shared.
#[tokio::test]
async fn a_task_and_a_shared_place_cannot_propose() {
    let r = rig();
    let parent = r.session();
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let task = rec.session_id;
    r.turn(&task, change("import.notes.enabled", json!(false)))
        .await;
    let (status, text) = r.result(&task);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("not for a task"), "{text}");
    assert!(r.asked().is_empty());
}

/// A change that reaches a sensitive setting gets a card that is marked as
/// needing the second proof: named whole, as a table, or deeper. With no app
/// paired it cannot be approved, and nothing is written.
#[tokio::test]
async fn a_sensitive_change_is_asked_with_the_second_proof_marked() {
    let r = rig();
    for call in [
        change("kernel.daily_spend_ceiling_usd", json!(300)),
        change("tools.roots", json!(["/tmp/elsewhere"])),
        change("mcp.servers.files", json!({"command": ["files-server"]})),
        change("policy.enforcement", json!("open")),
        change("model.system", json!("be brief")),
        change("discord.rest_proxy", json!("evil.test:80")),
        change("model.provider", json!("anthropic")),
    ] {
        let sid = r.session();
        r.turn(&sid, call.clone()).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{call}: {text}");
        assert!(text.contains("with the second proof"), "{call}: {text}");
        let q = r.asked().pop().expect("a card");
        assert!(q.needs_proof && !q.needs_fresh_proof, "{call}");
        let e = r
            .core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap_err();
        assert!(
            e.to_string().contains("theseus owner proof enroll"),
            "{call}: {e:#}"
        );
        r.core
            .confirm_action(&q.correlation_id, false, None, "cli")
            .unwrap();
    }
    assert_eq!(r.file_text(), FILE);
}

/// Line 2: an approved change that needs a restart keeps its marker first,
/// and the daemon asks to restart onto it, by the path the exec carries.
#[tokio::test]
async fn a_yes_that_needs_a_restart_keeps_a_marker_and_asks_for_one() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    let marker = super::restart::marker_path(&r.state);
    let p = super::restart::load(&marker).expect("the marker is kept before the restart");
    assert_eq!(p.stage, super::restart::Stage::Restarting);
    assert_eq!(p.paths, ["import.notes.enabled"]);
    assert_eq!(std::fs::read_to_string(&p.previous).unwrap(), FILE);
    assert_eq!(p.digest_after, crate::config_copy::sha256(&r.file_text()));
    assert_eq!(
        (p.session_id.as_str(), p.approved_by.as_str()),
        (sid.as_str(), "cli")
    );
    let asked = loop {
        if let Some(a) = r.core.restart_requested() {
            break a;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(asked.reference, format!("change:{}", marker.display()));
    assert_eq!(asked.tables, ["import.notes.enabled"]);
}

/// Back and healthy: the marker is put away as confirmed, with how long it took.
#[tokio::test]
async fn a_start_that_serves_confirms_the_change() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    let marker = super::restart::marker_path(&r.state);
    let super::restart::Begin::Watch(p) = super::restart::begin(&marker) else {
        panic!("the first start onto the change watches")
    };
    let sock = r.state.join("scratch.sock");
    let _serving = tokio::net::UnixListener::bind(&sock).unwrap();
    r.core.clone().confirm_change(marker.clone(), p, sock).await;
    assert!(!marker.exists(), "nothing is pending any more");
    let done =
        super::restart::load(&marker.with_file_name(format!("done-{}.json", q.correlation_id)))
            .unwrap();
    assert_eq!(done.stage, super::restart::Stage::Confirmed);
    assert!(done.took_ms.is_some());
    assert!(r.file_text().contains("enabled = false"));
}

/// Not healthy within the stated time: the settings before the change are
/// put back as the config, the restart is asked once more, and the next start
/// only tells (it never tries the change again).
#[tokio::test(start_paused = true)]
async fn an_unhealthy_start_goes_back_once_and_stays_on_the_old_settings() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    let marker = super::restart::marker_path(&r.state);
    let super::restart::Begin::Watch(p) = super::restart::begin(&marker) else {
        panic!("the first start onto the change watches")
    };
    let nowhere = r.state.join("nothing-listens-here.sock");
    r.core
        .clone()
        .confirm_change(marker.clone(), p, nowhere)
        .await;
    assert_eq!(r.file_text(), FILE, "the old settings are the config again");
    let p = super::restart::load(&marker).unwrap();
    assert_eq!(p.stage, super::restart::Stage::RolledBack);
    assert!(
        p.why
            .as_deref()
            .unwrap()
            .contains("not healthy within 120 s"),
        "{p:?}"
    );
    let again = r
        .core
        .restart_requested()
        .expect("one more restart is asked");
    assert_eq!(
        (again.copy_sha256, again.vault_sha256),
        (p.digest_after.clone(), p.digest_before.clone())
    );
    // The start after it tells, and nothing in the marker asks for another restart.
    assert!(matches!(
        super::restart::begin(&marker),
        super::restart::Begin::Tell(_)
    ));
    r.core.tell_rolled_back(&p);
    let row = &r.rows("config.rolled_back")[0];
    assert_eq!(row["paths"], json!(["import.notes.enabled"]));
    assert!(
        row["why"].as_str().unwrap().contains("not healthy"),
        "{row}"
    );
    super::restart::finish(&marker, &p);
    assert_eq!(
        super::restart::begin(&marker),
        super::restart::Begin::Normal
    );
}

/// The copies, the candidates and the marker are inside a path the floor
/// holds, so a tool's read or write there waits for the owner as the store's
/// own files do. They were beside the store, in a directory the floor does not
/// name, and a turn could plant a marker there.
#[test]
fn the_copies_and_the_marker_are_on_the_floor() {
    let r = rig();
    let floor = &r.core.tools.ctx.floor;
    let held = |p: &std::path::Path| {
        let p = theseus_tools::paths::canonical_best_effort(p);
        floor.iter().any(|f| theseus_tools::paths::within(&p, f))
    };
    let dir = super::write::dir(&r.state);
    assert!(held(&dir), "{dir:?} is not under the floor {floor:?}");
    assert!(held(&super::restart::marker_path(&r.state)));
    assert!(held(&dir.join("previous-0001.toml")));
    assert!(
        !held(&r.state.join("config-changes")),
        "the old place is not on the floor"
    );
}

/// What cannot be shown whole is refused, never cut: a long prompt, and a
/// change that reaches more settings than one card holds.
#[tokio::test]
async fn a_card_never_cuts_what_it_cannot_show_whole() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("model.system", json!("p".repeat(5_000))))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("model.system") && text.contains("longer than a card can show whole"),
        "{text}"
    );
    assert!(r.asked().is_empty());
    // A short one is shown whole, with its quotes and line breaks escaped.
    let sid = r.session();
    r.turn(
        &sid,
        change(
            "telemetry.service_name",
            json!("be brief\n- narrative: on → off (at once)"),
        ),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().remove(0);
    assert!(
        q.reason
            .contains(r#"→ "be brief\n- narrative: on → off (at once)" (after a restart)"#),
        "{}",
        q.reason
    );
    assert_eq!(q.reason.matches("\n- ").count(), 1, "{}", q.reason);
}

/// Outside text read since the owner's last message is said; before it, not.
#[test]
fn outside_text_is_read_from_the_results_since_the_owner_last_spoke() {
    use crate::node::{Body, Node};
    let msg = |id: &str, at: u64| {
        let mut n = Node::harness_body(
            "nod",
            "ses_a",
            "test",
            Body::UserMessage {
                text: "hi".into(),
                attachments: vec![],
                parts: Vec::new(),
            },
        );
        n.id = id.into();
        n.created_at_ms = at;
        n
    };
    let page = |id: &str, at: u64, url: Option<&str>| {
        let mut n = Node::harness_body(
            "nod",
            "ses_a",
            "test",
            Body::ToolResult {
                tool_use_id: format!("tu_{id}"),
                tool: "http.fetch".into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "text".into(),
                correlation_id: None,
                bytes_total: 4,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
                image: None,
                external: url.map(|u| theseus_tools::External { url: u.into() }),
                payload: None,
                parts: Vec::new(),
            },
        );
        n.id = id.into();
        n.created_at_ms = at;
        n
    };
    let nodes = |v: Vec<Node>| {
        v.into_iter()
            .enumerate()
            .map(|(i, n)| (i as u64, n))
            .collect::<Vec<_>>()
    };
    assert!(
        !super::outside_since_owner(&nodes(vec![msg("a", 1), page("b", 2, None)])),
        "a local result"
    );
    assert!(super::outside_since_owner(&nodes(vec![
        msg("a", 1),
        page("b", 2, Some("https://x.test/"))
    ])));
    assert!(
        !super::outside_since_owner(&nodes(vec![
            page("b", 1, Some("https://x.test/")),
            msg("a", 2)
        ])),
        "read before the owner spoke again"
    );
}

/// The owner's plain word: one card that says what a restart cuts, a yes that
/// restarts with a marker and writes nothing, and the start that follows says
/// it is back.
#[tokio::test]
async fn the_owners_word_restarts_with_one_card_and_says_it_is_back() {
    let r = rig();
    let sid = r.session();
    r.turn_with("daemon_restart", &sid, json!({"reason": "it feels stuck"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().remove(0);
    assert_worded(
        &r.core,
        &q,
        "restart Theseus, with no change to its settings",
    );
    assert!(
        q.reason
            .starts_with("Restart Theseus?\nNo turn is running now."),
        "{}",
        q.reason
    );
    assert!(
        q.reason.contains("cuts what is running") && q.reason.contains("waits up to 120 s"),
        "{}",
        q.reason
    );
    assert!(
        q.reason.contains("in its own words: \"it feels stuck\""),
        "{}",
        q.reason
    );
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert_eq!(r.file_text(), FILE, "nothing is written");
    let marker = super::restart::marker_path(&r.state);
    let p = super::restart::load(&marker).expect("a marker for the start that follows");
    assert!(p.restart_only && p.paths.is_empty());
    assert!(
        r.core.restart_requested().is_some() || {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            r.core.restart_requested().is_some()
        }
    );
    let super::restart::Begin::Watch(p) = super::restart::begin(&marker) else {
        panic!("watch")
    };
    let sock = r.state.join("scratch.sock");
    let _serving = tokio::net::UnixListener::bind(&sock).unwrap();
    r.core.clone().confirm_change(marker.clone(), p, sock).await;
    let done =
        super::restart::load(&marker.with_file_name(format!("done-{}.json", q.correlation_id)))
            .unwrap();
    assert_eq!(done.stage, super::restart::Stage::Confirmed);
    assert_eq!(r.rows("config.changed")[0]["restart_only"], json!(true));
}

/// A second yes while a change is still being applied is refused, and a
/// second card made after the first yes cannot replace its marker.
#[tokio::test]
async fn a_second_change_waits_for_the_first_to_be_applied() {
    let r = rig();
    let (s1, s2) = (r.session(), r.session());
    r.turn(&s1, change("import.notes.enabled", json!(false)))
        .await;
    r.turn(&s2, change("standing.yes_fact", json!(true))).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 2);
    let card = |path: &str| {
        asks.iter()
            .find(|q| q.reason.contains(path))
            .unwrap()
            .correlation_id
            .clone()
    };
    r.core
        .confirm_action(&card("import.notes.enabled"), true, None, "cli")
        .unwrap();
    let after_first = r.file_text();
    let done = r
        .core
        .confirm_action(&card("standing.yes_fact:"), true, None, "cli")
        .unwrap();
    assert!(!done.approved);
    assert_eq!(r.file_text(), after_first, "the second wrote nothing");
    let d = &r.rows("config.declined")[0];
    assert!(
        d["why"]
            .as_str()
            .unwrap()
            .contains("another change is still being applied"),
        "{d}"
    );
    let p = super::restart::load(&super::restart::marker_path(&r.state)).unwrap();
    assert_eq!(
        p.paths,
        ["import.notes.enabled"],
        "the first change's marker is intact"
    );
}

/// An agent's process on the CLI is not the owner at a card: its yes is
/// refused and recorded, the question keeps waiting, and the owner's own
/// answer then counts.
#[tokio::test]
async fn an_agents_yes_is_refused_and_the_owners_counts() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    let agent = crate::approval::Answerer {
        label: "agent:claude".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, agent)
        .unwrap_err();
    assert!(e.to_string().contains("only the owner answers it"), "{e:#}");
    assert_eq!(r.file_text(), FILE);
    assert_eq!(r.asked().len(), 1, "it still waits");
    assert_eq!(r.rows("approval.refused").len(), 1);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(r.file_text().contains("enabled = false"));
}

/// The card's reason line, cut at its length, stops before a value the
/// vault's board knows: the card holds the value whole or not at all, and
/// so does every post the question made.
#[tokio::test]
async fn a_proposals_card_reason_stops_before_a_known_value() {
    use crate::tests_cut_sites::{whole_or_absent, V};
    let r = rig();
    r.core
        .secrets
        .hold("cut_test", crate::secrets::Secret::new(V.into()));
    let sid = r.session();
    let mut call = change("import.notes.enabled", json!(false));
    call["reason"] = json!(format!("{}{V} and more", "x".repeat(190)));
    r.turn(&sid, call).await;
    let q = r.asked().remove(0);
    whole_or_absent("the card", &q.reason, V);
    assert!(
        q.reason
            .contains(&format!("in its own words: \"{}…\"", "x".repeat(190))),
        "{}",
        q.reason
    );
    for a in r.core.kernel.outbox_actions().unwrap() {
        whole_or_absent("a post", &crate::outbox::body_of(&a).to_string(), V);
    }
}
