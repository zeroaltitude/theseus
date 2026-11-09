#!/usr/bin/env bash
# The daily supply-chain job (theseus-goa8; review 2's SC2). The commit gate runs
# `cargo deny --offline` against the advisory database in ~/.cargo/advisory-dbs, which
# only changes when the owner runs `cargo deny fetch db` on purpose (theseus-4vsrz):
# the gate judges the tree against a pinned set, so main is not red tomorrow with no
# commit in between. This job is the other contract: it fetches into a database of its
# own (default ~/.cache/theseus/deny-daily-db), never the gate's, so its fetch can
# never redden a gate, and it finds a new advisory or a yanked crate the day it appears:
#
#   1. fetch the advisory database (into its own copy) and the registry index;
#   2. run the whole `cargo deny check` against them;
#   3. when it fails, file one Beads issue (owner main, waiting for an
#      available agent) and say so, unless one is already open (`bd create` has no
#      status flag: the issue is made, then its status set).
#
# A failed fetch is an outage, not a finding: the check then runs on what is
# cached and files nothing. Exit status: 0 all clear; 1 the check failed; 2 the
# fetch failed and the check was clean on what is cached. The job reads the repository and never builds or
# changes it (`--locked`), so it can run in the chain's tree. It is not
# installed: scripts/AGENTS.md, "The daily deny job", has the timer to install.
#
#   THESEUS_REPO         the tree to check (default ~/projects/theseus)
#   THESEUS_CARGO        the cargo to run (default cargo; a test's stand-in)
#   THESEUS_DENY_CONFIG  a deny.toml to use instead of the tree's (a test's)
#   THESEUS_DENY_DRY_RUN a non-empty value prints the issue it would file
#   THESEUS_DENY_LOG     the log (default ~/.cache/theseus/deny-daily.log)
#   THESEUS_DENY_DB      the daily job's own advisory database
#                        (default ~/.cache/theseus/deny-daily-db)
set -euo pipefail
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/home/linuxbrew/.linuxbrew/bin:$PATH"
repo="${THESEUS_REPO:-$HOME/projects/theseus}"
log="${THESEUS_DENY_LOG:-$HOME/.cache/theseus/deny-daily.log}"
cd "$repo"
mkdir -p "$(dirname "$log")"
cargo="${THESEUS_CARGO:-cargo}"
dbdir="${THESEUS_DENY_DB:-$HOME/.cache/theseus/deny-daily-db}"
mkdir -p "$dbdir"

out="$(mktemp "${TMPDIR:-/tmp}/theseus-deny.XXXXXX")"
cfg="$(mktemp "${TMPDIR:-/tmp}/theseus-deny-cfg.XXXXXX")"
trap 'rm -f "$out" "$cfg"' EXIT
# The deny.toml in use, with `db-path` pointing at this job's own database, so that
# neither its fetch nor its check touches the one the gate reads. `db-path` is not
# set in deny.toml, so the line goes right after its `[advisories]` header.
src="${THESEUS_DENY_CONFIG:-$repo/deny.toml}"
awk -v d="$dbdir" '{print} /^\[advisories\]/{print "db-path = \"" d "\""}' "$src" >"$cfg"
grep -q '^db-path = ' "$cfg" || { echo "deny-daily: $src has no [advisories] table to point at $dbdir" >&2; exit 3; }
config=(--config "$cfg")
{
  echo "== deny-daily $(date -Iseconds): $repo at $(git rev-parse --short HEAD)"
  fetched=yes
  timeout 300 "$cargo" deny --locked --log-level error "${config[@]}" fetch db index || fetched=no
  [ "$fetched" = yes ] || echo "deny-daily: the fetch failed (an outage upstream?): checking against what is cached"
  status=0
  timeout 300 "$cargo" deny --locked --offline --log-level error "${config[@]}" check >"$out" 2>&1 || status=$?
  tail -60 "$out"
  if [ "$status" -eq 0 ] && [ "$fetched" = yes ]; then
    echo "deny-daily: ok"
  elif [ "$status" -eq 0 ]; then
    # Clean on what is cached, but the cache is not fresh: the unit shows as
    # failed, so an outage that lasts is seen.
    echo "deny-daily: clean on the cached database, which the failed fetch left as it was"
    status=2
  elif [ "$fetched" = no ]; then
    echo "deny-daily: the check failed ($status) and so did the fetch: not filed, since the database may be stale"
  else
    title="cargo deny failed on $(date +%F): $(grep -m1 -oE 'RUSTSEC-[0-9]+-[0-9]+|yanked|license|ban' "$out" || echo see the log)"
    if bd list --label deny-daily --json 2>/dev/null | grep -q '"title"'; then
      echo "deny-daily: the check failed ($status), and a deny-daily issue is already open: not filing another"
    else
      body="The daily cargo deny check failed at $(date -Iseconds) on $repo ($(git rev-parse --short HEAD)); its last lines are in ${log}. Fix the finding, or ignore it in deny.toml with its reason, as each ignore there does."
      args=(create "$title" --assignee main -p 2 -l security,deny-daily --silent -d "$body")
      if [ -n "${THESEUS_DENY_DRY_RUN:-}" ]; then
        echo "deny-daily: would file: bd ${args[*]}, then bd update <its id> --status waiting_for_available_agent"
      else
        id="$(bd "${args[@]}")"
        bd update "$id" --status waiting_for_available_agent
        echo "deny-daily: filed $id"
      fi
    fi
  fi
  exit "$status"
} 2>&1 | tee -a "$log"
exit "${PIPESTATUS[0]}"
