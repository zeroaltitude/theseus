#!/usr/bin/env python3
"""The status page's version line, held to the tree (issue 15's last line; theseus-fly32).

    python3 scripts/status-page.py                       # check; one line per fault, then a summary line
    python3 scripts/status-page.py --tag release-1.0.7   # before tagging: is this the tree's version?

**Why this exists.** `docs/status.md` shipped two releases stale inside a tagged release, because nothing read its
first line; and `release-1.0.5` was tagged on a commit whose workspace version still said 1.0.4, so the binaries of
that release print the version before it. Both are facts the tree holds, and neither was checked.

**The line it reads** is the page's first line that starts `_Updated `, and it must have this shape:

    _Updated <date> [<HH:MM> <zone>]. Workspace version <X>; newest release tag `release-<Y>` (<name>) at `<sha>`, <date>. The design document is at v<D>._

(the time, `11:5x` allowed, and the last sentence may be left out; nothing else may stand anywhere in the line:
free text there is how the page went stale)

Everything in it can be read from the tree when the line is written, and this script reads each one back:

  - `<X>` is `[workspace.package] version` in `Cargo.toml`;
  - `release-<Y>` is the newest `release-*` tag reachable from HEAD, by version, and `<sha>` (8 or more hex digits) a
    prefix of its commit, and the date after it the tag's own date. On the tagged commit itself the page may still
    name the tag before it, because a page is written before its release is tagged: what the tagged tree says is what
    was true when it was written;
  - `<X>` is not older than `<Y>`: a workspace version behind its newest tag is the stale page this exists to stop;
  - `<name>` is the release's name by the rule in `docs/development.md` ("Version cuts and releases"): `v1` for
    1.0.0, `v1 patch<N>` for 1.0.N. Other lines have no naming rule yet, so their name is not checked;
  - `<D>`, when given, is the version in the first heading of `docs/spec/01-front.md`;
  - `<date>` after `_Updated` is not earlier than the tag's date.

**Every reachable release tag is checked against its own tree:** the workspace version at the tagged commit must be
the tag's version, which is what `theseus --version` prints. `release-1.0.5` is the one recorded exception
(`KNOWN_MISTAGGED`): a tag is never moved, so it stays wrong, named here rather than failing every gate.

**Before a tag** (`--tag release-<version>`), the name is held to the tree's workspace version, so a release cannot
be tagged with the version before it. That is the one point where a mistag can be prevented, not only caught.

**A clone with no release tags** (a box that fetched one commit) checks the workspace version line only, and says
`status page: skipped: ...` so the gate registers the stand-down rather than calling it a pass.

Exit status: 0 the line is true (or the tag half was skipped, said so); 1 a fault; 2 the script could not read the
page or the workspace version. Standard library only.
"""
import os
import re
import subprocess
import sys

PAGE = "docs/status.md"
FRONT = "docs/spec/01-front.md"
# A tag is never moved, so a release tagged on the wrong version stays so. Each entry is the version its tree holds.
KNOWN_MISTAGGED = {"release-1.0.5": "1.0.4"}

HEADER = re.compile(
    r"^_Updated (?P<updated>\d{4}-\d{2}-\d{2})(?: \d{1,2}:[0-9x]{2} [A-Z]{2,5})?\. "
    r"Workspace version (?P<workspace>\d+\.\d+\.\d+); "
    r"newest release tag `release-(?P<tag>\d+\.\d+\.\d+)` \((?P<name>[^)]+)\) "
    r"at `(?P<sha>[0-9a-f]{8,40})`, (?P<tagged>\d{4}-\d{2}-\d{2})\."
    r"(?: The design document is at v(?P<design>\d+(?:\.\d+)*)\.)?_$"
)
FRONT_VERSION = re.compile(r"^# The Ship of Theseus — v(\d+(?:\.\d+)*)\s*$", re.M)


class Unreadable(Exception):
    pass


def git(root, *args):
    r = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None


def vt(version):
    return tuple(int(p) for p in version.split("."))


def workspace_version(cargo_toml):
    """`[workspace.package] version` from a Cargo.toml's text, or None."""
    section = None
    for line in cargo_toml.splitlines():
        s = line.strip()
        if s.startswith("["):
            section = s
        elif section == "[workspace.package]":
            m = re.match(r'version\s*=\s*"([^"]+)"', s)
            if m:
                return m.group(1)
    return None


def release_name(version):
    """The release's name by the rule for the 1.0 line, or None where no rule is written."""
    major, minor, patch = vt(version)
    if (major, minor) != (1, 0):
        return None
    return "v1" if patch == 0 else f"v1 patch{patch}"


def release_tags(root):
    """Every `release-X.Y.Z` tag reachable from HEAD, oldest first: (name, version, commit, date)."""
    out = git(root, "for-each-ref", "--merged", "HEAD", "--format=%(refname:short) %(*objectname) %(objectname) "
              "%(creatordate:short)", "refs/tags/release-*")
    tags = []
    for line in (out or "").splitlines():
        name, peeled, obj, date = line.split(" ")
        m = re.fullmatch(r"release-(\d+\.\d+\.\d+)", name)
        if m:
            tags.append((name, m.group(1), peeled or obj, date))
    return sorted(tags, key=lambda t: vt(t[1]))


def check(root):
    """(faults, notes, skipped): what is wrong with the page's line, what was read, and whether the tag half ran."""
    try:
        page = open(os.path.join(root, PAGE), encoding="utf-8").read()
        cargo = open(os.path.join(root, "Cargo.toml"), encoding="utf-8").read()
    except OSError as e:
        raise Unreadable(str(e)) from e
    workspace = workspace_version(cargo)
    if workspace is None:
        raise Unreadable("Cargo.toml has no [workspace.package] version")
    line = next((ln for ln in page.splitlines() if ln.startswith("_Updated ")), None)
    if line is None:
        return [f"{PAGE} has no line starting `_Updated `"], [], False
    m = HEADER.match(line)
    if m is None:
        return [f"{PAGE}'s `_Updated` line does not have the shape this script documents: {line[:200]}"], [], False
    faults = []
    if m["workspace"] != workspace:
        faults.append(f"the page says workspace version {m['workspace']}, Cargo.toml says {workspace}")
    if m["design"]:
        try:
            front = FRONT_VERSION.search(open(os.path.join(root, FRONT), encoding="utf-8").read())
        except OSError:
            front = None
        if front is None:
            faults.append(f"the page names the design document's version, and {FRONT} has no version heading")
        elif front.group(1) != m["design"]:
            faults.append(f"the page says the design document is at v{m['design']}, {FRONT} says v{front.group(1)}")
    tags = release_tags(root)
    if not tags:
        return faults, [f"workspace version {workspace}"], True
    for name, version, commit, _ in tags:
        held = workspace_version(git(root, "show", f"{commit}:Cargo.toml") or "")
        if held != version and KNOWN_MISTAGGED.get(name) != held:
            faults.append(f"{name} is tagged on a tree whose workspace version is {held}: its binaries print {held}")
    newest = tags[-1]
    head = (git(root, "rev-parse", "HEAD") or "").strip()
    allowed = [newest]
    if newest[2] == head and len(tags) > 1:
        allowed.append(tags[-2])
    named = next((t for t in allowed if t[1] == m["tag"]), None)
    if named is None:
        faults.append(f"the page names release-{m['tag']}, and the newest release tag reachable from HEAD is "
                      f"{newest[0]}")
    else:
        if not named[2].startswith(m["sha"]):
            faults.append(f"the page puts {named[0]} at {m['sha']}, and it is at {named[2][:8]}")
        if m["tagged"] != named[3]:
            faults.append(f"the page dates {named[0]} {m['tagged']}, and it was tagged {named[3]}")
        if m["updated"] < named[3]:
            faults.append(f"the page was updated {m['updated']}, before {named[0]} was tagged ({named[3]})")
        want = release_name(named[1])
        if want is not None and m["name"] != want:
            faults.append(f"the page calls {named[0]} \"{m['name']}\", and its name is \"{want}\"")
    if vt(workspace) < vt(newest[1]):
        faults.append(f"the workspace version {workspace} is older than the newest release tag, {newest[0]}")
    return faults, [f"workspace version {workspace}", f"newest tag {newest[0]} at {newest[2][:8]}"], False


def tag_fault(root, name):
    """Before tagging: the fault in tagging HEAD `name`, or None."""
    m = re.fullmatch(r"release-(\d+\.\d+\.\d+)", name)
    if m is None:
        return f"{name} is not a release tag name (release-X.Y.Z)"
    try:
        held = workspace_version(open(os.path.join(root, "Cargo.toml"), encoding="utf-8").read())
    except OSError as e:
        raise Unreadable(str(e)) from e
    if held != m.group(1):
        return f"{name} would tag a tree whose workspace version is {held}: its binaries would print {held}"
    return None


def main(argv=None):
    argv = list(argv or [])
    tag = None
    if argv[:1] == ["--tag"]:
        if len(argv) < 2:
            print("usage: status-page.py [--tag release-X.Y.Z] [ROOT]")
            return 2
        tag, argv = argv[1], argv[2:]
    root = argv[0] if argv else os.getcwd()
    try:
        if tag is not None:
            fault = tag_fault(root, tag)
            if fault:
                print(f"status page: {fault}; bump [workspace.package] version in Cargo.toml first")
                return 1
        faults, notes, skipped = check(root)
    except Unreadable as e:
        print(f"status page: could not read the page or the workspace version: {e}")
        return 2
    for f in faults:
        print(f"status page: {f}")
    if faults:
        print(f"status page: {len(faults)} fault(s) in {PAGE}'s `_Updated` line; fix the line, from `date` and the tree")
        return 1
    if skipped:
        print(f"status page: skipped: no release tag is reachable from HEAD in this clone, so only the workspace "
              f"version was checked ({'; '.join(notes)}); `git fetch --tags origin` gives it the tags")
        return 0
    print(f"status page: ok ({'; '.join(notes)})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
