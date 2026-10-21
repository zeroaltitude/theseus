//! The plugin's pane-scoped actions under herdr 0.9.1's real action environment (theseus-us8e8).
//!
//! herdr 0.9.1 runs a plugin action with `HERDR_PLUGIN_ACTION_ID`, `HERDR_PLUGIN_EVENT(_JSON)`,
//! `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, `HERDR_PANE_ID` (the focused pane), `HERDR_PLUGIN_CONTEXT_JSON`
//! and `HERDR_PLUGIN_CLICKED_URL` (a link click): it never sets `HERDR_ACTIVE_PANE_ID`. Observed on a
//! scratch herdr 0.9.1: `HERDR_PANE_ID` and the context's `focused_pane_id` follow the focus. The API
//! contract test checks the wire, not this environment, so the manifest's command lines are run here:
//! each is `sh -c` with a stand-in `theseus` that records its arguments.

use std::path::Path;
use std::process::Command;

/// The actions whose command line takes the focused pane.
const PANE_ACTIONS: [&str; 5] = ["answer", "stop", "inspect", "pause", "open-link"];

const FOCUSED: &str = "w1:p7";
const CLICKED: &str = "theseus://answer/act_0c7e527d2a";

fn manifest() -> toml::Table {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contrib/herdr/theseus/herdr-plugin.toml");
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn command_line(action: &str) -> String {
    let m = manifest();
    let a = m["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"].as_str() == Some(action))
        .unwrap_or_else(|| panic!("no action {action}"));
    let argv = a["command"].as_array().unwrap();
    assert_eq!(argv[0].as_str(), Some("sh"));
    assert_eq!(argv[1].as_str(), Some("-c"));
    argv[2].as_str().unwrap().to_string()
}

/// herdr 0.9.1's action environment for an action invoked with `pane` focused, and nothing else
/// of herdr's: the list above, as observed.
fn action_env(action: &str, pane: Option<&str>) -> Vec<(String, String)> {
    let mut env = vec![
        ("HERDR_PLUGIN_ACTION_ID".to_string(), action.to_string()),
        ("HERDR_PLUGIN_ID".to_string(), "theseus".to_string()),
        ("HERDR_WORKSPACE_ID".to_string(), "w1".to_string()),
        ("HERDR_TAB_ID".to_string(), "w1:t1".to_string()),
        ("HERDR_PLUGIN_CLICKED_URL".to_string(), CLICKED.to_string()),
    ];
    if let Some(p) = pane {
        env.push(("HERDR_PANE_ID".to_string(), p.to_string()));
        let ctx = serde_json::json!({
            "workspace_id": "w1", "tab_id": "w1:t1", "focused_pane_id": p,
            "invocation_source": "keybinding",
        });
        env.push(("HERDR_PLUGIN_CONTEXT_JSON".to_string(), ctx.to_string()));
    }
    env
}

/// Run an action's command line with `env` and a stand-in `theseus` first on the PATH; the arguments
/// it was called with, one per element.
fn run(action: &str, env: &[(String, String)]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("args");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fake = bin.join("theseus");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", out.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let status = Command::new("/bin/sh")
        .args(["-c", &command_line(action)])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .envs(env.iter().cloned())
        .status()
        .unwrap();
    assert!(status.success(), "{action}: the command line failed");
    std::fs::read_to_string(&out)
        .unwrap_or_else(|_| panic!("{action}: `theseus` was not called"))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The value after `--pane`, if the arguments have one.
fn pane_of(args: &[String]) -> Option<&str> {
    let i = args.iter().position(|a| a == "--pane")?;
    args.get(i + 1).map(String::as_str)
}

/// Every pane-scoped action passes the focused pane under the environment herdr 0.9.1 gives it.
#[test]
fn every_pane_action_gets_the_focused_pane_from_herdrs_action_environment() {
    for action in PANE_ACTIONS {
        let args = run(action, &action_env(action, Some(FOCUSED)));
        assert_eq!(
            pane_of(&args),
            Some(FOCUSED),
            "{action}: --pane must be the focused pane: {args:?}"
        );
        assert_eq!(
            args.iter().filter(|a| *a == "--pane").count(),
            1,
            "{args:?}"
        );
    }
}

/// A herdr that does set `HERDR_ACTIVE_PANE_ID` (and not `HERDR_PANE_ID`) still works; the focused pane
/// of 0.9.1's variable wins when both are set.
#[test]
fn the_active_pane_variable_stays_a_fallback() {
    for action in PANE_ACTIONS {
        let mut env = action_env(action, None);
        env.push(("HERDR_ACTIVE_PANE_ID".into(), "w2:p3".into()));
        let args = run(action, &env);
        assert_eq!(pane_of(&args), Some("w2:p3"), "{action}: {args:?}");
        env.push(("HERDR_PANE_ID".into(), FOCUSED.into()));
        let args = run(action, &env);
        assert_eq!(pane_of(&args), Some(FOCUSED), "{action}: {args:?}");
    }
}

/// With no pane in the environment no `--pane` is passed at all (never an empty one), so the helper
/// can refuse by itself.
#[test]
fn no_pane_in_the_environment_passes_no_pane_flag() {
    for action in PANE_ACTIONS {
        let args = run(action, &action_env(action, None));
        assert!(!args.iter().any(|a| a == "--pane"), "{action}: {args:?}");
    }
}

/// A click's link still reaches `link`, with the pane after it.
#[test]
fn the_link_action_passes_the_clicked_url() {
    let args = run("open-link", &action_env("open-link", Some(FOCUSED)));
    assert!(args.iter().any(|a| a == CLICKED), "{args:?}");
}
