#!/usr/bin/env python3
"""What makes `theseusd` big (theseus-2m7jg): the top crates by bytes, and the embedded cockpit bundle's.

    scripts/size-breakdown.py [--profile release-thin|release] [--top 20] [--binary PATH] [--package theseusd]

Both profiles strip (`strip = true`), and a stripped binary has no symbols to count, so this builds an unstripped
copy with `CARGO_PROFILE_<PROFILE>_STRIP=false`, which changes nothing else, into its own target directory
(`target/size-breakdown`, so the stripped binaries in `target/<profile>` and their build stamps are left alone).
With `--binary PATH` it reads an unstripped binary you built yourself and builds nothing.

It groups by crate with `cargo bloat --crates` where that is installed, and otherwise from `nm -S --size-sort -C`:
each symbol's size goes to the first path segment of its demangled name (`<crate::Type as Trait>::f` goes to `crate`;
`<T as crate::Trait>::f` to `T`'s crate when `T` is a path, else to the trait's). That is the heuristic
cargo-bloat itself uses, so a generic instantiated in `theseusd` counts to its definer's crate. Bytes of the binary
that no symbol covers (debug-free sections, string data, tables) are one line, `[not in symbols]`.

The cockpit bundle is `crates/theseusd/cockpit/dist` (built by `npm run build` in cockpit/); rust-embed puts those
files in the binary's data as they are, so their summed size is what the binary carries for the web UI.

Python 3, standard library only.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))


def mb(n):
    return f"{n / 1e6:8.2f} MB"


def build(profile, package):
    env = dict(os.environ)
    env["CARGO_PROFILE_" + profile.upper().replace("-", "_") + "_STRIP"] = "false"
    env["CARGO_TARGET_DIR"] = os.path.join(ROOT, "target", "size-breakdown")
    cmd = ["cargo", "build", "--locked", "--profile", profile, "-p", package]
    print("size-breakdown: " + " ".join(cmd), file=sys.stderr)
    subprocess.run(cmd, cwd=ROOT, env=env, check=True)
    return os.path.join(env["CARGO_TARGET_DIR"], profile, package)


SKIP = re.compile(r"^(\[|\.|_ZN|<\w+ as |dyn )")


def crate_of(sym):
    """The crate a demangled symbol belongs to."""
    s = sym.strip()
    # drop a leading `<` (impl blocks and trait casts) and `&`/`*` decorations
    while s.startswith(("<", "&", "*const ", "*mut ", "(", "[")):
        s = s.lstrip("<&").removeprefix("*const ").removeprefix("*mut ").lstrip("([")
    m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)(::|$)", s)
    if not m:
        return "[other]"
    name = m.group(1)
    if name in ("core", "alloc", "std"):
        return "std"
    if name.startswith(("anon", "str", "u8", "slice")):
        return "std" if "::" in s else "[other]"
    return name


def by_nm(binary):
    out = subprocess.run(["nm", "-S", "--size-sort", "-C", binary], capture_output=True, text=True, check=True).stdout
    sizes = {}
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        try:
            size = int(parts[1], 16)
        except ValueError:
            continue
        name = parts[3]
        if parts[2] in "bBsS":  # bss holds no bytes in the file
            continue
        sizes[crate_of(name)] = sizes.get(crate_of(name), 0) + size
    return sizes


def by_bloat(binary, profile, package):
    env = dict(os.environ)
    env["CARGO_PROFILE_" + profile.upper().replace("-", "_") + "_STRIP"] = "false"
    env["CARGO_TARGET_DIR"] = os.path.join(ROOT, "target", "size-breakdown")
    out = subprocess.run(
        ["cargo", "bloat", "--profile", profile, "-p", package, "--crates", "-n", "0", "--message-format", "json"],
        cwd=ROOT, env=env, capture_output=True, text=True,
    )
    if out.returncode != 0:
        return None
    import json

    data = json.loads(out.stdout)
    return {c["name"]: c["size"] for c in data.get("crates", [])}


def stripped_size(binary):
    """The binary's size once stripped, as the profile ships it: the symbol table is not part of what is shipped."""
    if not shutil.which("strip"):
        return None
    tmp = binary + ".stripped"
    try:
        subprocess.run(["strip", "--strip-all", "-o", tmp, binary], check=True, capture_output=True)
        return os.path.getsize(tmp)
    except subprocess.CalledProcessError:
        return None
    finally:
        if os.path.exists(tmp):
            os.remove(tmp)


def dist_bytes():
    dist = os.path.join(ROOT, "crates", "theseusd", "cockpit", "dist")
    total, n = 0, 0
    for d, _, files in os.walk(dist):
        for f in files:
            total += os.path.getsize(os.path.join(d, f))
            n += 1
    return total, n


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--profile", default="release-thin")
    ap.add_argument("--package", default="theseusd")
    ap.add_argument("--top", type=int, default=20)
    ap.add_argument("--binary", help="an unstripped binary to read instead of building one")
    a = ap.parse_args()
    binary = a.binary or build(a.profile, a.package)
    unstripped = os.path.getsize(binary)
    total = stripped_size(binary) or unstripped
    sizes, how = None, "nm -S --size-sort"
    if not a.binary and shutil.which("cargo-bloat"):
        sizes = by_bloat(binary, a.profile, a.package)
        how = "cargo bloat --crates"
    if sizes is None:
        sizes, how = by_nm(binary), "nm -S --size-sort -C, by first path segment"
    covered = sum(sizes.values())
    print(f"{binary}: {unstripped} bytes unstripped, {total} stripped ({mb(total).strip()}, what ships); crates by {how}")
    print(f"  {'crate':<28}{'bytes':>12}  {'MB':>10}  {'share':>6}")
    for name, size in sorted(sizes.items(), key=lambda kv: -kv[1])[: a.top]:
        print(f"  {name:<28}{size:>12}  {mb(size)}  {100 * size / total:5.1f}%")
    rest = total - covered
    print(f"  {'[not in symbols]':<28}{max(rest, 0):>12}  {mb(max(rest, 0))}  {100 * max(rest, 0) / total:5.1f}%")
    d, n = dist_bytes()
    print(f"embedded cockpit bundle (crates/theseusd/cockpit/dist, {n} files): {d} bytes ({mb(d).strip()})")
    print("shares are of the stripped size; a crate's symbols are its code and its own statics, not the data it pulls in without a symbol")


if __name__ == "__main__":
    main()
