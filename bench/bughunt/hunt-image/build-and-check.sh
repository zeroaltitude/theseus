#!/bin/bash
# Rebuild the Bug Hunt hunt image with normalized mtimes and gate it (theseus-8qx1i). On the ARM host,
# as a lane unit from the main machine:
#   arm.sh send p3-bench5 lane/p3-bench5
#   arm.sh run  p3-bench5 sh $PWD/bench/bughunt/hunt-image/build-and-check.sh   # a local path: arm.sh copies it
#   arm.sh wait p3-bench5 sh 100      # repeat until it prints a summary line
# $0 model calls, no network for the checks. The candidate is built under its own tag and takes the live tag
# (what task.toml's docker_image names) only when every check passes; the old image is kept as :pre-mtime.
set -u
# A lane unit runs under the user manager, which may not carry the docker group a login has.
if ! docker info > /dev/null 2>&1 && [ -z "${HUNT_IN_SG:-}" ]; then
    HUNT_IN_SG=1 exec sg docker -c "bash $(printf '%q' "$0")"
fi
W=$HOME/bughunt
LANE=${LANE_DIR:-$HOME/lanes/p3-bench5}/bench/bughunt/hunt-image
LIVE=bughunt-rs-hunt-core:0.1-pilot
CAND=bughunt-rs-hunt-core:0.1-pilot-mtime
T=$W/pilot-harbor/bughunt-rs/hunt-core/tests
OUT=$W/mtime-check
rm -rf "$OUT"; mkdir -p "$OUT/seeded" "$OUT/fixed"
log() { echo "$(date -Is) $*"; }
die() { log "FAIL: $*"; exit 1; }

log "build $CAND from $W/ctx with $LANE/Dockerfile"
nice -n 10 docker build -t "$CAND" -f "$LANE/Dockerfile" "$W/ctx" || die "docker build"

log "mtime check (no network)"
docker run --rm --network none -v "$LANE/mtime-check.sh:/mtime-check.sh:ro" "$CAND" bash /mtime-check.sh \
    | tee "$OUT/mtime-check.log" | tail -20
[ "${PIPESTATUS[0]}" = 0 ] || die "mtime-check"

# The old image, for the same check: it must FAIL there (a check that passes on the leak is not a check).
if docker image inspect "$LIVE" > /dev/null 2>&1; then
    if docker run --rm --network none -v "$LANE/mtime-check.sh:/mtime-check.sh:ro" "$LIVE" bash /mtime-check.sh \
        > "$OUT/mtime-check-old.log" 2>&1; then die "mtime-check passed on the old image: the check is blind"; fi
    log "the old image fails the check, as it should: $(tail -1 "$OUT/mtime-check-old.log")"
fi

log "cargo still sees the warm target/ fresh (offline, must not compile a crate)"
docker run --rm --network none --cpus 4 -m 10g "$CAND" bash -c \
    'cargo nextest run --locked -p theseus-store -p theseus-kernel -p theseus-tools --no-run 2>&1 | tail -5' \
    | tee "$OUT/fresh.log"
if grep -q "Compiling" "$OUT/fresh.log"; then log "NOTE: cargo recompiled after the touch (see fresh.log)"; fi

log "grader self-check: seeded must score 0/13, fixed 13/13"
docker run --rm --network none --cpus 4 -m 10g -v "$T:/tests:ro" -v "$OUT/seeded:/logs/verifier" "$CAND" \
    bash /tests/test.sh > "$OUT/grader-seeded.log" 2>&1
s=$(cat "$OUT/seeded/reward.txt" 2>/dev/null)
docker run --rm --network none --cpus 4 -m 10g -v "$T:/tests:ro" -v "$OUT/fixed:/logs/verifier" \
    -v "$W/fixed-crates:/fixed-crates:ro" "$CAND" bash -c \
    'for c in theseus-store theseus-kernel theseus-tools; do rm -rf /app/crates/$c/src && cp -a /fixed-crates/$c/src /app/crates/$c/src; done; bash /tests/test.sh' \
    > "$OUT/grader-fixed.log" 2>&1
f=$(cat "$OUT/fixed/reward.txt" 2>/dev/null)
log "grader: seeded reward=$s (want 0), fixed reward=$f (want 1)"
case "$s:$f" in 0:1|0.0:1.0|0.0000:1.0000|0.0:1|0:1.0) ;; *) die "grader self-check seeded=$s fixed=$f";; esac

docker tag "$LIVE" bughunt-rs-hunt-core:pre-mtime 2> /dev/null
docker tag "$CAND" "$LIVE" || die "tag"
log "PASS: $LIVE is now the normalized image ($(docker image inspect "$LIVE" --format '{{.Id}}' | cut -c1-19)); old kept as :pre-mtime"
