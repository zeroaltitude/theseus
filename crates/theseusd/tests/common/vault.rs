//! A stand-in for the vault's program (`op`), for a test that has a real
//! daemon keep a secret (issue 29): it lists one vault and two items, takes an
//! item on its standard input, and reads back what was last stored. It logs
//! its arguments and keeps what it was given beside the script, so a test can
//! find the value in the vault and nowhere else.

use std::path::Path;

const SCRIPT: &str = r#"#!/bin/sh
D='@D@'; LOG="$D/op.argv"
printf '%s\n' "$*" >> "$LOG"
case "$1 $2" in
  "vault list")
    case " $* " in
      *" --permission "*) [ -f "$D/op.readonly" ] && echo '[]' || echo '[{"id":"vaultid00000000000000000001","name":"Harbor"}]' ;;
      *) echo '[{"id":"vaultid00000000000000000001","name":"Harbor"}]' ;;
    esac ;;
  "item list")
    EXTRA=''
    [ -f "$D/op.created" ] && EXTRA=",{\"id\":\"itemid00000000000000000099\",\"title\":\"$(cat "$D/op.created")\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"API_CREDENTIAL\"}"
    echo "[{\"id\":\"itemid00000000000000000001\",\"title\":\"Lantern key\",\"vault\":{\"id\":\"vaultid00000000000000000001\",\"name\":\"Harbor\"},\"category\":\"API_CREDENTIAL\"}$EXTRA]" ;;
  "item create"|"item edit")
    # A test that holds the write open (`hold`): say so, then wait to be let go. Nothing polls and nothing sleeps.
    if [ -p "$D/op.gate" ]; then : > "$D/op.holding"; read _ < "$D/op.gate"; fi
    cat > "$D/op.stdin"
    sed -n 's/.*"id":"credential","[^}]*"value":"\([^"]*\)".*/\1/p' "$D/op.stdin" > "$D/op.stored"
    [ "$1 $2" = "item create" ] && sed -n 's/.*"title":"\([^"]*\)".*/\1/p' "$D/op.stdin" > "$D/op.created"
    echo '{}' ;;
  "read --no-newline")
    [ -f "$D/op.stored" ] && { printf '%s' "$(cat "$D/op.stored")"; exit 0; }
    echo '[ERROR] 2026/10/19 08:00:00 isn'"'"'t an item' >&2; exit 1 ;;
  *) echo "[ERROR] unexpected: $*" >&2; exit 1 ;;
esac
"#;

/// The program, written as `bin/op` under `dir`, keeping its files in `dir`.
pub fn install(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let bin = dir.join("bin/op");
    std::fs::write(&bin, SCRIPT.replace("@D@", &dir.display().to_string())).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// What the vault holds in the credential field, once something was kept.
pub fn stored(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("op.stored")).ok()
}

/// The program's argument lines so far.
pub fn argv(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("op.argv"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Make the next write wait until [`release`]: the program says it is holding (`holding`) and then blocks
/// reading a pipe, so a test can look at everything that happens while the vault is slow, with no sleep.
pub fn hold(dir: &Path) {
    let gate = dir.join("op.gate");
    let made = std::process::Command::new("mkfifo")
        .arg(&gate)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo");
}

/// Whether the held write has begun and is waiting.
pub fn holding(dir: &Path) -> bool {
    dir.join("op.holding").exists()
}

/// Let the held write go on.
pub fn release(dir: &Path) {
    use std::io::Write;
    let mut gate = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("op.gate"))
        .unwrap();
    gate.write_all(b"go\n").unwrap();
}
