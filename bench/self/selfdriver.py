"""The self bench's driver (theseus-pw1q.6): seeded gaps given to Theseus's self-improvement loop and to Claude Code.

    python3 bench/self/selfdriver.py run --set open --arm theseus --standin target/debug/theseus-sim   # a smoke
    python3 bench/self/selfdriver.py run --set sealed --sealed-dir "$SEALED" --arm both                # the bench
    python3 bench/self/selfdriver.py report <out>/scores.json                                          # again

For each gap the driver makes a scratch copy of the fixture repo (only `repo/`; never the checks), then

- **Theseus**: starts a scratch `theseusd` of its own (a fresh state dir and socket, never the operator's) on it,
  with `[self] mode = "act"` and the scratch store resumed, files the gap's report as a user's message, lets the
  backlog and the branch builder run for a bounded time, and reads the result from the scratch daemon's ledger
  (`theseus --json self log`). The columns the rows of self-backlog or self-pr would fill read "not measurable:
  <row>" until those rows have joined (`scoring.landed_rows`, or `--landed`).
- **Claude Code** (pinned, as the Harbor bench pins it): the gap's report as its prompt, in a fresh copy.

Both arms' branches are then judged the same way (`fixture.judge_branch`): the symptom check, the whole suite, the
planted revert, the keel guard, the holdout. A run costs at most `--budget` dollars (default 5.0) between its arms;
the driver stops a gap at its cap and the run at the line, and the gaps it did not reach read "not run: budget".

Standard library only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import date
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
BENCH = HERE.parent
ROOT = BENCH.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(BENCH / "harbor"))

import fixture  # noqa: E402
import gaps as gaps_mod  # noqa: E402
import scoring  # noqa: E402

MODEL = "claude-sonnet-5-5"
DEFAULT_BUDGET = 5.0
ARMS = ("theseus", "claude-code")
STANDIN_RULES = [{"when": "", "steps": [{"text": "selfbench-{marker} the stand-in answers end-{marker}"}],
                  "ttfb_ms": 50, "chunks": 4, "chunk_ms": 10}]


def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


h2h_theseus = _load("h2h_theseus", BENCH / "h2h" / "theseus.py")
h2h_standin = _load("h2h_standin", BENCH / "h2h" / "standin.py")
h2h_claude = _load("h2h_claude", BENCH / "h2h" / "claude_code.py")


def pinned_claude_version() -> str:
    """The Claude Code release the Harbor bench pins (`PINNED_VERSION` in bench/harbor/claude_code_agent.py): read
    from the file, since that module needs Harbor."""
    m = re.search(r'^PINNED_VERSION\s*=\s*"([^"]+)"', (BENCH / "harbor" / "claude_code_agent.py").read_text(), re.M)
    if not m:
        raise RuntimeError("PINNED_VERSION not found in bench/harbor/claude_code_agent.py")
    return m.group(1)


def reservation_usd() -> float:
    """One maximum reservation at the arm's model: a scratch daemon's backstop is a gap's cap plus this, since the
    kernel reserves a call's worst case before it runs (bench/README.md, THESEUS_BENCH_SPEND_LIMIT)."""
    import theseus_bench as tb
    return tb.reservation_usd(MODEL)


# ---------------------------------------------------------------- the budget

class Budget:
    """The run's line, split evenly between the arms it runs. `cap(arm, left)` is what one gap may spend: twice an
    equal share of the arm's line, never more than what is left of it. A gap that would start with nothing left is
    not run."""

    def __init__(self, total: float, arms: list[str], per_gap: float | None = None) -> None:
        if total <= 0:
            raise ValueError("--budget must be positive")
        self.total, self.arms = float(total), list(arms)
        self.line = self.total / len(self.arms)
        self.per_gap = per_gap
        self.spent = {a: 0.0 for a in self.arms}

    def left(self, arm: str) -> float:
        return round(self.line - self.spent[arm], 6)

    def cap(self, arm: str, gaps_left: int) -> float:
        share = self.per_gap if self.per_gap else 2 * self.line / max(1, gaps_left)
        return round(max(0.0, min(share, self.left(arm))), 6)

    def book(self, arm: str, usd: float | None) -> None:
        self.spent[arm] += usd or 0.0

    def stopped(self, arm: str) -> bool:
        return self.left(arm) <= 0.005

    @property
    def total_spent(self) -> float:
        return round(sum(self.spent.values()), 6)


# ---------------------------------------------------------------- Theseus's arm

def self_config(base_url: str | None, work: Path, cap_usd: float) -> str:
    """The scratch daemon's config: h2h's (the bench profile's choices), plus `[self] mode = "act"`, and a day
    ceiling at the gap's cap plus one reservation as a backstop under the driver's own watch."""
    text = h2h_theseus.config(base_url or "http://127.0.0.1:1", work)
    if base_url is None:  # a real run: the provider's own endpoint, the key from the environment
        text = text.replace('api_base = "http://127.0.0.1:1"\n', "").replace(
            f"env:{h2h_theseus.KEY_ENV}", "env:ANTHROPIC_API_KEY")
    ceiling = round(cap_usd + reservation_usd(), 2)
    return (text + f'\n[kernel]\ndaily_spend_ceiling_usd = {ceiling}\ndaily_spend_ceiling_mode = "stop"\n'
            '\n[self]\nmode = "act"\n')


class SelfDaemon(h2h_theseus.Daemon):
    """h2h's scratch daemon with `[self] mode = "act"`, the real key when there is one, and the fixture as the
    repository its self steps work on (`THESEUS_SELF_REPO`, the seam self-pr reads)."""

    def __init__(self, bin_dir: Path, root: Path, base_url: str | None, work: Path, cap_usd: float) -> None:
        super().__init__(bin_dir, root, base_url or "http://127.0.0.1:1", work)
        self.conf.write_text(self_config(base_url, work, cap_usd))
        self.env["THESEUS_SELF_REPO"] = str(work)
        if base_url is None:
            key = os.environ.get("ANTHROPIC_API_KEY")
            if not key:
                raise RuntimeError("ANTHROPIC_API_KEY is not set: a real run needs it (or --standin)")
            self.env["ANTHROPIC_API_KEY"] = key

    def json(self, *args: str, timeout: float = 30) -> Any:
        p = self.cli("--json", *args, check=False, timeout=timeout)
        try:
            return json.loads(p.stdout)
        except json.JSONDecodeError:
            return None

    def log_rows(self) -> list[dict[str, Any]]:
        r = self.json("self", "log", "--limit", "500")
        return list(r.get("rows", [])) if isinstance(r, dict) else []

    def day_spend(self) -> float | None:
        """What the scratch daemon has spent: health's day spend where it shows one, else its total cost (a fresh
        daemon's total is this gap's)."""
        return spend_of(self.json("health"))


def spend_of(h: Any) -> float | None:
    if not isinstance(h, dict):
        return None
    ds = h.get("day_spend")
    if isinstance(ds, dict) and isinstance(ds.get("spent_usd"), (int, float)):
        return float(ds["spent_usd"])
    c = h.get("cost_usd_total")
    return float(c) if isinstance(c, (int, float)) else None


def terminal(rows: list[dict[str, Any]], gap: Any, landed: dict[str, bool]) -> str | None:
    """Why a gap's wait can end before its time: its branch was built, or the self budget stopped it."""
    items = scoring.items_of(rows, gap.signals)
    if landed.get("pr") and scoring.built_rows(rows, items):
        return "built"
    if scoring.budget_stopped(rows):
        return "self budget"
    return None


def run_theseus(gap: Any, work: Path, a: argparse.Namespace, cap: float, landed: dict[str, bool]
                ) -> dict[str, Any]:
    repo = work / "repo"
    base = fixture.make_fixture(gap, repo)
    standin = None
    d = None
    try:
        base_url = None
        if a.standin:
            rules = work / "rules.json"
            rules.write_text(json.dumps(STANDIN_RULES))
            standin = h2h_standin.StandIn(a.standin, rules, work / "standin.jsonl")
            base_url = standin.base_url
        d = SelfDaemon(Path(a.bin_dir), work / "daemon", base_url, repo, cap)
        d.start()
        resumed = d.cli("self", "resume", check=False)
        info: dict[str, Any] = {"resumed": resumed.returncode == 0, "stopped_by": None}
        session = d.open_session(f"selfbench-{gap.label}")
        prompt = gap.report + ("\n\nmarker=selfbench" if a.standin else "")
        ask = subprocess.Popen([d.bin("theseus"), "--socket", str(d.socket), "ask", "-s", session, prompt],
                               env=d.env, cwd=str(repo), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        t0, rows, spent = time.monotonic(), [], None
        while time.monotonic() - t0 < a.wait:
            time.sleep(a.poll)
            rows = d.log_rows()
            spent = d.day_spend()
            why = terminal(rows, gap, landed)
            if why:
                info["stopped_by"] = why
                break
            if spent is not None and spent >= cap:
                info["stopped_by"] = "driver cap"
                break
            if ask.poll() is not None and a.standin and not landed.get("backlog") and not landed.get("pr"):
                info["stopped_by"] = "nothing to wait for"
                break
        ask.terminate()
        rows = d.log_rows() or rows
        spent = d.day_spend() if d.day_spend() is not None else spent
        d.cli("self", "halt", "bench over", check=False)
        s = scoring.score_ledger(gap, rows, landed, spent)
        s["info"] = info
        s["wall_s"] = round(time.monotonic() - t0, 1)
        head = resolve_head(repo, s.get("head"), s.get("branch"))
        if landed.get("pr") and s.get("fixed") is not None and head:
            scoring.merge_branch(s, fixture.judge_branch(gap, repo, base, head))
        elif landed.get("pr") and s.get("fixed"):
            s["notes"] = ["the branch is not in the scratch repository"]
        return s
    finally:
        if d:
            d.stop()
        if standin:
            standin.stop()


def resolve_head(repo: Path, head: Any, branch: Any) -> str | None:
    for ref in (head, branch):
        if isinstance(ref, str) and ref:
            out = subprocess.run(["git", "-C", str(repo), "rev-parse", "--verify", "-q", ref + "^{commit}"],
                                 capture_output=True, text=True)
            if out.returncode == 0:
                return out.stdout.strip()
    return None


# ---------------------------------------------------------------- Claude Code's arm

def claude_argv(binary: str, prompt: str, cap: float) -> list[str]:
    """One `claude -p` run on a person's report: no word of how to fix it, the permissions skipped (Theseus's
    tools are open in its arm), the same model, a spend cap."""
    return [binary, "-p", prompt, "--dangerously-skip-permissions", "--model", MODEL,
            "--max-budget-usd", f"{cap:.2f}", "--output-format", "json"]


def claude_env(root: Path, work: Path, standin_url: str | None) -> dict[str, str]:
    env = h2h_claude.scratch(root, work, standin_url or "http://127.0.0.1:1")
    if standin_url is None:
        env.pop("ANTHROPIC_BASE_URL", None)
        key = os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            raise RuntimeError("ANTHROPIC_API_KEY is not set: a real run needs it (or --standin)")
        env["ANTHROPIC_API_KEY"] = key
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    return env


def claude_cost(stdout: str) -> float | None:
    """`claude -p --output-format json`'s `total_cost_usd`."""
    try:
        v = json.loads(stdout)
    except json.JSONDecodeError:
        return None
    if isinstance(v, list):  # a stream of events: the result event
        v = next((e for e in reversed(v) if isinstance(e, dict) and e.get("type") == "result"), {})
    c = v.get("total_cost_usd") if isinstance(v, dict) else None
    return float(c) if isinstance(c, (int, float)) else None


def run_claude(gap: Any, work: Path, a: argparse.Namespace, cap: float) -> dict[str, Any]:
    repo = work / "repo"
    base = fixture.make_fixture(gap, repo)
    fixture.git(repo, "checkout", "-q", "-b", f"claude/{gap.label}")
    standin = None
    try:
        url = None
        if a.standin:
            rules = work / "rules.json"
            rules.write_text(json.dumps(STANDIN_RULES))
            standin = h2h_standin.StandIn(a.standin, rules, work / "standin.jsonl")
            url = standin.base_url
        env = claude_env(work / "claude-home", repo, url)
        prompt = gap.report + ("\n\nmarker=selfbench" if a.standin else "")
        t0 = time.monotonic()
        timed_out = False
        try:
            p = subprocess.run(claude_argv(a.claude, prompt, cap), cwd=str(repo), env=env, capture_output=True,
                               text=True, timeout=a.wait)
            stdout = p.stdout
        except subprocess.TimeoutExpired as e:
            timed_out, stdout = True, (e.stdout.decode() if isinstance(e.stdout, bytes) else e.stdout or "")
        fixture.git(repo, "add", "-A")
        if fixture.git(repo, "status", "--porcelain").strip():
            fixture.git(repo, "commit", "-q", "-m", "claude code's change")
        head = fixture.git(repo, "rev-parse", "HEAD").strip()
        s: dict[str, Any] = {"found": None, "na": {"found": "n/a: handed the report"}, "info":
                             {"timed_out": timed_out}, "wall_s": round(time.monotonic() - t0, 1),
                             "cost_usd": claude_cost(stdout), "cost_from": "claude -p total_cost_usd",
                             "branch": f"claude/{gap.label}", "head": head, "rank": None, "best_rank": None}
        judged = fixture.judge_branch(gap, repo, base, head)
        scoring.merge_branch(s, judged)
        s["fixed"] = bool(judged["verified"]) and bool(judged["no_regress"])
        return s
    finally:
        if standin:
            standin.stop()


# ---------------------------------------------------------------- a run

def sanitise(s: dict[str, Any], gap: Any) -> dict[str, Any]:
    """A gap's scores as the data file keeps them. Nothing of a sealed gap's own text goes in: not its id, its
    title, its report, its signals, a file or test name from its fixture, or a branch named after it."""
    out = {"gap": gap.label, "family": gap.family, "trap": gap.trap}
    out.update(s)
    out.pop("items", None)
    if gap.sealed:
        out.pop("branch", None)
        out["erosion"] = sorted({ln.split(": ")[1] for ln in s.get("erosion") or [] if ": " in ln})
        out["notes"] = [n for n in (s.get("notes") or []) if n in fixture.NOTES]
    return out


def run_set(a: argparse.Namespace, gap_list: list[Any]) -> dict[str, Any]:
    arms = list(ARMS) if a.arm == "both" else [a.arm]
    landed = scoring.landed_rows() if not a.landed else {k: k in a.landed.split(",") for k in ("backlog", "pr")}
    budget = Budget(a.budget, arms, a.per_gap)
    results: dict[str, Any] = {}
    claude_version = None
    if "claude-code" in arms:
        claude_version = check_claude(a)
    for arm in arms:
        scores = []
        for i, gap in enumerate(gap_list):
            if budget.stopped(arm):
                scores.append(sanitise({"not_run": "budget", "na": {}}, gap))
                continue
            cap = budget.cap(arm, len(gap_list) - i)
            work = Path(tempfile.mkdtemp(prefix="selfbench-"))
            try:
                s = run_theseus(gap, work, a, cap, landed) if arm == "theseus" else run_claude(gap, work, a, cap)
            except Exception as e:  # one gap's failure is its own row, never the run's end
                s = {"error": type(e).__name__, "error_detail": "" if gap.sealed else str(e)[:300],
                     "na": {c: f"run error: {type(e).__name__}" for c in scoring.COLUMNS}}
                print(f"{arm} {gap.label}: {type(e).__name__}: {str(e)[:200] if not gap.sealed else ''}",
                      file=sys.stderr)
            finally:
                if a.keep and not gap.sealed:
                    print(f"kept {work}", file=sys.stderr)
                else:
                    shutil.rmtree(work, ignore_errors=True)
            budget.book(arm, s.get("cost_usd"))
            s["cap_usd"] = cap
            scores.append(sanitise(s, gap))
        results[arm] = {"scores": scores, "summary": scoring.summarise(scores)}
    return {"date": a.date or date.today().isoformat(), "set": a.set, "demo": a.set == "open",
            "model": MODEL, "commit": git_head(), "budget_usd": a.budget, "spent_usd": budget.total_spent,
            "stopped": any(budget.stopped(x) for x in arms), "landed": landed, "claude_version": claude_version,
            "claude_pin": pinned_claude_version(), "arms": results}


def check_claude(a: argparse.Namespace) -> str | None:
    binary = h2h_claude.find(a.claude)
    if not binary:
        raise SystemExit("claude is not installed (--claude PATH): the baseline arm cannot run")
    a.claude = binary
    v = h2h_claude.version(binary)
    pin = pinned_claude_version()
    if v != pin and not a.allow_version_drift:
        raise SystemExit(f"claude is {v}, the bench pins {pin} (bench/harbor/claude_code_agent.py): "
                         "install the pin or pass --allow-version-drift (the report then says so)")
    return v


def git_head() -> str | None:
    p = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    return p.stdout.strip() or None


def select(a: argparse.Namespace) -> list[Any]:
    if a.set == "open":
        g = gaps_mod.read_open()
    else:
        g = gaps_mod.read_sealed(a.sealed_dir, require_full=not a.gap)
    if a.gap:
        g = [x for x in g if x.id in a.gap or x.label in a.gap]
    if not g:
        raise SystemExit("no gap selected")
    return g


def find_bins(a: argparse.Namespace) -> None:
    if a.bin_dir:
        return
    for d in ("target/release-thin", "target/release", "target/debug"):
        if (ROOT / d / "theseusd").is_file() and (ROOT / d / "theseus").is_file():
            a.bin_dir = str(ROOT / d)
            return
    raise SystemExit("no theseusd and theseus to run: build them or pass --bin-dir")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="run the set")
    r.add_argument("--set", choices=("open", "sealed"), default="open")
    r.add_argument("--sealed-dir", default=None, help=f"[self.sealed] dir; else ${gaps_mod.SEALED_ENV}")
    r.add_argument("--arm", choices=("theseus", "claude-code", "both"), default="both")
    r.add_argument("--budget", type=float, default=DEFAULT_BUDGET, help="the run's line in dollars, across its arms")
    r.add_argument("--per-gap", type=float, default=None, help="one gap's cap (default: twice an equal share)")
    r.add_argument("--wait", type=float, default=900.0, help="seconds one gap may take")
    r.add_argument("--poll", type=float, default=3.0)
    r.add_argument("--bin-dir", default=None)
    r.add_argument("--standin", default=None, metavar="THESEUS_SIM", help="run on `theseus-sim fake-model`")
    r.add_argument("--claude", default=None, help="the claude binary")
    r.add_argument("--allow-version-drift", action="store_true")
    r.add_argument("--landed", default=None, help="rows in the build: backlog,pr (default: read from the tree)")
    r.add_argument("--gap", action="append", help="only this gap (its id, or sealed-NN)")
    r.add_argument("--out", default=None)
    r.add_argument("--date", default=None)
    r.add_argument("--keep", action="store_true", help="keep an open gap's scratch directories")
    p = sub.add_parser("report", help="write the report again from scores.json")
    p.add_argument("scores")
    p.add_argument("--out", default=None)
    a = ap.parse_args(argv)
    import selfreport
    if a.cmd == "report":
        data = json.loads(Path(a.scores).read_text())
        print(selfreport.write(data, Path(a.out or Path(a.scores).parent)))
        return 0
    if a.set == "sealed" and a.keep:
        raise SystemExit("--keep is refused on the sealed set: its scratch holds the gaps' text")
    gap_list = select(a)
    find_bins(a)
    for k in ("bin_dir", "standin", "claude"):  # the arms change directory: paths must not be relative
        if getattr(a, k):
            setattr(a, k, str(Path(getattr(a, k)).resolve()) if os.sep in getattr(a, k) else getattr(a, k))
    data = run_set(a, gap_list)
    out = Path(a.out or f"self-bench-{data['date']}-{a.set}")
    out.mkdir(parents=True, exist_ok=True)
    (out / "scores.json").write_text(json.dumps(data, indent=1) + "\n")
    print(selfreport.write(data, out))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
