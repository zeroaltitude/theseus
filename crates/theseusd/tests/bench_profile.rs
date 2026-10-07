//! The bench profile (theseus-n88g.1): `bench/theseus-bench.toml`, the config a
//! benchmark's task container runs, loads as it is, holds the posture it
//! promises, and starts a real `theseusd check` with no vault, its one secret
//! from the environment. Headless turns on it, as a trial runs them (`theseus
//! --spawn theseusd --json ask`, against the stand-in model), hold what b5's
//! losses asked of it (theseus-7gir.19 to .21).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use common::model::FakeModel;
use serde_json::{json, Value};
use theseus_core::config::Config;
use theseus_core::policy::Posture;
use theseus_core::web::net::PrivateAddresses;

fn profile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/theseus-bench.toml")
}

/// The model's key, invented, from the daemon's environment as in a trial.
const KEY: &str = "tv-bench-key-7f3a9c";

/// A trial's directory: `config.toml`, the bench profile as it is but for its
/// model's API base (the stand-in's), the task's directory (`app/`), and
/// `tweak`'s lines; and `state/`.
fn trial(model: &FakeModel, tweak: impl FnOnce(&mut toml::Table)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let text = std::fs::read_to_string(profile_path()).unwrap();
    let mut t: toml::Table = text.parse().unwrap();
    set(&mut t, &["model", "api_base"], model.base.clone().into());
    let app = app.to_string_lossy().into_owned();
    set(&mut t, &["tools", "projects_dir"], app.into());
    tweak(&mut t);
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

fn set(t: &mut toml::Table, path: &[&str], value: toml::Value) {
    let (last, tables) = path.split_last().unwrap();
    let mut at = t;
    for key in tables {
        at = at
            .entry(*key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap();
    }
    at.insert((*last).into(), value);
}

struct Run {
    code: i32,
    turn: Value,
    stderr: String,
}

/// `theseus --spawn <theseusd> --json ask PROMPT` in a trial's directory, as
/// the adapter's run script runs it, to its end.
fn ask(dir: &Path, prompt: &str) -> Run {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--spawn")
        .arg(&theseusd)
        .args(["--json", "ask", prompt])
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        .env("ANTHROPIC_API_KEY", KEY)
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        turn: serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The stand-in model's tool calls: none, so every turn is one call that
/// ends it.
fn no_calls(_: &str) -> Vec<(&'static str, Value)> {
    vec![]
}

/// The output cap is the model's own (theseus-7gir.19): the bench profile
/// names none, so a call on Sonnet 5.5 asks for the catalog's 128000, as
/// Claude Code does, where it asked for 32000 and b5's regex-chess and
/// schemelike-metacircular-eval were cut there.
#[test]
fn a_bench_call_asks_for_the_models_whole_output() {
    let model = FakeModel::start(no_calls);
    let dir = trial(&model, |_| {});
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    let req = &model.requests()[0];
    assert_eq!(
        (req["model"].as_str(), req["max_tokens"].as_u64()),
        (Some("claude-sonnet-5-5"), Some(128_000)),
    );
    assert!(!run.stderr.contains(KEY), "the key's value was printed");
    let text = std::fs::read_to_string(profile_path()).unwrap();
    let (cfg, _) = Config::parse(&text).unwrap();
    let catalog = theseus_core::catalog::Catalog::builtin();
    let bench = &cfg.profiles["bench"];
    assert_eq!(bench.max_output_tokens, None, "the profile caps the output");
    assert_eq!(cfg.model.max_output_tokens, None, "[model] caps the output");
    assert_eq!(bench.effective_max_tokens(&catalog), 128_000);
}

/// A first call that times out before its first byte is made again inside
/// the turn (theseus-7gir.21): the bench profile's `[model.retries]`, so a
/// headless trial, which ends with its turn, ends 0 on the second call. With
/// `transient = 0`, every other config's default, the turn fails as before,
/// and the trial exits 1, as b5's hf-model-inference did.
#[test]
fn a_first_byte_timeout_is_retried_inside_the_headless_turn() {
    // The first byte's timeout at 1 s, the table's other three as the defaults.
    let short = |t: &mut toml::Table| {
        for (key, secs) in [
            ("connect_secs", 10),
            ("first_byte_secs", 1),
            ("stream_idle_secs", 60),
            ("total_secs", 600),
        ] {
            set(t, &["model", "timeouts", key], secs.into());
        }
    };
    let model = FakeModel::start(no_calls);
    model.stall_next(1);
    let dir = trial(&model, short);
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "no_tool_calls", "{}", run.turn);
    assert_eq!(model.requests().len(), 2, "the call and its retry");
    let model = FakeModel::start(no_calls);
    model.stall_next(1);
    let dir = trial(&model, |t| {
        short(t);
        set(t, &["model", "retries", "transient"], 0.into());
    });
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 1, "{}\n{}", run.turn, run.stderr);
    assert!(run.stderr.contains("FirstByte"), "{}", run.stderr);
    assert_eq!(
        model.requests().len(),
        1,
        "no retry inside the turn\n{}\n{}",
        seen(&model),
        run.stderr
    );
}

/// Each request the stand-in saw (theseus-jtrc): its arrival after the
/// first's, its connection (the client's port), and its body, for a count
/// that came out wrong.
fn seen(model: &FakeModel) -> String {
    let arrivals = model.arrivals();
    let first = arrivals.first().copied();
    // The wall clock's time of each, to set beside the daemon's log.
    let (now, wall) = (std::time::Instant::now(), std::time::SystemTime::now());
    let lines = arrivals
        .iter()
        .zip(model.peers())
        .zip(model.requests())
        .enumerate()
        .map(|(i, ((at, port), body))| {
            let after = first.map_or(0, |f| at.duration_since(f).as_millis());
            let unix = (wall - now.duration_since(*at))
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            format!("request {i}: +{after} ms (unix {unix:.6}), port {port}: {body}")
        });
    lines.collect::<Vec<_>>().join("\n")
}

/// A request the model refuses is made once more on its fallback inside the
/// turn (theseus-7gir.18): Sonnet 5.5's is Sonnet 5, so a headless trial ends
/// 0 on its answer, as Claude Code's trials did where b5's three refused.
/// With `[model.retries] refusal = false` the trial exits 7, as those did.
#[test]
fn a_refused_request_is_answered_by_its_fallback_inside_the_headless_turn() {
    let prompt = "Find the word inside the locked archive and write it to answer.txt.";
    let model = FakeModel::start(no_calls);
    model.decline_next(1);
    let dir = trial(&model, |_| {});
    let run = ask(dir.path(), prompt);
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["model"], "claude-sonnet-5", "{}", run.turn);
    assert_eq!(
        run.turn["fallback"],
        json!({"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "category": "cyber", "answered": true})
    );
    let models: Vec<Value> = model
        .requests()
        .iter()
        .map(|r| r["model"].clone())
        .collect();
    assert_eq!(
        models,
        [json!("claude-sonnet-5-5"), json!("claude-sonnet-5")]
    );
    let model = FakeModel::start(no_calls);
    model.decline_next(1);
    let dir = trial(&model, |t| {
        set(t, &["model", "retries", "refusal"], false.into())
    });
    let run = ask(dir.path(), prompt);
    assert_eq!(run.code, 7, "{}\n{}", run.turn, run.stderr);
    assert_eq!(model.requests().len(), 1, "no fallback");
}

/// A page on this machine, as a task's own web server serves one: every
/// request is answered with its HTML, and each one's first line is kept.
fn page_server() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let kept = seen.clone();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            let _ = r.read_line(&mut line);
            kept.lock().unwrap().push(line.trim_end().to_string());
            let mut header = String::new();
            while r.read_line(&mut header).is_ok_and(|n| n > 2) {
                header.clear();
            }
            let body = "<html><title>noVNC</title><body>a screen</body></html>";
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (port, seen)
}

/// A fetch of this machine's own page runs under the bench profile, and no
/// call waits (theseus-7gir.20): its `private_addresses = "open"`. Before,
/// the gate asked for an approval no one gives in a headless trial, and b5's
/// install-windows-3.11 ended waiting (exit 6) on its check of its own
/// `localhost/vnc.html`.
#[test]
fn a_fetch_of_this_machines_page_runs_and_waits_for_no_one() {
    let (port, seen) = page_server();
    let page = format!("http://127.0.0.1:{port}/vnc.html");
    let model = FakeModel::start(move |prompt| match prompt.contains("check the page") {
        true => vec![("http_fetch", json!({ "url": page.as_str() }))],
        false => vec![],
    });
    let dir = trial(&model, |_| {});
    let run = ask(dir.path(), "check the page your server serves");
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    assert_eq!(
        (&run.turn["stop_reason"], &run.turn["tool_calls"]),
        (&json!("no_tool_calls"), &json!(1)),
        "{}",
        run.turn
    );
    assert!(run.turn["awaiting_confirm"].is_null(), "{}", run.turn);
    assert_eq!(*seen.lock().unwrap(), ["GET /vnc.html HTTP/1.1"]);
}

/// It parses and validates with no warning, as the template does, and says
/// what its header says: no vault, every tool open, roots at `/`, L0, and
/// Discord, the web UI, and the index tender off.
#[test]
fn the_bench_profile_loads_and_opens_every_tool() {
    let text = std::fs::read_to_string(profile_path()).unwrap();
    assert!(!text.contains("op://"), "the bench profile names a vault");
    let (cfg, warnings) = Config::parse(&text).unwrap();
    cfg.validate().unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        cfg.secrets.iter().collect::<Vec<_>>(),
        [(
            &"anthropic_api_key".to_string(),
            &"env:ANTHROPIC_API_KEY".to_string()
        )]
    );
    assert_eq!(cfg.model.live, "bench");
    let bench = &cfg.profiles["bench"];
    assert_eq!(
        (bench.provider.as_str(), bench.max_loops),
        ("anthropic", 200)
    );
    // Every tool inherits the enforcement: no tool or MCP names its own.
    assert_eq!(cfg.policy.enforcement, Posture::Open);
    assert!(cfg.policy.tools.is_empty(), "{:?}", cfg.policy.tools);
    assert!(cfg.policy.mcp.is_empty() && cfg.policy.aws.is_empty());
    assert!(cfg.policy.allow_argv.is_empty() && cfg.policy.approve_argv.is_empty());
    assert!(cfg.policy.external_programs.is_empty());
    // A private address is judged as any other (theseus-7gir.20).
    assert_eq!(cfg.policy.private_addresses, PrivateAddresses::Open);
    assert_eq!(cfg.tools.roots, ["/"]);
    assert!(cfg.tools.approve_paths.is_empty());
    assert_eq!(cfg.tools.proc_sync_secs, 900);
    assert!(!cfg.discord.enabled && !cfg.web.enabled && !cfg.index.enabled);
    assert_eq!((cfg.server.disk_warn_mb, cfg.server.disk_floor_mb), (0, 0));
}

/// `theseusd check` on the profile with no 1Password access at all: the
/// daemon starts on no vault, the key resolves from the environment, and the
/// check names it as from outside the vault, never its value.
#[test]
fn theseusd_check_passes_on_the_bench_profile_with_no_vault() {
    let dir = tempfile::tempdir().unwrap();
    let key = "tv-bench-key-7f3a9c";
    let out = Command::new(env!("CARGO_BIN_EXE_theseusd"))
        .arg("--config")
        .arg(profile_path())
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .arg("check")
        .env("ANTHROPIC_API_KEY", key)
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_CONFIG")
        .env_remove("THESEUS_STATE_DIR")
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{}\n{stdout}\n{stderr}", out.status);
    assert!(
        stdout.contains("1 secret(s) resolved")
            && stdout.contains("(local): anthropic_api_key")
            && stdout.contains("outside the vault: anthropic_api_key (env)"),
        "{stdout}"
    );
    assert!(
        !stdout.contains(key) && !stderr.contains(key),
        "the key's value was printed"
    );
    // The same profile with the variable unset: the check fails, naming it.
    let out = Command::new(env!("CARGO_BIN_EXE_theseusd"))
        .arg("--config")
        .arg(profile_path())
        .arg("--state-dir")
        .arg(dir.path().join("state"))
        .arg("check")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("anthropic_api_key: env:ANTHROPIC_API_KEY: the variable is unset or empty"),
        "{stderr}"
    );
}
