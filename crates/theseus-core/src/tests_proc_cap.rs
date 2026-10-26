//! `proc.run`'s time limit is told, not just applied (theseus-spntl): an ask past `[tools] proc_timeout_max_secs`
//! is named in the result of that call and the cap in the tool's description, a call that names no limit gets the
//! default (the maximum), and a call inside the cap hears nothing of it.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = crate::rpc::Parts::for_tests(cfg, fake.clone(), store);
    p.launcher = Arc::new(crate::toolrun::InlineLauncher);
    Rig {
        core: Core::build(p).unwrap(),
        fake,
        _dir: dir,
    }
}

impl Rig {
    /// One turn that runs `input` as `proc_run`: its result's text and meta.
    async fn ask(&self, input: Value) -> (String, Value) {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        let sid = rec.session_id.clone();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some(input.to_string()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap();
        self.core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    status: ResultStatus::Ok,
                    content,
                    meta,
                    ..
                } => Some((content, meta)),
                _ => None,
            })
            .expect("an ok result")
    }
}

fn script(input: Value) -> Vec<Scripted> {
    vec![
        Scripted::tools("", &[("t1", "proc_run", input)]),
        Scripted::text("Done."),
    ]
}

/// Asked 7200 s, the call is held to 3600 s and the result says what was asked, what it was held to, and that the
/// tree stops at the limit; the description the model was given names the cap.
#[tokio::test]
async fn a_clamped_ask_is_told_in_the_result_and_the_description() {
    let input = json!({"argv": ["true"], "timeout_secs": 7200});
    let r = rig(script(input.clone()));
    let (text, meta) = r.ask(input).await;
    assert!(text.contains("7200 s was asked, held to 3600 s"), "{text}");
    assert!(text.contains("whole process tree is stopped"), "{text}");
    assert_eq!(meta["timeout_clamped"]["asked_secs"], 7200, "{meta}");
    assert_eq!(meta["timeout_clamped"]["held_to_secs"], 3600, "{meta}");
    let tools = &r.fake.requests()[0].tools;
    let run = tools.iter().find(|t| t["name"] == "proc_run").unwrap();
    let said = run["description"].as_str().unwrap();
    assert!(said.contains("A run lasts at most 3600 s"), "{said}");
    assert!(said.contains("the whole process tree is stopped"), "{said}");
}

/// A call that names no limit gets the maximum, 3600 s, not a tenth of it.
#[tokio::test]
async fn an_unstated_ask_gets_the_maximum() {
    let r = rig(vec![]);
    // The default itself, and the template that restates it, agree.
    assert_eq!(crate::config::ToolsConfig::default().proc_timeout_secs, 3600);
    assert_eq!(Config::example().tools.proc_timeout_secs, 3600);
    let tool = r.core.tools.registry.get("proc.run").unwrap();
    let ms = r
        .core
        .tools
        .deadline_ms(tool.as_ref(), &json!({"argv": ["true"]}));
    assert_eq!(ms, (3600 + 30) * 1000);
}

/// An ask inside the cap hears nothing of it: no note in the text, none in the meta.
#[tokio::test]
async fn an_ask_inside_the_cap_is_silent() {
    let input = json!({"argv": ["true"], "timeout_secs": 60});
    let r = rig(script(input.clone()));
    let (text, meta) = r.ask(input).await;
    assert!(!text.contains("was asked"), "{text}");
    assert!(meta.get("timeout_clamped").is_none(), "{meta}");
}
