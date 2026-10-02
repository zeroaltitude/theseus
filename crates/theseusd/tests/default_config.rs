//! The default config names no one's vault (theseus-8d1b). Without `--config`
//! or `THESEUS_CONFIG`, `theseusd` reads `~/.theseus/theseus.toml`; with no file
//! there, it says where a config comes from instead of reading a vault note,
//! and `--help` shows the file as the default. A deployment kept in 1Password
//! names its note in its own environment.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn theseusd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_theseusd"))
}

/// `theseusd` with `home` as its home, a stand-in `op` first on its PATH (it
/// answers nothing), and no inherited Theseus settings.
fn command(home: &Path) -> Command {
    let bin = home.join("bin");
    if !bin.join("op").exists() {
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("op"), "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut c = Command::new(theseusd());
    c.env("HOME", home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_CONFIG")
        .env_remove("THESEUS_STATE_DIR")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_RESTARTED_ONTO_VAULT")
        .env_remove("THESEUS_CONFIG_VAULT_FIRST")
        .stdin(Stdio::null());
    c
}

#[test]
fn the_default_config_is_a_local_file_and_a_missing_one_says_where_one_comes_from() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let state = state.to_str().unwrap();

    // `--help` names the file as the default (`[default: …]`, which clap
    // may wrap after its colon).
    let help = command(home.path()).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("~/.theseus/theseus.toml]"), "{help}");

    // No file there: the start says where a config comes from, and reads no
    // vault (the stand-in `op` would fail, with another message).
    let out = command(home.path())
        .args(["--state-dir", state, "config"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no config: set THESEUS_CONFIG (or --config) to your config's op:// reference or file, or write one at ~/.theseus/theseus.toml"),
        "{err}"
    );

    // A file there is the config.
    let projects = home.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::create_dir_all(home.path().join(".theseus")).unwrap();
    std::fs::write(
        home.path().join(".theseus/theseus.toml"),
        common::safe_note(&theseusd(), &projects, 100.0),
    )
    .unwrap();
    let out = command(home.path())
        .args(["--state-dir", state, "config"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with("# source: ~/.theseus/theseus.toml\n"),
        "{text}"
    );
}
