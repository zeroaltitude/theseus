#!/usr/bin/env python3
"""The hand image without Docker (build.sh --rootless; theseus-ongv).

    rootless.py build --dockerfile F --context DIR --work W     the layers, in W, and the image's smoke run
    rootless.py assemble --work W (--tag REF | --output TAR)    the image: pushed to REF, or saved as TAR

It builds the Dockerfile beside it, reading it here: FROM (pinned by digest), ARG, RUN (shell form), COPY (one
file from the context, with --chmod), ENV, USER, WORKDIR, ENTRYPOINT and CMD (JSON form). Any other instruction is
refused. Every RUN comes before every COPY, and the RUNs make one layer.

- The base is the FROM image's linux/amd64 filesystem (`crane export`). The RUN steps run in it as root in a user
  namespace (`unshare --map-root-user`), chrooted, with /dev's common nodes and /proc mounted, the host's
  resolv.conf, and apt's sandbox user set to root. A namespace that maps one id refuses a chown to any other, so
  rootless-owners.c (built here with `cc`) records each chown by inode and lets it pass.
- The RUN layer is what changed against the base: new and changed files, with whiteouts for removed ones. A file's
  owner is the one its inode was last given, else the base's, else root (a base path keeps the base's ids unless
  one was set to another id). /proc, /dev, resolv.conf and this build's
  own files are left out.
- Each COPY is a layer of its own, the file owned by root.
- The smoke run is build.sh's: the image's entry point with `hand` must say THESEUS_HAND is not set. It runs as root
  in the namespace: one id is mapped, so not as the image's USER.
- The image is the base, the layers, and the Dockerfile's config, made by `crane mutate` (CRANE names the binary).
  Pushing uses the registry credentials crane finds (build.sh logs it in to ECR in a scratch DOCKER_CONFIG).
"""
import argparse
import gzip
import hashlib
import json
import os
import shlex
import shutil
import stat
import subprocess
import sys
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
CRANE = os.environ.get("CRANE", "crane")
PLATFORM = "linux/amd64"
OWN = ".theseus-rootless"
DEVICES = ("null", "zero", "random", "urandom", "tty")
# Left out of the RUN layer: paths, with what is under them, and directories whose contents alone are left out.
SKIP = (OWN, "etc/resolv.conf")
SKIP_UNDER = ("proc", "dev")
HAND_LINE = "THESEUS_HAND is not set"


class Refused(Exception):
    pass


def instructions(text):
    """The Dockerfile's instructions as (KEYWORD, rest), its continuations joined and comments dropped."""
    out, cur = [], None
    for raw in text.splitlines():
        line = raw.rstrip()
        if line.lstrip().startswith("#") or (cur is None and not line.strip()):
            continue
        if cur is None:
            cur = line.lstrip()
        else:
            cur += line
        if cur.endswith("\\"):
            cur = cur[:-1]
            continue
        word, _, rest = cur.partition(" ")
        out.append((word.upper(), rest.strip()))
        cur = None
    if cur is not None:
        raise Refused("the Dockerfile ends inside a continued line")
    return out


def parse(text):
    """The recipe the Dockerfile gives, or Refused naming what this build does not do."""
    r = {"base": None, "args": {}, "runs": [], "copies": [], "env": [], "user": None, "workdir": None,
         "entrypoint": None, "cmd": None}
    for word, rest in instructions(text):
        if word == "FROM":
            if r["base"] is not None or " " in rest or "@sha256:" not in rest:
                raise Refused(f"FROM {rest}: one FROM, pinned by digest, with no stage name")
            r["base"] = rest
        elif word == "ARG":
            name, eq, value = rest.partition("=")
            if not eq or not name.isidentifier() or any(c in value for c in "\"' $"):
                raise Refused(f"ARG {rest}: a NAME=value with a plain value")
            r["args"][name] = value
        elif word == "RUN":
            if rest.startswith("[") or rest.startswith("--"):
                raise Refused(f"RUN {rest[:40]}: the shell form, with no flags")
            if r["copies"]:
                raise Refused("a RUN after a COPY")
            r["runs"].append(rest)
        elif word == "COPY":
            parts = rest.split()
            mode = None
            if parts and parts[0].startswith("--chmod="):
                mode = int(parts.pop(0).split("=", 1)[1], 8)
            if len(parts) != 2 or any(p.startswith("--") for p in parts) or "/" in parts[0]:
                raise Refused(f"COPY {rest}: one file of the context's top level, to a path, with --chmod alone")
            r["copies"].append({"src": parts[0], "dest": parts[1], "mode": mode})
        elif word == "ENV":
            for pair in rest.split():
                k, eq, v = pair.partition("=")
                if not eq or not k.isidentifier() or any(c in v for c in "\"'$\\"):
                    raise Refused(f"ENV {rest}: KEY=value pairs with plain values")
                r["env"].append([k, v])
        elif word in ("USER", "WORKDIR"):
            if " " in rest or not rest:
                raise Refused(f"{word} {rest}: one word")
            r[word.lower()] = rest
        elif word in ("ENTRYPOINT", "CMD"):
            try:
                v = json.loads(rest)
            except json.JSONDecodeError:
                v = None
            if not (isinstance(v, list) and v and all(isinstance(s, str) and "," not in s for s in v)):
                raise Refused(f"{word} {rest}: the JSON form, with no commas in its words")
            r[word.lower()] = v
        else:
            raise Refused(f"{word}: not an instruction this build knows")
    if r["base"] is None:
        raise Refused("no FROM")
    return r


def skipped(rel):
    return any(rel == p or rel.startswith(p + "/") for p in SKIP) or any(rel.startswith(p + "/") for p in SKIP_UNDER)


def norm(name):
    """A tar member's path relative to the root: `./etc/` and `etc` are both `etc`, and the root itself is ``."""
    name = name.strip("/")
    while name == "." or name.startswith("./"):
        name = name[2:].lstrip("/")
    return name


def base_index(base_tar):
    """Each path of the base's filesystem: its type, mode, owner, symlink target and content hash."""
    idx, links = {}, {}
    with tarfile.open(base_tar) as t:
        for m in t:
            rel = norm(m.name)
            if not rel:
                continue
            e = {"mode": m.mode & 0o7777, "uid": m.uid, "gid": m.gid}
            if m.isdir():
                e["type"] = "dir"
            elif m.issym():
                e["type"], e["target"] = "sym", m.linkname
            elif m.islnk():
                links[rel] = norm(m.linkname)
                e["type"] = "reg"
            elif m.isreg():
                e["type"], e["size"] = "reg", m.size
                e["sha"] = hashlib.sha256(t.extractfile(m).read()).hexdigest()
            else:
                e["type"] = "other"
            idx[rel] = e
    for rel, target in links.items():
        if target in idx:
            idx[rel].update(size=idx[target].get("size"), sha=idx[target].get("sha"))
    return idx


def owners(log):
    """{(dev, ino): (uid, gid)} from the shim's records, each -1 left at the earlier value (None when unknown)."""
    rec = {}
    if not log.exists():
        return rec
    for line in log.read_text().splitlines():
        dev, ino, uid, gid = (int(x) for x in line.split())
        old = rec.get((dev, ino), (None, None))
        rec[(dev, ino)] = (old[0] if uid == -1 else uid, old[1] if gid == -1 else gid)
    return rec


def sha_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def kind(st):
    if stat.S_ISDIR(st.st_mode):
        return "dir"
    if stat.S_ISLNK(st.st_mode):
        return "sym"
    if stat.S_ISREG(st.st_mode):
        return "reg"
    return "other"


def diff(base, rootfs, recorded):
    """What the RUN layer holds: {rel: (lstat, uid, gid)} for new or changed paths and their parents, and the
    whiteouts, a sorted list of removed paths whose parents remain."""
    now = {}
    for dirpath, dirnames, filenames in os.walk(rootfs):
        for name in sorted(dirnames + filenames):
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, rootfs)
            if skipped(rel):
                continue
            now[rel] = os.lstat(full)
        dirnames[:] = [d for d in dirnames if not skipped(os.path.relpath(os.path.join(dirpath, d), rootfs))]

    def owner(rel, st):
        # The base was unpacked with every file the caller's, so a tool that copies a file's owner to its
        # replacement (useradd rewriting /etc/shadow, root:shadow) records root: a base path keeps the base's ids
        # unless one was set to another id.
        b = base.get(rel)
        uid, gid = (b["uid"], b["gid"]) if b else (0, 0)
        r = recorded.get((st.st_dev, st.st_ino))
        if r:
            uid = r[0] if r[0] or (r[0] == 0 and not b) else uid
            gid = r[1] if r[1] or (r[1] == 0 and not b) else gid
        return uid, gid

    keep = {}
    for rel, st in now.items():
        b = base.get(rel)
        uid, gid = owner(rel, st)
        k = kind(st)
        if k == "other":
            raise Refused(f"{rel}: the RUN step made a special file")
        changed = (b is None or b["type"] != k or b["mode"] != stat.S_IMODE(st.st_mode)
                   or (b["uid"], b["gid"]) != (uid, gid))
        if not changed and k == "reg":
            changed = b.get("size") != st.st_size or b.get("sha") != sha_of(os.path.join(rootfs, rel))
        if not changed and k == "sym":
            changed = b.get("target") != os.readlink(os.path.join(rootfs, rel))
        if changed:
            keep[rel] = (st, uid, gid)
    for rel in list(keep):
        parent = os.path.dirname(rel)
        while parent and parent not in keep:
            st = now[parent]
            keep[parent] = (st, *owner(parent, st))
            parent = os.path.dirname(parent)
    gone = sorted(rel for rel in base if not skipped(rel) and rel not in now)
    gone_set = set(gone)
    whiteouts = [rel for rel in gone if os.path.dirname(rel) not in gone_set]
    for rel in whiteouts:
        parent = os.path.dirname(rel)
        while parent and parent not in keep:
            st = now[parent]
            keep[parent] = (st, *owner(parent, st))
            parent = os.path.dirname(parent)
    return keep, whiteouts


def write_layer(out, rootfs, keep, whiteouts):
    """A gzipped tar layer, its entries sorted, files that share an inode written once and linked after."""
    entries = sorted(list(keep) + [os.path.join(os.path.dirname(w), ".wh." + os.path.basename(w)) for w in whiteouts])
    first = {}
    with open(out, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as gz, \
            tarfile.open(fileobj=gz, mode="w|", format=tarfile.PAX_FORMAT) as t:
        for rel in entries:
            if rel not in keep:
                ti = tarfile.TarInfo(rel)
                ti.mode, ti.mtime = 0o644, 0
                t.addfile(ti)
                continue
            st, uid, gid = keep[rel]
            full = os.path.join(rootfs, rel)
            ti = tarfile.TarInfo(rel + "/" if stat.S_ISDIR(st.st_mode) else rel)
            ti.mode, ti.uid, ti.gid, ti.mtime = stat.S_IMODE(st.st_mode), uid, gid, int(st.st_mtime)
            if stat.S_ISDIR(st.st_mode):
                ti.type = tarfile.DIRTYPE
                t.addfile(ti)
            elif stat.S_ISLNK(st.st_mode):
                ti.type, ti.linkname = tarfile.SYMTYPE, os.readlink(full)
                t.addfile(ti)
            elif (st.st_dev, st.st_ino) in first and st.st_nlink > 1:
                ti.type, ti.linkname = tarfile.LNKTYPE, first[(st.st_dev, st.st_ino)]
                t.addfile(ti)
            else:
                first[(st.st_dev, st.st_ino)] = rel
                ti.size = st.st_size
                with open(full, "rb") as f:
                    t.addfile(ti, f)


def copy_layer(out, rootfs, src, dest, mode):
    """A COPY's layer: the destination's parent directories as the image has them, then the file, owned by root."""
    rel = dest.strip("/")
    parents, p = [], os.path.dirname(rel)
    while p:
        parents.append(p)
        p = os.path.dirname(p)
    with open(out, "wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as gz, \
            tarfile.open(fileobj=gz, mode="w|", format=tarfile.PAX_FORMAT) as t:
        for p in reversed(parents):
            st = os.lstat(os.path.join(rootfs, p))
            ti = tarfile.TarInfo(p + "/")
            ti.type, ti.mode, ti.mtime = tarfile.DIRTYPE, stat.S_IMODE(st.st_mode), int(st.st_mtime)
            t.addfile(ti)
        st = os.stat(src)
        ti = tarfile.TarInfo(rel)
        ti.mode = mode if mode is not None else stat.S_IMODE(st.st_mode)
        ti.size, ti.mtime = st.st_size, int(st.st_mtime)
        with open(src, "rb") as f:
            t.addfile(ti, f)


def run(args, **kw):
    r = subprocess.run(args, **kw)
    if r.returncode != 0:
        sys.exit(f"rootless: {shlex.join(str(a) for a in args[:4])} … exited {r.returncode}")
    return r


def image_env(base):
    out = run([CRANE, "config", "--platform", PLATFORM, base], capture_output=True, text=True).stdout
    return json.loads(out).get("config", {}).get("Env") or []


def namespace(work, step, *extra):
    """Run this script's `inside` STEP in a user namespace of its own (root mapped to the caller)."""
    run(["unshare", "--map-root-user", "--mount", "--pid", "--fork",
         sys.executable, "-I", str(Path(__file__).resolve()), "inside", step, str(work), *extra])


def inside(step, work):
    """In the namespace: mount the chroot's /dev nodes and /proc, then run the RUN steps or the smoke run."""
    work = Path(work)
    rootfs = work / "rootfs"
    recipe = json.loads((work / "recipe.json").read_text())
    for d in DEVICES:
        node = rootfs / "dev" / d
        node.unlink(missing_ok=True)
        node.touch()
        run(["mount", "--bind", f"/dev/{d}", str(node)])
    run(["mount", "-t", "proc", "proc", str(rootfs / "proc")])

    def chrooted(argv, env, cwd="/", **kw):
        def enter():
            os.chroot(rootfs)
            os.chdir(cwd)
        return subprocess.run(argv, env=env, preexec_fn=enter, **kw)

    base_env = dict(e.split("=", 1) for e in recipe["base_env"])
    if step == "run":
        shutil.copy("/etc/resolv.conf", rootfs / "etc" / "resolv.conf")
        conf = rootfs / "etc" / "apt" / "apt.conf.d"
        if conf.is_dir():
            (conf / "00theseus-rootless").write_text('APT::Sandbox::User "root";\n')
        env = {"HOME": "/root", **base_env, **recipe["args"],
               "LD_PRELOAD": f"/{OWN}/owners.so", "THESEUS_OWNERS_LOG": f"/{OWN}/owners.log"}
        try:
            for body in recipe["runs"]:
                r = chrooted(["/bin/sh", "-c", body], env, stdin=subprocess.DEVNULL)
                if r.returncode != 0:
                    sys.exit(f"rootless: a RUN step exited {r.returncode}")
        finally:
            (conf / "00theseus-rootless").unlink(missing_ok=True)
    elif step == "smoke":
        env = {"HOME": "/root", **base_env, **dict(recipe["env"])}
        argv = (recipe["entrypoint"] or []) + ["hand"]
        r = chrooted(argv, env, cwd=recipe["workdir"] or "/", capture_output=True, text=True)
        (work / "smoke.txt").write_text(r.stdout + r.stderr)


def build(a):
    work = Path(a.work)
    recipe = parse(Path(a.dockerfile).read_text())
    rootfs = work / "rootfs"
    if rootfs.exists():
        sys.exit(f"rootless: {rootfs} exists; the work directory is fresh or not used")
    (work / "layers").mkdir(parents=True)
    rootfs.mkdir()
    base_tar = work / "base.tar"
    run([CRANE, "export", "--platform", PLATFORM, recipe["base"], str(base_tar)])
    recipe["base_env"] = image_env(recipe["base"])
    (work / "recipe.json").write_text(json.dumps(recipe))
    run(["unshare", "--map-root-user", "tar", "-xpf", str(base_tar), "-C", str(rootfs), "--no-same-owner"])
    (rootfs / OWN).mkdir()
    run(["cc", "-shared", "-fPIC", "-O2", "-Wall", "-o", str(rootfs / OWN / "owners.so"),
         str(HERE / "rootless-owners.c"), "-ldl"])
    if recipe["runs"]:
        namespace(work, "run")
    keep, whiteouts = diff(base_index(base_tar), str(rootfs), owners(rootfs / OWN / "owners.log"))
    layers = []
    if recipe["runs"]:
        layers.append(str(work / "layers" / "run.tar.gz"))
        write_layer(layers[-1], str(rootfs), keep, whiteouts)
        print(f"rootless: the RUN layer holds {len(keep)} paths and {len(whiteouts)} whiteouts")
    for i, c in enumerate(recipe["copies"]):
        src = Path(a.context) / c["src"]
        layers.append(str(work / "layers" / f"copy-{i}.tar.gz"))
        copy_layer(layers[-1], str(rootfs), str(src), c["dest"], c["mode"])
        dest = rootfs / c["dest"].strip("/")
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(src, dest)
        if c["mode"] is not None:
            dest.chmod(c["mode"])
    recipe["layers"] = layers
    (work / "recipe.json").write_text(json.dumps(recipe))
    namespace(work, "smoke")
    said = (work / "smoke.txt").read_text()
    if HAND_LINE not in said:
        sys.exit(f"rootless: the image does not run theseusd hand: {said.strip()[:300]}")
    print("rootless: the image answers as a hand")


def assemble(a):
    recipe = json.loads((Path(a.work) / "recipe.json").read_text())
    args = [CRANE, "mutate", "--platform", PLATFORM, recipe["base"]]
    for layer in recipe["layers"]:
        args += ["--append", layer]
    for k, v in recipe["env"]:
        args += ["--env", f"{k}={v}"]
    if recipe["entrypoint"]:
        args += ["--entrypoint", ",".join(recipe["entrypoint"])]
    if recipe["cmd"]:
        args += ["--cmd", ",".join(recipe["cmd"])]
    if recipe["user"]:
        args += ["--user", recipe["user"]]
    if recipe["workdir"]:
        args += ["--workdir", recipe["workdir"]]
    args += ["--set-platform", PLATFORM]
    args += ["--tag", a.tag] if a.tag else ["--tag", "theseus/hand:local", "--output", a.output]
    run(args, stdout=subprocess.DEVNULL)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--dockerfile", default=str(HERE / "Dockerfile"))
    b.add_argument("--context", required=True)
    b.add_argument("--work", required=True)
    s = sub.add_parser("assemble")
    s.add_argument("--work", required=True)
    g = s.add_mutually_exclusive_group(required=True)
    g.add_argument("--tag")
    g.add_argument("--output")
    i = sub.add_parser("inside")
    i.add_argument("step", choices=["run", "smoke"])
    i.add_argument("work")
    a = ap.parse_args()
    try:
        if a.cmd == "build":
            build(a)
        elif a.cmd == "assemble":
            assemble(a)
        else:
            inside(a.step, a.work)
    except Refused as e:
        sys.exit(f"rootless: {e}")


if __name__ == "__main__":
    main()
