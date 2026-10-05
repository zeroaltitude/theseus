//! The client against the real servers, on the fixtures (`fixtures/`): one
//! planted type error and one call across files per language. Ignored,
//! since the gate has no servers; each runs with one command:
//!
//! ```text
//! cargo nextest run -p theseus-lsp --run-ignored only --no-capture -E 'test(=live_ty)'
//! ```
//!
//! Each server's command is its preset's (`servers.rs`) found on `PATH`, or
//! the path in `THESEUS_LSP_<NAME>` (`THESEUS_LSP_TY`, `THESEUS_LSP_PYRIGHT`,
//! `THESEUS_LSP_BASEDPYRIGHT`, `THESEUS_LSP_TSGO`,
//! `THESEUS_LSP_TYPESCRIPT_LANGUAGE_SERVER`, `THESEUS_LSP_RUST_ANALYZER`).
//! typescript-language-server also needs `THESEUS_LSP_TSSERVER`, the path of
//! TypeScript 5's `tsserver.js`. A run prints the probe's row: the server's
//! version, the time from the spawn to `initialize`'s answer, from the open
//! to the planted error's diagnostic, a definition's latency, and the
//! process group's memory after them.
//!
//! `live_rust_analyzer_on_a_workspace` points rust-analyzer at
//! `THESEUS_LSP_WORKSPACE` (this repository, say) and reports its time to
//! quiescence and its memory.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use theseus_lsp::servers::{self, Preset};
use theseus_lsp::spawn;
use theseus_lsp::types::Location;
use theseus_lsp::{locate, Client, Freshness};

/// What one fixture holds, and what to ask of it.
struct Case {
    fixture: &'static str,
    /// The file with the planted error and the call.
    main: &'static str,
    /// The planted error's 1-based line.
    error_line: u32,
    /// The call's 1-based line, and the called name.
    call_line: u32,
    /// The file the call's definition is in.
    defined_in: &'static str,
}

const PYTHON: Case = Case {
    fixture: "python",
    main: "main.py",
    error_line: 3,
    call_line: 4,
    defined_in: "ledger.py",
};

const TYPESCRIPT: Case = Case {
    fixture: "typescript",
    main: "main.ts",
    error_line: 3,
    call_line: 4,
    defined_in: "ledger.ts",
};

const RUST: Case = Case {
    fixture: "rust",
    main: "src/lib.rs",
    error_line: 4,
    call_line: 5,
    defined_in: "src/ledger.rs",
};

/// A copy of a fixture in a scratch directory: servers write caches, and a
/// rename must never reach the repository.
fn scratch(fixture: &str) -> tempfile::TempDir {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture);
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&src, dir.path());
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn command(preset: &Preset) -> Vec<String> {
    let var = format!(
        "THESEUS_LSP_{}",
        preset.name.to_uppercase().replace('-', "_")
    );
    let mut argv = preset.argv.clone();
    if let Ok(path) = std::env::var(&var) {
        argv[0] = path;
    }
    argv
}

fn short(l: &Location, root: &Path) -> String {
    let p = theseus_lsp::uri::to_path(&l.uri).unwrap_or_default();
    let p = p
        .strip_prefix(root.canonicalize().unwrap())
        .or_else(|_| p.strip_prefix(root))
        .unwrap_or(&p);
    format!("{}:{}", p.display(), l.range.start.line + 1)
}

/// The probe's measures for one server.
#[derive(Debug)]
struct Row {
    server: String,
    version: String,
    initialize: Duration,
    first_diagnostics: Duration,
    definition: Duration,
    rss_mib: f64,
}

async fn probe(preset: Preset, case: &Case) -> Row {
    let dir = scratch(case.fixture);
    let root = dir.path().to_path_buf();
    let argv = command(&preset);
    let started = Instant::now();
    let (server, proc) = spawn::spawn(&argv, &root, &[], Stdio::null()).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (install it, or set its THESEUS_LSP_ variable)",
            argv[0]
        )
    });
    let (c, _events) = Client::start(server, preset.options(&root))
        .await
        .expect("initialize");
    let initialize = started.elapsed();
    let info = c.server_info();
    let version = info
        .as_ref()
        .and_then(|i| i.version.clone())
        .unwrap_or_else(|| "?".into());
    println!(
        "{}: {:?} answered initialize in {initialize:?}; pull diagnostics: {}",
        preset.name,
        info,
        c.capabilities().pull_diagnostics
    );

    let main = root.join(case.main);
    let text = std::fs::read_to_string(&main).unwrap();
    let opened = Instant::now();
    c.open(&main).await.unwrap();
    let first_diagnostics = wait_for_error(&c, &main, case.error_line).await;
    println!("  the planted error after {first_diagnostics:?}");

    let pos = locate(&text, case.call_line, "total", None).unwrap();
    let t = Instant::now();
    let defs = c.definition(&main, pos).await.expect("definition");
    let definition = t.elapsed();
    let shown: Vec<String> = defs.iter().map(|l| short(l, &root)).collect();
    println!("  definition of total in {definition:?}: {shown:?}");
    assert!(
        defs.iter().any(|l| l.uri.ends_with(case.defined_in)),
        "the definition is in {}: {shown:?}",
        case.defined_in
    );
    navigate(&c, &main, pos, &root, case).await;
    let rss_mib = spawn::group_rss_kib(proc.pid) as f64 / 1024.0;
    let s = c.stop().await;
    println!("  stop: {s:?}");
    assert!(s.shutdown_answered, "{s:?}");
    let _ = opened;
    Row {
        server: preset.name.to_string(),
        version,
        initialize,
        first_diagnostics,
        definition,
        rss_mib,
    }
}

/// Ask for the file's diagnostics until the planted error is among them;
/// the time from the first ask.
async fn wait_for_error(c: &Client, main: &Path, line: u32) -> Duration {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(120);
    let mut saved = false;
    loop {
        let d = c
            .diagnostics(main, Duration::from_secs(10))
            .await
            .expect("diagnostics");
        let found = d.errors().find(|e| e.range.start.line + 1 == line);
        if let Some(e) = found {
            println!(
                "  {:?} after {:?} (version {}): line {}: {} [{}]",
                d.freshness,
                d.waited,
                d.version,
                line,
                e.message.lines().next().unwrap_or_default(),
                e.source.clone().unwrap_or_default()
            );
            return start.elapsed();
        }
        println!(
            "  {:?} after {:?}: {} items, none on line {line} yet",
            d.freshness,
            d.waited,
            d.items.len()
        );
        assert!(Instant::now() < deadline, "no planted error within 120 s");
        if !saved {
            // rust-analyzer runs `cargo check` on a save.
            c.save(main).unwrap();
            saved = true;
        }
        if d.freshness != Freshness::Stale {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

async fn navigate(
    c: &Client,
    main: &Path,
    pos: theseus_lsp::types::Position,
    root: &Path,
    case: &Case,
) {
    let hover = c
        .hover(main, pos)
        .await
        .expect("hover")
        .expect("a hover on total");
    println!(
        "  hover: {:?}",
        theseus_lsp::jsonrpc::clip(&hover.text().replace('\n', " "), 100)
    );
    assert!(hover.text().contains("total"));
    let refs = c.references(main, pos, true).await.expect("references");
    let shown: Vec<String> = refs.iter().map(|l| short(l, root)).collect();
    println!("  references: {shown:?}");
    assert!(refs.len() >= 2, "the call and the definition: {shown:?}");
    let syms = c
        .document_symbols(&root.join(case.defined_in))
        .await
        .expect("symbols");
    let names: Vec<String> = syms.flatten().into_iter().map(|s| s.1).collect();
    println!("  document symbols of {}: {names:?}", case.defined_in);
    assert!(names.iter().any(|n| n == "total"));
    match c.workspace_symbols("total").await {
        Ok(ws) => println!("  workspace symbols for total: {}", ws.len()),
        Err(e) => println!("  workspace symbols: {e}"),
    }
    // At the definition: TypeScript renames an imported name only where it
    // was imported (`import { total as sum_all }`), as an editor does.
    let def_file = root.join(case.defined_in);
    let def_text = std::fs::read_to_string(&def_file).unwrap();
    let def_line = def_text.lines().position(|l| l.contains("total")).unwrap() as u32 + 1;
    let def_pos = locate(&def_text, def_line, "total", None).unwrap();
    let edit = c
        .rename(&def_file, def_pos, "sum_all")
        .await
        .expect("rename")
        .expect("an edit");
    let files: Vec<(String, usize)> = edit
        .text_edits()
        .iter()
        .map(|(u, _, e)| {
            (
                u.rsplit('/').next().unwrap_or_default().to_string(),
                e.len(),
            )
        })
        .collect();
    println!("  rename's edit: {files:?}");
    assert!(
        files
            .iter()
            .any(|(f, _)| case.defined_in.ends_with(f.as_str())),
        "{files:?}"
    );
    assert!(
        files.iter().any(|(f, _)| case.main.ends_with(f.as_str())),
        "across files: {files:?}"
    );
    let text = std::fs::read_to_string(main).unwrap();
    assert!(text.contains("total"), "the rename was not applied");
}

fn report(row: &Row) {
    println!(
        "| {} | {} | {:.0} ms | {:.0} ms | {:.0} ms | {:.0} MiB |",
        row.server,
        row.version,
        row.initialize.as_secs_f64() * 1e3,
        row.first_diagnostics.as_secs_f64() * 1e3,
        row.definition.as_secs_f64() * 1e3,
        row.rss_mib
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs ty on PATH or THESEUS_LSP_TY"]
async fn live_ty() {
    report(&probe(servers::ty(), &PYTHON).await);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs pyright-langserver on PATH or THESEUS_LSP_PYRIGHT"]
async fn live_pyright() {
    report(&probe(servers::pyright(), &PYTHON).await);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs basedpyright-langserver on PATH or THESEUS_LSP_BASEDPYRIGHT"]
async fn live_basedpyright() {
    report(&probe(servers::basedpyright(), &PYTHON).await);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs TypeScript 7's tsc on PATH or THESEUS_LSP_TSGO"]
async fn live_tsgo() {
    report(&probe(servers::tsgo(), &TYPESCRIPT).await);
    js_is_checked(servers::tsgo()).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs typescript-language-server and THESEUS_LSP_TSSERVER"]
async fn live_typescript_language_server() {
    let preset = servers::typescript_language_server(&tsserver());
    report(&probe(preset.clone(), &TYPESCRIPT).await);
    js_is_checked(preset).await;
}

fn tsserver() -> String {
    std::env::var("THESEUS_LSP_TSSERVER")
        .expect("THESEUS_LSP_TSSERVER: the path of TypeScript 5's tsserver.js")
}

/// A JavaScript file under `checkJs` and `// @ts-check` has its error too.
async fn js_is_checked(preset: Preset) {
    let dir = scratch("typescript");
    let root = dir.path().to_path_buf();
    let (server, _proc) = spawn::spawn(&command(&preset), &root, &[], Stdio::null()).unwrap();
    let (c, _e) = Client::start(server, preset.options(&root)).await.unwrap();
    let js = root.join("check.js");
    let t = wait_for_error(&c, &js, 5).await;
    println!("  {}: check.js's error after {t:?}", preset.name);
    c.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs rust-analyzer on PATH or THESEUS_LSP_RUST_ANALYZER, and rustup's cargo first on PATH"]
async fn live_rust_analyzer() {
    report(&probe(servers::rust_analyzer(), &RUST).await);
}

/// What a job writes reaches rust-analyzer's answers (theseus-m9hj): a
/// separate process, as a job's command is, rewrites a file the client never
/// opened and never announces (`ledger.rs`), and the diagnostics of the file
/// that calls it (`lib.rs`, open) follow, by rust-analyzer's own watcher. With
/// the client offering to watch files for it (before m9hj), rust-analyzer
/// watched nothing itself, and `lib.rs` stayed clean.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs rust-analyzer on PATH or THESEUS_LSP_RUST_ANALYZER, and rustup's cargo first on PATH"]
async fn live_rust_analyzer_sees_a_file_a_job_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"job-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n",
    )
    .unwrap();
    let lib = root.join("src/lib.rs");
    std::fs::write(
        &lib,
        "pub mod ledger;\n\npub fn run() -> i32 {\n    ledger::total(&[1, 2])\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/ledger.rs"),
        "pub fn total(xs: &[i32]) -> i32 {\n    xs.iter().sum()\n}\n",
    )
    .unwrap();
    let preset = servers::rust_analyzer();
    let (server, _proc) = spawn::spawn(&command(&preset), &root, &[], Stdio::null()).unwrap();
    let (c, _e) = Client::start(server, preset.options(&root))
        .await
        .expect("initialize");
    let d = c.diagnostics(&lib, Duration::from_secs(60)).await.unwrap();
    assert_eq!(d.errors().count(), 0, "clean before the job: {:?}", d.items);

    // The job: another process, which the client knows nothing of.
    let job = std::process::Command::new("sh")
        .arg("-c")
        .arg("printf 'pub fn total(xs: &[i32]) -> u64 {\\n    xs.len() as u64\\n}\\n' > src/ledger.rs")
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(job.success());
    let wrote = Instant::now();
    let found = loop {
        let d = c.diagnostics(&lib, Duration::from_secs(10)).await.unwrap();
        if let Some(e) = d.errors().find(|e| e.range.start.line + 1 == 4) {
            break e.message.lines().next().unwrap_or_default().to_string();
        }
        assert!(
            wrote.elapsed() < Duration::from_secs(30),
            "lib.rs still clean 30 s after the job rewrote ledger.rs: rust-analyzer did not see it"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    println!(
        "  lib.rs line 4, {:?} after the job wrote ledger.rs: {found}",
        wrote.elapsed()
    );
    assert!(found.contains("u64"), "{found}");
    let s = c.stop().await;
    assert!(s.shutdown_answered, "{s:?}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs rust-analyzer and THESEUS_LSP_WORKSPACE; takes minutes and gigabytes"]
async fn live_rust_analyzer_on_a_workspace() {
    let root =
        PathBuf::from(std::env::var("THESEUS_LSP_WORKSPACE").expect("THESEUS_LSP_WORKSPACE"));
    let preset = servers::rust_analyzer();
    let started = Instant::now();
    let (server, proc) = spawn::spawn(&command(&preset), &root, &[], Stdio::null()).unwrap();
    let mut opts = preset.options(&root);
    opts.initialize_timeout = Duration::from_secs(120);
    // A long grace, to measure how long it takes to exit on its own with a
    // whole workspace loaded (it took more than the default 1 s here).
    opts.exit_grace = Duration::from_secs(10);
    let (c, _e) = Client::start(server, opts).await.unwrap();
    let initialize = started.elapsed();
    let mut peak = 0u64;
    let ready = loop {
        let r = c.wait_ready(Duration::from_secs(5)).await;
        peak = peak.max(spawn::group_rss_kib(proc.pid));
        if let Ok(r) = r {
            break r;
        }
        assert!(
            started.elapsed() < Duration::from_secs(1200),
            "not quiescent in 20 minutes"
        );
    };
    let quiescent = started.elapsed();
    let rss = spawn::group_rss_kib(proc.pid);
    println!(
        "rust-analyzer on {}: initialize {initialize:?}, quiescent after {quiescent:?} ({:?}), {:.0} MiB then, {:.0} MiB at the peak sampled",
        root.display(),
        ready.status,
        rss as f64 / 1024.0,
        peak.max(rss) as f64 / 1024.0
    );
    let s = c.stop().await;
    println!("stop: {s:?}");
}
