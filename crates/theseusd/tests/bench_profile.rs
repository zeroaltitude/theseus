//! The bench profile (theseus-n88g.1): `bench/theseus-bench.toml`, the config a
//! benchmark's task container runs, loads as it is, holds the posture it
//! promises, and starts a real `theseusd check` with no vault, its one secret
//! from the environment.

use std::path::PathBuf;
use std::process::Command;

use theseus_core::config::Config;
use theseus_core::policy::Posture;

fn profile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/theseus-bench.toml")
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
