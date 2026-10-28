//! The persona as data (theseus-slvl.2), through the whole core: the built-in
//! default when no persona is configured, replaced whole by an owner's file,
//! the order session, place, default, built-in, a missing file compiling on
//! the default with its row and one notice, `persona.changed` once for a file
//! edit and once for a field edit, a task speaking as its parent, and sessions
//! of one persona sharing the cached blocks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use theseus_protocol::{PersonaListParams, SessionKind};

use crate::bus::EventSink;
use crate::config::persona::DEFAULT_TEXT;
use crate::config::PersonaConfig;
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::test_support::rows_100k as rows;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 271_828_182_845_904_523;

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
    dir: tempfile::TempDir,
    store: Store,
}

fn config(dir: &Path) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg
}

fn rig(tweak: impl FnOnce(&mut Config, &Path)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    tweak(&mut cfg, dir.path());
    let store = Store::open(&dir.path().join("store")).unwrap();
    let (core, model) = core_on(cfg, store.clone());
    Rig {
        core,
        model,
        dir,
        store,
    }
}

fn core_on(cfg: Config, store: Store) -> (Arc<Core>, Arc<Model>) {
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    (core, model)
}

/// A file with `text`, and its path as a config names it.
fn file(dir: &Path, name: &str, text: &str) -> String {
    let p: PathBuf = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().into_owned()
}

fn persona(files: &[&str]) -> PersonaConfig {
    PersonaConfig {
        files: files.iter().map(|f| f.to_string().into()).collect(),
        ..Default::default()
    }
}

/// Write a persona's short fields to its category, as `persona.set` does.
fn set_fields(core: &Core, v: &crate::config::PersonaView) {
    let key = v.key.as_deref().unwrap();
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let mut c = o
        .category(&theseus_ontology::CategoryId::new("persona", key).unwrap())
        .unwrap()
        .clone();
    let card = c.persona.as_mut().unwrap();
    card.card = v.card.clone();
    card.emoji = v.emoji.clone();
    core.runner
        .ontology
        .write(
            &core.store,
            vec![theseus_ontology::Record::Category(c)],
            theseus_ontology::Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
}

pub(crate) fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

pub(crate) async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
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
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
            surface: None,
        })
        .await
        .unwrap();
}

fn blocks(req: &ProviderRequest) -> Vec<String> {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str().map(str::to_string))
        .collect()
}

fn system(req: &ProviderRequest) -> String {
    blocks(req).join("\n")
}

/// Who the request's tail line says answers (two or more personas: every
/// persona's files are in the prefix, and the one in play is this line,
/// theseus-c1uz).
fn answering(req: &ProviderRequest) -> Option<String> {
    let last = req.messages.last()?;
    last["content"].as_array()?.iter().rev().find_map(|b| {
        b["text"]
            .as_str()?
            .strip_suffix(" answers this turn")
            .map(str::to_string)
    })
}

impl Rig {
    fn last(&self) -> ProviderRequest {
        self.model.requests.lock().unwrap().last().unwrap().clone()
    }
}

fn dm() -> String {
    format!("discord:dm:{OWNER}")
}

fn bind_dm(core: &Core, persona: Option<&str>) {
    core.bind_places(vec![BoundPlace {
        target: dm(),
        name: "DM @wren".into(),
        persona: persona.map(str::to_string),
        ..Default::default()
    }]);
}

/// An install with no persona has more than three sentences: the default
/// file, under a kilobyte, static in the header.
#[tokio::test]
async fn with_no_persona_the_header_carries_the_built_in_default() {
    let r = rig(|_, _| {});
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "hello").await;
    let blocks = blocks(&r.last());
    assert!(blocks[0].starts_with(DEFAULT_TEXT), "{}", blocks[0]);
    assert!(DEFAULT_TEXT.len() < 1024, "{} bytes", DEFAULT_TEXT.len());
    for want in [
        "Candour first",
        "Terminal:",
        "Spoken aloud:",
        "shared channel",
        "MCP",
    ] {
        assert!(DEFAULT_TEXT.contains(want), "{want}");
    }
    assert_eq!(crate::turn::PERSONA, DEFAULT_TEXT);
}

/// The built-in persona sizes a spoken reply to the moment, not to a sentence
/// count (theseus-dgbcx): the owner's line, and no "one to three" cap.
#[test]
fn the_built_in_persona_sizes_spoken_replies_to_the_moment() {
    let voice = DEFAULT_TEXT
        .lines()
        .find(|l| l.starts_with("Voice."))
        .expect("a Voice paragraph");
    assert!(
        voice.contains(
            "Spoken aloud: focused on what the moment needs, as long as the answer deserves; \
             plain sentences that sound right said out loud (no lists, tables or code), brief \
             for a quick back-and-forth."
        ),
        "{voice}"
    );
    assert!(!voice.contains("one to three short sentences"), "{voice}");
    for kept in [
        "Terminal: terse, code first.",
        "A direct message: conversational.",
        "A shared channel: neutral.",
        "MCP: plain.",
    ] {
        assert!(voice.contains(kept), "{kept}");
    }
}

/// The built-in persona acts, then tells (theseus-dfoug): one sentence ends its
/// `Values.` paragraph, and nothing else of the persona moved. With the sentence
/// taken off, the paragraph is the one before it byte for byte, and the first
/// paragraph and `Voice.` are the base's.
#[test]
fn the_built_in_persona_acts_then_tells_and_nothing_else_moved() {
    const SENTENCE: &str =
        " Act, then tell: anything reversible and within your permissions, do it and \
                            report; ask only for what is genuinely the owner's.";
    let paragraphs: Vec<&str> = DEFAULT_TEXT.trim_end().split("\n\n").collect();
    assert_eq!(paragraphs.len(), 3, "{paragraphs:?}");
    assert!(paragraphs[0].starts_with("You are Theseus, a durable agent:"));
    assert!(paragraphs[1].starts_with("Voice. Terminal: terse, code first."));
    assert!(paragraphs[2].starts_with("Values. "));
    assert!(paragraphs[2].ends_with(SENTENCE), "{}", paragraphs[2]);
    assert_eq!(
        paragraphs[2].strip_suffix(SENTENCE),
        Some(
            "Values. Candour first: take a clear position, give the evidence, say how sure you \
             are. No flattery. Keep a hypothesis apart from a conclusion. Lead with what you \
             found or did. A promise to act later is scheduled in the same turn."
        )
    );
    assert!(paragraphs[1].ends_with("A shared channel: neutral. MCP: plain."));
    assert_eq!(DEFAULT_TEXT.matches("Act, then tell").count(), 1);
}

/// An owner's persona file replaces the default whole: the header holds none
/// of it, the file's words are in the context block under their header.
#[tokio::test]
async fn an_owners_persona_file_replaces_the_default_whole() {
    let r = rig(|cfg, dir| {
        let f = file(dir, "pier.md", "You are Pier, the lighthouse keeper.");
        cfg.personas.insert("pier".into(), persona(&[&f]));
        cfg.context.default_persona = Some("pier".into());
    });
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "hello").await;
    let blocks = blocks(&r.last());
    assert!(
        !system(&r.last()).contains("Candour first"),
        "merged: {blocks:?}"
    );
    assert!(!blocks[0].contains("You are Theseus"), "{}", blocks[0]);
    assert!(
        blocks[0].starts_with("Context. The harness assembles"),
        "{}",
        blocks[0]
    );
    assert!(
        blocks
            .iter()
            .any(|b| b.contains("You are Pier, the lighthouse keeper.")),
        "{blocks:?}"
    );
}

/// Session, then place, then `default_persona`, then the built-in; a name the
/// config lacks is passed over to the next.
#[tokio::test]
async fn the_persona_is_the_sessions_then_the_places_then_the_default_then_the_built_in() {
    let r = rig(|cfg, dir| {
        for (k, text) in [
            ("alder", "ALDER words."),
            ("birch", "BIRCH words."),
            ("cedar", "CEDAR words."),
        ] {
            let f = file(dir, &format!("{k}.md"), text);
            cfg.personas.insert(k.into(), persona(&[&f]));
        }
        cfg.context.default_persona = Some("alder".into());
    });
    bind_dm(&r.core, Some("birch"));
    // With three personas every persona's files are in the request whoever
    // is in play, and the one in play is the tail line (theseus-c1uz).
    let everyone = |req: &ProviderRequest| {
        let s = system(req);
        ["ALDER words.", "BIRCH words.", "CEDAR words."]
            .iter()
            .all(|w| s.contains(w))
    };
    // No place, no choice: the default's. A cli session's persona is fixed, so its request carries that
    // persona's files alone and no tail line (theseus-nsp9l.1).
    let plain = session(&r.core, None);
    turn(&r.core, &plain, "one").await;
    assert_eq!(answering(&r.last()), None);
    let only = system(&r.last());
    assert!(only.contains("ALDER words.") && !only.contains("BIRCH words."));
    // The place's.
    let placed = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &placed, "two").await;
    assert_eq!(answering(&r.last()).as_deref(), Some("Birch"));
    assert!(everyone(&r.last()));
    // The session's own, over its place's.
    let own = session(&r.core, Some(&format!("dm:{OWNER}")));
    r.core.set_session_persona(&own, "cedar").unwrap();
    turn(&r.core, &own, "three").await;
    assert_eq!(answering(&r.last()).as_deref(), Some("Cedar"));
    assert!(everyone(&r.last()));
    // A place naming a persona the config lacks falls through to the default's.
    bind_dm(&r.core, Some("gone"));
    let lost = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &lost, "four").await;
    assert_eq!(answering(&r.last()).as_deref(), Some("Alder"));
    // The pure rule.
    let cfg = &r.core.cfg;
    assert_eq!(
        crate::personas::Book::of_config(cfg).persona_key_for(Some("cedar"), Some("birch")),
        Some("cedar")
    );
    assert_eq!(
        crate::personas::Book::of_config(cfg).persona_key_for(Some("nope"), Some("birch")),
        Some("birch")
    );
    assert_eq!(
        crate::personas::Book::of_config(cfg).persona_key_for(None, None),
        Some("alder")
    );
    assert_eq!(
        crate::personas::Book::of_config(cfg)
            .persona_for(None, Some("nope"))
            .name,
        "Alder"
    );
}

/// With no default and no choice the built-in is in play, and a view of it
/// has the neutral name.
#[test]
fn the_built_in_is_in_play_when_nothing_chooses() {
    let (cfg, _) = Config::parse("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n").unwrap();
    let v = crate::personas::Book::of_config(&cfg).persona_for(None, None);
    assert_eq!(
        (v.key, v.name.as_str(), v.initiative.as_str()),
        (None, "Theseus", "normal")
    );
}

/// A task speaks as its parent: a session with no choice of its own reads the
/// choice of the session that opened it.
#[tokio::test]
async fn a_task_speaks_as_its_parents_persona() {
    let r = rig(|cfg, dir| {
        let f = file(dir, "birch.md", "BIRCH words.");
        cfg.personas.insert("birch".into(), persona(&[&f]));
    });
    let parent = session(&r.core, None);
    r.core.set_session_persona(&parent, "birch").unwrap();
    let mut child = SessionRecord::new(SessionKind::Task, None);
    child.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_none".into(),
        by: "toolu_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&child.session_id, &child).unwrap();
    assert_eq!(r.core.runner.persona_of(&child).as_deref(), Some("birch"));
    let loose = SessionRecord::new(SessionKind::Conversation, None);
    assert_eq!(r.core.runner.persona_of(&loose), None);
}

/// `session.open { persona }` keeps the choice (it survives a restart: it is a
/// META record) and refuses a name the config lacks, naming the ones it has.
#[tokio::test]
async fn session_open_takes_a_persona_and_refuses_an_unknown_one() {
    let r = rig(|cfg, dir| {
        let f = file(dir, "birch.md", "BIRCH words.");
        cfg.personas.insert("birch".into(), persona(&[&f]));
    });
    let e = r
        .core
        .open_session(theseus_protocol::SessionOpenParams {
            persona: Some("birsh".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(format!("{e:#}").contains("the personas are birch"), "{e:#}");
    let rec = r
        .core
        .open_session(theseus_protocol::SessionOpenParams {
            persona: Some("birch".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(r.core.runner.persona_of(&rec).as_deref(), Some("birch"));
    let kept: Option<String> = r
        .core
        .store
        .get_meta(&format!(
            "{}{}",
            crate::persona::SESSION_PREFIX,
            rec.session_id
        ))
        .unwrap();
    assert_eq!(kept.as_deref(), Some("birch"));
    // A restart reads it back.
    let again = crate::persona::Board::default();
    assert_eq!(
        again.session(&r.store, &rec.session_id).as_deref(),
        Some("birch")
    );
    let listed = r.core.persona_list(PersonaListParams {
        session_id: Some(rec.session_id),
    });
    assert_eq!(listed.unwrap().in_play.as_deref(), Some("birch"));
}

/// A persona file edited between compiles is one `persona.changed` row at the
/// compile that first sees it, and none after; the first sighting says nothing.
#[tokio::test]
async fn a_file_edit_is_one_persona_changed_row() {
    let mut path = String::new();
    let r = rig(|cfg, dir| {
        path = file(dir, "pier.md", "First words.");
        cfg.personas.insert("pier".into(), persona(&[&path]));
        cfg.context.default_persona = Some("pier".into());
    });
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "one").await;
    assert!(
        rows(&r.core, "persona.changed").is_empty(),
        "a first sighting is no change"
    );
    // A later edit, past the filesystem's timestamp granularity.
    std::thread::sleep(std::time::Duration::from_millis(2100));
    std::fs::write(&path, "Second words, longer.").unwrap();
    turn(&r.core, &sid, "two").await;
    turn(&r.core, &sid, "three").await;
    let changed = rows(&r.core, "persona.changed");
    assert_eq!(changed.len(), 1, "{changed:?}");
    let c = &changed[0];
    assert_eq!(
        (c["persona"].as_str(), c["field"].as_str()),
        (Some("pier"), Some("file"))
    );
    assert_eq!(c["path"].as_str(), Some(path.as_str()));
    assert_ne!(c["old"], c["new"]);
    assert_eq!(
        c["old"].as_str(),
        Some(crate::persona::digest("First words.").as_str())
    );
}

/// A field edited in the config shows at the first compile after the daemon
/// starts onto it: one row for the field, and nothing for the unchanged.
#[tokio::test]
async fn a_field_edit_is_one_persona_changed_row() {
    let r = rig(|cfg, dir| {
        let f = file(dir, "pier.md", "Words.");
        let mut p = persona(&[&f]);
        p.card = Some("Keeps the light.".into());
        cfg.personas.insert("pier".into(), p);
        cfg.context.default_persona = Some("pier".into());
    });
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "one").await;
    assert!(rows(&r.core, "persona.changed").is_empty());
    // The owner edits the persona's fields (its category), and the daemon
    // starts again over the same store.
    let mut edited = r.core.personas().get("pier").unwrap().clone();
    edited.card = Some("Keeps the light, and the log.".into());
    edited.emoji = Some("🌊".into());
    set_fields(&r.core, &edited);
    let (core, _) = core_on(r.core.cfg.as_ref().clone(), r.store.clone());
    let sid = session(&core, None);
    turn(&core, &sid, "two").await;
    turn(&core, &sid, "three").await;
    let mut fields: Vec<String> = rows(&core, "persona.changed")
        .iter()
        .map(|c| c["field"].as_str().unwrap().to_string())
        .collect();
    fields.sort();
    assert_eq!(fields, ["card", "emoji"]);
}

/// A persona whose only file is missing compiles on the built-in default,
/// never thinner: the row names the persona, the owner gets one notice, health
/// warns, and a second turn repeats none of it.
#[tokio::test]
async fn a_missing_persona_file_compiles_on_the_default_with_one_notice() {
    let r = rig(|cfg, dir| {
        let gone = dir.join("gone.md").to_string_lossy().into_owned();
        cfg.personas.insert("pier".into(), persona(&[&gone]));
        cfg.context.default_persona = Some("pier".into());
    });
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "one").await;
    turn(&r.core, &sid, "two").await;
    let blocks = blocks(&r.last());
    assert!(blocks[0].starts_with(DEFAULT_TEXT), "{}", blocks[0]);
    let missing = rows(&r.core, "context.file_missing");
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert_eq!(missing[0]["persona"].as_str(), Some("pier"));
    let notices = r
        .core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .filter(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .is_some_and(|t| t.contains("cannot be read"))
        })
        .count();
    assert_eq!(notices, 1);
    let warnings = r.core.context_status().warnings;
    assert!(
        warnings.len() == 1 && warnings[0].contains("built-in default"),
        "{warnings:?}"
    );
    let _ = &r.dir;
}

/// With two or more personas every session shares the cached blocks byte for
/// byte whoever answers (theseus-c1uz): the header is the built-in default and
/// the persona layer holds every persona's files, so only the tail line
/// differs between a session of one persona and a session of another.
#[tokio::test]
async fn sessions_of_one_persona_share_the_cached_blocks() {
    let r = rig(|cfg, dir| {
        for (k, text) in [("alder", "ALDER words."), ("birch", "BIRCH words.")] {
            let f = file(dir, &format!("{k}.md"), text);
            cfg.personas.insert(k.into(), persona(&[&f]));
        }
        cfg.context.default_persona = Some("alder".into());
    });
    let mut got = Vec::new();
    let mut said = Vec::new();
    // DM sessions: where a message can engage another persona (theseus-nsp9l.1).
    for own in [None, None, Some("birch")] {
        let sid = session(&r.core, Some(&format!("dm:{OWNER}")));
        if let Some(p) = own {
            r.core.set_session_persona(&sid, p).unwrap();
        }
        turn(&r.core, &sid, "hello").await;
        got.push(blocks(&r.last()));
        said.push(answering(&r.last()));
    }
    assert_eq!(
        said,
        [
            Some("Alder".into()),
            Some("Alder".into()),
            Some("Birch".into())
        ],
        "only the tail line tells them apart"
    );
    assert_eq!(
        got[0][..2],
        got[1][..2],
        "two alder sessions share both blocks"
    );
    assert_eq!(
        got[0][0], got[2][0],
        "the header is the same whoever answers"
    );
    assert_eq!(
        got[0][1], got[2][1],
        "the persona layer holds every persona's files, so it is the same too"
    );
    assert!(got[0][1].contains("ALDER words.") && got[0][1].contains("BIRCH words."));
}
