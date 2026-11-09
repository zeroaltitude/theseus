#!/usr/bin/env bash
# Reproduces the batch un-ack (theseus-4vsrz, bucket B7): a test deleted on one branch is paired, once a second branch
# is merged beside it, with an unrelated NEW test in another file, so the finding changes rule and file and a trailer
# that named the real file stops covering it. Judges throwaway repositories only; touches nothing in the tree.
#   scripts/keel-batch-repro.sh [path/to/keel-guard.py]     (default: the tree's own)
set -euo pipefail
guard="$(cd "$(dirname "${1:-$(dirname "$0")/keel-guard.py}")" && pwd)/$(basename "${1:-keel-guard.py}")"
d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT
cd "$d"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b main . && mkdir -p routing voice
cat >routing/test_effort.py <<'P'
def test_no_match_is_the_packs():
    pack = load_pack("effort")
    value = pack.declared_no_match()
    assert value == "none"
    assert pack.version == 3
    assert daemon.no_match(pack) == value
P
cat >voice/test_mute.py <<'P'
def test_existing_mute():
    assert True
P
git add -A && git commit -qm base
base=$(git rev-parse HEAD)
git checkout -q -b a && : >routing/test_effort.py && git commit -qam "a: delete the routing test"
git checkout -q main && git checkout -q -b b
cat >>voice/test_mute.py <<'P'

def test_mute_reads_the_pack():
    pack = load_pack("mute")
    value = pack.declared_no_match()
    assert value == "none"
P
git commit -qam "b: add a voice test"
git checkout -q a
run() { python3 "$guard" --repo . --base "$base" --quiet 2>&1 | grep -E '^  |keel: [0-9]|ok' || true; }
echo "== branch a alone (the deleted test's real home)"; run
git checkout -q -b batch && git merge -q --no-edit b
echo "== a and b batched"; run
