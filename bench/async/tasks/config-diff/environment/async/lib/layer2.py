#!/usr/bin/env python3
"""The async bench's Layer 2 families: chores with independent slow sub-steps
(theseus-2wxa), on the ledger of `asyncbench.py`.

One file beside `asyncbench.py`, standard library only, copied with it into
every Layer 2 task (`sync.py`): its image's `/opt/async/lib/` (the tools the
agent runs, from `/opt/async/bin`) and its `tests/` (the verifier's copy).

    layer2.py tool NAME ARGS...   a family's tool (the bin wrappers)
    layer2.py check FAMILY        the verifier: reward.json, the ledger copied

Every slow sub-step is a tool whose duration is drawn at its start (10 to
45 s before the time scale) and slept, so load does not move the wall, and
logged as asyncbench's steps are: the ledger gives the ideal wall and the
overlap achieved. The facts a task asks about (which test fails, which host
logged the error, which commit is the first bad one) are drawn on a tool's
first use and kept in the tools' state, so only a tool's run can tell them,
and the check reads them back from the ledger.

Each family's check holds the ledger to its order rules (`DEPS`): a step
that needs another's result and started before that result's end is an
order violation, and reward 0. Four families are controls, where the order
is the point; two mixed ones have an order rule beside their independent
steps.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))

import asyncbench as ab  # noqa: E402

_rng = ab._rng


# ---------------------------------------------------------------- the world

# Each tool's step durations, in seconds before the time scale.
DURATIONS = {
    "run-suite": (20.0, 45.0),
    "lint": (10.0, 25.0),
    "typecheck": (15.0, 35.0),
    "unit-tests": (20.0, 45.0),
    "build-config": (20.0, 45.0),
    "bench-batch": (15.0, 40.0),
    "fetch-log": (10.0, 35.0),
    "repo-status": (10.0, 35.0),
    "convert-export": (10.0, 30.0),
    "health": (10.0, 20.0),
    "restart": (15.0, 30.0),
    "archive-get": (10.0, 30.0),
    "module-source": (10.0, 30.0),
    "advisory": (10.0, 30.0),
    "env-config": (10.0, 30.0),
    "validate-config": (10.0, 20.0),
    "run-tests": (20.0, 40.0),
    "build": (15.0, 30.0),
    "run-app": (10.0, 20.0),
    "check-output": (10.0, 20.0),
    "start-service": (10.0, 25.0),
    "query-service": (10.0, 20.0),
    "migrate-db": (10.0, 20.0),
    "codemod": (10.0, 20.0),
    "test-suite": (15.0, 30.0),
    "build-lib": (20.0, 40.0),
    "link-app": (10.0, 20.0),
    "make-worktree": (10.0, 15.0),
    "test-commit": (15.0, 35.0),
}
# A refusal (a step whose prerequisite has not ended) fails fast.
REFUSAL = (1.0, 3.0)

SUITES = ["auth", "billing", "catalog", "search"]
CONFIGS = ["debug", "release", "minsize"]
BATCHES = ["16", "32", "64", "128"]
HOSTS = [f"web-{i}" for i in range(1, 7)]
REPOS = ["api", "web", "worker", "infra", "docs"]
EXPORTS = ["jan", "feb", "mar", "apr", "may"]
SERVICES = ["auth", "billing", "search", "mail"]
NOTES = [f"notes-{i:02d}" for i in range(1, 9)]
MODULES = ["core", "io", "net", "auth", "cache", "util"]
ADVISORIES = [f"ADV-{i}" for i in range(1001, 1006)]
ENVS = ["dev", "staging", "prod", "perf"]
MIGRATIONS = ["001", "002", "003"]
LIBS = ["core", "ui"]
COMMITS = ["3f9a2c1", "8b04d7e", "c71e5a9", "e2d8f40"]

WORDS = ["amber", "basil", "cedar", "delta", "ember", "fjord", "garnet", "harbor", "indigo", "juniper",
         "kestrel", "lumen", "maple", "nimbus", "onyx", "pepper", "quartz", "raven", "saffron", "tundra",
         "umber", "violet", "willow", "xenon", "yarrow", "zephyr"]

# The scaffold family's spec: each file under /app/config and its keys.
SCAFFOLD = {
    "api.yaml": {"service": "api", "port": "8080", "replicas": "3"},
    "worker.yaml": {"service": "worker", "queue": "jobs", "replicas": "2"},
    "scheduler.yaml": {"service": "scheduler", "interval_s": "60"},
    "cache.yaml": {"service": "cache", "port": "6379", "max_mb": "512"},
    "db.yaml": {"service": "db", "port": "5432", "pool": "20"},
    "search.yaml": {"service": "search", "port": "9200", "shards": "4"},
    "mail.yaml": {"service": "mail", "port": "2525", "from": "noreply@example.test"},
    "gateway.yaml": {"service": "gateway", "port": "443", "upstream": "api:8080"},
}

# The rename family's six modules, each with its uses of the old name.
RENAME_OLD, RENAME_NEW = "fetch_rows", "load_rows"
RENAME_FILES = {"m1.py": f'"""Rows from the store."""\n\n\ndef {RENAME_OLD}(table):\n    return [r for r in table if r]\n'}
RENAME_FILES.update({
    f"m{i}.py": (f'"""Step {i}."""\n\nfrom m1 import {RENAME_OLD}\n\n\n'
                 f"def step_{i}(table):\n    rows = {RENAME_OLD}(table)\n    return len(rows) + {i}\n")
    for i in range(2, 7)
})

# The edit-test family's tree: the codemod renames the flag in src/, and its
# test still names the old one until the agent fixes it.
FLAG_OLD, FLAG_NEW = "legacy_mode", "compat_mode"
EDIT_FILES = {
    "src/settings.py": f'DEFAULTS = {{"{FLAG_OLD}": False, "workers": 4}}\n',
    "src/server.py": f'from settings import DEFAULTS\n\n\ndef mode():\n    return DEFAULTS["{FLAG_OLD}"]\n',
    "tests/test_flags.py": f'from settings import DEFAULTS\n\n\ndef test_flag():\n    assert "{FLAG_OLD}" in DEFAULTS\n',
}


def _world(name: str, draw: Callable[[], Any]) -> Any:
    """The family's facts, drawn on a tool's first use and kept."""
    with ab._state(f"world-{name}.json", draw) as st:
        return st["data"]


def _step(tool: str, step: str, **fields: Any) -> ab.Step:
    return ab.Step(tool, step, *DURATIONS[tool], **fields)


def _refuse(tool: str, step: str, why: str, code: int = 3) -> int:
    """A step whose prerequisite has not ended: it fails fast, as the real
    one would, and says why."""
    s = ab.Step(tool, step, *REFUSAL, refused=True)
    s.wait()
    ab.append("fail", tool, step, error=why)
    print(f"{tool}: {why}", file=sys.stderr)
    return code


def _ended(tool: str, step: str | None = None) -> list[dict[str, Any]]:
    return [r for r in ab.read() if r["kind"] == "end" and r["tool"] == tool
            and (step is None or r["step"] == step)]


def _usage(msg: str) -> int:
    return ab._usage(msg)


def _one_of(args: list[str], allowed: list[str], what: str, tool: str) -> str | None:
    if len(args) != 1 or args[0] not in allowed:
        _usage(f"usage: {tool} {what}, {what} one of {' '.join(allowed)}")
        return None
    return args[0]


def tree_hash(paths: list[Path]) -> str:
    h = hashlib.sha256()
    for p in sorted(paths):
        h.update(p.as_posix().encode())
        try:
            h.update(p.read_bytes())
        except OSError:
            h.update(b"\0missing")
    return h.hexdigest()[:16]


def _tree(sub: str) -> list[Path]:
    base = ab.app() / sub
    return sorted(p for p in base.rglob("*.py")) if base.is_dir() else []


# ---------------------------------------------------------------- independent programs, then combine


def tool_run_suite(args: list[str]) -> int:
    pkg = _one_of(args, SUITES, "PACKAGE", "run-suite")
    if pkg is None:
        return 2

    def draw() -> dict[str, Any]:
        out: dict[str, Any] = {}
        failing = _rng.sample(SUITES, _rng.randint(1, 3))
        for p in SUITES:
            names = [f"test_{w}" for w in _rng.sample(WORDS, _rng.randint(5, 8))]
            out[p] = {"tests": names, "failed": sorted(_rng.sample(names, _rng.randint(1, 2)))
                      if p in failing else []}
        return out

    w = _world("suites", draw)[pkg]
    s = _step("run-suite", pkg)
    s.wait()
    for t in w["tests"]:
        print(f"{'FAIL' if t in w['failed'] else 'PASS'} {pkg}::{t}")
    print(f"{pkg}: {len(w['tests']) - len(w['failed'])} passed, {len(w['failed'])} failed")
    s.end(failed=[f"{pkg}::{t}" for t in w["failed"]], total=len(w["tests"]))
    return 1 if w["failed"] else 0


def _ci_world() -> dict[str, Any]:
    def draw() -> dict[str, Any]:
        total = _rng.randint(40, 90)
        return {"warnings": _rng.randint(0, 12), "errors": _rng.randint(0, 5), "total": total,
                "failed": _rng.randint(0, 3)}
    return _world("ci", draw)


def tool_lint(args: list[str]) -> int:
    if args:
        return _usage("usage: lint")
    w = _ci_world()
    s = _step("lint", "lint")
    s.wait()
    print(f"lint: {w['warnings']} warnings")
    s.end(warnings=w["warnings"])
    return 0


def tool_typecheck(args: list[str]) -> int:
    if args:
        return _usage("usage: typecheck")
    w = _ci_world()
    s = _step("typecheck", "typecheck")
    s.wait()
    print(f"typecheck: {w['errors']} errors")
    s.end(errors=w["errors"])
    return 1 if w["errors"] else 0


def tool_unit_tests(args: list[str]) -> int:
    if args:
        return _usage("usage: unit-tests")
    w = _ci_world()
    s = _step("unit-tests", "unit-tests")
    s.wait()
    passed = w["total"] - w["failed"]
    print(f"unit-tests: {passed} passed, {w['failed']} failed, {w['total']} in all")
    s.end(passed=passed, total=w["total"])
    return 1 if w["failed"] else 0


def tool_build_config(args: list[str]) -> int:
    cfg = _one_of(args, CONFIGS, "CONFIG", "build-config")
    if cfg is None:
        return 2
    w = _world("configs", lambda: {c: _rng.randint(400_000, 9_000_000) for c in CONFIGS})
    s = _step("build-config", cfg)
    s.wait()
    print(f"built {cfg}: out/{cfg}/app, {w[cfg]} bytes")
    s.end(size=w[cfg])
    return 0


def tool_bench_batch(args: list[str]) -> int:
    b = _one_of(args, BATCHES, "SIZE", "bench-batch")
    if b is None:
        return 2
    w = _world("bench", lambda: dict(zip(BATCHES, _rng.sample(range(900, 9000), len(BATCHES)))))
    s = _step("bench-batch", b)
    s.wait()
    print(f"batch size {b}: {w[b]} ops/s")
    s.end(ops=w[b])
    return 0


def tool_fetch_log(args: list[str]) -> int:
    host = _one_of(args, HOSTS, "HOST", "fetch-log")
    if host is None:
        return 2
    w = _world("logs", lambda: {"culprit": _rng.choice(HOSTS), "errors": _rng.randint(1, 3),
                                "seed": _rng.getrandbits(32)})
    s = _step("fetch-log", host)
    s.wait()
    rnd = __import__("random").Random(f"{w['seed']}-{host}")
    lines = [f"2026-10-10T0{rnd.randint(0, 9)}:{rnd.randint(10, 59)}:{rnd.randint(10, 59)} "
             f"{rnd.choice(['INFO', 'INFO', 'INFO', 'WARN'])} {host} {rnd.choice(WORDS)} request served"
             for _ in range(rnd.randint(20, 40))]
    errors = w["errors"] if host == w["culprit"] else 0
    for _ in range(errors):
        lines.insert(rnd.randint(0, len(lines)), f"2026-10-10T04:12:09 ERROR {host} disk quota exceeded")
    print("\n".join(lines))
    s.end(errors=errors)
    return 0


def tool_repo_status(args: list[str]) -> int:
    repo = _one_of(args, REPOS, "REPO", "repo-status")
    if repo is None:
        return 2

    def draw() -> dict[str, int]:
        behind = _rng.sample(REPOS, _rng.randint(1, len(REPOS) - 1))
        return {r: (_rng.randint(1, 7) if r in behind else 0) for r in REPOS}

    w = _world("repos", draw)
    s = _step("repo-status", repo)
    s.wait()
    n = w[repo]
    print(f"{repo}: fetched origin")
    print(f"{repo}: your branch is behind 'origin/main' by {n} commits" if n
          else f"{repo}: your branch is up to date with 'origin/main'")
    s.end(behind=n)
    return 0


def tool_convert_export(args: list[str]) -> int:
    name = _one_of(args, EXPORTS, "EXPORT", "convert-export")
    if name is None:
        return 2

    def draw() -> dict[str, Any]:
        ids = _rng.sample(range(1000, 9999), 40)
        out = {}
        for e in EXPORTS:
            n = _rng.randint(3, 6)
            out[e] = [f"{ids.pop()},{_rng.randint(1, 9999)}.{_rng.randint(0, 99):02d}" for _ in range(n)]
        return out

    rows = _world("exports", draw)[name]
    s = _step("convert-export", name)
    s.wait()
    out = ab.app() / "exports"
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{name}.csv").write_text("id,amount\n" + "\n".join(rows) + "\n")
    print(f"converted {name}: exports/{name}.csv, {len(rows)} rows")
    s.end(rows=rows)
    return 0


def tool_health(args: list[str]) -> int:
    svc = _one_of(args, SERVICES, "SERVICE", "health")
    if svc is None:
        return 2
    w = _world("services", lambda: {"failing": _rng.choice(SERVICES)})
    restarted = {r["step"] for r in _ended("restart")}
    s = _step("health", svc)
    s.wait()
    ok = svc != w["failing"] or svc in restarted
    print(f"GET http://{svc}.internal/healthz: " + ("200 ok" if ok else "503 unhealthy"))
    s.end(healthy=ok)
    return 0 if ok else 1


def tool_restart(args: list[str]) -> int:
    svc = _one_of(args, SERVICES, "SERVICE", "restart")
    if svc is None:
        return 2
    _world("services", lambda: {"failing": _rng.choice(SERVICES)})
    s = _step("restart", svc)
    s.wait()
    print(f"{svc}: restarted")
    s.end()
    ab.append("effect", "restart", svc)
    return 0


# ---------------------------------------------------------------- independent reads and fetches

FACTS = {"codename": "the release codename", "rotation": "the on-call rotation, in days",
         "port": "the primary database's port"}


def _filler(rnd: Any, n: int) -> list[str]:
    return [" ".join(rnd.choice(WORDS) for _ in range(rnd.randint(8, 14))).capitalize() + "." for _ in range(n)]


def tool_archive_get(args: list[str]) -> int:
    name = _one_of(args, NOTES, "NAME", "archive-get")
    if name is None:
        return 2

    def draw() -> dict[str, Any]:
        where = _rng.sample(NOTES, 3)
        return {"seed": _rng.getrandbits(32),
                "codename": {"file": where[0], "value": _rng.choice(WORDS).upper()},
                "rotation": {"file": where[1], "value": str(_rng.randint(3, 14))},
                "port": {"file": where[2], "value": str(_rng.randint(5400, 5499))}}

    w = _world("archive", draw)
    s = _step("archive-get", name)
    s.wait()
    rnd = __import__("random").Random(f"{w['seed']}-{name}")
    lines = [f"# {name}", ""] + _filler(rnd, rnd.randint(60, 90))
    said = {"codename": "The release codename is {}.", "rotation": "The on-call rotation lasts {} days.",
            "port": "The primary database listens on port {}."}
    held = []
    for k, tmpl in said.items():
        if w[k]["file"] == name:
            lines.insert(rnd.randint(2, len(lines)), tmpl.format(w[k]["value"]))
            held.append(k)
    print("\n".join(lines))
    s.end(holds=held)
    return 0


def tool_module_source(args: list[str]) -> int:
    mod = _one_of(args, MODULES, "MODULE", "module-source")
    if mod is None:
        return 2

    def draw() -> dict[str, Any]:
        out = {}
        for m in MODULES:
            pub, priv = _rng.randint(2, 4), _rng.randint(1, 2)
            names = _rng.sample(WORDS, pub + priv)
            out[m] = {"public": [f"{n}_{m}" for n in names[:pub]],
                      "private": [f"_{n}_{m}" for n in names[pub:]]}
        return out

    w = _world("modules", draw)[mod]
    s = _step("module-source", mod)
    s.wait()
    out = [f'"""toolkit.{mod}."""', ""]
    for f in w["public"] + w["private"]:
        out += ["", f"def {f}(value, *, strict=False):", f'    """{f.strip("_").replace("_", " ")}."""',
                "    return value", ""]
    print("\n".join(out))
    s.end(public=w["public"], private=w["private"])
    return 0


def tool_advisory(args: list[str]) -> int:
    adv = _one_of(args, ADVISORIES, "ID", "advisory")
    if adv is None:
        return 2
    w = _world("advisories", lambda: {a: {"severity": _rng.choice(["low", "medium", "high", "critical"]),
                                          "fixed": f"{_rng.randint(1, 4)}.{_rng.randint(0, 20)}.{_rng.randint(0, 9)}"}
                                      for a in ADVISORIES})[adv]
    s = _step("advisory", adv)
    s.wait()
    print(f"{adv}\n  severity: {w['severity']}\n  fixed in: {w['fixed']}\n  package: toolkit")
    s.end(severity=w["severity"], fixed=w["fixed"])
    return 0


CONFIG_KEYS = ["log_level", "pool_size", "timeout_s", "cache_mb", "region", "feature_beta", "retries",
               "batch", "tls", "workers"]


def tool_env_config(args: list[str]) -> int:
    env = _one_of(args, ENVS, "ENV", "env-config")
    if env is None:
        return 2

    def draw() -> dict[str, Any]:
        base = {k: str(_rng.randint(1, 64)) for k in CONFIG_KEYS}
        differ = _rng.sample(CONFIG_KEYS, _rng.randint(2, 4))
        out = {}
        for e in ENVS:
            out[e] = dict(base)
        for k in differ:
            e = _rng.choice(ENVS)
            out[e][k] = str(int(base[k]) + _rng.randint(1, 9))
        return out

    w = _world("envs", draw)[env]
    s = _step("env-config", env)
    s.wait()
    print("\n".join(f"{k}={v}" for k, v in w.items()))
    s.end(values=w)
    return 0


# ---------------------------------------------------------------- independent writes


def parse_yaml(text: str) -> dict[str, str]:
    out = {}
    for line in text.splitlines():
        if ":" in line and not line.lstrip().startswith("#"):
            k, _, v = line.partition(":")
            out[k.strip()] = v.strip().strip('"').strip("'")
    return out


def tool_validate_config(args: list[str]) -> int:
    name = _one_of(args, sorted(SCAFFOLD), "FILE", "validate-config")
    if name is None:
        return 2
    path = ab.app() / "config" / name
    before = tree_hash([path])
    s = _step("validate-config", name, tree=before)
    s.wait()
    try:
        got = parse_yaml(path.read_text())
    except OSError:
        got = None
    if got is None:
        problems = [f"config/{name} does not exist"]
    else:
        problems = [f"config/{name}: {k} is {got.get(k)!r}, not {v!r}" for k, v in SCAFFOLD[name].items()
                    if got.get(k) != v]
    print("\n".join(problems) if problems else f"config/{name}: valid")
    s.end(ok=not problems, tree=before)
    return 1 if problems else 0


def tool_run_tests(args: list[str]) -> int:
    if args:
        return _usage("usage: run-tests")
    files = [ab.app() / "src" / f for f in RENAME_FILES]
    before = tree_hash(files)
    s = _step("run-tests", "tests", tree=before)
    texts = {f.name: (f.read_text() if f.exists() else "") for f in files}
    s.wait()
    problems = []
    for f, text in texts.items():
        if re.search(rf"\b{RENAME_OLD}\b", text):
            problems.append(f"FAIL test_{f[:-3]}: NameError: name '{RENAME_OLD}' is not defined")
        elif RENAME_NEW not in text:
            problems.append(f"FAIL test_{f[:-3]}: {RENAME_NEW} is never called")
    print("\n".join(problems) if problems else "6 passed")
    s.end(passed=not problems, tree=before)
    return 1 if problems else 0


# ---------------------------------------------------------------- dependent controls


def tool_build(args: list[str]) -> int:
    if args:
        return _usage("usage: build")
    s = _step("build", "build")
    s.wait()
    bid = f"b{_rng.getrandbits(24):06x}"
    with ab._state("artifact.json", dict) as st:
        st["data"]["app"] = bid
    print(f"built bin/app ({bid})")
    s.end(build=bid)
    return 0


def tool_run_app(args: list[str]) -> int:
    if args:
        return _usage("usage: run-app")
    with ab._state("artifact.json", dict) as st:
        # Before any build ends, bin/app is yesterday's.
        bid = st["data"].get("app", "stale")
    s = _step("run-app", "run", build=bid)
    s.wait()
    out = hashlib.sha256(f"out-{bid}".encode()).hexdigest()[:12]
    with ab._state("artifact.json", dict) as st:
        st["data"]["output"] = out
    print(f"bin/app ({bid}) wrote out/result.dat: {out}")
    s.end(output=out, build=bid)
    return 0


def tool_check_output(args: list[str]) -> int:
    if args:
        return _usage("usage: check-output")
    with ab._state("artifact.json", dict) as st:
        out = st["data"].get("output")
    if out is None:
        return _refuse("check-output", "check", "out/result.dat does not exist: run the app first", 4)
    s = _step("check-output", "check", output=out)
    s.wait()
    verdict = hashlib.sha256(f"verdict-{out}".encode()).hexdigest()[:10]
    print(f"out/result.dat checked: verdict {verdict}")
    s.end(verdict=verdict, output=out)
    return 0


def tool_start_service(args: list[str]) -> int:
    if args:
        return _usage("usage: start-service")
    s = _step("start-service", "inventory")
    s.wait()
    stock = _world("inventory", lambda: {"stock": _rng.randint(100, 9999)})["stock"]
    with ab._state("service-up.json", dict) as st:
        st["data"]["up"] = True
    print("inventory service ready on 127.0.0.1:7070")
    s.end(stock=stock)
    return 0


def tool_query_service(args: list[str]) -> int:
    if args != ["stock"]:
        return _usage("usage: query-service stock")
    with ab._state("service-up.json", dict) as st:
        up = bool(st["data"].get("up"))
    if not up:
        return _refuse("query-service", "stock", "connect 127.0.0.1:7070: connection refused", 7)
    s = _step("query-service", "stock")
    s.wait()
    stock = _world("inventory", lambda: {"stock": _rng.randint(100, 9999)})["stock"]
    print(f"in stock: {stock}")
    s.end(stock=stock)
    return 0


def tool_migrate_db(args: list[str]) -> int:
    step = _one_of(args, MIGRATIONS, "MIGRATION", "migrate-db")
    if step is None:
        return 2
    with ab._state("schema.json", lambda: {"applied": [], "sum": "0" * 12}) as st:
        applied, prev = list(st["data"]["applied"]), st["data"]["sum"]
    want = MIGRATIONS[len(applied)] if len(applied) < len(MIGRATIONS) else None
    if step != want:
        at = applied[-1] if applied else "000"
        return _refuse("migrate-db", step, f"the schema is at {at}; migration {step} needs {want or 'none'} first"
                       if want else f"migration {step} is already applied", 5)
    s = _step("migrate-db", step, after=prev)
    s.wait()
    new = hashlib.sha256(f"{prev}-{step}".encode()).hexdigest()[:12]
    with ab._state("schema.json", lambda: {"applied": [], "sum": "0" * 12}) as st:
        st["data"]["applied"].append(step)
        st["data"]["sum"] = new
    print(f"applied {step}; schema checksum {new}")
    s.end(checksum=new)
    return 0


def _edit_paths() -> list[Path]:
    return [ab.app() / p for p in EDIT_FILES]


def tool_codemod(args: list[str]) -> int:
    if args:
        return _usage("usage: codemod")
    s = _step("codemod", "rename-flag")
    s.wait()
    changed = []
    for p in _edit_paths():
        if p.parts[-2] != "src" or not p.exists():
            continue
        text = p.read_text()
        if FLAG_OLD in text:
            p.write_text(text.replace(FLAG_OLD, FLAG_NEW))
            changed.append(f"src/{p.name}")
    print(f"codemod: renamed {FLAG_OLD} to {FLAG_NEW} in " + (", ".join(changed) or "nothing"))
    s.end(changed=changed)
    return 0


def tool_test_suite(args: list[str]) -> int:
    if args:
        return _usage("usage: test-suite")
    before = tree_hash(_edit_paths())
    texts = {p: (p.read_text() if p.exists() else "") for p in _edit_paths()}
    s = _step("test-suite", "tests", tree=before)
    s.wait()
    problems = []
    for p, text in texts.items():
        rel = f"{p.parts[-2]}/{p.name}"
        if FLAG_OLD in text:
            problems.append(f"FAIL {rel}: KeyError: '{FLAG_OLD}'" if p.parts[-2] == "tests"
                            else f"FAIL {rel}: still reads the removed flag {FLAG_OLD}")
        elif FLAG_NEW not in text:
            problems.append(f"FAIL {rel}: never reads {FLAG_NEW}")
    print("\n".join(problems) if problems else "3 passed")
    s.end(passed=not problems, tree=before)
    return 1 if problems else 0


# ---------------------------------------------------------------- mixed


def tool_build_lib(args: list[str]) -> int:
    lib = _one_of(args, LIBS, "LIB", "build-lib")
    if lib is None:
        return 2
    s = _step("build-lib", lib)
    s.wait()
    obj = f"lib{lib}-{_rng.getrandbits(20):05x}.a"
    with ab._state("libs.json", dict) as st:
        st["data"][lib] = obj
    print(f"built out/{obj}")
    s.end(object=obj)
    return 0


def tool_link_app(args: list[str]) -> int:
    if args:
        return _usage("usage: link-app")
    with ab._state("libs.json", dict) as st:
        objs = dict(st["data"])
    missing = [lib for lib in LIBS if lib not in objs]
    if missing:
        return _refuse("link-app", "link", "undefined references: " + ", ".join(f"lib{m}.a" for m in missing), 6)
    s = _step("link-app", "link", objects=[objs[lib] for lib in LIBS])
    s.wait()
    art = "app-" + hashlib.sha256("+".join(objs[lib] for lib in LIBS).encode()).hexdigest()[:10]
    print(f"linked out/{art}")
    s.end(artifact=art)
    return 0


def tool_make_worktree(args: list[str]) -> int:
    c = _one_of(args, COMMITS, "COMMIT", "make-worktree")
    if c is None:
        return 2
    s = _step("make-worktree", c)
    s.wait()
    with ab._state("worktrees.json", dict) as st:
        st["data"][c] = True
    print(f"worktree at /tmp/wt-{c} on {c}")
    s.end()
    return 0


def tool_test_commit(args: list[str]) -> int:
    c = _one_of(args, COMMITS, "COMMIT", "test-commit")
    if c is None:
        return 2
    with ab._state("worktrees.json", dict) as st:
        ready = bool(st["data"].get(c))
    if not ready:
        return _refuse("test-commit", c, f"/tmp/wt-{c}: no such worktree", 8)
    bad = _world("bisect", lambda: {"first_bad": _rng.randint(1, len(COMMITS) - 1)})["first_bad"]
    s = _step("test-commit", c)
    s.wait()
    ok = COMMITS.index(c) < bad
    print(f"{c}: " + ("all tests pass" if ok else "FAIL test_checkout: total is off by one"))
    s.end(passed=ok)
    return 0 if ok else 1


TOOLS = {
    "run-suite": tool_run_suite,
    "lint": tool_lint,
    "typecheck": tool_typecheck,
    "unit-tests": tool_unit_tests,
    "build-config": tool_build_config,
    "bench-batch": tool_bench_batch,
    "fetch-log": tool_fetch_log,
    "repo-status": tool_repo_status,
    "convert-export": tool_convert_export,
    "health": tool_health,
    "restart": tool_restart,
    "archive-get": tool_archive_get,
    "module-source": tool_module_source,
    "advisory": tool_advisory,
    "env-config": tool_env_config,
    "validate-config": tool_validate_config,
    "run-tests": tool_run_tests,
    "build": tool_build,
    "run-app": tool_run_app,
    "check-output": tool_check_output,
    "start-service": tool_start_service,
    "query-service": tool_query_service,
    "migrate-db": tool_migrate_db,
    "codemod": tool_codemod,
    "test-suite": tool_test_suite,
    "build-lib": tool_build_lib,
    "link-app": tool_link_app,
    "make-worktree": tool_make_worktree,
    "test-commit": tool_test_commit,
}


# ---------------------------------------------------------------- order rules

# A dependent step and what must have ended before it starts: (its tool, its
# step or None for any, the prerequisite's tool, its step: None for any, "="
# for the same step). A start with no such end before it, by seq, is an order
# violation. A refused step is one too: it started before its prerequisite.
DEPS: dict[str, list[tuple[str, str | None, str, str | None]]] = {
    "pipeline": [("run-app", None, "build", None), ("check-output", None, "run-app", None)],
    "service": [("query-service", None, "start-service", None)],
    "migrations": [("migrate-db", "002", "migrate-db", "001"), ("migrate-db", "003", "migrate-db", "002")],
    "edit-test": [("test-suite", None, "codemod", None)],
    "link": [("link-app", None, "build-lib", "core"), ("link-app", None, "build-lib", "ui")],
    "bisect": [("test-commit", None, "make-worktree", "=")],
}
CONTROLS = ["pipeline", "service", "migrations", "edit-test"]


def order_violations(family: str, rec: list[dict[str, Any]]) -> list[str]:
    """Each dependent start that came before its prerequisite's end."""
    out = []
    for dtool, dstep, ptool, pstep in DEPS.get(family, []):
        for r in rec:
            if r["kind"] != "start" or r["tool"] != dtool or (dstep is not None and r["step"] != dstep):
                continue
            want = r["step"] if pstep == "=" else pstep
            if not any(e["kind"] == "end" and e["tool"] == ptool and (want is None or e["step"] == want)
                       and e["seq"] < r["seq"] for e in rec):
                need = ptool + (f" {want}" if want else "")
                out.append(f"{dtool} {r['step']} started (seq {r['seq']}) before {need} ended")
    return out


# ---------------------------------------------------------------- the ideal and the overlap


def _first_runs(rec: list[dict[str, Any]], tool: str) -> dict[str, float]:
    """Each step of `tool`: the drawn duration of its first run that ended,
    else of its first start; refusals aside."""
    ended = {(r["step"], r["pid"]) for r in rec if r["kind"] == "end" and r["tool"] == tool}
    out: dict[str, float] = {}
    for r in rec:
        if r["kind"] == "start" and r["tool"] == tool and not r.get("refused"):
            if (r["step"], r["pid"]) in ended and r["step"] not in out:
                out[r["step"]] = r["duration"]
    for r in rec:
        if r["kind"] == "start" and r["tool"] == tool and not r.get("refused"):
            out.setdefault(r["step"], r["duration"])
    return out


def _stage(rec: list[dict[str, Any]], tools: list[str]) -> float:
    return max([d for t in tools for d in _first_runs(rec, t).values()] or [0.0])


# Each family's stages: steps side by side within a stage, stages in turn.
STAGES: dict[str, list[list[str]]] = {
    "suites": [["run-suite"]],
    "ci-checks": [["lint", "typecheck", "unit-tests"]],
    "build-configs": [["build-config"]],
    "bench-settings": [["bench-batch"]],
    "host-logs": [["fetch-log"]],
    "repos-behind": [["repo-status"]],
    "csv-merge": [["convert-export"]],
    "doc-questions": [["archive-get"]],
    "api-summary": [["module-source"]],
    "advisories": [["advisory"]],
    "config-diff": [["env-config"]],
    "scaffold": [["validate-config"]],
    "rename": [["run-tests"]],
    "pipeline": [["build"], ["run-app"], ["check-output"]],
    "service": [["start-service"], ["query-service"]],
    "link": [["build-lib"], ["link-app"]],
}


def ideal(family: str, rec: list[dict[str, Any]]) -> float | None:
    """The family's ideal wall, in seconds, from the ledger's drawn
    durations: its critical path, each step's first run."""
    if not any(r["kind"] == "start" for r in rec):
        return None
    if family in STAGES:
        return sum(_stage(rec, s) for s in STAGES[family])
    if family == "health-restart":
        checks = [r for r in rec if r["kind"] == "start" and r["tool"] == "health"]
        restarts = [r for r in rec if r["kind"] == "start" and r["tool"] == "restart"]
        first = {}
        for r in checks:
            first.setdefault(r["step"], r["duration"])
        total = max(first.values() or [0.0])
        if restarts:
            total += restarts[0]["duration"]
            after = [r for r in checks if r["seq"] > restarts[0]["seq"] and r["step"] == restarts[0]["step"]]
            total += after[0]["duration"] if after else 0.0
        return total
    if family == "migrations":
        runs = _first_runs(rec, "migrate-db")
        return sum(runs.get(m, 0.0) for m in MIGRATIONS)
    if family == "edit-test":
        tests = [r["duration"] for r in rec if r["kind"] == "start" and r["tool"] == "test-suite"
                 and not r.get("refused")]
        return _stage(rec, ["codemod"]) + (tests[0] + tests[-1] if len(tests) > 1 else sum(tests))
    if family == "bisect":
        wt, tc = _first_runs(rec, "make-worktree"), _first_runs(rec, "test-commit")
        return max([wt.get(c, 0.0) + tc.get(c, 0.0) for c in set(wt) | set(tc)] or [0.0])
    return None


def overlap(rec: list[dict[str, Any]], tools: set[str]) -> dict[str, Any] | None:
    """The overlap the run achieved: the sum of its slow steps' times over
    the slow phase's wall (the first slow start to the last slow end), by the
    ledger's monotonic clock. 1 is one step at a time; N is N at once."""
    starts = {(r["tool"], r["step"], r["pid"]): r for r in rec if r["kind"] == "start" and r["tool"] in tools}
    spans = []
    for r in rec:
        s = starts.get((r["tool"], r["step"], r["pid"]))
        if r["kind"] in ("end", "fail", "stopped") and s is not None:
            spans.append((s["mono"], r["mono"]))
    if not spans:
        return None
    busy = sum(b - a for a, b in spans)
    phase = max(b for _, b in spans) - min(a for a, _ in spans)
    return {"steps": len(spans), "busy_s": round(busy, 3), "phase_s": round(phase, 3),
            "overlap": round(busy / phase, 3) if phase > 0 else None}


# ---------------------------------------------------------------- checks


def _lines(path: Path) -> list[str]:
    try:
        return [line.strip() for line in path.read_text().splitlines() if line.strip()]
    except OSError:
        return []


def _pairs(path: Path) -> dict[str, str]:
    return {p[0]: p[1] for p in (line.split() for line in _lines(path)) if len(p) == 2}


def _all_ended(rec: list[dict[str, Any]], tool: str, steps: list[str]) -> list[str]:
    done = {r["step"] for r in ab.by(rec, "end", tool)}
    return [f"{tool} {s} never finished" for s in steps if s not in done]


def _last_end(rec: list[dict[str, Any]], tool: str, step: str) -> dict[str, Any] | None:
    ends = [r for r in ab.by(rec, "end", tool) if r["step"] == step]
    return ends[-1] if ends else None


def check_suites(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "run-suite", SUITES)
    want = {t for r in ab.by(rec, "end", "run-suite") for t in r["failed"]}
    got = set(_lines(ab.app() / "failing.txt"))
    if got != want:
        problems.append(f"failing.txt names {sorted(got)}, not the failing tests {sorted(want)}")
    return problems


def check_ci(rec: list[dict[str, Any]]) -> list[str]:
    problems = []
    for tool in ("lint", "typecheck", "unit-tests"):
        problems += _all_ended(rec, tool, [tool])
    if problems:
        return problems
    want = {"lint": str(_last_end(rec, "lint", "lint")["warnings"]),
            "types": str(_last_end(rec, "typecheck", "typecheck")["errors"]),
            "tests": "{passed}/{total}".format(**_last_end(rec, "unit-tests", "unit-tests"))}
    got = _pairs(ab.app() / "summary.txt")
    for k, v in want.items():
        if got.get(k) != v:
            problems.append(f"summary.txt's {k} is {got.get(k)!r}, not {v}")
    return problems


def _per_step(rec: list[dict[str, Any]], tool: str, field: str) -> dict[str, Any]:
    return {r["step"]: r[field] for r in ab.by(rec, "end", tool)}


def check_build_configs(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "build-config", CONFIGS)
    sizes, got = _per_step(rec, "build-config", "size"), _pairs(ab.app() / "sizes.txt")
    for c, v in sizes.items():
        if got.get(c) != str(v):
            problems.append(f"sizes.txt's {c} is {got.get(c)!r}, not {v}")
    return problems


def check_bench(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "bench-batch", BATCHES)
    if problems:
        return problems
    ops = _per_step(rec, "bench-batch", "ops")
    best = max(ops, key=lambda b: ops[b])
    try:
        text = (ab.app() / "bench.toml").read_text()
    except OSError:
        text = ""
    m = re.search(r"^\s*batch_size\s*=\s*(\d+)\s*$", text, re.M)
    if not m or m.group(1) != best:
        problems.append(f"bench.toml's batch_size is {m.group(1) if m else None!r}, not the best, {best}")
    return problems


def check_host_logs(rec: list[dict[str, Any]]) -> list[str]:
    errs = {s: n for s, n in _per_step(rec, "fetch-log", "errors").items() if n}
    if not errs:
        return ["no fetched log holds the error"]
    culprit = next(iter(errs))
    got = (_lines(ab.app() / "culprit.txt") or [None])[0]
    return [] if got == culprit else [f"culprit.txt names {got!r}, not {culprit}"]


def check_repos(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "repo-status", REPOS)
    want = {s for s, n in _per_step(rec, "repo-status", "behind").items() if n}
    got = set(_lines(ab.app() / "behind.txt"))
    if got != want:
        problems.append(f"behind.txt names {sorted(got)}, not {sorted(want)}")
    return problems


def check_csv(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "convert-export", EXPORTS)
    want = sorted(r for rows in _per_step(rec, "convert-export", "rows").values() for r in rows)
    lines = _lines(ab.app() / "merged.csv")
    if not lines or lines[0].replace(" ", "") != "id,amount":
        problems.append("merged.csv does not start with the header id,amount")
    got = sorted(line.replace(" ", "") for line in lines[1:])
    if got != want:
        problems.append(f"merged.csv holds {len(got)} rows, not the {len(want)} rows of the five exports")
    return problems


def check_health(rec: list[dict[str, Any]]) -> list[str]:
    bad = [r["step"] for r in ab.by(rec, "end", "health") if not r["healthy"]]
    if not bad:
        return ["no health check found the failing service"]
    svc = bad[0]
    problems = [f"restarted {r['step']}, a healthy service" for r in ab.by(rec, "effect", "restart")
                if r["step"] != svc]
    restarts = [r for r in ab.by(rec, "end", "restart") if r["step"] == svc]
    if not restarts:
        return problems + [f"{svc} was never restarted"]
    if not [r for r in ab.by(rec, "end", "health") if r["step"] == svc and r["healthy"]
            and r["seq"] > restarts[0]["seq"]]:
        problems.append(f"{svc} was not checked healthy after its restart")
    got = (_lines(ab.app() / "restarted.txt") or [None])[0]
    if got != svc:
        problems.append(f"restarted.txt names {got!r}, not {svc}")
    return problems


def check_doc_questions(rec: list[dict[str, Any]]) -> list[str]:
    w = _world("archive", dict)
    fetched = {r["step"] for r in ab.by(rec, "end", "archive-get")}
    got = _pairs(ab.app() / "answers.txt")
    problems = []
    for i, k in enumerate(("codename", "rotation", "port"), start=1):
        if not w.get(k) or w[k]["file"] not in fetched:
            problems.append(f"the note that holds {FACTS[k]} was never read")
        elif got.get(str(i), "").upper() != w[k]["value"].upper():
            problems.append(f"answer {i} is {got.get(str(i))!r}, not {w[k]['value']}")
    return problems


def check_api(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "module-source", MODULES)
    try:
        text = (ab.app() / "API.md").read_text()
    except OSError:
        return problems + ["API.md does not exist"]
    sections = {}
    for part in re.split(r"^##\s+", text, flags=re.M)[1:]:
        head, _, body = part.partition("\n")
        words = head.split()
        if words:
            sections[words[0].strip("`").removeprefix("toolkit.")] = body
    for r in ab.by(rec, "end", "module-source"):
        body = sections.get(r["step"])
        if body is None:
            problems.append(f"API.md has no section ## {r['step']}")
            continue
        problems += [f"API.md's {r['step']} leaves out {f}" for f in r["public"] if f not in body]
        problems += [f"API.md lists the private {f}" for f in r["private"] if re.search(rf"\b{f}\b", text)]
    return problems


def check_advisories(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "advisory", ADVISORIES)
    got = {p[0]: p[1:] for p in (line.split() for line in _lines(ab.app() / "advisories.txt")) if p}
    for r in ab.by(rec, "end", "advisory"):
        want = [r["severity"], r["fixed"]]
        if got.get(r["step"]) != want:
            problems.append(f"advisories.txt's {r['step']} is {got.get(r['step'])}, not {want}")
    return problems


def check_config_diff(rec: list[dict[str, Any]]) -> list[str]:
    problems = _all_ended(rec, "env-config", ENVS)
    vals = _per_step(rec, "env-config", "values")
    want = {k for k in CONFIG_KEYS if len({v.get(k) for v in vals.values()}) > 1}
    got = set(_lines(ab.app() / "differs.txt"))
    if got != want:
        problems.append(f"differs.txt names {sorted(got)}, not {sorted(want)}")
    return problems


def check_scaffold(rec: list[dict[str, Any]]) -> list[str]:
    problems = []
    for name, keys in SCAFFOLD.items():
        path = ab.app() / "config" / name
        got = parse_yaml(path.read_text()) if path.exists() else None
        if got is None:
            problems.append(f"config/{name} does not exist")
            continue
        problems += [f"config/{name}: {k} is {got.get(k)!r}, not {v!r}" for k, v in keys.items() if got.get(k) != v]
        h = tree_hash([path])
        if not [r for r in ab.by(rec, "end", "validate-config") if r["step"] == name and r["ok"] and r["tree"] == h]:
            problems.append(f"config/{name} as it stands was never validated")
    return problems


def check_rename(rec: list[dict[str, Any]]) -> list[str]:
    problems = []
    files = [ab.app() / "src" / f for f in RENAME_FILES]
    for f in files:
        text = f.read_text() if f.exists() else ""
        old = len(re.findall(rf"\b{RENAME_OLD}\b", RENAME_FILES[f.name]))
        if re.search(rf"\b{RENAME_OLD}\b", text):
            problems.append(f"src/{f.name} still names {RENAME_OLD}")
        elif (new := len(re.findall(rf"\b{RENAME_NEW}\b", text))) != old:
            problems.append(f"src/{f.name} names {RENAME_NEW} {new} times, not {old}")
    h = tree_hash(files)
    if not [r for r in ab.by(rec, "end", "run-tests") if r["passed"] and r["tree"] == h]:
        problems.append("no passing run-tests ran on the files as they stand")
    return problems


def check_pipeline(rec: list[dict[str, Any]]) -> list[str]:
    builds = {r["build"] for r in ab.by(rec, "end", "build")}
    if not builds:
        return ["build never finished"]
    good = {r["output"] for r in ab.by(rec, "end", "run-app") if r["build"] in builds}
    verdicts = {r["verdict"] for r in ab.by(rec, "end", "check-output") if r["output"] in good}
    got = (_lines(ab.app() / "verdict.txt") or [None])[0]
    if not verdicts:
        return ["check-output never checked the output of this build's app"]
    return [] if got in verdicts else [f"verdict.txt holds {got!r}, not the check of this build's output"]


def check_service(rec: list[dict[str, Any]]) -> list[str]:
    stocks = {str(r["stock"]) for r in ab.by(rec, "end", "query-service")}
    if not stocks:
        return ["query-service never answered"]
    got = (_lines(ab.app() / "stock.txt") or [None])[0]
    return [] if got in stocks else [f"stock.txt holds {got!r}, not what the service answered"]


def check_migrations(rec: list[dict[str, Any]]) -> list[str]:
    last = _last_end(rec, "migrate-db", MIGRATIONS[-1])
    if last is None:
        return [f"migration {MIGRATIONS[-1]} never finished"]
    got = (_lines(ab.app() / "schema.txt") or [None])[0]
    return [] if got == last["checksum"] else [f"schema.txt holds {got!r}, not {last['checksum']}"]


def check_edit_test(rec: list[dict[str, Any]]) -> list[str]:
    problems = [] if ab.by(rec, "end", "codemod") else ["codemod never finished"]
    for rel in EDIT_FILES:
        p = ab.app() / rel
        text = p.read_text() if p.exists() else ""
        if FLAG_OLD in text or FLAG_NEW not in text:
            problems.append(f"{rel} does not read {FLAG_NEW} alone")
    h = tree_hash(_edit_paths())
    if not [r for r in ab.by(rec, "end", "test-suite") if r["passed"] and r["tree"] == h]:
        problems.append("no passing test-suite ran on the tree as it stands")
    return problems


def check_link(rec: list[dict[str, Any]]) -> list[str]:
    arts = {r["artifact"] for r in ab.by(rec, "end", "link-app")}
    if not arts:
        return ["link-app never finished"]
    got = (_lines(ab.app() / "artifact.txt") or [None])[0]
    return [] if got in arts else [f"artifact.txt holds {got!r}, not the linked artifact"]


def check_bisect(rec: list[dict[str, Any]]) -> list[str]:
    res = _per_step(rec, "test-commit", "passed")
    bad = [c for c in COMMITS if res.get(c) is False]
    if not bad:
        return ["no tested commit failed"]
    first = bad[0]
    i = COMMITS.index(first)
    problems = []
    # The oldest is good by the instruction's word: a search may trust it.
    if i > 1 and res.get(COMMITS[i - 1]) is not True:
        problems.append(f"the commit before {first}, {COMMITS[i - 1]}, was never tested good")
    got = (_lines(ab.app() / "first-bad.txt") or [None])[0]
    if got != first:
        problems.append(f"first-bad.txt names {got!r}, not {first}")
    return problems


CHECKS = {
    "suites": check_suites,
    "ci-checks": check_ci,
    "build-configs": check_build_configs,
    "bench-settings": check_bench,
    "host-logs": check_host_logs,
    "repos-behind": check_repos,
    "csv-merge": check_csv,
    "health-restart": check_health,
    "doc-questions": check_doc_questions,
    "api-summary": check_api,
    "advisories": check_advisories,
    "config-diff": check_config_diff,
    "scaffold": check_scaffold,
    "rename": check_rename,
    "pipeline": check_pipeline,
    "service": check_service,
    "migrations": check_migrations,
    "edit-test": check_edit_test,
    "link": check_link,
    "bisect": check_bisect,
}


def _checked(family: str) -> Callable[[list[dict[str, Any]]], list[str]]:
    """The family's outcome, after its order rules: a violation is reward 0
    whatever the outcome."""
    def run(rec: list[dict[str, Any]]) -> list[str]:
        return [f"order violation: {v}" for v in order_violations(family, rec)] + CHECKS[family](rec)
    return run


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[0] == "tool" and argv[1] in TOOLS:
        return TOOLS[argv[1]](argv[2:])
    if len(argv) == 2 and argv[0] == "check" and argv[1] in CHECKS:
        return 0 if ab.check(argv[1], {argv[1]: _checked(argv[1])})["reward"] == 1 else 1
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
