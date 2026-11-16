#!/usr/bin/env python3
"""Name the owners of a heap profile's live bytes (malloc_prof.c): by the first
Theseus frame of each sampled stack, and by the allocation's own type (the
innermost frame that names one).

  heap_owners.py --exe theseusd PROFILE.txt [OTHER.txt] [--top 25]

With two profiles it prints the second less the first, which is what an import
left behind. Run it where the binary the daemon ran is (it calls addr2line on it).
"""
import argparse, collections, re, subprocess

PLUMBING = re.compile(r"^(alloc::|<alloc::|core::|<core::|std::|__rust|malloc_prof|hashbrown::|<hashbrown|"
                      r"tokio::runtime::|<T as|_start|__libc|start_thread|clone)")


def load(path):
    rows = []
    for line in open(path):
        head, _, frames = line.partition("|")
        live, count, total = (float(x) for x in head.split())
        rows.append((live, count, total, frames.split()))
    return rows


def symbolize(exe, rows):
    want = collections.OrderedDict()
    for _, _, _, frames in rows:
        for f in frames:
            m = re.match(r"(.*)\+(0x[0-9a-f]+)$", f)
            if m and m.group(1).endswith(exe):
                want[f] = m.group(2)
    names = {}
    if want:
        # An address a call returns to lies after the call: look one byte before it.
        offs = [hex(int(o, 16) - 1) for o in want.values()]
        out = subprocess.run(["addr2line", "-e", exe, "-f", "-C", "-i"] + offs, capture_output=True, text=True).stdout
        # addr2line -i prints for each address its frames, innermost first, as function / file:line pairs.
        # It does not mark where one address ends, so ask for one at a time when inlining matters.
        funcs = []
        for o in offs:
            r = subprocess.run(["addr2line", "-e", exe, "-f", "-C", "-i", o], capture_output=True, text=True).stdout.splitlines()
            funcs.append([r[i] for i in range(0, len(r), 2)])
        for k, fs in zip(want, funcs):
            names[k] = fs
    return names


def owner(frames, names):
    """(the first Theseus function, the innermost generic type named in the plumbing)."""
    chain = []
    for f in frames:
        chain.extend(names.get(f, [f.split("+")[0].rsplit("/", 1)[-1] + "+" + f.split("+")[-1]]))
    mine = next((c for c in chain if re.match(r"^(<)?theseus", c) or "theseus_" in c.split("(")[0]), None)
    typ = next((re.search(r"<([^<>]*(?:<[^<>]*>)?[^<>]*)>", c).group(1) for c in chain
                if re.match(r"^(alloc::|<alloc::)", c) and re.search(r"<(.*)>", c)), "?")
    first = next((c for c in chain if not PLUMBING.match(c)), chain[0] if chain else "?")
    return (mine or first)[:100], typ[:70]


def tally(exe, rows, names):
    by_fn, by_type = collections.Counter(), collections.Counter()
    for live, _, _, frames in rows:
        fn, typ = owner(frames, names)
        by_fn[fn] += live
        by_type[typ] += live
    return by_fn, by_type


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("profile")
    ap.add_argument("other", nargs="?")
    ap.add_argument("--exe", required=True)
    ap.add_argument("--top", type=int, default=25)
    a = ap.parse_args()
    one = load(a.profile)
    two = load(a.other) if a.other else None
    names = symbolize(a.exe, one + (two or []))
    fn1, ty1 = tally(a.exe, one, names)
    if two:
        fn2, ty2 = tally(a.exe, two, names)
        fn = collections.Counter({k: fn2.get(k, 0) - fn1.get(k, 0) for k in set(fn1) | set(fn2)})
        ty = collections.Counter({k: ty2.get(k, 0) - ty1.get(k, 0) for k in set(ty1) | set(ty2)})
        print(f"live total: {sum(r[0] for r in one) / 1048576:.1f} MiB then, {sum(r[0] for r in two) / 1048576:.1f} MiB now")
    else:
        fn, ty = fn1, ty1
        print(f"live total: {sum(r[0] for r in one) / 1048576:.1f} MiB")
    print("-- by the first Theseus function on the stack (MiB):")
    for k, v in fn.most_common(a.top):
        print(f"{v / 1048576:8.2f}  {k}")
    print("-- by the allocated type, as the allocator's own frame names it (MiB):")
    for k, v in ty.most_common(a.top):
        print(f"{v / 1048576:8.2f}  {k}")


if __name__ == "__main__":
    main()
