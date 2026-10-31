#!/usr/bin/env bash
# scrub-added.sh [BASE] (theseus-ev3ir): the sensitive-name scrub over what a branch ADDS, as a gate phase.
#
# The unit is the added lines, not the changed files: whole files drown in the example config's `op://`
# placeholders, and a name is a leak only where a branch put it. Scanned, from BASE (default: the merge base of
# origin/main and HEAD) to the working tree:
#   - every added line of every changed file (`git diff -U0`), committed or not;
#   - every line of a new file that is not yet tracked (a new `.cases.toml` is exactly that);
#   - the commit messages of BASE..HEAD, as one more file.
# The scrubber is outside the repo, because its patterns are the names: THESEUS_SCRUB_CHECK, default
# ~/reports/theseus-docs-v076/scrub_check.py. It runs with --strict, so a hit in a blocking family (the employer,
# the vault's names, a person's name, the configured `[privacy] sensitive_names`) exits 1. It prints every family's
# count, and for a failure the file:line of each blocking hit and never the match: a name must not reach a log.
#
# Exit: 0 clean; 1 a hit; 2 no scrubber, no base, or the configured names are unreadable (it never skips); 3 nothing
# added, which is not a pass (a wrong base looks the same), so the caller says it chose to accept it.
# SCRUB_REPO: the repo to scan (default: here). SCRUB_EXTRA_NAMES and THESEUS_CONFIG: see scrub_check.py.
set -u
scrubber=${THESEUS_SCRUB_CHECK:-$HOME/reports/theseus-docs-v076/scrub_check.py}
cd "${SCRUB_REPO:-.}" || exit 2
if [ ! -f "$scrubber" ]; then
  echo "scrub-added: no scrubber at $scrubber (THESEUS_SCRUB_CHECK); its patterns are names, so it is not in the repo"
  exit 2
fi
base=${1:-$(git merge-base origin/main HEAD 2>/dev/null)}
if [ -z "$base" ] || ! git rev-parse -q --verify "$base^{commit}" >/dev/null; then
  echo "scrub-added: no base commit (pass one, or fetch origin/main)"
  exit 2
fi
work=$(mktemp -d) || exit 2
trap 'rm -rf "$work"' EXIT
files=()
added() { git diff -U0 --no-color "$base" -- "$1" | awk '/^@@/ { h = 1; next } h && /^\+/ { print substr($0, 2) }'; }
while IFS= read -r f; do
  mkdir -p "$work/$(dirname "$f")"
  added "$f" >"$work/$f"
  files+=("$f")
done < <(git diff --name-only --diff-filter=AMR "$base")
while IFS= read -r f; do
  [ -f "$f" ] || continue
  mkdir -p "$work/$(dirname "$f")"
  cp "$f" "$work/$f"
  files+=("$f")
done < <(git ls-files --others --exclude-standard)
git log --format=%B "$base..HEAD" >"$work/.commit-messages"
lines=$(cd "$work" && { [ "${#files[@]}" -eq 0 ] || cat -- "${files[@]}"; } | wc -l)
msgs=$(wc -l <"$work/.commit-messages")
echo "scrub-added: ${#files[@]} file(s), $lines added line(s), $msgs commit-message line(s), base $(git rev-parse --short "$base")"
files+=(.commit-messages)
if [ "$((lines + msgs))" -eq 0 ]; then
  echo "scrub-added: scanned nothing: no added lines and no commit messages since base (a wrong base looks the same)"
  exit 3
fi
export SCRUB_REQUIRE_NAMES=${SCRUB_REQUIRE_NAMES:-1}
cd "$work" || exit 2
python3 "$scrubber" --strict --out "$work/.hits" --files "${files[@]}"
code=$?
if [ "$code" -ne 0 ] && [ -s "$work/.hits" ]; then
  echo "scrub-added: where, in a blocking family (file:line; the match is not printed):"
  grep '^! ' "$work/.hits" | sed -E 's/^! (\[[^]]*\] [^:]*:[0-9]+):.*/\1/' | head -40
fi
exit "$code"
