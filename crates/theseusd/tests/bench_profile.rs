//! The bench profile (theseus-n88g.1): `bench/theseus-bench.toml`, the config a
//! benchmark's task container runs, loads as it is, holds the posture it
//! promises, and starts a real `theseusd check` with no vault, its one secret
//! from the environment. Headless turns on it, as a trial runs them (`theseus
//! --spawn theseusd --json ask`, against the stand-in model), hold what b5's
//! losses asked of it (theseus-7gir.19 to .21).

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use serde_json::Value;
use theseus_core::config::Config;
use theseus_core::policy::Posture;

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
