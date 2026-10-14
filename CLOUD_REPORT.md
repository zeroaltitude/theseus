# CLOUD_REPORT: ext-hands (theseus-pw1q.14)

Branch `cloud/20261013-ext-hands`, cloned at `0baeed6` (store format 33 there, not 32 as the brief said). Two code
commits on top of the task commit:

- `f2c30e0` mcp: a client answers the requests of its server's own that the core gives one connection
- `c4aac34` extend: an extension may ask for hands with a day's cap; its own MCP pipe is the broker, launching through
  aws.hands.run's path under both reservations and the brake; depth one

## The channel's threat model (written first, as the brief asks)

**What is being protected.** Money in the owner's AWS account (Lambda and Fargate time), the account itself (its
credentials, the hands' roles), and the daemon (its socket, its store, its other sessions). **The adversary** is the
extension's own code: a model wrote it, so treat it as hostile once loaded, and treat what its hands print as hostile
too. It runs in L1 (its own namespaces, seccomp, the frozen copy, no network unless the ack granted hosts, no secret).

**What the extension can reach after this row**, and nothing else: three JSON-RPC requests, `hands.run`,
`hands.status`, `hands.cancel`, sent up its own stdout to the daemon, which is its MCP client. Answered on the same
pipe.

**Why the stdio pipe, not a per-extension Unix socket.**
- *Identity is the pipe.* The daemon spawned the process and holds the other end; no other process on the machine has
  it, and nothing names it. A socket is a path: it must be bound into L1's view (a new hole in the view the sandbox
  tests hold), anything else that can reach the path can connect, and identity would have to be proven per connection
  (SO_PEERCRED of a namespaced pid, or a token handed in, which is one more secret).
- *Its life is the process's.* Revoke drops the server, which closes the pipe and SIGTERMs its group: no stale socket,
  no listener to tear down, nothing left behind by a crash.
- *No new endpoint.* Nothing listens; the daemon's own socket stays out of every L1 view, as before.
- *Gated by construction.* `theseus_mcp::Options::serve` is `None` for every server but an extension the owner acked
  for `hands` (`Extensions::channel`): any other server asking `hands.run` gets JSON-RPC "method not found", exactly
  as before this row. The client answers at most 8 of the server's requests at once and refuses the 9th at once
  (`SERVED_AT_ONCE`), so a flood cannot pile up tasks.

**Threats and answers.**
1. *Spending past what the owner said.* Each hand's worst case (its TTL at its size's rate, the same formula
   `aws.hands.run` uses) is reserved twice: against the extension's day account (its hands session for the local day,
   an execution whose limit is pinned at `usd_cap`, so the kernel refuses the reservation in the group's own frame, and
   two concurrent requests cannot both take the last dollar), and against the account's global hour and day lines
   (runaway mode, unchanged). Launches are serialized per daemon (`Extensions::launches`). A Lambda hand that cannot be
   stopped keeps its reservation held until its envelope books its real cost, as today.
2. *Running more than it said at a time.* `max_group` counts the hands of all its open groups (running and waiting)
   plus the new group's.
3. *Running somewhere it was not granted, or as something stronger.* The backend must be in `backends`. `image`,
   `profile`, `account` and `region` are refused by name (deny_unknown_fields): an extension's hands always run the
   hand image under the `basic` profile on the default account and region. `owner` (allow-all under the guards) is out
   of its reach.
4. *Stealing credentials.* The broker hands back only the group's id, counts, cost and the result text: never a key,
   a role ARN, a presigned URL, or the per-dispatch HMAC key. The hand's own key stays in its spec, which goes to
   Lambda/ECS, never to the extension.
5. *Recursion (a hand launching hands).* A hand runs in AWS with its spec: no route to the daemon's machine, no
   socket, no key but its own dispatch's, and nothing naming the extension or its connection (tested over all 20 specs
   of the 200-file batch). A request that says it is relayed from a hand (`hand`) is refused (`from_hand`) and
   ledgered. Depth one.
6. *Granting itself more.* Nothing on the channel grants anything. A grant is a manifest field frozen into the digest
   (`freeze::with_caps`), shown on the owner's Load card in plain words, and acked only by the owner from a private
   place (`judge_act`, unchanged). The self loop's `"self"` principal acks nothing (S1). A new ask is a new digest, a new
   copy, and a new card. The day session runs under `ext:<name>`, never an owner handle.
7. *Touching another extension's groups.* `hands.status` and `hands.cancel` resolve only groups whose session is one
   of this extension's day sessions (`not_its_group`).
8. *After revoke.* The record is gone, so `hands.run` is refused (`revoked`); the pipe is closed; the sweep (spawned by
   the revoke, after any launch under way) stops every open group by its backend's means: Fargate `StopTask`,
   verified STOPPED; Lambda `unsupported` (its TTL bounds it).
9. *Its hands' output.* Envelopes are checked by the poller as every hand's are (HKDF HMAC); the result text an
   extension reads is the same aggregate a model reads.

**Floods.** A hostile extension can send refused requests in a loop: each is answered, but the ledger takes at most
20 of one extension's refusal rows a minute (`REFUSALS_A_MINUTE`; the next minute's first row counts what it left),
so a loop cannot turn into a frame per request. Answers run at most 8 at once (`SERVED_AT_ONCE`).

## Step 1: the manifest field

**Found.** The "manifest" is the `extend.manifest.<name>.<digest>` META record that `extend.propose` writes, and its
digest covered only the tree.

**Changed (`c4aac34`).**
- `extend.propose { hands: { usd_cap, backends, max_group } }` (`extend/hands.rs`, `HandsCap`, deny_unknown_fields,
  every field required). Rules: `0 < usd_cap <= 100`; backends not empty and no repeats; `1 <= max_group <= 100`.
- The ask is folded into the digest (`freeze::with_caps`: sha256 of a tag, the tree's digest, and the ask's canonical
  JSON). With no ask the digest is the tree's, byte for byte as before. The frozen copy lives under the new digest.
- The Load card: `Load <name> <short>: … no network? This extension may run up to 20 hands at a time on Lambda, at most
  $2.00 a day.` The ack's args carry `hands`. An extension card's two buttons are now **Load** and **Don't load**
  (theseus-protocol `work.rs`; the ids stay `ack`/`decline`). That rewording applies to every extension card.
- Store format **34** (`MANIFEST_FORMAT`): `capabilities.hands` on a manifest and on a `Loaded`, `extension` on a
  hands group record, and the new META key `extend.hands.<name>`. In `tests_layouts.rs`: literal format-33 samples of
  a manifest, the `extensions` record and a hands group (each reads back byte for byte with no hands or extension),
  plus a format-33 store that opens and moves to 34 on its first write. theseusd's `tests/versions.rs` and
  core's `store.rs` assertions move 33→34 and 34→35, as the format-33 step's did. No golden changed
  (core_output.txt and kernel_frames.txt pass unchanged). The group sample's bytes come from today's encoder with
  `extension: None`, which leaves the field out; the struct is otherwise unchanged since step 40. The manifest and
  record samples are written by hand.

**Proved.** `extend::hands::tests` (parsing, the card's sentence, refusal of every missing field and bad value),
`freeze::tests::an_ask_for_hands_is_in_the_digest`, `extend::tests::an_ask_for_hands_is_frozen_named_on_the_card_and_granted_by_the_ack`
(digest, card text, labels, ack, channel; the same tree with no ask gets its own digest and no channel), and the
layouts tests.

## Step 2: the broker

**Found.** A group belongs to a turn: `run_hands` needs a held turn (`plan_action`) and the execution's budget. An
extension has neither: its MCP tool call runs inside someone's turn, but the broker can't use that turn's guard.

**Changed.**
- The channel is the extension's own MCP stdio pipe. theseus-mcp `Options::serve` (`f2c30e0`) answers the server's
  own requests the core names; `Connect::connect` takes it; the board stores it on the `Server`
  (`McpBoard::load_extension(.., serve)`, `has_channel`), and `Core::board_load` passes `Extensions::channel(l)`,
  which is `None` unless the loaded record has `hands`. Why the pipe and not a socket: see the threat model above.
- `aws/hands/start.rs`: `run_hands`' group start, moved out so the broker shares one path: account and region, where
  hands run, the worst case, `over` (the caller's budget words), runaway mode's `Sink::admit`, the quota's wave, and
  the one frame of the record and every hand's action (`plan`), then `launch_first`. `toolrun/hands.rs` now calls it.
  Its words and order are unchanged; `tests_hands` and `tests_part2` pass.
- `extend/broker.rs`: `hands.run` refuses with JSON-RPC `-32001` and a reason. In order:
  1. `invalid`: an unknown field, which includes `image`, `profile`, `account` and `region`. The hands always run the
     hand image under `basic`.
  2. `from_hand`: a request relayed from a hand.
  3. `revoked` or `not_granted`: the extension is no longer loaded, or was loaded without hands.
  4. `backend`: the backend is not in the grant.
  5. `group_size`: the hands running or waiting in its open groups, plus this group's, would exceed `max_group`.

  A request that passes all five takes the one-at-a-time lock, then runs on the day's session. That session is opened
  on the day's first launch as `ext:<name>` with its execution's limit pinned at `usd_cap`, and listed in META
  `extend.hands.<name>`. One short turn (`admit_input`, then `end_turn` waiting on input) plans the group's
  `aws.hands.run` call and its hands. Each hand's worst case reserves on that execution, and the kernel refuses past
  the cap in the group's frame. The same reservation is what runaway mode's meter counts. A refusal inside the start
  (`cap`, `runaway`, `unavailable`) settles the group's call failed at $0. The session never wakes for a turn: its
  results queue on an execution waiting on input, so no model is ever asked.
- `hands.status {id}`: counts, spend, and once settled the aggregate text and each hand. `hands.cancel {id}`:
  `cancel::stop_group`. Both work only for its own groups (`not_its_group`).
- Rows (`fact/extend_hands.rs`; LedgerKinds `extend.hands.launched|settled|refused`): `settled` is written in the
  group's own settle or cancel frame, beside `aws.hands.settled` (`group::extension_row`).

**Proved.** `aws/hands/tests_broker.rs` on the hands rig's fake AWS (8 tests):
`a_200_file_batch_runs_inside_the_cap`, `the_broker_refuses_what_the_manifest_did_not_name`,
`a_group_over_the_extensions_cap_is_refused_though_the_global_line_has_room`, `runaway_mode_refuses_an_extensions_group`,
`a_revoke_cancels_a_running_group_and_closes_the_channel`, `an_extension_without_hands_has_no_channel`,
`the_channel_answers_for_its_own_extension_alone`, `a_flood_of_refusals_writes_a_bounded_number_of_rows`. theseus-mcp's
`a_served_method_is_answered_and_declared_and_nothing_else_is` and `without_a_serve_the_client_offers_nothing`. "Both
accounts reserved" is checked in the 200-file test: the day execution's `reserved_micros` equals the group's worst
case, and `runaway::spend_since` for the hour equals it too.

**Measurement (fake backend).** A seeded 200-file batch (20 Lambda hands × 10 files, ttl 60 s): worst case $0.04,
inside a $0.25 cap; all 20 envelopes checked and settled `met`; nothing reserved afterwards; spend ≤ $0.25. Runaway:
with the hour line at $0.01 × factor 1, a group is refused at the line (`aws.runaway` row, nothing invoked).

## Step 3: depth one

- A hand's spec carries nothing that reaches the broker. The 200-file test asserts that no spec contains the
  extension's name, `ext-`, `sock` or `THESEUS_SESSION`. A hand runs in AWS with no route to this machine anyway.
- A request with `hand` is refused (`from_hand`) and ledgered.
- Nothing on the channel grants anything. Only the owner's ack of a proposal grants `hands`, through the existing
  `judge_act`, unchanged. A new ask is a new digest and a new card. The day session's principal is `ext:<name>`.

## Step 4: visible and revocable

- `ExtendLoadedInfo.hands` (`ExtendHandsInfo`: the grant, today's spend and hold, all-time spend, hands running) with
  `line()`, the one wording. `theseus extend list` prints `  hands: up to 20 hands at a time on lambda · $0.08 of $2.00
  today · $1.20 in all · 3 running`, and the cockpit's Extensions card prints the same (`handsWords`, tested in
  `cockpit/test/extensions.test.ts`). The TypeScript is regenerated (`ExtendHandsInfo.ts`).
- A revoke unloads the server, which closes the pipe, then spawns `hands_revoked`. The sweep takes the launch lock (so
  it runs after any launch under way) and stops every open group by its backend's means. The revoke test checks
  Fargate StopTask, `termination_verified` on each hand, the group `cancelled` with `extend.hands.settled`, and the
  next request refused `revoked`. Lambda's verdict stays `unsupported` (its TTL bounds it), as today.

## Step 5: the self loop

Nothing to build. A self-proposed extension's ack is the owner's Load card like any other, and S1's `"self"`
principal acks nothing (`judge_act`, unchanged). I wrote no new test for the `"self"` answerer: the existing refusal
tests of the ack (`an_ack_from_a_shared_place_is_refused`) cover the place rule it goes through.

**Under load** (AGENTS.md's recipe: four `sh` busy loops at nice 0, tests at `nice -n 19`, the loops killed by
their pids): the 17 tests of this row (broker, card, digest, ask, MCP serve, format-33) passed 3 of 3 runs (16 to
17 s each). Run alone, the revoke test passed 10 of 10.

## Planted reverts (each restored and `touch`ed, `git status` clean after)

1. **The cap checked against the global line only** (the day session opened unpinned, `None` for `Some(cap)`): caught
   by `a_group_over_the_extensions_cap_is_refused_though_the_global_line_has_room` (the group launched,
   `left_today_usd` 99.96) and `a_200_file_batch_runs_inside_the_cap` (limit not 250,000).
2. **The channel open without the field** (`Extensions::channel` without its `hands` check): caught by
   `an_extension_without_hands_has_no_channel` and the extend card test's no-ask half.
3. **A revoke that leaves running groups** (no sweep spawned): caught by `a_revoke_cancels_…` (20 s wait failed).
4. **The ask left out of the digest**: caught by `freeze::…an_ask_for_hands_is_in_the_digest` and the card test
   (`the ask is in the digest`).
5. **A hand's request let through**: caught by `the_broker_refuses_what_the_manifest_did_not_name` (the run succeeded).

## Also changed, and why

- `tests_tiering`'s window 41,100→41,300 and `tests_route_keep`'s WINDOW/BILLED 42,200/36,700→42,400/36,900. The
  larger `extend.propose` schema made their first compile ring and compact, the same thing four earlier rows hit (their
  comments list each). I trimmed my schema text first, and that alone was not enough.
- A race in my own first revoke test: a group's `cancelled` state lands one frame before its settled row. The test now
  waits for both.

## Keel findings expected

None. `python3 scripts/keel-guard.py`: `keel: ok (… 0 findings acked)`. The only note it prints is a renamed store
test already on main.

## FAST

- No timer was added, and nothing new runs on a turn's path or the start path. The broker runs only when an extension
  asks.
- Turn bench (`theseus-sim bench turn --check --runs 5 --burst 0`): ok. A plain turn is 4 frames and a tool turn 8, as
  on main.
- `theseus-sim bench lifecycle --runs 10 --check` missed twice on this 4-core VM, on one phase: "health, after the
  seed, each call its own connection", p95 5.7 ms against 5 ms (p50 4.8). This row changed nothing on that path. The
  maintainer should rerun it on the owner's machine.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` was run over the final tree:
- Passed: keel, fmt, shape, features, clippy, cockpit, test build, registry.
- Suite: 4,490 run, 4,456 passed, 34 failed.
  - 33 are the known L1 failures (theseus-sandbox contract and `spawn_100`, theseusd `sandbox`; the root VM).
  - 1 is `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, which
    times out at 21 s here on every run. It is the test theseus-wpv2g fixed on a branch my base does not hold
    (b495389). Not this row's.
- Then by hand: protocol types (clean once committed), turn bench ok, `cargo deny --offline check` ok.
- The gate ran once on the whole tree, not separately per commit. `f2c30e0` alone touches only theseus-mcp, and its
  tests and clippy pass.
- The first gate run also failed `theseusd::versions` (×2, the format bump), `tests_tiering` (×2) and
  `tests_route_keep`. All are fixed above and pass.

Python under bench/ was not touched.

## The live check (the maintainer, on the Home account)

A scratch daemon with the Home account bound, never the operator's own. Put the stand-in extension below in the
scratch daemon's workspace as `ext-batch/server.py` (standard library only):

```python
#!/usr/bin/env python3
"""A stand-in extension with hands (theseus-pw1q.14's live check): an MCP server over stdio, standard library only.

Its one tool, `batch`, splits `files` names into `hands` groups, asks the daemon for `hands.run` on its own pipe, polls
`hands.status` every 5 s until the group settles, and answers with the group's result text.
"""
import json
import sys
import time

PENDING = {}


def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()


def ask(method, params, rid):
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    while True:
        line = sys.stdin.readline()
        if not line:
            sys.exit(0)
        m = json.loads(line)
        if m.get("id") == rid and "method" not in m:
            return m
        PENDING.setdefault("queued", []).append(m)


def batch(args):
    files = [f"in/file-{i:03}.txt" for i in range(int(args.get("files", 200)))]
    hands = int(args.get("hands", 20))
    per = (len(files) + hands - 1) // hands
    inputs = [files[i:i + per] for i in range(0, len(files), per)]
    spec = {"argv": ["sh", "-c", 'echo "$THESEUS_HAND_INPUT" | tr -cd , | wc -c'], "inputs": inputs,
            "ttl_secs": int(args.get("ttl_secs", 60)), "backend": "lambda"}
    got = ask("hands.run", spec, "run-1")
    if "error" in got:
        return f"refused: {got['error']['message']}", True
    gid = got["result"]["id"]
    n = 0
    while True:
        n += 1
        st = ask("hands.status", {"id": gid}, f"st-{n}")
        if "error" in st:
            return f"status refused: {st['error']['message']}", True
        if st["result"].get("settled"):
            return json.dumps({"run": got["result"], "status": st["result"]}), False
        time.sleep(5)


def main():
    while True:
        m = PENDING.get("queued", []).pop(0) if PENDING.get("queued") else None
        if m is None:
            line = sys.stdin.readline()
            if not line:
                return
            m = json.loads(line)
        method, rid = m.get("method"), m.get("id")
        if method == "initialize":
            send({"jsonrpc": "2.0", "id": rid, "result": {
                "protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}},
                "serverInfo": {"name": "batcher", "version": "1"}}})
        elif method == "tools/list":
            send({"jsonrpc": "2.0", "id": rid, "result": {"tools": [{
                "name": "batch", "description": "Transform files in a batch of hands.",
                "inputSchema": {"type": "object", "properties": {
                    "files": {"type": "integer"}, "hands": {"type": "integer"}, "ttl_secs": {"type": "integer"}}}}]}})
        elif method == "tools/call":
            text, err = batch(m["params"].get("arguments", {}))
            send({"jsonrpc": "2.0", "id": rid, "result": {
                "content": [{"type": "text", "text": text}], "isError": err}})
        elif method == "ping":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
        elif rid is not None:
            send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "no such method"}})


if __name__ == "__main__":
    main()
```

```bash
# 1. A scratch daemon on the Home account: its own --config, --socket, --state-dir; [discord] and [web] off.
theseusd --config /tmp/eh/theseus.toml --socket /tmp/eh/sock --state-dir /tmp/eh/state &
# 2. Have its model propose the stand-in with hands (expect the card's question in its result):
theseus --socket /tmp/eh/sock ask 'Call extend_propose with name "batch", dir "ext-batch", command ["python3","server.py"], description "Transforms files in batches of hands.", hands {"usd_cap": 0.25, "backends": ["lambda"], "max_group": 20}.'
theseus --socket /tmp/eh/sock confirm            # with no id: lists what waits
#    expect: "Load batch <short>: 1 tool, 0 of 0 tests passed, no network? This extension may run up to 20 hands at a time on Lambda, at most $0.25 a day."
theseus --socket /tmp/eh/sock confirm <id>        # Load, from the CLI (the owner's private surface)
theseus --socket /tmp/eh/sock extend list         # expect: "hands: up to 20 hands at a time on lambda · $0.00 of $0.25 today …"
# 3. The 200-item group under $0.25:
theseus --socket /tmp/eh/sock ask 'Call mcp__ext-batch__batch with {"files": 200, "hands": 20, "ttl_secs": 60} and tell me what it returned.'
#    expect: a result whose status says settled "met", succeeded 20; `theseus ledger` shows extend.hands.launched
#    then extend.hands.settled; `theseus extend list` today's spend well under $0.25.
# 4. A lowered test line refusing the next group: in /tmp/eh/theseus.toml set
#    [aws.accounts.<home id>] hourly_alert_usd = 0.01 and runaway_factor = 1, restart the scratch daemon, then ask step 3 again.
#    expect: "refused: Not run: AWS runaway mode on account …"; rows aws.runaway and extend.hands.refused (reason runaway); no Invoke.
# 5. Revoke: theseus --socket /tmp/eh/sock extend revoke batch   (expect the server gone; any open group cancelled)
theseus --socket /tmp/eh/sock shutdown
```

If the scratch daemon is root, L1 refuses the extension's start (theseus-pv6i). Run it as a user with a job cgroup.

## Left, uncertain, and for the owner

- **Choices to revisit.**
  1. Each day's spend has its own session, rather than one account with a daily reset. A group launched at 23:59
     reserves on that day's session and finishes there.
  2. `usd_cap` is at most $100.
  3. `max_group` counts every open group's hands together: it bounds "at a time", not one group's size.
  4. The extension's hands always run the hand image under `basic`, in the default account and region.
- **The Load and Don't load labels** now apply to every extension card. Discord renders the labels from the options,
  so its buttons change too. The Discord `/extensions` text does not show the hands line yet: the discord lane is in
  flight, so I left `runtime/extensions.rs` alone (one line, `ExtendHandsInfo::line`).
- **Docs for the maintainer**: the spec's Part III item, `docs/status.md`, and `docs/design/aws-toolset.md`'s hands
  section ("an extension's broker"). I updated theseus-core's and theseus-mcp's `AGENTS.md`.
- **Not done**: no rate limit on accepted runs beyond the cap and `max_group`; the cap is the brake there.
- **Disk**: the VM's allowance filled twice from incremental build dirs. Gates here ran with `CARGO_INCREMENTAL=0`.
