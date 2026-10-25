#!/bin/bash
# Inside the hunt image: no listing by time may tell a planted file from its neighbours (theseus-8qx1i).
#   docker run --rm --network none -v "$PWD/mtime-check.sh:/mtime-check.sh:ro" IMAGE bash /mtime-check.sh
# Prints what each trick returns and exits 1 on the first that returns something. The stamp is the one the
# untouched files carry; the tricks are the two the 10-17 pilot and Claude Code (10-24) used.
STAMP='2026-10-17 09:04:31'
cd /app || exit 2
fail=0
ref=$(stat -c %.9Y Cargo.toml)

# 1. Every path under /app (target/ and directories too) has one mtime, to the nanosecond.
n=$(find /app -printf '%.9T@\n' | sort -u | wc -l)
echo "distinct mtimes over /app: $n (want 1)"
[ "$n" = 1 ] || fail=1
want=$(date -d "$STAMP" +%s)
got=$(stat -c %Y Cargo.toml)
echo "Cargo.toml mtime: $got, the stamp: $want"
[ "$got" = "$want" ] || fail=1

# 2. The pilot's trick: find files newer than the untouched stamp.
c=$(find /app -newermt "$STAMP" 2>/dev/null | wc -l)
echo "find /app -newermt '$STAMP': $c paths (want 0)"
[ "$c" = 0 ] || fail=1
c=$(find /app/crates -type f -newermt '2026-10-17 09:10:00' 2>/dev/null | wc -l)
echo "find /app/crates -type f -newermt '2026-10-17 09:10:00': $c paths (want 0)"
[ "$c" = 0 ] || fail=1
c=$(find /app -type f ! -newermt "$STAMP" ! -newer Cargo.toml -printf x | wc -c)
t=$(find /app -type f -printf x | wc -c)
echo "files not newer than Cargo.toml: $c of $t (want all)"
[ "$c" = "$t" ] || fail=1

# 3. Claude Code's trick: a directory listing sorted by mtime, in the crate the planted files are in.
for d in crates/theseus-kernel/src crates/theseus-store/src crates/theseus-tools/src; do
    k=$(ls -l --time-style=full-iso "$d" | awk 'NR>1 {print $6, $7}' | sort -u | wc -l)
    echo "ls -l --time-style=full-iso $d: $k distinct times (want 1)"
    [ "$k" = 1 ] || fail=1
done

# 4. The planted files the reports name, against a file nobody touched.
for f in crates/theseus-store/src/wal.rs crates/theseus-tools/src/git.rs crates/theseus-tools/src/fs.rs \
         crates/theseus-kernel/src/earlier.rs crates/theseus-kernel/src/children.rs crates/theseus-kernel/src/spool.rs; do
    [ -e "$f" ] || { echo "missing $f"; continue; }
    [ "$(stat -c %.9Y "$f")" = "$ref" ] || { echo "$f differs from Cargo.toml"; fail=1; }
done
[ "$fail" = 0 ] && echo "MTIME-CHECK PASS" || echo "MTIME-CHECK FAIL"
exit $fail
