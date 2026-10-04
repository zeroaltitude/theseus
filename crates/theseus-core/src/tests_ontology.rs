//! The ontology wired in (row 26, step 21b; M4 design §2.8; theseus-8kk.1),
//! through the whole core: a topic's guidance in the system block under its
//! header; a membership change waits for the next recompile; a guidance
//! edit in play is one `system_changed`; given memberships refuse writes;
//! the place rule's admissions are the same with and without memberships;
//! an ontology write counts only from the owner in a private place; and the
//! snapshot a restart builds equals the one kept in memory.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_protocol::{
    DiscordOrigin, OntologyCategoryAddParams, OntologyGuidanceSetParams, OntologyListParams,
    OntologyMembershipSetParams, SessionKind,
};

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::compiler::{Compilation, Recompile};
use crate::context_files::{ContextEntry, ContextFileEntry, ContextReaders};
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The owner on Discord, and someone else.
const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
/// A guild channel the bindings file binds, shared.
const LAB: u64 = 314_159_265_358_979_323;

/// A model that answers every request "Hello." and keeps each request.
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
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
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let f = FakeProvider::scripted(vec![Scripted::text("Hello.")]);
            f.stream_message(req, on_delta).await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

/// A core whose context files are the owner's notes and a public README,
/// with the owner named and the shared channel and the owner's DM bound.
fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    std::fs::write(root.join("notes.md"), "the owner's notes\n").unwrap();
    std::fs::write(root.join("README.md"), "the open readme\n").unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = path(dir.path());
    cfg.tools.projects_dir = Some(path(&root));
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.context.files = vec![
        ContextEntry::Path(path(&root.join("notes.md"))),
        ContextEntry::Table(ContextFileEntry {
            path: path(&root.join("README.md")),
            readers: ContextReaders::Public,
        }),
    ];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @wren".into(),
            private: false,
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
        },
    ]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A session, posting to `place` (`channel:<id>`, `dm:<user>`) when given.
fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
}

impl Rig {
    fn last_request(&self) -> ProviderRequest {
        self.model.requests.lock().unwrap().last().unwrap().clone()
    }

    /// The session's current compilation.
    fn compilation(&self, sid: &str) -> Compilation {
        let rec = self
            .core
            .store
            .get_session::<SessionRecord>(sid)
            .unwrap()
            .unwrap();
        self.core
            .store
            .get_compilation(rec.compilation_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
    }

    /// The triggers of the session's compilations, oldest first.
    fn triggers(&self, sid: &str) -> Vec<String> {
        rows_of(&self.core, "context.recompiled", Some(sid))
            .iter()
            .map(|r| r["trigger"].as_str().unwrap().to_string())
            .collect()
    }

    fn topic(&self, name: &str, guidance: &str) {
        self.core
            .ontology_category_add(
                &OntologyCategoryAddParams {
                    name: name.into(),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
        self.guide(name, guidance);
    }

    fn guide(&self, category: &str, text: &str) {
        self.core
            .ontology_guidance_set(
                &OntologyGuidanceSetParams {
                    category: category.into(),
                    text: text.into(),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
    }

    fn member(&self, sid: &str, add: &[&str]) {
        self.core
            .ontology_membership_set(
                &OntologyMembershipSetParams {
                    session_id: sid.into(),
                    add: add.iter().map(|s| s.to_string()).collect(),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap();
    }
}

/// The ledger's rows of one kind (of one session, when given), oldest first.
fn rows_of(core: &Core, kind: &str, session: Option<&str>) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .filter(|(_, r)| session.is_none() || r.session_id.as_deref() == session)
        .map(|(_, r)| r.data)
        .collect()
}

fn system_text(req: &ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The owner, on Discord, in a DM or in `channel` of a guild.
fn on_discord(user: u64, channel: Option<u64>) -> Answerer {
    Answerer {
        label: format!("discord:{user}"),
        surface: Surface::Discord,
        discord: Some(DiscordOrigin {
            user_id: user.to_string(),
            channel_id: channel.unwrap_or(1).to_string(),
            guild_id: channel.map(|_| "27182818".to_string()),
        }),
    }
}

const RULES: &str = "Name the crate a change touches before anything else.";

/// The design's first test: a topic's guidance is in the system block,
/// after the context files, under its header, and the manifest records the
/// membership (kind, category, origin, as-of) and the guidance's digest.
#[tokio::test]
async fn a_topics_guidance_is_in_the_system_block_under_its_header() {
    let r = rig();
    r.topic("theseus", RULES);
    let sid = session(&r.core, None);
    r.member(&sid, &["theseus"]);
    turn(&r.core, &sid, "hi").await;

    let system = system_text(&r.last_request());
    let block = format!("# Guidance (topic theseus)\n\n{RULES}");
    assert!(system.contains(theseus_ontology::PREAMBLE), "{system}");
    assert!(system.contains(&block), "{system}");
    assert!(
        system.find("the owner's notes").unwrap() < system.find(&block).unwrap(),
        "the guidance follows the context files: {system}"
    );
    let m = r.compilation(&sid).manifest;
    assert_eq!(m.memberships.len(), 1, "{:?}", m.memberships);
    let used = &m.memberships[0];
    assert_eq!(
        (
            used.kind.as_str(),
            used.category.as_str(),
            used.origin.name()
        ),
        ("topic", "topic:theseus", "operator")
    );
    assert!(used.as_of_ms > 0);
    let g = r
        .core
        .runner
        .ontology
        .held()
        .unwrap()
        .guidance(&used.category)
        .cloned()
        .unwrap();
    assert_eq!(m.guidance.len(), 1);
    assert_eq!(
        (m.guidance[0].version, m.guidance[0].digest.as_str()),
        (1, g.digest.as_str())
    );
}

/// The design's second: a membership set while a session appends waits for
/// its next recompile, so its system block (and the prompt cache) stays as
/// it was; `theseus sessions recompile` applies it.
#[tokio::test]
async fn a_membership_change_waits_for_the_next_recompile() {
    let r = rig();
    r.topic("theseus", RULES);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "one").await;
    assert!(!system_text(&r.last_request()).contains(RULES));

    r.member(&sid, &["theseus"]);
    turn(&r.core, &sid, "two").await;
    assert!(
        !system_text(&r.last_request()).contains(RULES),
        "the change waits for a recompile"
    );
    assert_eq!(r.triggers(&sid), ["new_session"]);
    assert!(r.compilation(&sid).manifest.memberships.is_empty());

    r.core
        .request_recompile(&sid, Recompile::Transcript, "the CLI")
        .unwrap();
    turn(&r.core, &sid, "three").await;
    assert!(system_text(&r.last_request()).contains(RULES));
    assert_eq!(r.triggers(&sid), ["new_session", "manual_transcript"]);
    let m = r.compilation(&sid).manifest;
    assert_eq!(m.memberships.len(), 1);
    assert_eq!(m.memberships[0].category.as_str(), "topic:theseus");
}

/// The design's third: an edit of guidance a session carries is one
/// `system_changed` recompile at its next turn, and the turn after appends.
#[tokio::test]
async fn a_guidance_edit_in_play_forces_one_system_changed() {
    let r = rig();
    r.topic("theseus", RULES);
    let sid = session(&r.core, None);
    r.member(&sid, &["theseus"]);
    turn(&r.core, &sid, "one").await;

    let edited = "Name the crate, then the module.";
    r.guide("theseus", edited);
    turn(&r.core, &sid, "two").await;
    let system = system_text(&r.last_request());
    assert!(
        system.contains(edited) && !system.contains(RULES),
        "{system}"
    );
    turn(&r.core, &sid, "three").await;
    assert_eq!(r.triggers(&sid), ["new_session", "system_changed"]);
    let m = r.compilation(&sid).manifest;
    assert_eq!(m.guidance[0].version, 2);

    // Guidance the session does not carry changes nothing.
    r.topic("lamps", "Speak of lamps.");
    turn(&r.core, &sid, "four").await;
    assert_eq!(r.triggers(&sid), ["new_session", "system_changed"]);
}

/// The design's fourth: a given kind's memberships are the session's place,
/// and its categories the transport's; writing either is invalid input, and
/// nothing is written.
#[tokio::test]
async fn given_memberships_refuse_writes() {
    let r = rig();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    let e = r
        .core
        .ontology_membership_set(
            &OntologyMembershipSetParams {
                session_id: sid.clone(),
                add: vec![format!("channel:{LAB}")],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("session's place"), "{e}");
    let e = r
        .core
        .ontology_category_add(
            &OntologyCategoryAddParams {
                kind: Some("channel".into()),
                name: "elsewhere".into(),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("come from the transport"), "{e}");
    assert!(rows_of(&r.core, "ontology.membership", None).is_empty());
    // The place's own category was made at its first bind, by the transport,
    // and a second bind makes nothing more.
    let made = rows_of(&r.core, "ontology.category", None);
    assert_eq!(made.len(), 2, "{made:?}");
    assert!(made.iter().all(|c| c["origin"] == "transport"));
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
    }]);
    assert_eq!(rows_of(&r.core, "ontology.category", None).len(), 2);
    // And its session's given membership is read from the place.
    let list = r
        .core
        .ontology_list(&OntologyListParams {
            session_id: Some(sid),
        })
        .unwrap();
    assert_eq!(list.memberships.len(), 1);
    assert_eq!(
        (
            list.memberships[0].category.as_str(),
            list.memberships[0].origin.as_str()
        ),
        (format!("channel:{LAB}").as_str(), "transport")
    );
}

/// What the place rule admitted into a session's last request: its tools,
/// and its context files with what each class withheld.
fn admissions(r: &Rig, sid: &str) -> (Vec<String>, String) {
    let req = r.last_request();
    let tools = req
        .tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();
    let files = serde_json::to_string(&r.compilation(sid).manifest.context_files).unwrap();
    (tools, files)
}

/// The design's fifth, the guardrail: interpretations route context and
/// never grant access. The same session compiled with and without a
/// membership is admitted the same tools and context files, in a private
/// place and in a shared one; and a shared place takes no guidance from
/// what the operator assigned it, only from its own place.
#[tokio::test]
async fn admissions_are_identical_with_and_without_memberships() {
    let r = rig();
    r.topic("theseus", RULES);
    r.guide(&format!("channel:{LAB}"), "This is the lab's channel.");
    for place in [
        None,
        Some(format!("dm:{OWNER}")),
        Some(format!("channel:{LAB}")),
    ] {
        let sid = session(&r.core, place.as_deref());
        turn(&r.core, &sid, "one").await;
        let without = admissions(&r, &sid);
        r.member(&sid, &["theseus"]);
        r.core
            .request_recompile(&sid, Recompile::Transcript, "the CLI")
            .unwrap();
        turn(&r.core, &sid, "two").await;
        assert_eq!(admissions(&r, &sid), without, "{place:?}");
        let system = system_text(&r.last_request());
        let shared = place.as_deref() == Some(&format!("channel:{LAB}"));
        assert_eq!(system.contains(RULES), !shared, "{place:?}: {system}");
        assert_eq!(
            system.contains("This is the lab's channel."),
            shared,
            "{place:?}: {system}"
        );
    }
}

/// The design's sixth: an ontology write is the operator's act. It counts
/// from the CLI and from the owner's DM; from someone else, from a shared
/// place, or through a connection no listener named it is refused,
/// ledgered, and writes nothing. (The CLI refuses it inside a job before
/// sending: `client::refuse_in_a_job`.)
#[tokio::test]
async fn an_ontology_write_counts_only_from_the_owner_in_a_private_place() {
    let r = rig();
    r.topic("theseus", RULES);
    let set = |who: Answerer| {
        r.core.ontology_guidance_set(
            &OntologyGuidanceSetParams {
                category: "theseus".into(),
                text: "Planted.".into(),
                ..Default::default()
            },
            who,
        )
    };
    let unnamed = Answerer {
        label: "sock#9".into(),
        surface: Surface::Unnamed,
        discord: None,
    };
    for who in [
        unnamed,
        on_discord(ALICE, None),
        on_discord(OWNER, Some(LAB)),
    ] {
        let e = set(who).unwrap_err();
        assert!(
            e.downcast_ref::<crate::approval::Refusal>().is_some(),
            "{e}"
        );
    }
    let refused = rows_of(&r.core, "approval.refused", None);
    assert_eq!(refused.len(), 3);
    assert!(refused
        .iter()
        .all(|row| row["act"] == "ontology.guidance.set"));
    let g = r.core.runner.ontology.held().unwrap();
    let id = theseus_ontology::CategoryId::parse("topic:theseus").unwrap();
    assert_eq!(g.guidance(&id).unwrap().text, RULES);
    assert_eq!(rows_of(&r.core, "ontology.guidance", None).len(), 1);

    let ok = set(on_discord(OWNER, None)).unwrap();
    assert_eq!((ok.version, ok.text.as_str()), (2, "Planted."));
}

/// The snapshot a restart builds, by one META prefix scan, equals the one
/// kept current in memory through every kind of write.
#[tokio::test]
async fn the_snapshot_rebuilt_after_a_restart_equals_the_one_kept_in_memory() {
    let r = rig();
    r.topic("theseus", RULES);
    r.core
        .ontology_category_add(
            &OntologyCategoryAddParams {
                name: "rust harness".into(),
                parent: Some("theseus".into()),
                description: Some("The daemon's own code.".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    r.guide("rust harness", "Run the gate before a commit.");
    r.guide("theseus", "Edited once.");
    let sid = session(&r.core, None);
    r.member(&sid, &["theseus", "topic:rust-harness"]);
    r.core
        .ontology_membership_set(
            &OntologyMembershipSetParams {
                session_id: sid.clone(),
                remove: vec!["theseus".into()],
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();

    let kept = r.core.runner.ontology.held().unwrap();
    let rebuilt = crate::ontology::load(&r.core.store).unwrap();
    assert_eq!(*kept, rebuilt);
    r.core.runner.ontology.forget();
    assert_eq!(
        *r.core.runner.ontology.snapshot(&r.core.store).unwrap(),
        *kept
    );
    assert_eq!(rebuilt.memberships(&sid).len(), 1);
    // Every change wrote its row.
    assert_eq!(rows_of(&r.core, "ontology.category", None).len(), 4);
    assert_eq!(rows_of(&r.core, "ontology.guidance", None).len(), 3);
    assert_eq!(rows_of(&r.core, "ontology.membership", Some(&sid)).len(), 2);
}
