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

/// `example-config` with an overlay (theseus-dxgb): the template with an
/// operator's own values in place, ready to copy whole, from `--overlay`
/// (`~` read as home) or from `~/.config/theseus/template-overlay.toml` with
/// no flag; with neither, or `--plain`, the template as it is; and an overlay
/// that names a key no config has fails with the key named, printing nothing.
#[test]
fn example_config_with_an_overlay_prints_the_template_with_its_values_in_place() {
    let home = tempfile::tempdir().unwrap();
    let plain = command(home.path()).arg("example-config").output().unwrap();
    assert!(plain.status.success());
    let plain = String::from_utf8(plain.stdout).unwrap();
    assert!(plain
        .contains("anthropic_api_key = \"op://<your vault>/<Anthropic API key item>/notesPlain\""));

    let overlay = home.path().join("overlay.toml");
    std::fs::write(
        &overlay,
        "[secrets]\nanthropic_api_key = \"op://Ops Vault/anthropic key/notesPlain\"\n\n\
         [places]\nowner = [\"discord:314159265358979323\"]\n",
    )
    .unwrap();
    let out = command(home.path())
        .args(["example-config", "--overlay", "~/overlay.toml"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("\nanthropic_api_key = \"op://Ops Vault/anthropic key/notesPlain\"\n"),
        "{text}"
    );
    assert!(
        text.contains("\n[places]\nowner = [\"discord:314159265358979323\"]"),
        "{text}"
    );
    // One line more than the template, the first, which names the overlay;
    // the rest only in place.
    assert!(
        text.starts_with(
            "# theseusd example-config, with the overlay ~/overlay.toml in place (--plain prints the template alone).\n"
        ),
        "{text}"
    );
    assert_eq!(text.lines().count(), plain.lines().count() + 1);
    let toml: toml::Table = text.parse().unwrap();
    assert!(toml.contains_key("places"));

    // At the default path, the overlay needs no flag, and --plain prints the
    // template alone, as a missing overlay does.
    let default = home.path().join(".config/theseus/template-overlay.toml");
    std::fs::create_dir_all(default.parent().unwrap()).unwrap();
    std::fs::copy(&overlay, &default).unwrap();
    let auto = command(home.path()).arg("example-config").output().unwrap();
    assert!(
        auto.status.success(),
        "{}",
        String::from_utf8_lossy(&auto.stderr)
    );
    let auto = String::from_utf8(auto.stdout).unwrap();
    assert!(
        auto.starts_with("# theseusd example-config, with the overlay ~/.config/theseus/template-overlay.toml in place"),
        "{auto}"
    );
    assert_eq!(
        auto.lines().skip(1).collect::<Vec<_>>(),
        text.lines().skip(1).collect::<Vec<_>>()
    );
    let bare = command(home.path())
        .args(["example-config", "--plain"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(bare.stdout).unwrap(), plain);
    let both = command(home.path())
        .args(["example-config", "--plain", "--overlay", "~/overlay.toml"])
        .output()
        .unwrap();
    assert!(!both.status.success(), "--plain and --overlay conflict");

    std::fs::write(&overlay, "[places]\nowners = [\"discord:1\"]\n").unwrap();
    let bad = command(home.path())
        .args(["example-config", "--overlay", overlay.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty(), "nothing is printed");
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(
        err.contains("owners") && err.contains("does not load as a config"),
        "{err}"
    );
}
