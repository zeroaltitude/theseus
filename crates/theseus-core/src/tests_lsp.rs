//! The language-server board and its tools (L2, theseus-n88g.8), against
//! theseus-lsp's fake server served in this process: the read tools, the
//! lazy start (no server before the first call, one per root), the idle stop
//! on tokio's paused clock and the start again, the first start judged at
//! `proc.run`'s posture and not again, the place rule, the rename's gate, a
//! cancel's `$/cancelRequest`, and the requests' spans and metric.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_lsp::fake::{Config as FakeConfig, Fake};
use theseus_tools::{Tool, ToolCtx};

use crate::ledger::LedgerRow;
use crate::lsp::{Board, Spawn, Spawned};
use crate::policy::Posture;
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};

/// The fake, served in this process over pipes: each spawn counted, with
/// the root it was started on.
#[derive(Default)]
struct InProcess {
    spawns: Mutex<Vec<PathBuf>>,
    cfg: Mutex<Option<FakeConfig>>,
}

impl InProcess {
    fn count(&self) -> usize {
        self.spawns.lock().unwrap().len()
    }
}

impl Spawn for InProcess {
    fn spawn(
        &self,
        _argv: &[String],
        cwd: &Path,
        _env: &[(String, String)],
        _log: &Path,
    ) -> std::io::Result<Spawned> {
        self.spawns.lock().unwrap().push(cwd.to_path_buf());
        let (client, server) = tokio::io::duplex(1 << 20);
        let (sr, sw) = tokio::io::split(server);
        let cfg = self.cfg.lock().unwrap().clone().unwrap_or_default();
        tokio::spawn(Fake::serve(cfg, sr, sw));
        let (cr, cw) = tokio::io::split(client);
        Ok(Spawned {
            server: theseus_lsp::Server::pipes(cr, cw),
            pid: None,
            term: None,
        })
    }

    fn installed(&self, _program: &str, _path: Option<&str>) -> bool {
        true
    }
}

/// `[lsp]` on, with the fake as the server of `.fake` files, rooted at a
/// `fake.toml`.
fn lsp_config(cfg: &mut Config, idle_mins: f64) {
    cfg.lsp = toml::from_str(&format!(
        "enabled = true\nidle_stop_mins = {idle_mins}\n[servers.fake]\ncommand = [\"theseus-lsp-fake\"]\nextensions = [\"fake\"]\nroots = [\"fake.toml\"]\n"
    ))
    .unwrap();
}

/// A project: `a.fake` defines `total`, which `b.fake` uses; `a.fake`'s
/// fourth line holds a planted error. `sub/` is a project of its own.
fn project(dir: &Path) -> PathBuf {
    let work = dir.join("work");
    std::fs::create_dir_all(work.join("sub")).unwrap();
    std::fs::write(work.join("fake.toml"), "").unwrap();
    std::fs::write(
        work.join("a.fake"),
        "def total\nlet x = total\nlet y = 2\nERROR here\n",
    )
    .unwrap();
    std::fs::write(work.join("b.fake"), "use total here\n").unwrap();
    std::fs::write(work.join("sub/fake.toml"), "").unwrap();
    std::fs::write(work.join("sub/c.fake"), "def other\n").unwrap();
    work.canonicalize().unwrap()
}

struct Rig {
    _dir: tempfile::TempDir,
    work: PathBuf,
    board: Arc<Board>,
    spawner: Arc<InProcess>,
    rows: Arc<Mutex<Vec<LedgerRow>>>,
}

fn rig(idle_mins: f64) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let work = project(dir.path());
    let mut cfg = Config::example();
    lsp_config(&mut cfg, idle_mins);
    let board = Board::new(&cfg.lsp, vec![work.clone()], vec![], dir.path());
    let spawner = Arc::new(InProcess::default());
    board.set_spawner(spawner.clone());
    let rows: Arc<Mutex<Vec<LedgerRow>>> = Arc::default();
    let r = rows.clone();
    board.set_ledger(Arc::new(move |row| r.lock().unwrap().push(row)));
    Rig {
        _dir: dir,
        work,
        board,
        spawner,
        rows,
    }
}

impl Rig {
    fn tool(&self, name: &str) -> Arc<dyn Tool> {
        self.board
            .tools()
            .into_iter()
            .find(|t| t.name() == name)
            .unwrap()
    }

    fn ctx(&self) -> ToolCtx {
        ToolCtx::for_tests(&self.work)
    }

    /// One call, planned and run as the runtime runs an async tool.
    async fn call(&self, name: &str, input: Value) -> Result<String, String> {
        let t = self.tool(name);
        t.plan(&input, &self.ctx())?;
        t.run_async(&input, &self.ctx())
            .await
            .map(|(o, _)| o.text)
            .map_err(|f| f.message)
    }

    fn kinds(&self) -> Vec<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.kind.clone())
            .collect()
    }
}

/// Definition, references, hover, an outline, workspace symbols, and
/// diagnostics, each through its tool; no server before the first call,
/// one per root after, and its facts and health.
#[tokio::test]
async fn the_read_tools_answer_and_a_server_starts_at_its_roots_first_call() {
    let r = rig(10.0);
    assert_eq!(r.spawner.count(), 0, "no server before the first call");
    assert!(r.board.health().is_empty());
    let a = r.work.join("a.fake");
    let at = |line: u32| json!({"path": a, "line": line, "symbol": "total"});

    let def = r.call("lsp.definition", at(2)).await.unwrap();
    assert!(def.starts_with(&format!("{}:1:5", a.display())), "{def}");
    assert!(
        def.contains(">    1  def total") && def.contains("     2  let x = total"),
        "{def}"
    );
    let refs = r.call("lsp.references", at(1)).await.unwrap();
    assert!(
        refs.starts_with("3 references to `total` in 2 files:"),
        "{refs}"
    );
    assert!(
        refs.contains("b.fake (1)") && refs.contains("      1: use total here"),
        "{refs}"
    );
    let hover = r.call("lsp.hover", at(2)).await.unwrap();
    assert_eq!(hover, "`total`: a fake symbol");
    let outline = r.call("lsp.symbols", json!({"path": a})).await.unwrap();
    assert!(outline.contains("function total — line 1"), "{outline}");
    let found = r
        .call("lsp.symbols", json!({"query": "tot"}))
        .await
        .unwrap();
    assert!(
        found.contains("function total — ") && found.contains("a.fake:1"),
        "{found}"
    );
    let diags = r.call("lsp.diagnostics", json!({"path": a})).await.unwrap();
    assert!(
        diags.contains(":4:1 error [F1]: planted error (fake)"),
        "{diags}"
    );
    let open = r.call("lsp.diagnostics", json!({})).await.unwrap();
    assert!(open.contains("planted error"), "{open}");
    // The symbol's addressing is checked before any server is asked.
    let e = r
        .call("lsp.hover", json!({"path": a, "line": 2, "symbol": "nope"}))
        .await
        .unwrap_err();
    assert!(e.contains("`nope` is not on line 2"), "{e}");

    assert_eq!(r.spawner.count(), 1, "one server for the root");
    let c = r.work.join("sub/c.fake");
    r.call(
        "lsp.hover",
        json!({"path": c, "line": 1, "symbol": "other"}),
    )
    .await
    .unwrap();
    r.call("lsp.hover", at(1)).await.unwrap();
    assert_eq!(
        *r.spawner.spawns.lock().unwrap(),
        [r.work.clone(), r.work.join("sub")],
        "one server per root, each started once"
    );
    let h = r.board.health();
    assert_eq!(h.len(), 2, "{h:?}");
    assert!(
        h.iter().all(|s| s.server == "fake" && s.state == "ready"),
        "{h:?}"
    );
    assert_eq!(h[0].requests, 8, "{h:?}");
    assert_eq!(
        r.kinds(),
        ["lsp.started", "lsp.ready", "lsp.started", "lsp.ready"]
    );
    // A file no server serves is refused at its plan, with what is served.
    let e = r
        .tool("lsp.hover")
        .plan(
            &json!({"path": "notes.md", "line": 1, "symbol": "x"}),
            &r.ctx(),
        )
        .unwrap_err();
    assert!(e.contains("no language server serves .md files"), "{e}");
}

/// The root a file's server is started on: the nearest marker, inside the
/// workspace root; for Rust, the nearest `Cargo.toml` with `[workspace]`,
/// else the topmost.
#[test]
fn a_files_root_is_its_nearest_marker_and_cargos_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().canonicalize().unwrap();
    for d in ["ws/crates/one/src", "solo/inner/src", "py/pkg"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(work.join("ws/Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(work.join("ws/crates/one/Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(work.join("solo/Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(work.join("solo/inner/Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(work.join("py/pyproject.toml"), "").unwrap();
    std::fs::write(work.join("py/pkg/setup.py"), "").unwrap();
    let mut cfg = Config::example();
    cfg.lsp.enabled = true;
    let board = Board::new(&cfg.lsp, vec![work.clone()], vec![], &work);
    let specs = crate::lsp::Spec::all(&cfg.lsp);
    let spec = |n: &str| specs.iter().find(|s| s.name == n).unwrap().clone();
    let ra = spec("rust-analyzer");
    assert_eq!(
        board.root_for(&ra, &work.join("ws/crates/one/src/lib.rs")),
        work.join("ws")
    );
    assert_eq!(
        board.root_for(&ra, &work.join("solo/inner/src/lib.rs")),
        work.join("solo")
    );
    let py = spec("pyright");
    assert_eq!(
        board.root_for(&py, &work.join("py/pkg/m.py")),
        work.join("py/pkg")
    );
    assert_eq!(board.root_for(&py, &work.join("loose.py")), work);
    // Outside every root: the file's own directory, never above it.
    assert_eq!(
        board.root_for(&py, Path::new("/elsewhere/x.py")),
        Path::new("/elsewhere")
    );
}

/// A server unused for `idle_stop_mins` stops (`lsp.stopped`, `idle`) on
/// tokio's paused clock, not a minute early, and the next call starts it
/// again.
#[tokio::test]
async fn an_idle_server_stops_and_the_next_call_starts_it_again() {
    let r = rig(10.0);
    let at = json!({"path": r.work.join("a.fake"), "line": 1, "symbol": "total"});
    r.call("lsp.hover", at.clone()).await.unwrap();
    assert_eq!(r.board.up().len(), 1);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(9 * 60)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(r.board.up().len(), 1, "up after nine minutes");
    tokio::time::advance(Duration::from_secs(61)).await;
    for _ in 0..50 {
        if r.board.up().is_empty() && r.kinds().contains(&"lsp.stopped".to_string()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(r.board.up().is_empty(), "stopped after ten minutes idle");
    let stopped = r
        .rows
        .lock()
        .unwrap()
        .iter()
        .find(|row| row.kind == "lsp.stopped")
        .map(|row| row.data.clone())
        .unwrap();
    assert_eq!(stopped["why"], "idle", "{stopped}");
    tokio::time::resume();
    r.call("lsp.hover", at).await.unwrap();
    assert_eq!(r.spawner.count(), 2, "started again by the next call");
    assert_eq!(r.board.up().len(), 1);
}

/// A server that crashes is `lsp.failed`, and the next call starts it.
#[tokio::test]
async fn a_crashed_server_fails_and_the_next_call_starts_it_again() {
    let r = rig(10.0);
    *r.spawner.cfg.lock().unwrap() = Some(FakeConfig {
        crash_after: Some(1),
        ..FakeConfig::default()
    });
    let at = json!({"path": r.work.join("a.fake"), "line": 1, "symbol": "total"});
    let e = r.call("lsp.hover", at.clone()).await.unwrap_err();
    assert!(
        e.contains("ended") && e.contains("next call starts it again"),
        "{e}"
    );
    assert!(
        r.kinds().contains(&"lsp.failed".to_string()),
        "{:?}",
        r.kinds()
    );
    *r.spawner.cfg.lock().unwrap() = None;
    r.call("lsp.hover", at).await.unwrap();
    assert_eq!(r.spawner.count(), 2);
}

/// A cancelled call (its task aborted, as a cancel and `/stop` abort an
/// async call) sends `$/cancelRequest`, and the server stays up.
#[tokio::test]
async fn a_cancel_sends_cancel_request_and_the_server_stays_up() {
    let r = rig(10.0);
    *r.spawner.cfg.lock().unwrap() = Some(FakeConfig {
        slow_ms: 60_000,
        ..FakeConfig::default()
    });
    let input = json!({"path": r.work.join("a.fake"), "line": 1, "symbol": "total"});
    let t = r.tool("lsp.references");
    let task = tokio::spawn(t.run_async(&input, &r.ctx()));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while r
        .board
        .up()
        .first()
        .is_none_or(|l| l.client.in_flight() == 0)
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the request never went out"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let live = r.board.up().pop().expect("still up");
    assert_eq!(live.client.cancels_sent(), 1);
    let seen = live.client.request("fake/seen", Value::Null).await.unwrap();
    assert_eq!(seen["cancels"].as_array().unwrap().len(), 1, "{seen}");
    assert_eq!(r.spawner.count(), 1);
}

/// The requests a call's task made are its spans, `lsp.request` with
/// `lsp.server` and `lsp.method`, taken once.
#[tokio::test]
async fn a_calls_requests_are_its_spans() {
    let r = rig(10.0);
    let input = json!({"path": r.work.join("a.fake"), "line": 1, "symbol": "total"});
    let task = tokio::spawn(r.tool("lsp.definition").run_async(&input, &r.ctx()));
    let id = task.id();
    task.await.unwrap().unwrap();
    r.board.bind(id, "tu_1");
    let origin = std::time::Instant::now() - Duration::from_secs(1);
    let spans = r.board.spans("tu_1", |i| {
        i.saturating_duration_since(origin).as_micros() as u64
    });
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!(spans[0].name, "lsp.request");
    assert_eq!(spans[0].attrs["lsp.server"], "fake");
    assert_eq!(spans[0].attrs["lsp.method"], "textDocument/definition");
    assert_eq!(spans[0].attrs["outcome"], "ok");
    assert!(r.board.spans("tu_1", |_| 0).is_empty(), "taken once");
}

/// A core with `[lsp]` on over the fake, and `tweak` on its config.
fn core_in(
    dir: &Path,
    tweak: impl FnOnce(&mut Config, &Path),
) -> (Arc<Core>, PathBuf, Arc<InProcess>) {
    let work = project(dir);
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    lsp_config(&mut cfg, 10.0);
    tweak(&mut cfg, &work);
    let store = Store::open(&dir.join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap();
    let spawner = Arc::new(InProcess::default());
    core.tools
        .lsp
        .as_ref()
        .unwrap()
        .set_spawner(spawner.clone());
    (core, work, spawner)
}

/// The gate's decision for a call, as `toolrun`'s gate makes it for an L0
/// call: the call's own order, then the language server's step.
fn decided(core: &Core, name: &str, input: &Value) -> crate::policy::Decision {
    let rt = &core.tools;
    let tool = rt.registry.get(name).unwrap().clone();
    let plan = tool.plan(input, &rt.ctx).unwrap();
    let d = rt.policy.decide_with(tool.as_ref(), &plan, None);
    crate::lsp::gate(rt, tool.as_ref(), &plan, d)
}

/// The first call for a root is judged at `proc.run`'s posture for the
/// server's argv (here `approve`, past the read's own `open`), naming it;
/// once the server has started there, the call takes its own posture.
#[tokio::test]
async fn the_first_start_is_judged_as_proc_run_and_not_again_for_that_root() {
    let dir = tempfile::tempdir().unwrap();
    let (core, work, _) = core_in(dir.path(), |cfg, _| {
        cfg.policy.tools.insert("lsp.hover".into(), Posture::Open);
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
    });
    let input = json!({"path": work.join("a.fake"), "line": 1, "symbol": "total"});
    let d = decided(&core, "lsp.hover", &input);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(
        d.reason.contains("starts fake on") && d.reason.contains("`theseus-lsp-fake`"),
        "{}",
        d.reason
    );
    let tool = core.tools.registry.get("lsp.hover").unwrap().clone();
    tool.run_async(&input, &core.tools.ctx).await.unwrap();
    let d = decided(&core, "lsp.hover", &input);
    assert_eq!(
        d.posture,
        Posture::Open,
        "not again for that root: {}",
        d.reason
    );
    // Another root is another first start.
    let other = json!({"path": work.join("sub/c.fake"), "line": 1, "symbol": "other"});
    assert_eq!(
        decided(&core, "lsp.hover", &other).posture,
        Posture::Approve
    );
}

/// Under the template's postures (proc.run inheriting `notify`), the first
/// start is a notice that names the server and its command.
#[tokio::test]
async fn the_first_start_under_the_template_is_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let (core, work, _) = core_in(dir.path(), |cfg, _| {
        cfg.policy
            .tools
            .insert("lsp.definition".into(), Posture::Open);
    });
    let input = json!({"path": work.join("a.fake"), "line": 1, "symbol": "total"});
    let d = decided(&core, "lsp.definition", &input);
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    let n = d.notify.expect("a notice");
    assert!(
        n.rule.contains("starts fake on") && n.setting.contains("enforcement"),
        "{n:?}"
    );
}

/// The place rule: a language server reads the whole project, so no
/// `lsp.*` tool is offered in a shared place, even with public paths, and
/// the gate refuses one; a private place is offered them all.
#[tokio::test]
async fn a_shared_place_is_offered_no_lsp_tool() {
    let dir = tempfile::tempdir().unwrap();
    let (core, work, _) = core_in(dir.path(), |cfg, work| {
        cfg.places.public_paths = vec![work.to_string_lossy().into_owned()];
    });
    use crate::places::PlaceClass;
    let names = |c: PlaceClass| -> Vec<String> {
        core.tools
            .definitions_for(c)
            .iter()
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect()
    };
    let private = names(PlaceClass::Private);
    for n in crate::lsp::NAMES {
        assert!(private.contains(&theseus_tools::wire_name(n)), "{n}");
    }
    let shared = names(PlaceClass::Shared);
    assert!(
        shared.contains(&"fs_read".to_string()),
        "public paths offer the file tools"
    );
    assert!(!shared.iter().any(|n| n.starts_with("lsp_")), "{shared:?}");
    let plan = core
        .tools
        .registry
        .get("lsp.hover")
        .unwrap()
        .plan(
            &json!({"path": work.join("a.fake"), "line": 1, "symbol": "total"}),
            &core.tools.ctx,
        )
        .unwrap();
    let why = crate::places::refusal(
        PlaceClass::Shared,
        "lsp.hover",
        &plan,
        &core.tools.public_roots,
    );
    assert!(why.is_some_and(|w| w.contains("not offered in a shared place")));
    assert!(!core
        .tools
        .system_note_for(PlaceClass::Shared)
        .contains("lsp"));
}

/// A rename: its plan shows the diff and keeps it by digest, writing
/// nothing; its apply is gated as a write of every file it names, so one on
/// the approve list waits, and one outside the roots waits; applied, it
/// writes exactly the plan's edit, and refuses a file changed since.
#[tokio::test]
async fn a_rename_is_gated_on_every_file_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (core, work, _) = core_in(dir.path(), |cfg, work| {
        cfg.tools.approve_paths = vec![work.join("b.fake").to_string_lossy().into_owned()];
    });
    let rt = &core.tools;
    let plan_tool = rt.registry.get("lsp.rename.plan").unwrap().clone();
    let input =
        json!({"path": work.join("a.fake"), "line": 2, "symbol": "total", "new_name": "sum"});
    let out = plan_tool.run_async(&input, &rt.ctx).await.unwrap().0;
    assert!(
        out.text
            .contains("Rename `total` to `sum` (fake): 3 edits in 2 files."),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("-use total here\n+use sum here"),
        "{}",
        out.text
    );
    assert_eq!(
        std::fs::read_to_string(work.join("b.fake")).unwrap(),
        "use total here\n",
        "the plan writes nothing"
    );
    let digest = out.meta["digest"].as_str().unwrap().to_string();
    let apply = json!({"digest": digest});
    let d = decided(&core, "lsp.rename", &apply);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(d.reason.contains("b.fake is protected"), "{}", d.reason);
    let plan = rt
        .registry
        .get("lsp.rename")
        .unwrap()
        .plan(&apply, &rt.ctx)
        .unwrap();
    assert!(
        plan.summary.contains("write ")
            && plan.summary.contains("a.fake")
            && plan.summary.contains("b.fake")
    );
    assert!(plan
        .resources
        .iter()
        .all(|r| r.access == theseus_tools::Access::Write));

    // Outside the roots: the gate's step waits, whatever the posture.
    let outside = theseus_tools::Plan {
        resources: vec![theseus_tools::Resource {
            path: PathBuf::from("/elsewhere/x.fake"),
            access: theseus_tools::Access::Write,
        }],
        summary: "rename".into(),
        ..Default::default()
    };
    let tool = rt.registry.get("lsp.rename").unwrap().clone();
    let open = crate::policy::Decision {
        posture: Posture::Open,
        reason: "open".into(),
        notify: None,
        floor: false,
        granted: None,
        external: None,
    };
    let d = crate::lsp::gate(rt, tool.as_ref(), &outside, open);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(
        d.reason.contains("outside the workspace roots"),
        "{}",
        d.reason
    );

    // Applied (as the operator's approval would run it): exactly the edit.
    let wrote = tool.run_async(&apply, &rt.ctx).await.unwrap().0;
    assert!(
        wrote.text.starts_with("Renamed `total` to `sum`: wrote "),
        "{}",
        wrote.text
    );
    assert_eq!(
        std::fs::read_to_string(work.join("b.fake")).unwrap(),
        "use sum here\n"
    );
    assert!(std::fs::read_to_string(work.join("a.fake"))
        .unwrap()
        .starts_with("def sum\nlet x = sum\n"));
    // A digest applies once, and a file changed since its plan is refused.
    let e = tool.plan(&apply, &rt.ctx).unwrap_err();
    assert!(e.contains("no rename plan"), "{e}");
    let input = json!({"path": work.join("a.fake"), "line": 1, "symbol": "sum", "new_name": "s2"});
    let out = plan_tool.run_async(&input, &rt.ctx).await.unwrap().0;
    std::fs::write(work.join("b.fake"), "use sum here, changed\n").unwrap();
    let apply = json!({"digest": out.meta["digest"]});
    let e = tool.run_async(&apply, &rt.ctx).await.unwrap_err().message;
    assert!(
        e.contains("changed since plan") && e.contains("nothing was written"),
        "{e}"
    );
    assert!(std::fs::read_to_string(work.join("a.fake"))
        .unwrap()
        .starts_with("def sum\n"));
}

/// Without a posture of its own and with nothing protected, an applied
/// rename takes the inherited posture (here `notify`), whose notice names
/// every file it writes.
#[tokio::test]
async fn a_renames_notice_names_every_file_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (core, work, _) = core_in(dir.path(), |_, _| {});
    let rt = &core.tools;
    let input =
        json!({"path": work.join("a.fake"), "line": 1, "symbol": "total", "new_name": "sum"});
    let out = rt
        .registry
        .get("lsp.rename.plan")
        .unwrap()
        .run_async(&input, &rt.ctx)
        .await
        .unwrap()
        .0;
    let d = decided(&core, "lsp.rename", &json!({"digest": out.meta["digest"]}));
    assert_eq!(d.posture, Posture::Notify, "{}", d.reason);
    let plan = rt
        .registry
        .get("lsp.rename")
        .unwrap()
        .plan(&json!({"digest": out.meta["digest"]}), &rt.ctx)
        .unwrap();
    for f in ["a.fake", "b.fake"] {
        assert!(
            plan.summary.contains(&work.join(f).display().to_string()),
            "{}",
            plan.summary
        );
    }
}

/// `[lsp]` off (the default): no board, no `lsp.*` tool, and no health block.
#[tokio::test]
async fn lsp_off_is_no_board_and_no_tools() {
    let dir = tempfile::tempdir().unwrap();
    let (core, _, _) = core_in(dir.path(), |cfg, _| cfg.lsp.enabled = true);
    assert!(core.health().lsp.is_some_and(|l| l.is_empty()));
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap();
    assert!(core.tools.lsp.is_none() && core.health().lsp.is_none());
    assert!(core.tools.registry.all().all(|t| t.family() != "lsp"));
}
