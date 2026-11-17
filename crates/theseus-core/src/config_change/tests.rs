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

const OWNER: u64 = 4_100_000_000_000_000_001;
const LAB: u64 = 4_100_000_000_000_000_777;

/// The settings file the owner keeps: comments, an order of its own.
const FILE: &str = r#"# Theseus, on this machine.
narrative = false

[secrets]
anthropic_api_key = "op://v/i/f"

# The notes import.
[import.notes]
enabled = true   # turn it off to stop the import
"#;

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

fn change(path: &str, value: Value) -> Value {
    json!({"changes": [{"path": path, "value": value}], "reason": "the owner asked for it"})
}

impl Rig {
    fn session(&self) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    async fn turn(&self, sid: &str, call: Value) {
        self.model
            .calls
            .lock()
            .unwrap()
            .push(("config_propose".into(), call));
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
                recompile: None,
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

/// A change that reaches a sensitive setting is refused before a card is
/// made, with the seam's words: named whole, as a table, or deeper.
#[tokio::test]
async fn a_sensitive_change_is_refused_with_the_seams_words() {
    let r = rig();
    for call in [
        change("kernel.daily_spend_ceiling_usd", json!(300)),
        change("tools.roots", json!(["/tmp/elsewhere"])),
        change("mcp.servers.files", json!({"command": ["files-server"]})),
        change("policy.enforcement", json!("open")),
    ] {
        let sid = r.session();
        r.turn(&sid, call.clone()).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{call}: {text}");
        assert!(
            text.contains("this setting needs the second proof, which is not installed yet"),
            "{call}: {text}"
        );
        assert!(r.asked().is_empty(), "{call}");
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
