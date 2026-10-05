//! `proc.run`'s `steps` (theseus-7gir.3) through the whole core: the gate
//! judges each step as the call it would be alone, and the batch takes the
//! strictest.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    Rig {
        core,
        root,
        _dir: dir,
    }
}

fn run(input: Value) -> Scripted {
    Scripted::tools("", &[("t1", "proc_run", input)])
}

impl Rig {
    async fn turn(&self, input: &str) -> TurnSubmitResult {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                session: rec,
                input: Some(input.into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
            })
            .await
            .unwrap()
    }

    /// The call's gate: its posture, whether the floor asked, and its reason.
    fn gate(&self, sid: &str) -> (String, bool, String) {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    let d = g.decision.clone().unwrap_or_default();
                    Some((d.posture.unwrap_or_default(), d.floor, d.reason))
                }
                _ => None,
            })
            .expect("a tool call with its gate")
    }
}

/// Allow-listed steps run open; a batch with one step that is not runs at
/// the tool's posture, its reason naming that step.
#[tokio::test]
async fn a_batch_takes_its_strictest_steps_posture() {
    let listed = json!({"steps": [{"argv": ["true"]}, {"argv": ["printf", "x"]}]});
    let mixed = json!({"steps": [{"argv": ["true"]}, {"argv": ["ls"]}]});
    for (batch, want) in [(listed, "open"), (mixed, "notify")] {
        let r = rig(vec![run(batch), Scripted::text("Done.")], |c| {
            c.policy.allow_argv = vec![vec!["true".into()], vec!["printf".into()]];
        });
        let res = r.turn("run").await;
        let (posture, floor, reason) = r.gate(&res.session_id);
        assert_eq!(posture, want, "{reason}");
        assert!(!floor);
        if want == "notify" {
            assert!(reason.starts_with("step 2 of 2 (`ls`): "), "{reason}");
        }
    }
}

/// A step on the floor makes the whole batch wait as the floor does, its
/// reason naming the step; nothing runs before the answer.
#[tokio::test]
async fn a_step_on_the_floor_makes_the_batch_wait_as_the_floor() {
    let r = rig(
        vec![
            run(json!({"steps": [{"argv": ["touch", "first"]}, {"argv": ["op", "whoami"]}]})),
            Scripted::text("Done."),
        ],
        |c| c.policy.enforcement = Posture::Open,
    );
    let res = r.turn("run").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let (posture, floor, reason) = r.gate(&res.session_id);
    assert_eq!(posture, "approve");
    assert!(floor, "{reason}");
    assert!(
        reason.starts_with("step 2 of 2 (`op whoami`): "),
        "{reason}"
    );
    assert!(
        !r.root.join("first").exists(),
        "a step ran before the answer"
    );
}

/// The kernel's deadline covers every step's timeout.
#[test]
fn a_batchs_deadline_covers_every_steps_timeout() {
    let r = rig(vec![], |_| {});
    let tool = r.core.tools.registry.get("proc.run").unwrap().clone();
    let one = r
        .core
        .tools
        .deadline_ms(tool.as_ref(), &json!({"argv": ["true"], "timeout_secs": 7}));
    let batch = json!({"timeout_secs": 7, "steps": [{"argv": ["true"]}, {"argv": ["true"], "timeout_secs": 20}]});
    let both = r.core.tools.deadline_ms(tool.as_ref(), &batch);
    assert_eq!(one, (7 + 30) * 1000);
    assert_eq!(both, (7 + 20 + 30) * 1000);
}
