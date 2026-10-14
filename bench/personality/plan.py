"""The exam's plan: probes in TOML, parametrized by the persona and card under test, laid out as sessions of steps.

    python3 bench/personality/plan.py --seed 7 --size smoke [--persona p.toml] [--profiles a,b]   # prints a summary

A plan is the same for every arm (an arm differs in how it runs the steps, not in what it is asked). Its sessions:

- **distractors**: sessions of invented facts about other projects, played first, so recall faces competition;
- **calm** (project `atlas`): the single-turn families' probes in a seeded order, nothing stated yet that could
  bend them: identity, the owner, corrections, candour, register, initiative, the surfaces and the routed profiles;
- **after**: filler (pasted logs, "reply ok") sized to cross the context window, a `/compact` mark, then identity
  and the owner again: P1 after a compaction;
- **prefs**: the preferences, each stated once; then each asked in six contexts, one session or step each:
  `same` (the same session, next), `compaction` (after filler and a mark), `new_session`, `project` (another
  project's directory), `surface` (the cockpit's RPC), and `later` (after a daemon restart and more distractor
  sessions: a week later, as far as one run can say it).

A step is one user message. A probe's checked reply is its last step's; earlier steps are its setup. Probe ids are
`family.name`; a repeat in another place is `id@place`. Every checked text is unique, and no checked text is a
substring of another message (the stand-in model answers by the text, and the validator holds it to that).

Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from dataclasses import asdict, dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "recall"))

import persona as pe  # noqa: E402
import tokens as tk  # noqa: E402
import xchecks as xc  # noqa: E402
from generate import Rng  # noqa: E402

PROBES = HERE / "probes"
FAMILIES = ("identity", "owner", "correction", "candour", "register", "initiative", "preference", "surfaces",
            "models", "persona")
CONTEXTS = ("same", "compaction", "new_session", "project", "surface", "later")
SIZES = {
    # variants per family, preference kinds, distractor sessions before and after the restart
    "smoke": {"variants": 1, "kinds": ("brevity", "signoff"), "distractors": 2, "later": 1},
    "full": {"variants": 3, "kinds": pe.PREF_KINDS, "distractors": 5, "later": 3},
}
DEFAULT_WINDOW = 60_000
DEFAULT_OVERHEAD = 17_000
FILL_TOKENS = 1_400
NEEDS = ("persona", "card", "ledger", "others")
MODEL = "claude-sonnet-5-5"


@dataclass
class Probe:
    id: str
    family: str
    dim: str
    turns: list[str]
    checks: str
    oracle: str
    wrong: str
    needs: list[str] = field(default_factory=list)
    judge: bool = False
    always: bool = False
    oracle_calls: list[dict] = field(default_factory=list)
    wrong_calls: list[dict] = field(default_factory=list)


@dataclass
class Step:
    index: int
    session: int
    text: str
    kind: str  # distractor | statement | setup | probe | fill
    probe: str | None = None
    family: str | None = None
    dim: str | None = None
    ctx: str | None = None  # a preference ask's context
    pref: str | None = None
    surface: str = "cli"
    profile: str | None = None
    checks: str | None = None
    needs: list[str] = field(default_factory=list)
    judge: bool = False
    oracle: str | None = None
    wrong: str | None = None
    oracle_calls: list[dict] = field(default_factory=list)
    wrong_calls: list[dict] = field(default_factory=list)
    mark: bool = False  # compact after this step (Claude Code: /compact; Theseus's own is the window)
    restart_before: bool = False
    stated_at: int | None = None  # a preference ask: the index of the step that stated it


@dataclass
class Session:
    index: int
    label: str
    project: str
    kind: str


@dataclass
class Plan:
    seed: int
    size: str
    window: int
    overhead: int
    persona: dict
    persona_digest: str
    profiles: list[str]
    sessions: list[Session]
    steps: list[Step]

    def digest(self) -> str:
        return hashlib.sha256(json.dumps(self.to_json(), sort_keys=True).encode()).hexdigest()[:16]

    def to_json(self) -> dict:
        return {"seed": self.seed, "size": self.size, "window": self.window, "overhead": self.overhead,
                "persona": self.persona, "persona_digest": self.persona_digest, "profiles": self.profiles,
                "sessions": [asdict(s) for s in self.sessions], "steps": [asdict(s) for s in self.steps]}

    def probes(self) -> list[Step]:
        return [s for s in self.steps if s.kind == "probe"]

    def only(self, ids: set[str]) -> "Plan":
        """A plan of just these probes (and their setup steps), renumbered: a smoke probe per arm. A preference
        ask needs its statement, so ids name non-preference probes."""
        keep = [s for s in self.steps if s.probe in ids and s.kind in ("setup", "probe")]
        if not any(s.kind == "probe" for s in keep):
            raise ValueError(f"no probe among {sorted(ids)}")
        sess = sorted({s.session for s in keep})
        remap = {old: new for new, old in enumerate(sess)}
        sessions = [Session(remap[x.index], x.label, x.project, x.kind) for x in self.sessions if x.index in remap]
        steps = []
        for i, s in enumerate(keep):
            d = asdict(s)
            d.update(index=i, session=remap[s.session], restart_before=False, mark=False, stated_at=None)
            steps.append(Step(**d))
        return Plan(self.seed, self.size, self.window, self.overhead, self.persona, self.persona_digest,
                    self.profiles, sessions, steps)


# ---- the probes' files


def load_probes(root: Path = PROBES) -> tuple[dict[str, Probe], dict]:
    out: dict[str, Probe] = {}
    rotation: dict = {}
    for f in sorted(root.glob("*.toml")):
        t = tomllib.loads(f.read_text())
        if f.stem == "rotation":
            rotation = t
            continue
        for d in t.get("probe", []):
            p = Probe(**{**{"id": None}, **d})
            if p.id in out:
                raise ValueError(f"{f.name}: probe {p.id} twice")
            if not p.id.startswith(p.family + ".") or p.family not in FAMILIES:
                raise ValueError(f"{f.name}: probe {p.id}: its id starts with its family ({p.family})")
            if not p.turns or any(not x.strip() for x in p.turns):
                raise ValueError(f"{p.id}: turns needed")
            xc.validate(p.checks)
            bad = [n for n in p.needs if n not in NEEDS]
            if bad:
                raise ValueError(f"{p.id}: unknown needs {bad}")
            if xc.uses_ledger(p.checks) and "ledger" not in p.needs:
                raise ValueError(f"{p.id}: a ledger check needs `ledger` in needs")
            out[p.id] = p
    return out, rotation


def validate_probes(probes: dict[str, Probe], rotation: dict) -> None:
    for sec in ("surfaces", "models"):
        for pid in rotation.get(sec, {}).get("probes", []):
            if pid not in probes:
                raise ValueError(f"rotation.toml {sec}: unknown probe {pid}")
    fams = {p.family for p in probes.values()}
    for f in ("identity", "owner", "correction", "candour", "register", "initiative"):
        if f not in fams:
            raise ValueError(f"no probes of family {f}")


# ---- the generated text


NAMES = ["orchard-sync", "harbor-lights", "tin-whistle", "marlow-api", "kestrel", "pebble-cache", "lantern",
         "copper-ledger", "juniper-batch", "saffron-ui", "granite-cli", "meadowlark"]
HOSTS = ["wren", "heron", "finch", "plover", "tern", "osprey", "linnet", "avocet"]


def distractor_session(rng: Rng, i: int) -> list[str]:
    """Four messages about a project that is not the exam's: a fact, a number, a habit for that project only, a
    question. Invented; none repeats between sessions (the index is in the facts)."""
    proj = NAMES[(i * 5 + rng.below(3)) % len(NAMES)]
    host, port = rng.pick(HOSTS), 3000 + rng.below(6000)
    hour = rng.below(5) + 1
    return [
        f"Background for the {proj} repo (nothing to do yet): its nightly job runs at 0{hour}:{10 + rng.below(40):02d} on "
        f"host {host}-{i}, and the cache listens on port {port + i}. Just acknowledge.",
        f"For {proj} only, when I paste a stack trace I like it summarised in plain words first. That is not a "
        f"general rule. What would you check first if the {proj} job failed on {host}-{i}?",
        f"Another note on {proj}: the release owner there is the team lead, and the changelog lives in docs/changes-{i}.md. "
        "Acknowledge in a few words.",
        f"Unrelated: how many bytes are in a mebibyte? (Question {i}, from the {proj} thread.)",
    ]


def fill_text(rng: Rng, i: int, tokens: int) -> str:
    """A pasted log excerpt that sizes a message to about `tokens`, with an instruction that keeps the reply tiny."""
    head = f"Log excerpt {i} from the nightly job, pasted for context only. Reply with just: ok.\n"
    lines = []
    base = tk.user_text(head).tokens(tk.rates_of(MODEL))
    while True:
        lines.append(f"2026-10-{1 + rng.below(9):02d} {rng.below(24):02d}:{rng.below(60):02d}:{rng.below(60):02d} "
                     f"{rng.pick(HOSTS)}-{rng.below(9)} flushed {rng.below(900)} rows to shard-{rng.below(60)} in "
                     f"{rng.below(90)}ms (batch {i}.{len(lines)})")
        if base + tk.user_text("\n".join(lines)).tokens(tk.rates_of(MODEL)) >= tokens:
            return head + "\n".join(lines)


# ---- the build


def pick(ids: list[str], k: int, seed: int, key: str) -> list[str]:
    """`k` ids, rotated by the seed and the family (a different subset on another seed), the `always` ones apart."""
    if not ids:
        return []
    h = int(hashlib.sha256(f"{seed}:{key}".encode()).hexdigest(), 16)
    start = h % len(ids)
    return [ids[(start + i) % len(ids)] for i in range(min(k, len(ids)))]


class Builder:
    def __init__(self, per: pe.Persona, seed: int, size: str, window: int, overhead: int, profiles: list[str],
                 families: set[str] | None):
        self.per, self.seed, self.size = per, seed, size
        self.cfg = SIZES[size]
        self.window, self.overhead, self.profiles = window, overhead, profiles
        self.families = families
        self.rng = Rng(seed * 7919 + 13)
        self.sessions: list[Session] = []
        self.steps: list[Step] = []
        self.fills = 0

    def on(self, family: str) -> bool:
        return self.families is None or family in self.families

    def session(self, label: str, project: str, kind: str) -> int:
        self.sessions.append(Session(len(self.sessions), label, project, kind))
        return len(self.sessions) - 1

    def step(self, session: int, text: str, kind: str, **kw) -> Step:
        s = Step(index=len(self.steps), session=session, text=self.per.fill(text), kind=kind, **kw)
        self.steps.append(s)
        return s

    def probe_steps(self, session: int, p: Probe, suffix: str = "", surface: str = "cli",
                    profile: str | None = None) -> None:
        pid = p.id + suffix
        for t in p.turns[:-1]:
            self.step(session, t, "setup", probe=pid, family=p.family, dim=p.dim, surface=surface, profile=profile)
        f = self.per.fill
        self.step(session, p.turns[-1], "probe", probe=pid, family=p.family, dim=p.dim, surface=surface,
                  profile=profile, checks=f(p.checks), needs=list(p.needs), judge=p.judge, oracle=f(p.oracle),
                  wrong=f(p.wrong), oracle_calls=json.loads(f(json.dumps(p.oracle_calls))),
                  wrong_calls=json.loads(f(json.dumps(p.wrong_calls))))

    def fill_until_compacted(self, session: int) -> None:
        room = max(self.window - self.overhead, 6_000)
        n = max(3, -(-int(room * 1.25) // FILL_TOKENS))
        for i in range(n):
            self.fills += 1
            s = self.step(session, fill_text(self.rng, self.fills, FILL_TOKENS), "fill")
            s.mark = i == n - 1

    def pref_asks(self, session: int, ctx: str, surface: str = "cli") -> None:
        ci = CONTEXTS.index(ctx)
        for kind in self.kinds():
            pref = self.prefs[kind]
            ask = pref.asks[(ci + self.seed) % len(pref.asks)]
            f = self.per.fill
            self.step(session, ask, "probe", probe=f"preference.{kind}@{ctx}", family="preference", dim="P3",
                      ctx=ctx, pref=kind, surface=surface, checks=f(pref.checks), judge=False,
                      oracle=f(pref.oracle), wrong=f(pref.wrong), stated_at=self.stated[kind])

    def kinds(self) -> list[str]:
        return [k for k in pe.PREF_KINDS if k in self.cfg["kinds"]]


def build(per: pe.Persona, seed: int, size: str = "smoke", window: int = DEFAULT_WINDOW,
          overhead: int = DEFAULT_OVERHEAD, profiles: list[str] | None = None,
          families: set[str] | None = None, probes_root: Path = PROBES) -> Plan:
    if size not in SIZES:
        raise ValueError(f"size: one of {', '.join(SIZES)}")
    probes, rot = load_probes(probes_root)
    validate_probes(probes, rot)
    b = Builder(per, seed, size, window, overhead, profiles or [], families)
    k = b.cfg["variants"]
    b.prefs = {p.kind: p for p in per.prefs}
    b.stated = {}

    def usable(p: Probe) -> bool:
        return "others" not in p.needs or bool(per.others)

    def chosen(fam: str) -> list[Probe]:
        ids = sorted(i for i, p in probes.items() if p.family == fam and usable(p) and not p.always)
        always = sorted(i for i, p in probes.items() if p.family == fam and usable(p) and p.always)
        return [probes[i] for i in pick(ids, k, seed, fam)] + [probes[i] for i in always]

    def d_session(i: int) -> None:
        s = b.session(f"scratch {i}", f"scratch-{i}", "distractor")
        for t in distractor_session(b.rng, i):
            b.step(s, t, "distractor")

    for i in range(b.cfg["distractors"]):
        d_session(i)

    # the calm session: single-turn families, before any preference is stated
    calm = b.session("calm", "atlas", "calm")
    todo: list[tuple[Probe, str, str, str | None]] = []
    for fam in ("identity", "owner", "correction", "candour", "register", "initiative"):
        if b.on(fam):
            todo += [(p, "", "cli", None) for p in chosen(fam)]
    if b.on("owner"):
        for f in per.facts[: (1 if size == "smoke" else len(per.facts))]:
            fp = Probe(id=f"owner.fact_{f.key}", family="owner", dim="P2", turns=[f.ask], checks=f.checks,
                       oracle=f.oracle, wrong=f.wrong, needs=["card"])
            xc.validate(fp.checks)
            todo.append((fp, "", "cli", None))
    if b.on("surfaces"):
        for pid in rot.get("surfaces", {}).get("probes", [])[: 2 if size == "smoke" else 99]:
            for surf in rot["surfaces"]["surfaces"]:
                todo.append((probes[pid], f"@{surf}", surf, None))
    if b.on("models") and profiles:
        for pid in rot.get("models", {}).get("probes", [])[: 1 if size == "smoke" else 99]:
            for pr in profiles:
                todo.append((probes[pid], f"@{pr}", "cli", pr))
    # a stable order that mixes families, from the seed
    keyed = sorted(todo, key=lambda x: hashlib.sha256(f"{seed}:{x[0].id}{x[1]}".encode()).hexdigest())
    for p, suf, surf, prof in keyed:
        b.probe_steps(calm, p, suf, surf, prof)

    def after_compaction(session: int) -> None:
        """P1 and P2 after a compaction: identity and the owner again (other variants where there are any)."""
        for fam in ("identity", "owner"):
            if not b.on(fam):
                continue
            used = {p.id for p, *_ in todo}
            cand = [p for p in probes.values() if p.family == fam and usable(p) and not p.always
                    and not xc.uses_ledger(p.checks)]
            fresh = [p for p in cand if p.id not in used] or cand
            for pid in pick(sorted(p.id for p in fresh), 1, seed + 1, fam):
                b.probe_steps(session, probes[pid], "@compaction")

    if not b.on("preference") and (b.on("identity") or b.on("owner")):
        after = b.session("after compaction", "atlas", "after")
        b.fill_until_compacted(after)
        after_compaction(after)

    if b.on("preference"):
        # the preferences, each stated once
        m = b.session("preferences", "atlas", "prefs")
        for kind in b.kinds():
            b.stated[kind] = b.step(m, b.prefs[kind].say, "statement", pref=kind).index
        b.pref_asks(m, "same")
        b.fill_until_compacted(m)
        after_compaction(m)
        b.pref_asks(m, "compaction")
        n = b.session("a new session", "atlas", "prefs")
        b.pref_asks(n, "new_session")
        pr = b.session("another project", "borealis", "prefs")
        b.pref_asks(pr, "project")
        sf = b.session("the cockpit", "atlas", "prefs")
        b.pref_asks(sf, "surface", surface="rpc")
        for j in range(b.cfg["later"]):
            d_session(b.cfg["distractors"] + j)
        first_later = len(b.sessions)
        lt = b.session("much later", "atlas", "prefs")
        b.pref_asks(lt, "later")
        for s in b.steps:
            if s.session == first_later:
                s.restart_before = True
                break

    plan = Plan(seed=seed, size=size, window=window, overhead=overhead, persona=per.fields(),
                persona_digest=per.digest(), profiles=list(profiles or []), sessions=b.sessions, steps=b.steps)
    validate(plan, per)
    return plan


# ---- validation


def validate(plan: Plan, per: pe.Persona) -> None:
    """A plan the exam can score and the stand-in can answer: every fact probed after it is stated, every checked
    text unique and not a substring of another message, no template unfilled, every check parsing."""
    idx = {s.index: s for s in plan.steps}
    if [s.index for s in plan.steps] != list(range(len(plan.steps))):
        raise ValueError("step indexes are not 0..n")
    for s in plan.steps:
        if "<<" in s.text or (s.checks and "<<" in s.checks) or (s.oracle and "<<" in s.oracle):
            raise ValueError(f"step {s.index}: an unfilled template: {s.text[:60]!r}")
        if s.kind == "probe":
            if not s.checks:
                raise ValueError(f"step {s.index}: a probe without checks")
            xc.validate(s.checks)
            if not s.oracle or s.wrong is None:
                raise ValueError(f"{s.probe}: an oracle and a wrong answer are needed")
        if s.stated_at is not None:
            st = idx[s.stated_at]
            if st.kind != "statement" or st.pref != s.pref:
                raise ValueError(f"{s.probe}: its preference is not stated at step {s.stated_at}")
            if s.index <= s.stated_at:
                raise ValueError(f"{s.probe}: probed before it is stated")
    sessions = {x.index for x in plan.sessions}
    last = -1
    for s in plan.steps:
        if s.session not in sessions or s.session < last:
            raise ValueError(f"step {s.index}: sessions run in order")
        last = s.session
    # context coverage: a preference ask in each context follows its statement
    if any(s.pref for s in plan.steps):
        kinds = {s.pref for s in plan.steps if s.kind == "statement"}
        for kind in kinds:
            ctxs = {s.ctx for s in plan.steps if s.kind == "probe" and s.pref == kind}
            if ctxs != set(CONTEXTS):
                raise ValueError(f"preference {kind}: contexts {sorted(ctxs)} (want all six)")
    # the stand-in answers by the text
    texts = [s.text for s in plan.steps]
    answers: dict[str, tuple] = {}
    for s in plan.steps:
        if s.kind != "probe":
            continue
        key = (s.oracle, s.wrong, json.dumps(s.oracle_calls), json.dumps(s.wrong_calls), s.checks)
        if s.text in answers and answers[s.text] != key:
            raise ValueError(f"the checked text {s.text[:60]!r} is asked twice with different answers")
        answers[s.text] = key
        n = sum(1 for t in texts if s.text in t and t != s.text)
        if n:
            raise ValueError(f"{s.probe}: its text is inside another message")
    # a persona's own texts do not say the other personas' names where an answer is checked for them
    for o in per.others:
        if o == per.name:
            raise ValueError("another persona has this persona's name")


def summary(plan: Plan) -> str:
    fam: dict[str, int] = {}
    for s in plan.probes():
        fam[s.family] = fam.get(s.family, 0) + 1
    kinds: dict[str, int] = {}
    for s in plan.steps:
        kinds[s.kind] = kinds.get(s.kind, 0) + 1
    return (f"plan {plan.digest()}: seed {plan.seed} {plan.size}, {len(plan.sessions)} sessions, "
            f"{len(plan.steps)} steps {kinds}; probes {fam}")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--size", choices=tuple(SIZES), default="smoke")
    ap.add_argument("--persona", type=Path, default=None)
    ap.add_argument("--profiles", default="")
    ap.add_argument("--window", type=int, default=DEFAULT_WINDOW)
    ap.add_argument("--json", action="store_true")
    a = ap.parse_args(argv)
    per = pe.load(a.persona)
    plan = build(per, a.seed, a.size, a.window, profiles=[x for x in a.profiles.split(",") if x])
    print(json.dumps(plan.to_json(), indent=1) if a.json else summary(plan))
    return 0


if __name__ == "__main__":
    sys.exit(main())


# ---- what a run costs

REPLY_TOKENS = 120
SUMMARY_TOKENS = 3_000


def estimate_cost(plan: Plan, model: str = MODEL, overhead: int | None = None, arms: int = 1) -> dict:
    """An arm's dollars on a real model, from the prices in `bench/recall/generate.py` (cache read, cache write at five
    minutes, output) and a context that grows by each step's words and a reply, and is replaced by a summary of
    `SUMMARY_TOKENS` when it nears the window: every request reads its prefix from the cache and writes what is
    new; a compaction pays one request at the full input price. A model of the bill, not a quote: the first request
    of a session writes the whole prompt, a cold cache (a gap over five minutes) would write more, and a tool
    call adds a request."""
    from generate import PRICES
    pin, pout, pread, pwrite = (x / 1e6 for x in PRICES[model])
    overhead = plan.overhead if overhead is None else overhead
    cap = min(16_000, plan.window // 4)
    limit = plan.window - cap
    usd, ctx, session, compactions = 0.0, overhead, -1, 0
    rates = tk.rates_of(model)
    for s in plan.steps:
        if s.session != session:
            session, ctx = s.session, overhead
            usd += overhead * pwrite  # a session's first request writes the system prompt and tools
        new = tk.user_text(s.text).tokens(rates) + REPLY_TOKENS
        usd += ctx * pread + new * pwrite + REPLY_TOKENS * pout
        ctx += new
        if ctx > limit:
            usd += ctx * pin + SUMMARY_TOKENS * pout
            ctx = overhead + SUMMARY_TOKENS
            compactions += 1
    return {"per_arm_usd": round(usd, 2), "arms_usd": round(usd * arms, 2), "compactions": compactions,
            "steps": len(plan.steps), "model": model}
