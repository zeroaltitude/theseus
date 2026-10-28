//! Out of reach (theseus-pw1q.5): no tool call, job's shell, extension or
//! link can open the sealed dir. Each is the gate's or the L1 view's own
//! refusal, read off a real core built with `[self.sealed] dir` set.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use theseus_tools::Tool;

use crate::ceiling::PlaceView;
use crate::places::PlaceClass;
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::toolrun::order::At;
use crate::toolrun::Turned;
use crate::{Config, Core};

const CANARY: &str = "zebra-protocol-7f3a9c-canary";

struct Rig {
    core: Arc<Core>,
    work: PathBuf,
    sealed: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (state, work, sealed) = (base.join("state"), base.join("work"), base.join("sealed"));
    for d in [&state, &work, &sealed] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.self_improve.sealed.dir = Some(sealed.to_string_lossy().into_owned());
    cfg.validate().unwrap();
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    // One real item, with the canary in its task.
    let item = json!({"v": 1, "task": format!("Recite {CANARY}"), "check": "reply has \"x\""});
    core.sealed_put("core", "fact", item.to_string().as_bytes(), "reviewer-a")
        .unwrap();
    Rig {
        core,
        work,
        sealed,
        _dir: dir,
    }
}

fn rows_of(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(2000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

/// The gate's judgment of `input`: Ok(posture) when it would be let through
/// to be decided, Err(why) when it is refused outright.
fn judge(core: &Core, tool: &dyn Tool, input: &Value) -> Result<String, String> {
    let rt = &core.tools;
    // A plan that cannot be made is an invalid call: it never runs.
    let plan = tool
        .plan(input, &rt.ctx)
        .map_err(|e| format!("invalid: {e}"))?;
    let at = At {
        place: PlaceView::from(PlaceClass::Private),
        labels: &|| None,
        mcp: &|| None,
        glide: None,
        session: None,
        ctx: &rt.ctx,
        routine: None,
    };
    match rt.judge(&at, tool, &plan, input, &mut |_, _| {}) {
        Ok((d, _)) => Ok(d.posture.as_str().into()),
        Err(Turned::Place(why) | Turned::Invalid(why)) => Err(why),
    }
}

/// An input that names `path` in every string the schema offers, and a value
/// for what it requires.
fn naming(tool: &dyn Tool, path: &Path) -> Value {
    let schema = tool.input_schema();
    let required: Vec<String> = schema["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut input = Map::new();
    for (name, prop) in schema["properties"].as_object().into_iter().flatten() {
        let v = match prop["type"].as_str() {
            Some("array") => json!([path]),
            Some("integer" | "number") if required.contains(name) => json!(1),
            Some("boolean" | "integer" | "number") => continue,
            Some("object") => json!({"path": path}),
            _ => json!(path),
        };
        input.insert(name.clone(), v);
    }
    Value::Object(input)
}

#[test]
fn every_tool_is_refused_a_call_that_names_the_sealed_dir() {
    let r = rig();
    let inside = r.sealed.join("core").join("fact");
    let mut refused = 0;
    for tool in r.core.tools.registry.all() {
        let name = tool.name().to_string();
        for path in [r.sealed.clone(), inside.clone()] {
            let input = naming(tool.as_ref(), &path);
            match judge(&r.core, tool.as_ref(), &input) {
                Err(why) if why.starts_with("sealed:") => refused += 1,
                Err(why) => assert!(why.starts_with("invalid"), "{name}: {why}"),
                Ok(p) => panic!("{name} named the sealed dir and was judged {p}"),
            }
        }
    }
    assert!(
        refused >= 20,
        "the file, git, search, proc and term tools were all refused: {refused}"
    );
    let tool = r.core.tools.registry.get("fs.read").unwrap();
    let why = judge(
        &r.core,
        tool.as_ref(),
        &json!({"path": inside.join("x.item")}),
    )
    .unwrap_err();
    assert!(
        !why.contains(r.sealed.to_str().unwrap()),
        "the refusal names no path: {why}"
    );
}

#[test]
fn a_job_a_shell_string_and_a_terminals_keys_are_refused_too() {
    let r = rig();
    let s = r.sealed.to_string_lossy().into_owned();
    let proc = r.core.tools.registry.get("proc.run").unwrap();
    for input in [
        json!({"argv": ["cat", format!("{s}/core/fact/x.item")]}),
        json!({"argv": ["sh", "-c", format!("cat {s}/core/fact/*.item | head")]}),
        json!({"argv": ["ls"], "cwd": s}),
        json!({"steps": [{"argv": ["true"]}, {"argv": ["cat", format!("{s}/a")]}]}),
        json!({"argv": ["ls", "../sealed"], "cwd": r.work.to_string_lossy()}),
    ] {
        let why = judge(&r.core, proc.as_ref(), &input).unwrap_err();
        assert!(why.starts_with("sealed:"), "{input}: {why}");
    }
    // A terminal's keys are text: the input scan sees the path in them.
    let g = r.core.tools.sealed.clone();
    assert!(g
        .refusal_of_input(&json!({"keys": format!("cat {s}/core/fact/x.item\n")}))
        .is_some());
    assert!(g.refusal_of_input(&json!({"keys": "ls\n"})).is_none());
    // And a call that does not name it is judged as before.
    let fine = judge(
        &r.core,
        proc.as_ref(),
        &json!({"argv": ["ls"], "cwd": r.work.to_string_lossy()}),
    );
    assert!(fine.is_ok(), "{fine:?}");
}

#[test]
fn a_symlink_from_a_workspace_into_the_sealed_dir_is_refused() {
    let r = rig();
    let link = r.work.join("exams");
    std::os::unix::fs::symlink(&r.sealed, &link).unwrap();
    let file = std::fs::read_dir(r.sealed.join("core").join("fact"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let via = link
        .join("core")
        .join("fact")
        .join(file.file_name().unwrap());
    for (tool, input) in [
        ("fs.read", json!({"path": via})),
        ("fs.list", json!({"path": link})),
        ("fs.grep", json!({"pattern": "Recite", "path": link})),
        ("fs.glob", json!({"pattern": "**/*.item", "path": link})),
        (
            "fs.write",
            json!({"path": link.join("new.item"), "content": "x"}),
        ),
    ] {
        let t = r.core.tools.registry.get(tool).unwrap();
        let why = judge(&r.core, t.as_ref(), &input).unwrap_err();
        assert!(why.starts_with("sealed:"), "{tool}: {why}");
    }
    // A search from the workspace does not walk through the link either.
    let grep = r.core.tools.registry.get("fs.grep").unwrap();
    let out = grep
        .run(
            &json!({"pattern": "Recite", "path": r.work}),
            &r.core.tools.ctx,
        )
        .map(|o| o.text)
        .unwrap_or_else(|f| f.message);
    assert!(
        !out.contains(".item") && !out.contains("core/fact"),
        "{out}"
    );
}

#[test]
fn no_l1_view_shows_the_sealed_dir_to_a_job_an_mcp_server_or_an_extension() {
    let r = rig();
    // A job's view, an MCP server's (`mcp/l1.rs`) and a proposed extension's
    // are all `Sandbox::job_view`, whose hidden paths cover whatever binds.
    let hidden = r.core.tools.sandbox.job_view().hidden;
    assert!(hidden.contains(&r.sealed), "{hidden:?}");
    assert!(
        r.core.tools.policy.floor_paths.contains(&r.sealed),
        "and it is the floor at L0 too"
    );
    // The sandbox's own check of a spec refuses a relative hidden path only;
    // the sealed path is absolute and canonical.
    assert!(r.sealed.is_absolute());
}

#[test]
fn a_core_without_a_sealed_dir_judges_exactly_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = base.join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(base.to_string_lossy().into_owned());
    std::fs::create_dir_all(base.join("state")).unwrap();
    let store = Store::open(&base.join("state").join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    assert!(core.tools.sealed.is_none() && core.sealed().is_none());
    assert!(core.sealed_state().is_none(), "health names no sealed set");
    let t = core.tools.registry.get("fs.read").unwrap();
    assert!(judge(&core, t.as_ref(), &json!({"path": base.join("a.txt")})).is_ok());
}

#[test]
fn the_exam_row_names_the_item_and_no_text_and_health_counts_only() {
    let r = rig();
    // The rig put one item; its row is in the ledger with the id alone.
    let item = json!({"v": 1, "task": format!("Another {CANARY}"), "check": "reply has \"y\""});
    let id = r
        .core
        .sealed_put(
            "core",
            "decision",
            item.to_string().as_bytes(),
            "reviewer-b",
        )
        .unwrap();
    let rows = rows_of(&r.core, "self.exam.counted");
    assert_eq!(rows.len(), 2, "{rows:?}");
    let last = rows.last().unwrap();
    assert_eq!(last["item"], id.as_str());
    assert_eq!(last["sealed"], true);
    assert_eq!(
        (last["family"].as_str(), last["reviewer"].as_str()),
        (Some("decision"), Some("reviewer-b"))
    );
    assert!(
        !rows.iter().any(|r| r.to_string().contains(CANARY)),
        "{rows:?}"
    );
    let state = r.core.self_state();
    let sealed = state.sealed.clone().expect("a sealed dir is set");
    assert_eq!(sealed.sets, [("core".to_string(), 2)].into_iter().collect());
    let wire = serde_json::to_string(&state).unwrap();
    assert!(
        !wire.contains(CANARY) && !wire.contains(id.as_str()),
        "{wire}"
    );
}
