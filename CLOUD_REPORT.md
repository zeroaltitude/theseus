# CLOUD_REPORT: trim-tests-dedup (theseus-rwpbg.1)

Branch `cloud/20261017-trim-tests-dedup`, base `432bbbeb`. Two commits, both pure moves of test helpers: no test,
assertion, timeout, or fixture changed, and no production code. Net: 83 files, +824 / −2,839 lines (−2,015),
new files included (`common/fixture.rs` 129, `common/rpc.rs` 54, `test_support.rs` 212).

## Step 1: theseusd, `tests/common/` (41ce1ed2)

**Found.** At this base the identical-body tier is: `call` 14+6+4+4+3+2+2, `spawn` 9+2, `wait` 10+4+3+2, `table` 36,
`path` 33, `log` 19+7, `tail` 12, `FAKE_OP` 20+4 (script below; numbers differ a little from the review's).
Most are *methods* of a per-file `struct Rig { dir: TempDir, .. }`, so they cannot move as free fns.

**Changed.**
- Free items moved to `common/mod.rs`: `table`, `tail`, `FAKE_OP` (the `tv-<item>` op) and `FAKE_OP_FLAT` (the
  fixed-value op, 4 copies). `safe_note` uses the shared `table`.
- `common/rpc.rs`: `call` (60 s, error as the `error` value), `call_within` (secs given; used by `call` and by the
  two 120 s rigs), `call_text` (error as text, 2 copies).
- `common/fixture.rs`: trait `Fixture` (a rig says `fn dir(&self) -> &Path`; the trait gives `path`, `log`,
  `log_tail`, `call`, `wait`, `spawn`, `spawn_open_at_bind`, and an overridable `sock`). A method a rig defines
  itself wins over the trait's, so a rig with a different body keeps it. 37 rigs implement it.
- Copies deleted when identical to the shared one; variants with 2+ copies became a named shared fn plus a
  one-line delegate (`log` tail-30 → `log_tail`; 120 s call; text-error call; `spawn` with
  `THESEUS_TEST_OPEN_AT_BIND`; `projects/sock` rigs override `sock`). `job_wrapper.rs` gained `mod common;`.
- Equivalences I treated as identical (same behaviour, not byte-identical): `self.dir.path().join("sock")` ≡
  `self.path("sock")` (3 rigs); the `wait` that inlines `lines[len-30..]` ≡ `tail(&log, 30)` (3 rigs).

**Left (not identical-tier or not worth a trait method):** `wait` x4 (30 s/10 ms, broker, job_approval,
mcp_server, reaping) and x2 (whole log, capture, judge); the one-off `wait`/`spawn`/`call` bodies; `log` x7's
delegates; `task_start.rs`'s `table` (parameter named `k`); free `call` in cgroup/crash/early_signal/steps/
postmortem_incident (different bodies). A known soft spot: the trait's `spawn` waits with the trait's `wait`, so for
capture.rs/judge.rs (own `wait`) only the message of a failed socket wait differs (whole log vs last 30 lines).

**Proof.** `cargo nextest run -p theseusd` before (clean base) and after, run with `TZ=America/Phoenix`:
303 tests run, 290 passed, 13 failed, 2 skipped, *both times*, and `diff` of the two failure lists is empty (the 13 are
the known L1/sandbox ones). A first "after" run showed 55 failures: the disk had filled (ENOSPC), so those runs were
discarded and redone on a free disk. Pure move: no planted revert.

## Step 2: theseus-core, `test_support` (33b6d5b9)

**Found.** Free-fn identical groups: `turn` x8, x4, x3, x2; `rows` x5, x4, x4, x3, x2 (×6 more groups); `until` x7,
x4, x2. The `&self` method copies (`turn`, `rows` on per-file rigs) are different types and were left.

**Changed.** `#[cfg(test)] pub(crate) mod test_support` (`lib.rs` +2 lines). One fn per distinct body, named for what
it reads: `turn`, `turn_in` (Option session), `try_turn` (anyhow result); `rows` (Value, last 10,000),
`rows_100k`, `rows_2k`, `ledger_rows_2k` / `ledger_rows_100k` / `ledger_rows_5k` (whole `LedgerRow`),
`session_rows_1k`, `recall_rows`; `until` (20 s, 10 ms), `until_within` (secs, 20 ms), `until_within_10ms`.
Callers `use crate::test_support::x [as old_name]`; a helper another module imported from the defining file keeps a
`pub(crate) use` there. Two copies that are the same code with other parameter names or layout were folded in
(`rows(c, ..)` ×2 into `rows`; `turn` with a typed `get_session::<SessionRecord>` ×2 into `turn`).

**Windows are not merged.** The `rows` windows (last N rows): 2,000 (4 + 2 + ...), 5,000 (2), 10,000 (5 + 2), 100,000
(4 + 3), 1,000 (2, per session), plus scope-based ones; 59 `rows` copies/50 bodies remain elsewhere. A later pass may
make one whole-ledger `rows`.

**Left:** `rows(store, ..)` in judge/people tests (uses a local `SCOPE`), `tests_capture`/`tests_route_correct`'s
`rows` (uses a local `kinds`), and every one-off body; `&self` methods.

**Proof.** `cargo nextest run -p theseus-core` with `TZ=America/Phoenix`: 3,047 run, 3,046 passed, 1 failed:
`tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused` (asserts 200 lines,
saw 1; it touches none of the moved helpers and reads the clock). It passed alone, and 3 more times in the whole
`tests_horizon` module (10/10). Treat as a load flake; it is not on the flaky list. Pure move: no planted revert.

## Proof nothing was lost

- Test names: no `#[test]`, `#[tokio::test]` or `#[ignore]` line appears in `git diff 58ffce71 HEAD`; the only fn
  names removed are the helpers (`table 33, path 33, call 31, rows 26, log 19, turn 17, wait 13, until 13, tail 12,
  spawn 9`) and the only added are the shared ones. Before the change `cargo nextest list --workspace` showed 6,333
  tests (saved). I could not re-list after: the VM's disk allowance (about 40 GB) fills with debug test binaries and I
  had to delete them; the two per-crate runs above (303 = 303, and 3,047 + 21 skipped = the listed 3,068) stand in
  for it. The maintainer's `cargo nextest list --workspace` diff should be empty.
- Keel guard: `python3 scripts/keel-guard.py --base 432bbbeb --head HEAD --quiet` prints nothing (rc 0). (Against
  this clone's stale `origin/main` it reports three erosions that are main's own history, not this branch's.)
- No new dependency; Cargo.lock untouched. No `allow`/`expect` added.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in its first phases here: the `keel`/`door` phases
judge against `origin/main`, which in this clone is behind `432bbbeb`, so they report main's own history. I ran the
rest by hand: `cargo fmt --all -- --check` ok; `scripts/shape.sh` ok; `cargo clippy --workspace --all-targets -q --
-D warnings` ok; theseusd and theseus-core suites as above. Not run: the cockpit phase, `cargo deny`, the other
crates' suites (nothing outside the two crates changed).

## Live check for the maintainer

None needs keys. After merging: `cargo nextest list --workspace --message-format oneline | sort` must equal the
base's; `TZ=America/Phoenix cargo nextest run -p theseusd -p theseus-core` should show only the known L1/sandbox
failures (13 in theseusd here).

## Not done / for the owner

- `THESEUS_OPERATOR_UMASK` scrub drift (listed, nothing changed): of 66 files that start a daemon, only
  aws.rs, crash.rs, early_signal.rs, extend.rs, hands.rs, sandbox.rs, tender.rs and `common/mod.rs` scrub it. Every
  `Fixture::spawn` (and its older copies) skips it.
- Doc: `crates/theseusd/AGENTS.md` ("Tests") should name `common::Fixture`, `common::rpc`, `table`/`tail`/`FAKE_OP`;
  `crates/theseus-core/AGENTS.md` should name `test_support`.
- On a moved main re-run the scripts below (they ignore anything already moved).

## The scripts (kept in /tmp, not committed)

### group.py
```python
#!/usr/bin/env python3
"""Group helper fn definitions (and consts) by whitespace-normalised body.
usage: group.py 'glob1,glob2' name[,name...] [-v]"""
import re, sys, glob, collections

def bodies(path, name):
    src = open(path).read()
    out = []
    for m in re.finditer(r'^([ \t]*)((?:pub(?:\([a-z]+\))? )?(?:async )?fn %s\b)' % re.escape(name), src, re.M):
        i = src.index('{', m.start())
        depth, j = 0, i
        while j < len(src):
            c = src[j]
            if c == '{': depth += 1
            elif c == '}':
                depth -= 1
                if depth == 0: break
            j += 1
        text = src[m.start():j + 1]
        out.append((src.count('\n', 0, m.start()) + 1, text))
    return out

def consts(path, name):
    src = open(path).read()
    return [(src.count('\n', 0, m.start()) + 1, m.group(0)) for m in
            re.finditer(r'^[ \t]*(?:pub(?:\([a-z]+\))? )?const %s\b.*?";\n' % re.escape(name), src, re.M | re.S)]

def norm(t):
    t = re.sub(r'\s+', ' ', t.strip())
    return re.sub(r'^pub(\([a-z]+\))? ', '', t)  # visibility is not part of a body

if __name__ == '__main__':
    files = sorted(sum((glob.glob(g, recursive=True) for g in sys.argv[1].split(',')), []))
    for name in sys.argv[2].split(','):
        groups = collections.defaultdict(list)
        total = 0
        for f in files:
            for line, text in (consts(f, name) if name.isupper() else bodies(f, name)):
                groups[norm(text)].append((f, line, text.count('\n') + 1))
                total += 1
        removed = sum((len(v) - 1) * v[0][2] for v in groups.values())
        print(f'== {name}: {total} copies, {len(groups)} distinct, {removed} lines removable keeping one per group')
        for k, v in sorted(groups.items(), key=lambda kv: -len(kv[1])):
            print(f'  x{len(v)} ({v[0][2]} lines): ' + k[:100])
            if '-v' in sys.argv:
                for f, l, _ in v: print(f'      {f}:{l}')
```

### dedup_theseusd.py
```python
#!/usr/bin/env python3
"""theseusd tests: drop each helper copy that is identical (after whitespace
normalisation) to the one in tests/common, and say where the common one comes
from. Run from the repo root.  usage: dedup_theseusd.py [--dry]"""
import re, sys, glob, os
sys.path.insert(0, '/tmp')
from group import bodies, consts, norm

T = 'crates/theseusd/tests/'
DRY = '--dry' in sys.argv

def method(file, name, indent):
    """the (norm, text) of fn `name` at `indent` spaces in `file`, first one"""
    src = open(T + file).read()
    for line, text in bodies(T + file, name):
        if text.startswith(' ' * indent + 'fn') or text.startswith(' ' * indent + 'pub fn'):
            if len(text) - len(text.lstrip(' ')) == indent:
                return norm(text)
    raise SystemExit(f'no {name} in {file}')

CANON = {
    # name: {norm: action}   action: 'drop' | ('sock', path) | ('replace', text)
    'path': {method('ask.rs', 'path', 4): 'drop'},
    'log': {method('ask.rs', 'log', 4): 'drop',
            method('broker.rs', 'log', 4): ('replace', '    fn log(&self) -> String {\n        self.log_tail()\n    }\n')},
    'call': {method('ask.rs', 'call', 4): 'drop',
             method('capture.rs', 'call', 4): 'drop',
             method('files.rs', 'call', 4): 'drop',
             method('earned.rs', 'call', 4): 'drop-projects',
             method('job_handles.rs', 'call', 4): ('replace',
                 '    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {\n'
                 '        common::rpc::call_within(&self.sock(), 120, method, params)\n    }\n'),
             method('cancel_frames.rs', 'call', 4): ('replace',
                 '    fn call(&self, method: &str, params: Value) -> Result<Value, String> {\n'
                 '        common::rpc::call_text(&self.sock(), method, params)\n    }\n')},
    'wait': {method('ask.rs', 'wait', 4): 'drop', method('job_handles.rs', 'wait', 4): 'drop'},
    'spawn': {method('ask.rs', 'spawn', 4): 'drop',
              method('tasks.rs', 'spawn', 4): ('replace',
                  '    fn spawn(&self) -> Daemon {\n        self.spawn_open_at_bind()\n    }\n')},
}
FREE = {
    'call': {method('capture.rs', 'call', 0): 'drop'},  # fn call(sock: &Path, ...)
    'table': {method('ask.rs', 'table', 8): 'drop'},
    'tail': {method('ask.rs', 'tail', 0): 'drop'},
}
# (the `call(sock: &Path...)` free fn has the same text as rpc::call modulo its
# 60 s constant, and the one in task_start.rs differs in its argument name)

def fn_span(src, line, text):
    """start, end offsets of a definition, with its doc comment / attributes and one blank line"""
    lines = src.split('\n')
    start_line = line - 1
    while start_line > 0 and re.match(r'\s*(///|#\[)', lines[start_line - 1]):
        start_line -= 1
    end_line = line - 1 + text.count('\n')  # last line index (inclusive)
    # text may start after indentation: bodies() starts at the line start, so exact
    begin = sum(len(l) + 1 for l in lines[:start_line])
    end = sum(len(l) + 1 for l in lines[:end_line + 1])
    if end_line + 1 < len(lines) and lines[end_line + 1].strip() == '':
        end += len(lines[end_line + 1]) + 1
    elif start_line > 0 and lines[start_line - 1].strip() == '':
        begin -= len(lines[start_line - 1]) + 1
    return begin, end

def process(file):
    path = T + file
    src = open(path).read()
    orig = src
    need_fixture = False
    sock_override = None
    edits = []  # (begin, end, replacement)
    # methods in impl blocks (4-space) and free fns (0 / 8 spaces)
    for name, actions in CANON.items():
        for line, text in bodies(path, name):
            indent = len(text) - len(text.lstrip(' '))
            if indent != 4:
                continue
            a = actions.get(norm(text))
            if a is None:
                continue
            b, e = fn_span(src, line, text)
            if a == 'drop':
                edits.append((b, e, ''))
            elif a == 'drop-projects':
                edits.append((b, e, ''))
                sock_override = 'projects/sock'
            else:
                s0, e0 = fn_span(src, line, text)
                edits.append((b, e, a[1] + '\n'))
            need_fixture = True
    for name, actions in FREE.items():
        for line, text in bodies(path, name):
            indent = len(text) - len(text.lstrip(' '))
            if indent == 4:
                continue
            a = actions.get(norm(text))
            if a is None:
                continue
            b, e = fn_span(src, line, text)
            edits.append((b, e, ''))
    # FAKE_OP consts
    for line, text in consts(path, 'FAKE_OP'):
        n = norm(text)
        kind = 'FAKE_OP' if 'tv-' in n else 'FAKE_OP_FLAT'
        lines = src.split('\n')
        sl = line - 1
        while sl > 0 and lines[sl - 1].lstrip().startswith('///'):
            sl -= 1
        el = line - 1 + text.rstrip('\n').count('\n')
        b = sum(len(l) + 1 for l in lines[:sl])
        e = sum(len(l) + 1 for l in lines[:el + 1])
        if el + 1 < len(lines) and lines[el + 1].strip() == '':
            e += len(lines[el + 1]) + 1
        edits.append((b, e, ''))
        if kind == 'FAKE_OP_FLAT':
            edits.append(('rename', 'FAKE_OP', 'FAKE_OP_FLAT'))
        used_const = kind
    if not edits:
        return None
    ren = [x for x in edits if x[0] == 'rename']
    real = sorted([x for x in edits if x[0] != 'rename'], reverse=True)
    for b, e, r in real:
        src = src[:b] + r + src[e:]
    for _, a, b in ren:
        src = re.sub(r'\b%s\b' % a, b, src)
    # imports
    uses = []
    if 'FAKE_OP_FLAT' in src: uses.append('FAKE_OP_FLAT')
    elif re.search(r'\bFAKE_OP\b', src) and not consts(path, 'FAKE_OP') and 'FAKE_OP' in orig: uses.append('FAKE_OP')
    for n in ('table', 'tail'):
        if re.search(r'\b%s\(' % n, src) and not re.search(r'fn %s\b' % n, src): uses.append(n)
    if need_fixture:
        uses.append('Fixture')
    uses = sorted(set(uses), key=lambda x: (x[0].isupper(), x))
    if uses:
        imp = 'use common::{%s};\n' % ', '.join(sorted(uses)) if len(uses) > 1 else 'use common::%s;\n' % uses[0]
        m = re.search(r'^mod common;\n', src, re.M)
        if m:
            src = src[:m.end()] + '\n' + imp + src[m.end():]
        else:
            m = re.search(r'^use ', src, re.M)
            src = src[:m.start()] + 'mod common;\n\n' + imp + src[m.start():]
    if need_fixture:
        impl = ('impl Fixture for Rig {\n    fn dir(&self) -> &std::path::Path {\n        self.dir.path()\n    }\n')
        if sock_override:
            impl += '\n    fn sock(&self) -> std::path::PathBuf {\n        self.path("%s")\n    }\n' % sock_override
        impl += '}\n\n'
        m = re.search(r'^impl Rig \{', src, re.M)
        src = src[:m.start()] + impl + src[m.start():]
    if not DRY:
        open(path, 'w').write(src)
    return True

if __name__ == '__main__':
    for f in sorted(os.listdir(T)):
        if f.endswith('.rs'):
            if process(f): print('edited', f)
```

### dedup_core.py
```python
#!/usr/bin/env python3
"""theseus-core tests: move each identical free-fn helper group of `turn`,
`rows`, `until` into src/test_support.rs. Run from the repo root.
usage: dedup_core.py [--dry]"""
import re, sys, glob, collections
sys.path.insert(0, '/tmp')
from group import bodies, norm

SRC = 'crates/theseus-core/src/'
DRY = '--dry' in sys.argv
# (helper, representative file) -> name in test_support; `fold` lists files whose
# copy is the same code with other parameter names / layout (see the report)
SPEC = {
    ('turn', 'tests_arrangement.rs'): ('turn', ['tests_kept.rs', 'tests_recall.rs']),
    ('turn', 'tests_approvals.rs'): ('turn_in', []),
    ('turn', 'tests_books.rs'): ('try_turn', []),
    ('rows', 'tests_places.rs'): ('rows', ['rpc/tests_self_backlog.rs', 'rpc/tests_self_graduate.rs']),
    ('rows', 'card/tests_job.rs'): ('ledger_rows_2k', []),
    ('rows', 'tests_initiative.rs'): ('rows_100k', []),
    ('rows', 'tests_arrangement.rs'): ('ledger_rows_100k', []),
    ('rows', 'tests_books.rs'): ('session_rows_1k', []),
    ('rows', 'tests_cite.rs'): ('recall_rows', []),
    ('rows', 'tests_compaction.rs'): ('rows_2k', []),
    ('rows', 'tests_day_ceiling.rs'): ('ledger_rows_5k', []),
    ('until', 'tests_horizon.rs'): ('until_within', []),
    ('until', 'tests_external.rs'): ('until', []),
    ('until', 'tests_arrangement.rs'): ('until_within_10ms', []),
}

def free_fns(f, name):
    out = []
    for line, text in bodies(SRC + f, name):
        if text.startswith(('fn', 'pub', 'async')):
            out.append((line, text))
    return out

def span(src, line, text):
    lines = src.split('\n')
    s = line - 1
    while s > 0 and re.match(r'\s*(///|#\[)', lines[s - 1]):
        s -= 1
    e = line - 1 + text.count('\n')
    b = sum(len(l) + 1 for l in lines[:s])
    en = sum(len(l) + 1 for l in lines[:e + 1])
    if e + 1 < len(lines) and lines[e + 1].strip() == '':
        en += len(lines[e + 1]) + 1
    elif s > 0 and lines[s - 1].strip() == '':
        b -= len(lines[s - 1]) + 1
    return b, en

allfiles = sorted(g[len(SRC):] for g in glob.glob(SRC + '**/*.rs', recursive=True))
support = []
per_file = collections.defaultdict(list)  # file -> [(line, text, newname, localname)]
for (helper, rep), (newname, fold) in SPEC.items():
    (rl, rt), = [x for x in free_fns(rep, helper)]
    key = norm(rt)
    keys = {key}
    for f in fold:
        keys.add(norm(free_fns(f, helper)[0][1]))
    # the shared definition: the representative's text, doc comment kept, renamed
    src = open(SRC + rep).read()
    b, e = span(src, rl, rt)
    doc = ''.join(l + '\n' for l in src.split('\n')[:rl - 1][::-1] if False)
    lines = src.split('\n')
    s = rl - 1
    while s > 0 and re.match(r'\s*///', lines[s - 1]):
        s -= 1
    docs = '\n'.join(lines[s:rl - 1])
    body = re.sub(r'^(pub(\([a-z]+\))? )?(async )?fn %s\b' % helper,
                  lambda m: 'pub(crate) ' + (m.group(3) or '') + 'fn ' + newname, rt, count=1)
    support.append((docs + '\n' if docs else '') + body)
    for f in allfiles:
        for line, text in free_fns(f, helper):
            if norm(text) in keys:
                per_file[f].append((line, text, newname, helper))

for f, items in sorted(per_file.items()):
    src = open(SRC + f).read()
    spans = sorted([(span(src, l, t), n, h) for l, t, n, h in items], reverse=True)
    uses, pubs = [], []
    for (b, e), n, h in spans:
        vis = src[b:e].lstrip().startswith(('pub ', 'pub('))
        if not vis:
            m = re.search(r'^[ \t]*(pub(\([a-z]+\))? )?(async )?fn ', src[b:e], re.M)
            vis = bool(m and m.group(1))
        src = src[:b] + src[e:]
        (pubs if vis else uses).append(n if n == h else f'{n} as {h}')
    def stmt(vis, xs):
        xs.sort()
        return '' if not xs else '%suse crate::test_support::%s;\n' % (vis, xs[0] if len(xs) == 1 else '{%s}' % ', '.join(xs))
    imp = stmt('', uses) + stmt('pub(crate) ', pubs)
    last = None
    for m in re.finditer(r'^use [^;]*;\n', src, re.M | re.S):
        last = m
    if last is None:
        raise SystemExit(f'{f}: no use')
    src = src[:last.end()] + imp + src[last.end():]
    print('edit', f, uses)
    if not DRY:
        open(SRC + f, 'w').write(src)

header = '''//! What the core's tests share: a scripted turn on a test core, the ledger's
//! rows of a kind (each helper named for the window it reads), and a poll.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use crate::Core;

'''
if not DRY:
    open(SRC + 'test_support.rs', 'w').write(header + '\n\n'.join(support) + '\n')
```

Follow-ups after each script: `cargo fmt`, `cargo fix --allow-dirty --tests -p <crate>` (unused imports), then add `pub use fixture::Fixture;` / `pub mod` lines if missing, and fix any `FAKE_OP` import by hand.
