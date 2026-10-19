//! The trim-tokens row (theseus-rwpbg.3): learned loads inside the place's budget, the tools note as
//! exceptions, the DM's reminders, and tool names in descriptions as the model calls them.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::first_request::{budget, FirstRequest};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

const OWNER_SHAPE: &str = include_str!("../fixtures/owner_shape.toml");

fn rig_of(toml: &str) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::parse(toml).unwrap().0;
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    parts.cfg.tools.defer.enabled = true;
    (Core::build(parts).unwrap(), dir)
}

/// A ledger in which `place` called each of `families` in every turn of the last fortnight.
fn seed(core: &Core, place: &str, families: &[&str]) {
    let at = theseus_protocol::now_unix_ms() - 3 * 86_400_000;
    for i in 0..20u64 {
        core.runner.tools.defer.apply_used(
            at + i,
            &json!({"place": place, "loaded": ["files", "commands"], "used": families,
                    "searched": [], "unused": []}),
        );
    }
}

fn first(core: &Core, place: &str) -> FirstRequest {
    core.first_request(place).unwrap()
}

/// The owner's cli with the tasks and aws learned (the census of 2026-10-17 read 13,665 tokens exact):
/// the first request stays inside its 8,000, and what was demoted is still one search away.
#[test]
fn the_owner_shaped_cli_with_learned_tasks_fits_8000() {
    let (core, _dir) = rig_of(OWNER_SHAPE);
    seed(&core, "cli", &["tasks", "aws"]);
    let req = first(&core, "cli");
    let max = budget("cli").unwrap();
    assert!(req.tokens <= max, "{} tokens over {max}", req.tokens);
    assert!(!req.loaded.contains(&"task_create".to_string()));
    assert!(!req.loaded.contains(&"aws_call".to_string()));
    // Deferred, not dropped: the request still declares them, one search away.
    let (live, _) = core.live_profile();
    let next = core.runner.census_request(None, "cli", &live).unwrap();
    for tool in ["task_create", "wake_at", "aws_call"] {
        let t = next
            .compiled
            .request
            .tools
            .iter()
            .find(|t| t["name"] == tool);
        assert_eq!(
            t.map(|t| &t["defer_loading"]),
            Some(&Value::Bool(true)),
            "{tool}"
        );
    }
}

/// A place with room keeps what it learned: the demotion is for the budget, not for the family.
#[test]
fn a_learned_family_that_fits_stays_loaded() {
    let (core, _dir) = rig_of(OWNER_SHAPE);
    seed(&core, "dm", &["aws"]);
    assert!(first(&core, "dm").loaded.contains(&"aws_call".to_string()));
}

/// The DM keeps `wake_at` and defers the task tools; a channel keeps the families of both.
#[test]
fn the_dm_default_keeps_wake_at_and_defers_tasks() {
    use crate::tools_defer::PlaceKind;
    let (core, _dir) = rig_of(OWNER_SHAPE);
    let dm = first(&core, "dm");
    for t in ["wake_at", "wake_move", "wake_cancel", "http_fetch"] {
        assert!(dm.loaded.contains(&t.to_string()), "{t}");
    }
    for t in [
        "task_create",
        "task_update",
        "task_close",
        "task_claim",
        "task_split",
    ] {
        assert!(!dm.loaded.contains(&t.to_string()), "{t}");
    }
    assert!(dm.tokens < 10_500, "the DM reads {} tokens", dm.tokens);
    let channel = PlaceKind::Channel.defaults();
    assert!(channel.contains(&"tasks") && channel.contains(&"reminders"));
    assert!(!PlaceKind::Cli.defaults().contains(&"reminders"));
    assert!(!PlaceKind::Task.defaults().contains(&"reminders"));
}

/// The note says the posture most tools share once and names the exceptions; each tool's posture is
/// still determined by it.
#[test]
fn the_tools_note_lists_exceptions() {
    use crate::places::PlaceClass;
    let (core, _dir) = rig_of(OWNER_SHAPE);
    let rt = &core.runner.tools;
    let view: crate::ceiling::PlaceView = PlaceClass::Private.into();
    let note = rt.system_note_for(view);
    let line = note
        .split("approval): ")
        .nth(1)
        .and_then(|r| r.split(".\n").next())
        .and_then(|r| r.split(". proc_run runs").next())
        .expect("a postures line");
    assert!(line.contains("every other tool: "), "{line}");
    let names: Vec<(String, crate::policy::Posture)> = rt
        .registry
        .all()
        .map(|t| t.name())
        .filter(|n| view.offered(n, &[]) && crate::tools_defer::listed(n))
        .map(|n| (theseus_tools::wire_name(n), rt.policy.posture(n).0))
        .collect();
    assert!(names.len() > 30);
    let back = crate::toolrun::note::read(line, names.iter().map(|(n, _)| n.as_str()));
    assert_eq!(back, names, "every tool's posture reads back from the note");
    // Only the exceptions are named: well under the whole registry.
    let said = line.split(", ").count();
    assert!(said < names.len() / 2, "{said} names of {}", names.len());
    assert!(!note.contains("Prefer fs_read"));
}

/// Every tool a model-visible description or the tools note names is a registered tool's wire name:
/// the model calls `wake_move`, and `wake.move` is no tool it has.
#[test]
fn descriptions_name_wire_tools() {
    use crate::places::PlaceClass;
    let (core, _dir) = rig_of(OWNER_SHAPE);
    let rt = &core.runner.tools;
    let view: crate::ceiling::PlaceView = PlaceClass::Private.into();
    let mut seen = rt.system_note_for(view);
    for d in &rt.definitions_for(view) {
        seen.push_str(&serde_json::to_string(&d).unwrap());
    }
    for t in rt.registry.all() {
        let dotted = t.name();
        if !dotted.contains('.') {
            continue;
        }
        let mut at = 0;
        while let Some(i) = seen[at..].find(dotted) {
            let (s, e) = (at + i, at + i + dotted.len());
            let before = seen[..s]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || "_./".contains(c));
            let after = seen[e..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            assert!(
                before || after,
                "a model-visible text names `{dotted}`; the tool's name is `{}`",
                theseus_tools::wire_name(dotted)
            );
            at = e;
        }
    }
}
