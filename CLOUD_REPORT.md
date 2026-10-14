# CLOUD REPORT: scoped-grants (theseus-drjs)

Branch `cloud/20261013-scoped-grants`, on `a2474be` (main at `0baeed6`, store format 33). Started 06:36 UTC, report
08:20 UTC. Commits, oldest first:

- `2136441` policy: scoped grants: "yes, and don't ask again" for a command's prefix or edits in the project, for the
  conversation; listed and revocable
- `d0cecd6` f10: the Theseus arm answers S5's question with the grant's key, `a`, and records it
- `e3c10e9` policy: a grant's notice says what ran under it ("ran under your grant: python3 -m unittest, this
  conversation, sgr_…")

No store format change (new ledger kinds `policy.granted` and `policy.grant_ended`, and a field on a wire type only:
the stored gate record is unchanged, and the offer is computed again from it). No new dependency, no new config key.

## What I found

- earned-autonomy's `policy/earned.rs` had the matching and the gate step to reuse, and its doc named the hook
  ("`scoped-grants` would hook in beside `gate`"). Its `find::prefix_ok` refuses an interpreter whose first argument is
  an option, which would refuse the row's own example, `python3 -m unittest`; the scoped rule allows exactly an
  interpreter's `-m <module>` (never a module that runs what it is given: timeit, code, pdb, runpy, …) and asks
  `find::prefix_ok` for everything else.
- `normalized_argv` takes the program by its base name, so an earned grant on `python3 …` also covers
  `/tmp/x/python3 …` (a program the model wrote). A scoped grant keeps the program **as written** and matches it
  exactly. Earned grants still match by base name: worth the owner's look (not changed here, it is earned-autonomy's).
- The TUI's `a` is the "show all" filter. On a card that offers a prefix grant, `a` takes the grant; anywhere else it
  is the filter as before (`e` was free). Said in the TUI's tests.
- The TUI answers through `work.answer` option ids, so the grant is also an option, `approve_grant`, that names no
  pattern at all (the daemon grants its own offer). `action.confirm { grant: { pattern } }` is the echo the row asks
  for; both go through one path (`confirm_granting`).
- `toolrun.rs` is at 2,478 lines on this base (the brief said 2,499): the turn's ask gains one field line. Discord's
  `runtime.rs` was at 3,497 of 3,500: it gains three lines and is now at **3,500, its ceiling** (a `mod` line, the
  press's verb in `parse_confirm_id`, the answer's `grant` field); the button and the press's lookup are a module of
  their own, `runtime/grant.rs`. The next row that touches `runtime.rs` must move lines out first.

## What I changed (`2136441`, `e3c10e9`)

**The offer.** `ConfirmRequest.grant_offer: Option<GrantOffer>` (`theseus_protocol::grants`: `scope: "session"`,
`kind: "prefix" | "edits"`, `pattern`, `words`; lib.rs gains the field and the module line), and an answer option of
its own after the plain yes, `approve_grant` ("Approve + this command for the conversation" / "+ edits in this project
for the conversation"). Computed when the question is asked (`policy::grants::offered`, from the call's gate) and again
from the stored gate record wherever a request is rebuilt (`confirm.list`, a Discord card: `Core::grant_offer_of`),
one rule, `grants::offer`. Only when the posture alone asked: an approve whose reason is the policy's own (no later
layer changed it), `posture_only` (not the floor, a layer-1 change, the lists, the roots, an AWS rule), a private
place, not L1, no `env`, no `steps`, the tool not tightened, not held by external text.
- `proc.run`: `prefix_of` (the program and its subcommand: `cargo test`, `git status`; an interpreter's `-m <module>`:
  `python3 -m unittest`; a launcher's program too: `uv run pytest`). Nothing for a shell, a wrapper (`env`, `xargs`,
  `sudo`, `timeout`, …), inline code, a relative program path, an option or an assignment first, or a URL. The
  command's path arguments must be inside the roots.
- `fs.edit`, `fs.write`, `fs.patch`: `edits` under the session's directory (`cwd_for`, canonical), which must be inside
  a workspace root and never `/` or the home.

**The answer.** `ActionConfirmParams.grant: Option<GrantAnswer>` (the pattern, echoed) and `work.answer`'s
`approve_grant`. `confirm_granting` recomputes the daemon's own offer, refuses an answer whose pattern differs (and a
grant with a decline), makes the grant from the daemon's offer (`grant_of`), and passes its `policy.granted` row
(session, pattern, prefix, the program as written or the root, who, via, when, the question) into the answer's own
frame (`confirm_action_with` gained an `extra` records argument), after `judge_act`'s place rule. Then the grant is
held in memory. `ActionConfirmResult.granted` says what was granted.

**The gate.** `grants::gate`, in `toolrun/order.rs` right after `earned::gate` (a new `Layer::Scoped`, which
`policy.explain` skips as it skips the earned layer), with earned's conditions asked again: an approve the posture
alone made, in a private place, outside L1 (so an L1 job's own asks, its egress beyond `[sandbox] egress` among them,
stay), no `env`, no tightening, the path arguments inside the roots; a covered call (`grants::covers`, the one rule:
modes calls `Grants::covers(session, call)`) of the grant's own session runs at `notify`, never `open`, with the
notice "ran under your grant: python3 -m unittest, this conversation, sgr_…" and the rule naming
`theseus policy revoke sgr_…`. Every later layer (a granted secret, the LSP starts, the place's floor, a glide, T1's
hold, an MCP client's floor) still raises it. FAST: one in-memory lookup per gated call (an atomic "any" first, then a
map by session); no store read.

**Listed and revoked.** `policy.grants` (method table and dispatch in `rpc/earned.rs`'s routing) lists the live
session grants beside the earned ones; `theseus policy grants [--session ID]`. `policy.revoke sgr_…` ends one
(`policy.grant_ended`, why `revoked`), anyone's, as an earned revoke is; `PolicyRevokeResult.session_grant` says which.
A retire (`session.retire`) and a supersession (a place's `/new`, `bind_place_to`) end the session's grants in the
frame that retires it (why `retired`/`superseded`). After serving, `Core::warm_grants` reads the two ledger kinds once
(waiting on tokio's timer while the ledger's index is built) and rebuilds the live grants: no end row, the session
there, not retired, not reopened since the grant. A grant ended in this run is never added back by a racing read.

**The surfaces.** `watch --interactive`: `approve? [y/N/a/note] (a: yes, and don't ask again for `python3 -m
unittest` in this conversation)`, and `a`/`e` take the offer. The TUI's card: a line of its own, short so it stays
whole on an 80-column pane, `[a] don't ask again: python3 -m unittest, this conversation`, and the key line
`[y] approve  [a] approve + don't ask again  [n] decline`. Discord's approval card: a third button,
"Approve + this command for the conversation" (`confirm:grant:<corr>`, which reads the offer the question carries
now, never anything from the button). The cockpit's `ConfirmCard` draws it from the request's options with no change
(a test added to `test/answers.test.ts`).

**Docs.** theseus-core's AGENTS.md (a "Scoped grants" bullet beside earned autonomy, whose "Not built" note loses
`scoped-grants`) and theseus-protocol's AGENTS.md (`grants.rs`).

**F10 (`d0cecd6`).** `arm_theseus.py`'s `s5` presses `a` where it pressed `y`, and records `grant_keys=["a"]`. No
check, scorer or fixture changed; `scripts/f10/test_f10.py` passes (20 tests).

## How I proved it

- Pure tests, `policy/grants/tests.rs` (9): what an offer names; no offer for a shell, a wrapper, inline code, a
  launcher alone, an option first, a relative path, a URL; `prefix_ok` asked again of a stored grant; a prefix grant
  covers its prefix and not `sh -c`, `bash -c`, `env python3`, `python3x`, `/tmp/evil/python3`, `./python3`,
  `python3 -m pip`; an edits grant covers the three edit tools under its root and not `..`, a link out, a sibling
  `app2`, or no resource; never `/` or the home; the view canonicalises a raw plan's paths; a grant is made only of
  the offer it answers; the memory holds each session's own and never re-adds an ended grant.
- Whole cores, `tests_m3/grants.rs` (9): a granted prefix runs with the notice in its own conversation alone (another
  session's identical call asks), `policy.grants` lists it; a revoke ends it and the prefix asks again; a retire ends
  a session's grants in its frame and a restart (a fresh `Grants` rebuilt from the ledger) keeps only the others, and
  a reopened session gets none back; a wider answer (`echo`, `echo hello x y`, empty, `bash -c`) is refused over the
  core and the protocol, a grant with a decline is refused, an MCP connection is `REFUSED`, and nothing changes (the
  question still waits, no row, no grant); a shell or a wrapper offers nothing; under a grant a shell around the
  command asks; the gate applies a grant only where an earned one would (not shared, another session, an `env`, a
  URL, outside the roots, the approve list, the floor, another path to the program, an L1 job's egress, a tightened
  tool); an edits grant covers the project and a real `fs_write` of `src/../../outside.txt` or through a link out asks
  and writes nothing outside; only a posture's ask offers a grant.
- TUI, `tests_grant.rs` (2): at 80×24 the card shows the offer line whole and `a` sends `approve_grant`; `e` takes an
  edits offer and `a` there is the filter, answering nothing. CLI: `a_grant_key_takes_the_offer_it_names`. Discord:
  `the_grant_button_sits_between_approve_and_decline`. Protocol: `an_offer_says_its_pattern_and_key`.
- **Planted reverts** (each with `scripts`-style plant: edit, run `test(grants)`, restore, `touch`, `git status`):
  | Rule | Plant | What failed |
  |---|---|---|
  | the shell exclusion | `find::prefix_ok`'s result ignored in `prefix_ok` | `prefix_ok_is_asked_again_of_every_grant`, `no_offer_for_a_shell_a_wrapper_or_inline_code`, `a_shell_or_a_wrapper_asks_and_offers_nothing` (3 of 22) |
  | the path canonicalisation | the view takes a plan's paths raw | first run: nothing failed (the fs tools' plans resolve paths themselves); I added `the_view_canonicalises_a_plans_paths`, which then failed (1 of 23) |
  | offered equals answered | the pattern comparison skipped | `an_answer_wider_than_the_offer_is_refused_and_nothing_changes` (1 of 22) |
  | the floor first | `d.floor` and `posture_only` dropped from the gate step | `the_gate_applies_a_grant_where_an_earned_one_would_alone` (1 of 22) |
  | the posture-only offer | the offer's `posture_only` and policy-reason checks dropped | `only_a_postures_ask_offers_a_grant` (1 of 24) |
  | the session scope | the lookup over every session's grants | `the_memory_holds_each_sessions_own`, `a_granted_prefix_runs_with_a_notice_in_its_own_conversation_alone`, `the_gate_applies_…` (3 of 22) |
  Each file restored and touched; `git status` clean after each.
- Changed tests, with why: `tests_m3::answer::may_answer_is_false_for_a_shared_bindings_connection`'s options list
  gains `approve_grant` (that question is a posture's ask in a private place, so it offers the grant); the output
  golden `core_output.txt` gains the offer and its option on its three `fs.write` questions (`THESEUS_GOLDEN=write`;
  the diff is those six lines and nothing else). `kernel_frames.txt` is unchanged.
- Under load: not run; nothing here is timing-sensitive (one map lookup), and the gate's suite ran with the whole
  workspace beside it twice.

## The live check (this VM, a scratch daemon on the stand-in)

The script below (debug build, `target/debug`) starts `theseus-sim fake-model --rules`, a scratch `theseusd` with
`proc.run` at `approve`, opens a conversation with `theseus ask`, and drives `theseus watch <session> --interactive`
in tmux. What it showed, at `e3c10e9`:

```text
===== 1. the question, with the offer
  ? proc.run needs your confirmation: run `python3 -m unittest -v test_tides` in /tmp/grantlive/project: proc.run — approve ([policy.tools] "proc.run" = approve)
  approve? [y/N/a/note] (a: yes, and don't ask again for `python3 -m unittest` in this conversation)
===== 2. after `a`
a
  ✓ approved act_01a13969e2df7341b7495743b37cb099 (by theseus watch)
  → proc.run [python3 -m unittest -v test_tides] pid …
The tests ran.
===== 3. the same prefix again: it runs with the notice, no question
second: rerun
  ! notified: proc.run: run `python3 -m unittest -v test_tides` in /tmp/grantlive/project
      (ran under your grant: python3 -m unittest, this conversation, sgr_637312fb) · should have asked: theseus policy tighten proc.run --call act_…
  → proc.run [python3 -m unittest -v test_tides] pid …
The tests ran again.
===== 4. bash -c "python3 -m unittest" still asks
third: via bash
  ? proc.run needs your confirmation: run `bash -c python3 -m unittest -v test_tides` in /tmp/grantlive/project: proc.run — approve ([policy.tools] "proc.run" = approve)
  approve? [y/N/t/note]
n
  ✗ declined act_… (by theseus watch)
===== 5. theseus policy grants
the conversations' grants (`theseus policy revoke ID` ends one):
  sgr_637312fb: python3 -m unittest (this conversation, ses_01a13969dc9c721db3f2d7a038491d4e; granted by theseus watch)
===== 6. theseus policy revoke
python3 -m unittest asks first again: the grant sgr_637312fb is revoked.
===== 7. theseus policy grants after the revoke
no grants: every question asks
===== 8. after the revoke the prefix asks again
second: rerun
  ? proc.run needs your confirmation: run `python3 -m unittest -v test_tides` …
  approve? [y/N/a/note] (a: yes, and don't ask again for `python3 -m unittest` in this conversation)
===== 9/10. the rows
policy.granted     {"at_ms":…,"by":"theseus watch","correlation_id":"act_…","id":"sgr_637312fb","kind":"prefix","pattern":"python3 -m unittest",…}
policy.grant_ended {"by":"the CLI","granted_ms":…,"id":"sgr_637312fb","kind":"prefix","pattern":"python3 -m unittest","why":"revoked"}
```

(The first run of this check showed the notice reading only `(session grant sgr_…)`; `e3c10e9` makes it say what ran
under the grant, as the row asks.)

**For the maintainer**, on the install build: `python3 -I <the script below> ~/.local/bin /tmp/grantlive` (it needs
`tmux`, and `scripts/f10/project` from the tree its `TREE` names: set it to the checkout; it touches no port but a free loopback one for the stand-in and its
own socket under the output directory). It should print the eight captures above, exit 0, and leave
`/tmp/grantlive/captures.txt`. Then, by hand, in the TUI over the same kind of daemon: the card shows
`[a] don't ask again: python3 -m unittest, this conversation`, `a` approves and grants; on Discord's approval card the
third button "Approve + this command for the conversation" does the same (not checked here: no Discord on this VM);
the cockpit's card shows the same button.

<details><summary>The live check's script</summary>

```python
"""Live check for scoped grants (theseus-drjs) on a scratch daemon and the
stand-in model: watch --interactive shows the offer, `a` grants it, the next
`python3 -m unittest` runs with the notice, `bash -c` still asks, `theseus
policy grants` lists it, revoke ends it. Usage: live.py BIN_DIR OUT_DIR"""

import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

BIN = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
TREE = Path("/home/user/theseus")
if OUT.exists():
    shutil.rmtree(OUT)
OUT.mkdir(parents=True)
project = OUT / "project"
shutil.copytree(TREE / "scripts/f10/project", project)
state, sock = OUT / "state", OUT / "theseus.sock"
s = socket.socket()
s.bind(("127.0.0.1", 0))
port = s.getsockname()[1]
s.close()
TEST = ["python3", "-m", "unittest", "-v", "test_tides"]
rules = [
    {"when": "hello there", "text": "Hello."},
    {"when": "first: run the tests", "calls": [{"name": "proc_run", "input": {"argv": TEST, "cwd": str(project)}}],
     "then": "The tests ran."},
    {"when": "second: rerun", "calls": [{"name": "proc_run", "input": {"argv": TEST, "cwd": str(project)}}],
     "then": "The tests ran again."},
    {"when": "third: via bash", "calls": [{"name": "proc_run", "input": {
        "argv": ["bash", "-c", "python3 -m unittest -v test_tides"], "cwd": str(project)}}],
     "then": "Ran through bash."},
]
(OUT / "rules.json").write_text(json.dumps(rules, indent=1))
(OUT / "theseus.toml").write_text(f"""[model]
live = "f10"
api_base = "http://127.0.0.1:{port}"
api_key_secret = "anthropic_api_key"

[profiles.f10]
provider = "anthropic"
model = "claude-sonnet-5-5"

[secrets]
anthropic_api_key = "env:F10_STANDIN_KEY"

[server]
state_dir = "{state}"
socket = "{sock}"
disk_warn_mb = 0
disk_floor_mb = 0

[tools]
projects_dir = "{project}"
roots = ["{project}"]
proc_sync_secs = 5

[policy.tools]
"proc.run" = "approve"
"fs.edit" = "approve"
"fs.write" = "approve"
"fs.patch" = "approve"

[discord]
enabled = false

[web]
enabled = false

[index]
enabled = false

[judge]
enabled = false
""")
env = dict(os.environ, PATH=f"{BIN}:{os.environ['PATH']}", THESEUS_SOCKET=str(sock),
           THESEUS_CONFIG=str(OUT / "theseus.toml"), XDG_STATE_HOME=str(OUT / "xdg"), TZ="UTC",
           F10_STANDIN_KEY="f10-stand-in", TERM="xterm-256color")
fake = subprocess.Popen([BIN / "theseus-sim", "fake-model", "--addr", f"127.0.0.1:{port}", "--rules",
                         OUT / "rules.json"], stdout=open(OUT / "fake.log", "w"), stderr=subprocess.STDOUT)
daemon = subprocess.Popen([BIN / "theseusd", "--config", OUT / "theseus.toml", "--socket", sock,
                           "--state-dir", state], env=env, stdout=open(OUT / "theseusd.log", "w"),
                          stderr=subprocess.STDOUT, start_new_session=True)
log = open(OUT / "captures.txt", "w")


def cli(*args, timeout=30):
    return subprocess.run([BIN / "theseus", "--socket", sock, *args], capture_output=True, text=True, env=env,
                          timeout=timeout, cwd=project)


def note(title, text):
    log.write(f"\n===== {title}\n{text}\n")
    log.flush()
    print(f"===== {title}\n{text}")


def tmux(*args):
    return subprocess.run(["tmux", "-L", "grantlive", *args], capture_output=True, text=True, env=env)


def pane():
    return "\n".join(l.rstrip() for l in tmux("capture-pane", "-p", "-S", "-", "-t", "live").stdout.splitlines()).strip()


def wait(pred, secs, every=0.3):
    end = time.time() + secs
    while time.time() < end:
        if pred():
            return True
        time.sleep(every)
    return False


def asks():
    return sum(1 for l in pane().splitlines() if l.startswith("  approve? ["))


def say(text):
    tmux("send-keys", "-t", "live", "-l", text)
    tmux("send-keys", "-t", "live", "Enter")


try:
    assert wait(lambda: cli("health").returncode == 0, 20), "daemon did not answer"
    r = cli("ask", "hello there")
    note("0. theseus ask (opens the conversation)", r.stdout + r.stderr)
    listed = json.loads(cli("--json", "sessions", "list").stdout)
    sid = (listed.get("sessions", listed) if isinstance(listed, dict) else listed)[0]["session_id"]
    tmux("new-session", "-d", "-s", "live", "-x", "160", "-y", "48",
         f"cd {project} && {BIN}/theseus --socket {sock} watch {sid} --interactive; sleep 30")
    time.sleep(1.5)
    n = asks()
    say("first: run the tests")
    assert wait(lambda: asks() > n, 30), pane()
    note("1. the question, with the offer", pane())
    say("a")
    wait(lambda: "The tests ran." in pane(), 30)
    time.sleep(1)
    note("2. after `a`", pane())
    pass
    say("second: rerun")
    wait(lambda: "The tests ran again." in pane(), 30)
    time.sleep(1)
    note("3. the same prefix again: it runs with the notice, no question", pane())
    n = asks()
    say("third: via bash")
    assert wait(lambda: asks() > n, 30), pane()
    note("4. bash -c \"python3 -m unittest\" still asks", pane())
    say("n")
    time.sleep(2)
    r = cli("policy", "grants")
    note("5. theseus policy grants", r.stdout + r.stderr)
    gid = json.loads(cli("--json", "policy", "grants").stdout)["session"][0]["id"]
    r = cli("policy", "revoke", gid)
    note("6. theseus policy revoke", r.stdout + r.stderr)
    r = cli("policy", "grants")
    note("7. theseus policy grants after the revoke", r.stdout + r.stderr)
    pass
    time.sleep(0.5)
    n = asks()
    say("second: rerun")
    assert wait(lambda: asks() > n, 30), pane()
    note("8. after the revoke the prefix asks again", pane())
    say("n")
    time.sleep(1)
    r = cli("ledger", "-k", "policy.granted")
    note("9. the policy.granted row", r.stdout[-1500:] + r.stderr)
    r = cli("ledger", "-k", "policy.grant_ended")
    note("10. the policy.grant_ended row", r.stdout[-1500:] + r.stderr)
    r = cli("ledger", "-k", "tool.notified")
    note("11. the tool.notified row", r.stdout[-2000:] + r.stderr)
finally:
    tmux("kill-server")
    cli("shutdown")
    try:
        daemon.wait(15)
    except subprocess.TimeoutExpired:
        daemon.kill()
    fake.terminate()
    fake.wait(5)
```

</details>

## F10

`python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10` on the **release-thin** build of
`e3c10e9` (`scripts/build.sh --profile release-thin`), both sizes, then `scripts/f10/scorecard.py`. Main's column is
the recorded fixture (`scripts/f10/fixtures/theseus-120x40`, which scores the 11 of 2026-10-12's report).

| Step | main (11 of 42) | branch 120x40 | branch 80x24 |
|---|---|---|---|
| S1 Open | 1 of 4 (S1d) | 1 of 4 | 1 of 4 |
| S2 Ask | 0 of 4 | 0 of 4 | 0 of 4 |
| S3 Run the tests | 3 of 4 (a, b, d) | 3 of 4 | 3 of 4 |
| S4 Reviewed edit | 0 of 4 | 0 of 4 | 0 of 4 |
| **S5 Don't ask again** | **0 of 3** | **2 of 3 (S5a, S5b)** | **2 of 3 (S5a, S5b)** |
| S6 90-second command | 2 of 4 (c, d) | 2 of 4 | 2 of 4 |
| S7 Interrupt | 1 of 3 (c) | 1 of 3 | 1 of 3 |
| S8 Paste | 2 of 3 (a, c) | 2 of 3 | 2 of 3 |
| S9 Undo | 0 of 3 | 0 of 3 | 0 of 3 |
| S10 Context and cost | 1 of 3 (b) | 1 of 3 | 1 of 3 |
| S11 Leave and come back | 1 of 4 (a) | 1 of 4 | 1 of 4 |
| S12 Plan first | 0 of 3 | 0 of 3 | 0 of 3 |
| **Match** | **11** | **13** | **13** |
| Beat (B1 to B5) | 5 of 5 | 5 of 5 (B1: a prompt after 52 ms) | 5 of 5 |

Every check that passes on main passes on the branch (the yes ids are main's 11 plus S5a and S5b, at both sizes). The
evidence: `S5a yes: [a] don't ask again: python3 -m unittest, this conversation`, the same line for S5b. S5c ("one key
switches to accepting edits") stays no: it is modes' (theseus-lyxy), which can build its `edits` mode on an edits
grant through `grants::covers`.

## Security review: each way a grant could widen, and what holds it

| # | Way a grant could widen | What holds it | Test (and the plant that proved it) |
|---|---|---|---|
| 1 | A shell or wrapper around a granted command (`sh -c "python3 …"`, `bash -c`, `env python3`, `xargs`, `sudo`) | `prefix_of` offers nothing for a shell, a wrapper or inline code (`find::prefix_ok`'s `RUNS_WHAT_IT_IS_GIVEN`, `-c`/`-e`/`--exec`), `prefix_ok` is asked again of every stored grant at match time, and a prefix match compares whole words from argv[0] (`bash` ≠ `python3`) | `policy::grants::tests::no_offer_for_a_shell_a_wrapper_or_inline_code`, `prefix_ok_is_asked_again_of_every_grant`, `a_prefix_grant_covers_its_prefix_and_nothing_wider`; whole core: `tests_m3::grants::a_shell_or_a_wrapper_asks_and_offers_nothing`, `under_a_grant_a_shell_around_the_command_still_asks`. **Plant** (find::prefix_ok's result ignored): 3 tests failed |
| 2 | A near name (`python3x`) or another program of the same name (`/tmp/x/python3`, `./python3` the model wrote) | words compare exactly after the program's base name; the grant keeps the program **as written** (`SessionGrant.program`) and a call must write it the same way; no offer for a relative program path | `a_prefix_grant_covers_its_prefix_and_nothing_wider` (`python3x`, `/tmp/evil/python3`, `./python3`), `the_gate_applies_a_grant_where_an_earned_one_would_alone` ("another path") |
| 3 | An interpreter's inline code under a granted prefix (`python3 -m timeit "…"`, `python3 -c`) | only `-m <module>` is a prefix an interpreter may have, never a module that runs what it is given (`CODE_MODULES`: timeit, code, pdb, runpy, …); `-c` offers nothing | `prefix_ok_is_asked_again_of_every_grant`, `no_offer_for_…` |
| 4 | A launcher's subcommand (`uv run`, `npm exec`, `docker run`, `go run`) granting whatever follows | the prefix takes the launched program too (`uv run pytest`), which must not be a shell, a wrapper or an interpreter | `no_offer_for_…` (`uv run`, `uv run sh -c x`, `uv run python3 -c 1`), `an_offer_names_the_program_and_its_subcommand` |
| 5 | An option first that changes what runs (`git -C /tmp status`, `make FOO=1`) | no offer when the word after the program is an option or an assignment (but an interpreter's `-m`) | `no_offer_for_…` |
| 6 | A URL after the prefix (`git fetch https://…`) as an exit | `covers` refuses a word with `://` after the prefix (as earned grants do) | `a_prefix_grant_covers_…`, `the_gate_applies_…` ("a URL") |
| 7 | A path out of the project for an edits grant: `..`, a symlink, a sibling named like the root | every resource canonicalised (`paths::canonical_best_effort`: `..` taken, symlinks of the existing part resolved) by the grant itself, then a component-wise `starts_with` on the canonical root; `/` and the home directory are never a root | pure: `an_edits_grant_covers_the_three_edit_tools_under_its_root`, `the_view_canonicalises_a_plans_paths`, `an_edits_grant_never_names_the_root_or_the_home`; core: `an_edits_grant_covers_the_project_and_nothing_out_of_it` (a real `fs_write` of `src/../../outside.txt` and through a link asks, and nothing is written outside). **Plant** (resources taken raw): `the_view_canonicalises_a_plans_paths` failed. The fs tools' plans resolve paths too, so the whole-core test passed under the plant: the grant's own canonicalisation is a second wall, held by the pure test |
| 8 | The floor, a layer-1 change, the approve lists, a path outside the roots, an AWS rule | the gate step applies only where `ToolPolicy::posture_only` (the floor and lists come first) and `!d.floor`; a command's path arguments stay inside the roots (`args_within_roots`); the offer is made only when the policy's own decision is an approve by posture and no later layer changed its reason | `the_gate_applies_a_grant_where_an_earned_one_would_alone` (approve list, floor, outside the roots), `only_a_postures_ask_offers_a_grant`. **Plants**: removing `d.floor`/`posture_only` from the gate failed `the_gate_applies_…`; removing the posture checks from the offer failed `only_a_postures_ask_offers_a_grant` |
| 9 | An L1 job's own asks (its posture, egress hosts beyond `[sandbox] egress`) | `l1` returns the decision untouched, and no offer is made for an L1 call | `the_gate_applies_…` ("an L1 job's egress"), `only_a_postures_ask_offers_a_grant` |
| 10 | The external-text hold (T1), a granted secret's posture, the place's floor, an MCP client's floor | the grant's step sits right after the earned one, before the broker; every later layer still raises the posture; no offer when the hold raised it | `only_a_postures_ask_offers_a_grant` ("the hold asked"); the order itself in `toolrun/order.rs` |
| 11 | A grant crossing sessions (another conversation, a task) | grants are keyed by session id and looked up by the call's own session (`At.session`); a task session is a session of its own | `a_granted_prefix_runs_with_a_notice_in_its_own_conversation_alone` (another session asks), `the_memory_holds_each_sessions_own`, `the_gate_applies_…` (`ses_other`). **Plant** (lookup over every session): 3 tests failed |
| 12 | A shared place | the gate step and the offer require `PlaceClass::Private`; the place rule refuses a shared place's calls before the order runs | `the_gate_applies_…` (Shared), `only_a_postures_ask_offers_a_grant` (Shared) |
| 13 | A client answering with a wider pattern than offered (`echo` for `echo hello`, `bash -c`, empty) | the daemon recomputes its own offer from the stored gate record and refuses an answer whose pattern differs; the grant stored is the daemon's offer, never the client's words; `work.answer`'s `approve_grant` names no pattern at all; a grant with a decline is refused | `an_answer_wider_than_the_offer_is_refused_and_nothing_changes` (the question still waits, no row, no grant). **Plant** (the comparison skipped): that test failed |
| 14 | Someone other than the owner, or from a shared place, granting | a grant is an approval: `confirm_action_with` judges `Act::Answer` (`judge_act`, the place rule) before the frame that writes the grant's row; an MCP connection is refused | `an_answer_wider_…` (the MCP connection is `REFUSED`, nothing granted) |
| 15 | A grant outliving its conversation | a retire (`session.retire`, a place's `/new` supersession) ends the session's grants in the frame that retires it; a restart reads back only grants with no end row whose session is not retired and not reopened since | `a_retired_session_s_grants_end_and_a_restart_rebuilds_the_rest` |
| 16 | A race between the ledger read after serving and a revoke | the ends are read after the grants, and `Grants` remembers every id ended in this run, so a read that raced an end never adds it back | `the_memory_holds_each_sessions_own` (an ended grant is never added again) |

Residual risks, said plainly:
- **What follows the prefix is open.** A grant on `cargo test` covers `cargo test --config 'target.x.runner=…'`, and a grant on `python3 -m unittest` runs whatever test files the project holds. That is the nature of a prefix grant (an earned grant has the same), and the offer shows the prefix it covers; combined with an edits grant it is arbitrary code inside the project, which the owner chose twice.
- **Time of check and time of use.** An edit's path is resolved at the gate; a symlink made between the gate and the write (a parallel `proc.run` in the same response, which itself must be granted or approved) could redirect it. Earned grants share this; L1 is the boundary.
- **The project directory is the session's directory** (`cwd_for`), not a repository root found by walking up; a session started in a subdirectory offers edits under that subdirectory.
- **"Should have asked"** on a session grant's notice tightens the tool (the tightening makes the grant step skip the tool), but does not write a `policy.grant_ended` row for the grant; `theseus policy revoke sgr_…` does.


## Keel findings expected

None. `python3 scripts/keel-guard.py` at the head: `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD,
9fa84735c0; 440 files changed, 0 findings acked)` (it also prints the base's own pairing of a renamed store test,
not this branch's). An earlier draft moved `confirm_action_with`'s `#[expect(clippy::too_many_lines)]` to a renamed
function, which the guard reads as an `allow-added`; I kept the function's name and gave it the extra argument
instead. No test deleted, no assertion lowered (two assertions rewritten, said above), no cap or ceiling raised.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at `e3c10e9`'s tree (run 3; `CARGO_INCREMENTAL=0`,
since the incremental cache filled this VM's disk once): door, keel, keel tests, fmt, shape, features, clippy, the
cockpit (lint, 378 tests, build), the test build and the reader rule pass; the suite ran **4,496 tests: 4,461 passed,
35 failed, 47 skipped**. The failures:

- 33 known on this VM (root daemon, no job cgroup, theseus-pv6i): 19 of `theseus-sandbox::contract`, its bench's
  `spawn_100`, and 13 of `theseusd::sandbox`.
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: times out at
  21 s every run, alone too (3 of 3). It has no tool call. Its fix is test-side on another branch, `b495389` "discord:
  the thinking-answer test holds the process message's create whatever its text…" (theseus-wpv2g, joined as
  `a6bc001` on cloud/20261013-discord-ec2-timeout), which is not in this base; its message names exactly this 21 s
  wait on a fast host.
- `theseus-core tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory`: failed in both
  full gates and passes alone (3 of 3). Two of its records share a millisecond (`since_ms` …165), and the rebuilt
  list and the memory's order the tie differently: a defect of `standing.rs`'s ordering (theseus-a7vi), not code this
  row touches. Worth an issue: order by `(since_ms, id)` in both, or compare as sets.

Then the phases after the suite, run by hand as the brief says: protocol types (`cockpit/src/protocol.gen` matches the
commit) ok; the turn bench (`target/debug/theseus-sim bench turn --check --runs 5 --burst 0`) ok, exit 0;
`cargo deny --offline check`: advisories ok, bans ok, licenses ok, sources ok. The lifecycle and jobs benches are
skipped in a lane's gate. So the head counts green.

Python: no Python under `bench/` changed. `scripts/f10/test_f10.py`: 20 tests OK.

## What is left, or for the owner

- **Siblings.** tool-words (theseus-z6au) adds `confirm.requested.preview` beside `grant_offer` and rewords the same
  question lines (`watch --interactive`'s prompt, the TUI's card): an ordered conflict at the second join, as foreseen.
  chat-ask: whichever joins second adds `a` and `e` (take `approve_grant`) to the chat dock's key table. modes: call
  `Earned::scoped.covers(session, &Call {..})`, or hold an edits `SessionGrant` per session.
- **Discord's `runtime.rs` is at its ceiling (3,500).**
- **Earned grants match the program by base name** (see "What I found"); a scoped grant does not. The owner may want
  the same for earned grants.
- **The project directory is the session's directory**, not a repository root found by walking up.
- **"Should have asked" on a session grant's notice** tightens the tool, which stops the grant (a tightened tool is
  never granted) but writes no `policy.grant_ended`; `theseus policy revoke` does. A press could end the grant instead,
  as an earned grant's press does.
- **Docs to change** (the maintainer's): the spec's Part III item for theseus-drjs; `docs/status.md`'s roadmap row
  `scoped-grants` and the F10 line (13 of 42); `docs/benchmarks/f10/README.md`'s Theseus column after the re-record
  (S5a, S5b yes); the fixtures under `scripts/f10/fixtures/theseus-*` re-recorded after the join.
