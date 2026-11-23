//! The vault's reads, counted (issue 44). Every test here runs a fake `op`, a
//! shell script that writes its first argument to a `runs` file each time it
//! starts and answers by the word in its `mode` file: the count of its runs is
//! the assertion. No real vault is read, and every value is a made-up literal.

use super::pace::SystemClock;
use super::*;

/// The fake `op`: `ok` answers every read and injection; `missing` refuses
/// every reference as an item that is not there; `limited` says the vault's
/// rate limit; `some` fails an injection and the read of `gone`, and answers
/// every other read.
const FAKE_OP: &str = r#"#!/bin/sh
D="$(dirname "$0")"
printf '%s\n' "$1" >> "$D/runs"
case "$(cat "$D/mode")" in
  ok)
    case "$1" in
      read) printf 'made-up-read-value\n' ;;
      inject) awk '/^\{\{ .* \}\}$/ { n++; printf "made-up-inject-%d\n\n", n; next } { print }' ;;
    esac ;;
  missing) echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1 ;;
  limited) echo "[ERROR] 2026/11/22 12:00:00 Too many requests" >&2; exit 1 ;;
  some)
    if [ "$1" = inject ]; then echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1; fi
    case "$3" in
      *gone*) echo "[ERROR] 2026/11/22 12:00:00 \"gone\" isn't an item in the \"V\" vault" >&2; exit 1 ;;
      *) printf 'made-up-read-value\n' ;;
    esac ;;
esac
"#;

/// A fake `op` in `dir`, answering by `mode`, counting into `reads`.
fn fake_op(dir: &std::path::Path, mode: &str, reads: Arc<VaultReads>) -> Arc<OpReader> {
    use std::os::unix::fs::PermissionsExt;
    let op_bin = dir.join("op");
    std::fs::write(&op_bin, FAKE_OP).unwrap();
    std::fs::set_permissions(&op_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("mode"), mode).unwrap();
    Arc::new(OpReader::with_bin("test-not-a-token", op_bin).counting_into(reads))
}

/// What the fake `op` was started as, each run its first argument.
fn runs(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("runs"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn refs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A board over `refs` counting into `reads`.
fn board(refs: &BTreeMap<String, String>, reads: Arc<VaultReads>) -> Arc<SecretBoard> {
    let b = SecretBoard::for_config(refs, Instant::now());
    b.count_from(reads);
    b
}

/// Commit 1: each `op` the daemon starts is counted with the entries it asks
/// for, and health's `vault_today` says the day's count; a board with no
/// count says nothing.
#[tokio::test]
async fn every_op_the_daemon_starts_is_counted_and_health_says_the_days_reads() {
    let dir = tempfile::tempdir().unwrap();
    let reads = VaultReads::new(Arc::new(SystemClock));
    let op = fake_op(dir.path(), "ok", reads.clone());
    let refs = refs(&[
        ("a", "op://V/a/f"),
        ("b", "op://V/b/f"),
        ("c", "op://V/c/f"),
    ]);
    let b = board(&refs, reads.clone());
    tokio::spawn(resolve_into(b.clone(), refs.clone(), op.clone()));
    b.wait_settled(Duration::from_secs(10)).await.unwrap();
    assert_eq!(runs(dir.path()), ["inject"], "one op for three entries");
    let today = b.status().vault_today.expect("counted");
    assert_eq!((today.processes, today.references), (1, 3));
    assert_eq!(today.ceiling, pace::default_vault_reads_per_day());
    assert_eq!(today.stopped, None);
    op.read(&SecretRef::parse("op://V/d/f").unwrap()).await.unwrap();
    assert_eq!(runs(dir.path()), ["inject", "read"]);
    assert_eq!(reads.today(), (2, 4));
    let uncounted = SecretBoard::for_config(&refs, Instant::now());
    assert_eq!(uncounted.status().vault_today, None, "no count, no line");
}
