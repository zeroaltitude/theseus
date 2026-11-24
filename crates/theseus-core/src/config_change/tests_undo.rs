//! `config.history` and `config.undo` through the whole core, on a scratch
//! settings file and a scratch state directory (issue 22, undo).
//!
//! A change made earlier is seeded as the daemon makes one (the text before it
//! kept as the next numbered copy, a `config.changed` row naming its settings
//! and both digests, the file as it was left); what is under test is what
//! the owner is then shown and what a yes writes.
//!
//! - history lists the changes with before and after and who approved;
//! - an undo is a card like any proposal's: nothing is written until the yes,
//!   and a yes puts back exactly the settings of that change, keeping what
//!   was changed since to other settings;
//! - a setting changed again since is said on the card with both values;
//! - undoing a sensitive change is held to the second proof, as a proposal is;
//! - a task's session can neither read the history nor undo.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_protocol::{ConfigHistoryParams, DiscordOrigin, LedgerKind, SessionKind};

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 4_100_000_000_000_000_001;

/// The settings file the owner keeps: comments, an order of its own.
const FILE: &str = r#"# Theseus, on this machine.
[standing]
yes_fact = false

[secrets]
anthropic_api_key = "op://v/i/f"

# The notes import.
[import.notes]
enabled = true   # turn it off to stop the import
"#;

/// FILE after change 1 (the notes import off) and after change 2 (the standing yes fact on).
pub(super) fn after_1() -> String {
    FILE.replace("enabled = true", "enabled = false")
}

fn after_2() -> String {
    after_1().replace("yes_fact = false", "yes_fact = true")
}

/// Answers each turn's first request with the call its input names.
struct Model {
    calls: Mutex<Vec<(String, Value)>>,
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
        let answers_a_call = req
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
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

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    file: PathBuf,
    state: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
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
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        calls: Mutex::default(),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    Rig {
        core,
        model,
        file,
        state,
        _dir: dir,
    }
}

impl Rig {
    fn session(&self) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    async fn turn(&self, sid: &str, tool: &str, call: Value) {
        self.model.calls.lock().unwrap().push((tool.into(), call));
        let rec: SessionRecord = self.core.store.get_session(sid).unwrap().unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some("change a setting".into()),
                target,
                sink: EventSink::new(self.core.bus.clone(), sid, None),
                author: "test".into(),
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap();
    }

    fn result(&self, sid: &str) -> (ResultStatus, String) {
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
                } if tool_use_id == "p1" => Some((*status, content.clone())),
                _ => None,
            })
            .expect("the proposal's result")
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    fn asked(&self) -> Vec<theseus_protocol::ConfirmRequest> {
        self.core.confirm_list().unwrap()
    }

    fn file_text(&self) -> String {
        std::fs::read_to_string(&self.file).unwrap()
    }
}

pub(super) fn undo(change: Option<u32>) -> Value {
    match change {
        Some(n) => json!({"change": n, "reason": "the owner wants it back"}),
        None => json!({"reason": "the owner wants it back"}),
    }
}

impl Rig {
    /// A change made earlier: the text before it kept as the next copy, the
    /// file left as `after`, and its row. Returns the change's number.
    fn seed(&self, before: &str, after: &str, paths: &[&str]) -> u32 {
        let (n, _) = super::write::keep_previous(&self.state, before).unwrap();
        std::fs::write(&self.file, after).unwrap();
        let row = LedgerRow::new(
            LedgerKind::ConfigChanged,
            None,
            None,
            json!({
                "source": "owner", "question": format!("seed{n}"), "paths": paths,
                "asked_by": {"session_id": "ses_old", "place": "discord:dm:42"},
                "approved_by": "cli", "via": "cli", "restart": true, "previous_copy": n,
                "digest_before": sha256(before), "digest_after": sha256(after),
            }),
        );
        self.core.store.append_ledger(&row).unwrap();
        n
    }

    /// Changes 1 (notes off) and 2 (the standing yes fact on), the file after both.
    fn two_changes(&self) {
        self.seed(FILE, &after_1(), &["import.notes.enabled"]);
        self.seed(&after_1(), &after_2(), &["standing.yes_fact"]);
    }

    fn card(&self) -> String {
        self.asked().remove(0).reason
    }

    fn yes(&self) {
        let q = self.asked().remove(0);
        let done = self
            .core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        assert!(done.approved && !done.resumes);
    }
}

/// The history lists the last changes, newest first, with each setting's
/// value before and after, who asked and who approved; it makes no card.
#[tokio::test]
async fn history_lists_the_changes_with_who_approved_each() {
    let r = rig();
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_history", json!({})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("The last 2 changes to Theseus's settings, newest first:"),
        "{text}"
    );
    assert!(
        text.contains("asked in discord:dm:42; approved by cli; it needed a restart."),
        "{text}"
    );
    assert!(text.contains("- standing.yes_fact: off → on"), "{text}");
    assert!(text.contains("- import.notes.enabled: on → off"), "{text}");
    assert!(
        text.find("#2").unwrap() < text.find("#1").unwrap(),
        "{text}"
    );
    assert!(r.asked().is_empty(), "a read asks nothing");
    assert_eq!(r.file_text(), after_2());
}

/// The undo of the newest change is a card like a proposal's: nothing is
/// written until the yes, and the yes puts back that change's settings alone.
#[tokio::test]
async fn an_undo_goes_through_the_card_and_a_yes_puts_back_exactly_its_settings() {
    let r = rig();
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(None)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    super::tests::assert_worded(
        &r.core,
        &r.asked()[0],
        "undo change #2, putting back `standing.yes_fact` from on to off",
    );
    let card = r.card();
    assert!(card.starts_with("Undo change #2?\n"), "{card}");
    assert!(
        card.contains("- standing.yes_fact: on → off (after a restart)"),
        "{card}"
    );
    assert!(
        !card.contains("import.notes"),
        "only that change's settings: {card}"
    );
    assert!(
        card.contains("Theseus restarts itself to apply this."),
        "{card}"
    );
    assert!(card.contains("the owner wants it back"), "{card}");
    assert_eq!(r.file_text(), after_2(), "no yes, nothing written");
    assert_eq!(r.rows("config.changed").len(), 2);
    r.yes();
    assert_eq!(
        r.file_text(),
        after_1(),
        "exactly change 2's settings are back, comments kept"
    );
    assert_eq!(
        super::write::previous(&r.state).len(),
        3,
        "the text before the undo is kept too"
    );
    assert_eq!(r.rows("config.changed").len(), 3);
    let undone = &r.rows("config.undone")[0];
    assert_eq!(undone["undone"], json!(2));
    assert_eq!(undone["undone_question"], json!("seed2"));
    assert_eq!(undone["paths"], json!(["standing.yes_fact"]));
    assert_eq!(undone["approved_by"], json!("cli"));
    // It restarts and goes back as any approved change does.
    let marker = super::restart::load(&super::restart::marker_path(&r.state)).unwrap();
    assert_eq!(marker.paths, ["standing.yes_fact"]);
}

/// An older change is undone alone: a later change to another setting stays.
#[tokio::test]
async fn an_older_change_is_undone_and_later_changes_to_other_settings_stay() {
    let r = rig();
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(Some(1))).await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    let card = r.card();
    assert!(card.starts_with("Undo change #1?\n"), "{card}");
    assert!(
        card.contains("- import.notes.enabled: off → on (after a restart)"),
        "{card}"
    );
    assert!(!card.contains("yes_fact"), "{card}");
    assert!(!card.contains("Changed again since"), "{card}");
    r.yes();
    let now = r.file_text();
    assert!(
        now.contains("enabled = true   # turn it off to stop the import"),
        "{now}"
    );
    assert!(
        now.contains("yes_fact = true"),
        "the later change stays: {now}"
    );
    assert_ne!(now, FILE, "the old file is not written back");
}

/// A setting that was changed again since is put back too, and the card says
/// so, showing what the change set, what it is now and what comes back.
#[tokio::test]
async fn a_setting_changed_again_since_is_said_on_the_card() {
    let r = rig();
    let ttl = |n: u32| format!("{FILE}\n[kernel]\nconfirm_ttl_secs = {n}\n");
    r.seed(FILE, &ttl(50), &["kernel.confirm_ttl_secs"]);
    r.seed(&ttl(50), &ttl(70), &["kernel.confirm_ttl_secs"]);
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(Some(1))).await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    let card = r.card();
    assert!(
        card.contains(
            "Changed again since, so this overwrites a later change:\n- kernel.confirm_ttl_secs: it set 50; it is now 70; this puts back not set (its default)"
        ),
        "{card}"
    );
    assert!(
        card.contains("- kernel.confirm_ttl_secs: 70 → not set (its default) (after a restart)"),
        "{card}"
    );
    r.yes();
    assert!(
        !r.file_text().contains("confirm_ttl_secs"),
        "{}",
        r.file_text()
    );
    assert_eq!(
        r.rows("config.undone")[0]["changed_since"],
        json!(["kernel.confirm_ttl_secs"])
    );
}

/// A decline writes nothing and keeps the change as it was.
#[tokio::test]
async fn a_declined_undo_writes_nothing() {
    let r = rig();
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(None)).await;
    let q = r.asked().remove(0);
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    assert_eq!(r.file_text(), after_2());
    assert_eq!(super::write::previous(&r.state).len(), 2);
    assert!(r.rows("config.undone").is_empty());
    assert_eq!(r.rows("config.declined").len(), 1);
}

/// Undoing a sensitive change is a sensitive change: its card is marked as
/// needing the second proof, and a yes alone writes nothing.
#[tokio::test]
async fn undoing_a_sensitive_change_asks_with_the_second_proof() {
    let r = rig();
    let ceiling = format!("{FILE}\n[kernel]\ndaily_spend_ceiling_usd = 300.0\n");
    r.seed(FILE, &ceiling, &["kernel.daily_spend_ceiling_usd"]);
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(None)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().remove(0);
    assert!(q.needs_proof, "{q:?}");
    assert!(
        q.reason.contains("kernel.daily_spend_ceiling_usd"),
        "{}",
        q.reason
    );
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap_err();
    assert!(
        e.to_string().contains("theseus owner proof enroll"),
        "{e:#}"
    );
    assert_eq!(r.file_text(), ceiling);
}

/// What an undo cannot do is said, and nothing is asked.
#[tokio::test]
async fn an_undo_that_cannot_be_done_says_why_and_asks_nothing() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(None)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("nothing to undo"), "{text}");
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(Some(7))).await;
    let (_, text) = r.result(&sid);
    assert!(text.contains("no change #7"), "{text}");
    let sid = r.session();
    r.turn(
        &sid,
        "config_undo",
        json!({"change": 1, "toml": "narrative = true"}),
    )
    .await;
    assert_eq!(
        r.result(&sid).0,
        ResultStatus::Error,
        "the model has no field for text to write"
    );
    assert!(r.asked().is_empty());
}

/// A task's session can neither read the history nor undo, in each tool's own
/// words; the owner's other callers are the wall's (`wall::judged`).
#[tokio::test]
async fn a_task_cannot_read_the_history_or_undo() {
    let r = rig();
    r.two_changes();
    let parent = r.session();
    // A task's session runs one turn, so each tool gets a task of its own.
    let task_of = |parent: &str| {
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        rec.task = Some(crate::session::TaskOf {
            parent_session: parent.to_string(),
            parent_execution: "exe_x".into(),
            by: "tool_x".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    for (tool, call, name) in [
        ("config_history", json!({}), "config.history"),
        ("config_undo", undo(None), "config.undo"),
    ] {
        let task = task_of(&parent);
        r.turn(&task, tool, call).await;
        let (status, text) = r.result(&task);
        assert_eq!(status, ResultStatus::Error, "{tool}: {text}");
        assert!(
            text.contains(&format!("{name} is not for a task")),
            "{tool}: {text}"
        );
        assert!(
            !text.contains("yes_fact"),
            "no setting is shown to a task: {text}"
        );
    }
    assert!(r.asked().is_empty());
    assert_eq!(r.file_text(), after_2());
}

/// The history over the wire (`theseus config history`) is the owner's alone:
/// the CLI of the owner gets the words, a person in a shared channel is
/// refused, in words, and the refusal is ledgered.
#[tokio::test]
async fn the_history_over_the_wire_is_the_owners_alone() {
    let r = rig();
    r.two_changes();
    let said = r
        .core
        .config_history(&ConfigHistoryParams::default(), "cli")
        .unwrap();
    assert_eq!(said.changes, 2);
    assert!(
        said.words.contains("#2") && said.words.contains("approved by cli"),
        "{}",
        said.words
    );
    let one = ConfigHistoryParams {
        limit: Some(1),
        ..Default::default()
    };
    assert_eq!(r.core.config_history(&one, "cli").unwrap().changes, 1);
    let stranger = Answerer {
        label: "discord:999".into(),
        surface: Surface::Discord,
        discord: Some(DiscordOrigin {
            user_id: "999".into(),
            channel_id: "777".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    // An agent's process on the owner's own CLI is not the owner.
    let agent = Answerer {
        label: "agent:claude".into(),
        surface: Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    let e = r
        .core
        .config_history(&ConfigHistoryParams::default(), agent)
        .unwrap_err();
    assert!(
        e.to_string()
            .contains("an agent's process is not the owner"),
        "{e}"
    );
    let e = r
        .core
        .config_history(&ConfigHistoryParams::default(), stranger)
        .unwrap_err();
    assert!(
        e.to_string().contains("did not count") || e.to_string().contains("owner"),
        "{e}"
    );
    let refused = r.rows("approval.refused");
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(
        refused.iter().all(|x| x["act"] == json!("config.history")),
        "{refused:?}"
    );
}

/// Which changes exist comes from the daemon's own record, the ledger's rows,
/// and each copy is believed only when its digest is the one its row recorded.
/// A copy somebody put in the directory (it is on the floor, so no tool can,
/// but a program running as the owner could) is not listed, cannot be undone
/// to, and an altered copy of a real change is not believed.
#[tokio::test]
async fn a_planted_copy_is_not_listed_and_cannot_be_undone_to() {
    let r = rig();
    r.two_changes();
    let dir = super::write::dir(&r.state);
    let evil = "narrative = false\n\n[policy]\nenforcement = \"open\"\n";
    std::fs::write(dir.join("previous-0099.toml"), evil).unwrap();
    let said = r
        .core
        .config_history(&ConfigHistoryParams::default(), "cli")
        .unwrap();
    assert_eq!(said.changes, 2, "{}", said.words);
    assert!(
        !said.words.contains("#99") && !said.words.contains("enforcement"),
        "{}",
        said.words
    );
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(Some(99))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("no change #99"), "{text}");
    // A real change's copy, overwritten with other text, is not believed.
    std::fs::write(dir.join("previous-0001.toml"), evil).unwrap();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(Some(1))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("no longer kept") && !text.contains("enforcement"),
        "{text}"
    );
    assert!(r.asked().is_empty());
    assert_eq!(r.file_text(), after_2());
}

/// An undo's card is answered as a proposal's is: an agent's process on the
/// CLI is not the owner, so its yes is refused and recorded, nothing is
/// written, the question keeps waiting, and the owner's own yes then counts.
#[tokio::test]
async fn an_agents_yes_to_an_undo_is_refused_and_the_owners_counts() {
    let r = rig();
    r.two_changes();
    let sid = r.session();
    r.turn(&sid, "config_undo", undo(None)).await;
    let q = r.asked().remove(0);
    let agent = Answerer {
        label: "agent:claude".into(),
        surface: Surface::Cli,
        discord: None,
        agent: Some("claude".into()),
    };
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, agent)
        .unwrap_err();
    assert!(e.to_string().contains("only the owner answers it"), "{e:#}");
    assert_eq!(r.file_text(), after_2());
    assert_eq!(r.asked().len(), 1, "it still waits");
    assert_eq!(r.rows("approval.refused").len(), 1);
    assert!(r.rows("config.undone").is_empty());
    r.yes();
    assert_eq!(r.file_text(), after_1());
}
