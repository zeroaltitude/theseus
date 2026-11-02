#!/usr/bin/env bash
# scrub-added.sh [BASE] (theseus-ev3ir): the sensitive-name scrub over what a branch ADDS, as a gate phase.
#
# The unit is the added lines, not the changed files: whole files drown in the example config's `op://`
# placeholders, and a name is a leak only where a branch put it. Scanned, from BASE (default: the merge base of
# origin/main and HEAD) to the working tree:
#   - every added line of every changed file (`git diff -U0`), committed or not;
#   - every line of a new file that is not yet tracked (a new `.cases.toml` is exactly that);
#   - the commit messages of BASE..HEAD, and, at a staged join (`merge --no-ff --no-commit`, where HEAD is still main
#     and the branch's commits are only in MERGE_HEAD), of HEAD..MERGE_HEAD too, as one more file.
# The scrubber is outside the repo, because its patterns are the names: THESEUS_SCRUB_CHECK, default
# ~/reports/theseus-docs-v076/scrub_check.py. It runs with --strict, so a hit in a blocking family (the employer,
# the vault's names, a person's name, the configured `[privacy] sensitive_names`) exits 1. It prints every family's
# count, and for a failure the SOURCE file:line of each blocking hit (a message hit: its commit and line) and never
# the match: a name must not reach a log.
#
# Exit: 0 clean; 1 a hit; 2 no base, or the configured names are unreadable (a scrubber that is here must work);
# 3 nothing added, which is not a pass (a wrong base looks the same), so the caller says it chose to accept it;
# 4 no scrubber on this host: only the WSL host has it, so the gate skips loudly (scripts/gate.sh, scrub_added) and the
# join-time scrub on that host is the guard.
# SCRUB_REPO: the repo to scan (default: here). SCRUB_EXTRA_NAMES and THESEUS_CONFIG: see scrub_check.py.
set -u
scrubber=${THESEUS_SCRUB_CHECK:-$HOME/reports/theseus-docs-v076/scrub_check.py}
cd "${SCRUB_REPO:-.}" || exit 2
if [ ! -f "$scrubber" ]; then
  echo "scrub-added: no scrubber at $scrubber (THESEUS_SCRUB_CHECK); its patterns are names, so it is not in the repo"
  exit 4
fi
base=${1:-$(git merge-base origin/main HEAD 2>/dev/null)}
if [ -z "$base" ] || ! git rev-parse -q --verify "$base^{commit}" >/dev/null; then
  echo "scrub-added: no base commit (pass one, or fetch origin/main)"
  exit 2
fi
work=$(mktemp -d) || exit 2
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/.maps"
files=()
# Each extract has a map beside it, one source line number per extract line, so a hit is reported where a reviewer
# opens it and not at its place in the extract.
added() {
  git -c core.quotePath=false diff -U0 --no-color "$base" -- "$1" |
    awk -v map="$work/.maps/$1" '/^@@/ { match($0, /\+[0-9]+/); n = substr($0, RSTART + 1, RLENGTH - 1) + 0; h = 1; next }
      h && /^\+/ { print substr($0, 2); print n++ >map }'
}
# NUL-separated, so a path with a non-ASCII byte (which git quotes) or a space is read as it is on disk.
while IFS= read -r -d '' f; do
  mkdir -p "$work/$(dirname -- "$f")" "$work/.maps/$(dirname -- "$f")"
  added "$f" >"$work/$f"
  [ -e "$work/.maps/$f" ] || : >"$work/.maps/$f"
  files+=("$f")
done < <(git diff -z --name-only --diff-filter=AMR "$base")
while IFS= read -r -d '' f; do
  [ -f "$f" ] || continue
  mkdir -p "$work/$(dirname -- "$f")" "$work/.maps/$(dirname -- "$f")"
  cp -- "$f" "$work/$f"
  awk '{ print NR }' "$f" >"$work/.maps/$f"
  files+=("$f")
done < <(git ls-files -z --others --exclude-standard)
# The messages of every commit this tree is on its way to carrying: base..HEAD, and at a staged join the branch's
# commits, which only MERGE_HEAD reaches. `gate && git commit` leaves the last commit's message to the join's gate.
revs=$(git rev-list --reverse "$base..HEAD"; if git rev-parse -q --verify MERGE_HEAD >/dev/null; then git rev-list --reverse HEAD..MERGE_HEAD; fi)
for c in $revs; do
  git log -1 --format=%B "$c" >>"$work/.commit-messages"
  git log -1 --format=%B "$c" | awk -v c="$(git rev-parse --short "$c")" '{ print "commit " c " message line " NR }' >>"$work/.maps/.commit-messages"
done
: >>"$work/.commit-messages"
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
  echo "scrub-added: where, in a blocking family (source file:line, or commit and message line; the match is not printed):"
  python3 -I - "$work" <<'PY' | head -40
import re, sys
work = sys.argv[1]
for line in open(work + "/.hits", encoding="utf-8"):
    m = re.match(r"! \[([^\]]*)\] (.*?):(\d+): ", line)
    if not m:
        continue
    fam, path, n = m.group(1), m.group(2), int(m.group(3))
    try:
        mapped = open(f"{work}/.maps/{path}", encoding="utf-8").read().split("\n")[n - 1]
    except (OSError, IndexError):
        mapped = ""
    print(f"[{fam}] {mapped}" if path == ".commit-messages" and mapped else f"[{fam}] {path}:{mapped or f'extract line {n}'}")
PY
fi
exit "$code"
