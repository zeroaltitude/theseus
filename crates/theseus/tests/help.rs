//! Golden output for `theseus help` (the everyday screen) and `theseus help all` (every command), from the real
//! binary, which needs no daemon. `THESEUS_GOLDEN=write` rewrites `tests/golden/help*.txt`. The commands each
//! lists are held by `help_cmd`'s own tests, which read clap's tree.

use std::path::PathBuf;
use std::process::Command;

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn run(args: &[&str]) -> String {
    let out = Command::new(THESEUS)
        .args(args)
        .env_remove("COLUMNS")
        .output()
        .unwrap();
    format!(
        "$ theseus {}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1)
    )
}

fn golden(name: &str, got: &str) {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "golden",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    assert_eq!(got, want, "{name} differs from {}", path.display());
}

#[test]
fn the_everyday_screen_is_the_golden() {
    golden("help", &run(&["help"]));
}

#[test]
fn help_all_is_the_golden() {
    golden("help_all", &run(&["help", "all"]));
}

#[test]
fn a_hidden_command_runs_its_own_help_page() {
    let page = run(&["ledger", "--help"]);
    assert!(page.contains("Usage: theseus ledger"), "{page}");
    assert!(page.contains("--- exit 0"), "{page}");
    let via_help = run(&["help", "ledger"]);
    assert!(via_help.contains("Usage: theseus ledger"), "{via_help}");
}

#[test]
fn an_unknown_command_names_help_all() {
    let page = run(&["help", "nonesuch"]);
    assert!(page.contains("theseus help all"), "{page}");
    assert!(page.contains("--- exit 2"), "{page}");
}
