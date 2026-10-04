//! The default config names no one's vault (theseus-8d1b), and the lookup
//! finds it in order (theseus-5aqz): `--config`, then `THESEUS_CONFIG`, then
//! `~/.theseus/theseus.toml` if it exists, then `/etc/theseus/theseus.toml`.
//! With neither file, a start says where a config comes from instead of
//! reading a vault note, and `--help` says the order. `config` and `check`
//! name the file in use, and `install --user`'s plan names it for the unit. A
//! deployment kept in 1Password names its note in its own environment.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn theseusd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_theseusd"))
}

/// The machine's config file as these tests' `theseusd` sees it: a path in
/// `home`, never the machine's own `/etc/theseus/theseus.toml`.
fn machines(home: &Path) -> PathBuf {
    home.join("etc/theseus/theseus.toml")
}

/// `theseusd` with `home` as its home, a stand-in `op` first on its PATH (it
/// answers nothing), no inherited Theseus settings, and `machines(home)` as
/// the machine's config file (a debug build's `THESEUS_TEST_SYSTEM_CONFIG`).
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
        .env("THESEUS_TEST_SYSTEM_CONFIG", machines(home))
        .stdin(Stdio::null());
    c
}

#[test]
fn the_default_config_is_a_local_file_and_a_missing_one_says_where_one_comes_from() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let state = state.to_str().unwrap();

    // `--help` says the lookup's order (clap may wrap the flag's line).
    let help = command(home.path()).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flat.contains("the default lookup: ~/.theseus/theseus.toml if it exists, else /etc/theseus/theseus.toml"),
        "{help}"
    );

    // Neither file: the start says where a config comes from, and reads no
    // vault (the stand-in `op` would fail, with another message).
    let out = command(home.path())
        .args(["--state-dir", state, "config"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no config: set THESEUS_CONFIG (or --config) to your config's op:// reference or file, or write one at ~/.theseus/theseus.toml or at /etc/theseus/theseus.toml, read in that order"),
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
        text.starts_with(
            "# source: ~/.theseus/theseus.toml (the default: nothing named a config)\n"
        ),
        "{text}"
    );
}

/// A config with no vault reference: a stand-in model's key from a private
/// file, so `check` resolves every secret with no vault.
fn standin_config(home: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let key = home.join("standin-key");
    if !key.exists() {
        std::fs::write(&key, "stand-in-not-a-key").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let at = home.join(name);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(
        &at,
        format!(
            "[model]\napi_base = \"http://127.0.0.1:9\"\n\n[discord]\nenabled = false\n\n\
             [web]\nenabled = false\n\n[index]\nenabled = false\n\n\
             [secrets]\nanthropic_api_key = \"file:{}\"\n",
            key.display()
        ),
    )
    .unwrap();
    at
}

/// Each step of the order, in turn (theseus-5aqz): `--config` over
/// `THESEUS_CONFIG`, the variable over both files, the operator's own file
/// over the machine's, and the machine's when the operator has none. `config`
/// names the source and how it was found, `check` names the file it loaded,
/// and `install --user`'s plan writes the machine's file into the unit.
#[test]
fn the_lookup_takes_the_flag_then_the_variable_then_the_operators_file_then_the_machines() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let state = h.join("state");
    let flag = standin_config(h, "flag.toml");
    let var = standin_config(h, "var.toml");
    let own = standin_config(h, ".theseus/theseus.toml");
    let etc = machines(h);
    standin_config(h, "etc/theseus/theseus.toml");
    let source = |c: &mut Command| -> String {
        let out = c
            .arg("--state-dir")
            .arg(&state)
            .arg("config")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        text.lines().next().unwrap_or_default().to_string()
    };

    // 1. The flag, over the variable and both files.
    let mut c = command(h);
    c.env("THESEUS_CONFIG", &var).arg("--config").arg(&flag);
    assert_eq!(source(&mut c), format!("# source: {}", flag.display()));
    // 2. The variable, over both files.
    let mut c = command(h);
    c.env("THESEUS_CONFIG", &var);
    assert_eq!(source(&mut c), format!("# source: {}", var.display()));
    // 3. The operator's own file, over the machine's.
    assert_eq!(
        source(&mut command(h)),
        "# source: ~/.theseus/theseus.toml (the default: nothing named a config)"
    );
    // 4. The machine's, once the operator has none.
    std::fs::remove_file(&own).unwrap();
    assert_eq!(
        source(&mut command(h)),
        format!(
            "# source: {} (the default: nothing named a config, and there is no ~/.theseus/theseus.toml)",
            etc.display()
        )
    );
    // `check` names the file it loaded, and how it was found.
    let out = command(h)
        .arg("--state-dir")
        .arg(&state)
        .arg("check")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with(&format!(
            "ok: config loaded from {} (the default: nothing named a config, and there is no ~/.theseus/theseus.toml); 1 secret(s) resolved",
            etc.display()
        )),
        "{text}"
    );
    // The unit a plan writes names that file, so the service needs no
    // THESEUS_CONFIG (a plan changes nothing; `--user` refuses root).
    let out = command(h).args(["install", "--user"]).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.contains("not as root") {
        let plan = String::from_utf8_lossy(&out.stdout);
        assert!(
            plan.contains(&format!("\n  config:    {}\n", etc.display())),
            "{plan}{err}"
        );
        assert!(
            plan.contains(&format!(" --config {}\n", etc.display())),
            "{plan}"
        );
    }
}

/// `example-config` prints the public template, byte for byte, and takes no
/// flags (theseus-vwar: the overlay, `--overlay`, and `--plain` are gone).
/// `config --sparse` prints the loaded note cut to what differs from the
/// defaults: the whole template as a note cuts to a fraction of its lines,
/// and the cut note loads to the same config, which `config` prints the same.
#[test]
fn example_config_is_the_template_and_config_sparse_cuts_a_note_to_what_differs() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join("state");
    let state = state.to_str().unwrap();
    let template = include_str!("../../theseus-core/config/theseus.example.toml");
    let out = command(home.path()).arg("example-config").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), template);
    for gone in [&["--plain"][..], &["--overlay", "overlay.toml"]] {
        let out = command(home.path())
            .arg("example-config")
            .args(gone)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{gone:?}");
    }

    let run = |config: &Path, args: &[&str]| {
        let out = command(home.path())
            .arg("--config")
            .arg(config)
            .args(["--state-dir", state])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let whole = home.path().join("whole.toml");
    std::fs::write(&whole, template).unwrap();
    let sparse = run(&whole, &["config", "--sparse"]);
    assert!(
        sparse.starts_with("# Theseus config: only what differs from the defaults"),
        "{sparse}"
    );
    assert!(
        sparse.lines().count() * 2 < template.lines().count(),
        "{sparse}"
    );
    let cut = home.path().join("sparse.toml");
    std::fs::write(&cut, &sparse).unwrap();
    // The same config, but for the line that names its source.
    let printed = |config: &Path| -> Vec<String> {
        run(config, &["config"])
            .lines()
            .skip(1)
            .map(str::to_string)
            .collect()
    };
    assert_eq!(printed(&cut), printed(&whole));
}
