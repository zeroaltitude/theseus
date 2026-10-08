//! The routed bench profile (theseus-eo3h): `bench/theseus-bench-routed.toml`,
//! "Theseus as shipped", loads as it is with the daemon's own config loader,
//! turns the judge and the routing table on, takes the Jev key from the
//! environment by reference, and passes `theseusd check` with no vault.

use std::path::PathBuf;
use std::process::Command;

use theseus_core::config::{Config, Effort};

fn profile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/theseus-bench-routed.toml")
}

/// Both keys, invented.
const KEY: &str = "tv-bench-key-7f3a9c";
const JEV: &str = "jv-bench-key-2b8e41";

/// It parses and validates with no warning, names no vault and no value, and
/// holds what its header says: the judge on, route.v2 acting live, the owner's
/// table over the profiles it defines, and the plain profile's posture.
#[test]
fn the_routed_profile_loads_with_the_judge_and_the_routing_table_on() {
    let text = std::fs::read_to_string(profile_path()).unwrap();
    assert!(!text.contains("op://"), "the routed profile names a vault");
    let (cfg, warnings) = Config::parse(&text).unwrap();
    cfg.validate().unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        cfg.secrets.iter().collect::<Vec<_>>(),
        [
            (
                &"anthropic_api_key".to_string(),
                &"env:ANTHROPIC_API_KEY".to_string()
            ),
            (
                &"jev_api_key".to_string(),
                &"env:TYPESAFE_API_KEY".to_string()
            ),
        ]
    );
    assert!(cfg.judge.enabled);
    assert_eq!(cfg.judge.key_secret, "jev_api_key");
    assert!(cfg.routing.enabled);
    let modes = &cfg.routing.modes;
    let table = |m: &str| modes.of(m).to_vec();
    assert_eq!(table("trivial"), ["haiku"]);
    assert_eq!(table("quick"), ["haiku"]);
    assert!(table("chat").is_empty() && table("other").is_empty());
    assert_eq!(table("sophisticated"), ["fable", "opus"]);
    assert_eq!(table("deep_coding"), ["opus", "fable"]);
    assert_eq!(table("routine_coding"), ["haikuhi", "sonnet"]);
    // Every profile the table names exists, and the models and efforts are
    // the owner's.
    let catalog = theseus_core::catalog::Catalog::builtin();
    for (name, model, effort) in [
        ("sonnet", "claude-sonnet-5-5", None),
        ("opus", "claude-opus-5-5", None),
        ("fable", "claude-fable-5-1", None),
        ("haiku", "claude-haiku-5-5", Some(Effort::Low)),
        ("haikuhi", "claude-haiku-5-5", Some(Effort::High)),
    ] {
        let p = &cfg.profiles[name];
        assert_eq!(p.model, model, "{name}");
        assert_eq!(p.effort, effort, "{name}");
        assert_eq!(p.max_output_tokens, None, "{name} caps the output");
        assert_eq!(p.max_loops, 200, "{name}");
        assert!(p.effective_max_tokens(&catalog) > 0, "{name}");
    }
    // The session's own profile is the plain arm's.
    assert_eq!(cfg.model.live, "bench");
    let bench = &cfg.profiles["bench"];
    assert_eq!(
        (bench.model.as_str(), bench.max_loops, bench.max_output_tokens),
        ("claude-sonnet-5-5", 200, None)
    );
    assert_eq!(cfg.kernel.spend_limit_usd, 2.0);
    assert_eq!(cfg.policy.enforcement, theseus_core::policy::Posture::Open);
    assert!(!cfg.discord.enabled && !cfg.web.enabled && !cfg.index.enabled);
}

/// `theseusd check` with no 1Password access: both keys resolve from the
/// environment and are named as outside the vault, never printed; with the
/// Jev variable unset, the check fails naming it.
#[test]
fn theseusd_check_passes_on_the_routed_profile_with_both_keys_and_names_a_missing_one() {
    let dir = tempfile::tempdir().unwrap();
    let check = |jev: Option<&str>| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_theseusd"));
        cmd.arg("--config")
            .arg(profile_path())
            .arg("--state-dir")
            .arg(dir.path().join("state"))
            .arg("check")
            .env("ANTHROPIC_API_KEY", KEY)
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR");
        if let Some(j) = jev {
            cmd.env("TYPESAFE_API_KEY", j);
        }
        cmd.output().unwrap()
    };
    let out = check(Some(JEV));
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{}\n{stdout}\n{stderr}", out.status);
    assert!(
        stdout.contains("2 secret(s) resolved")
            && stdout.contains("outside the vault: anthropic_api_key (env), jev_api_key (env)"),
        "{stdout}"
    );
    for seen in [&stdout, &stderr] {
        assert!(!seen.contains(KEY) && !seen.contains(JEV), "a key's value was printed");
    }
    let out = check(None);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("jev_api_key: env:TYPESAFE_API_KEY: the variable is unset or empty"),
        "{stderr}"
    );
}
