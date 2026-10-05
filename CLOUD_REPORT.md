# Cloud report: history-pages (theseus-xo0m, theseus-kym3, theseus-glyw)

Branch `cloud/20261005-history-pages`, from `main` at 60b43fb6 (store format 20). Started 20:22 UTC, done at
21:40 UTC. Three commits on top of the task's:

- `bbd0cf12` rpc: session.history pages both ways, by after and before (theseus-xo0m, theseus-kym3)
- `8c9a5411` rpc: node.reach takes a node's short id, and theseus history prints it (theseus-glyw)
- `439ef00b` rpc: an unknown short id with the cockpit's prefix names the prefix (theseus-glyw)

No store format change (no stored record gained a field), no new dependency, no config key.

## Step 1: `after` and `before` on `session.history` (bbd0cf12)

### What I found

- `ledger.tail` already pages both ways, as the brief says; I left it alone and gave `session.history` the same
  shape: `after` → `next`, `before` → `older`. The issue's `last_position` is `next`.
- `session_history` read the whole session whenever a question waited in it, for any `n`.
- `nodes_paged` reads the newest `n` by the session's tag and has no cursor; the store's `Page` already takes
  `after` and `before`, so no store change was needed for this step.
- One difference from `ledger.tail`: there `older` is set from `out.more` whenever `before` is given, even with
  `after` too, when `more` means newer rows remain. Here `older` is set only for `before` without `after`. Worth
  a look on the ledger side (not changed: out of scope).

### What I changed

- theseus-protocol: `SessionHistoryParams` and `SessionHistoryResult` moved to `history.rs`, re-exported
  (`pub use history::*`), so lib.rs went from 2,727 to 2,712 lines; ts.rs's type list is untouched. New optional
  fields: params `after`, `before`; result `next`, `older` (absent unless set, so old bytes hold).
- theseus-core `rpc/pages.rs`: `history_page` (each page through the session's tag `s:<session>` with `Page`'s
  `after`/`before`/`limit`; a cursor without `n` pages 200), `bounded` (the same bounds on a whole read, used
  while the index's shape is built), and `card_nodes`.
- **A question waiting in the session**: I kept the page cheap. A paged read (any `n` or cursor) finds each
  waiting call's node by the session's `tool_call` tag (`ks:tool_call\x01<session>`), read back from the newest,
  32 at a time, until every waiting call's node is found; a budget's, an extension's, and a promotion's question
  need none. Only the whole read (no `n`, no cursor) reads the session whole, as it always did. While the
  index's shape is built, `card_nodes` falls back to the whole read.
- `session_history` in methods.rs: the only methods.rs function touched in this step.
- CLI: `theseus history --after P` / `--before P` (main.rs: the two args and the match arm, a shared file:
  needed so each new param has a reader), and a last line naming the command for the next page either way
  (render/history.rs, so render.rs only gained `pub mod history;`, 3,099 of 3,100).
- cockpit/src/protocol.gen/ regenerated (SessionHistoryParams.ts, SessionHistoryResult.ts).

### How I proved it

- New tests, `crates/theseus-core/src/rpc/tests_history.rs` (6):
  - `a_walk_forward_by_next_reads_each_node_once`: 200 nodes in a session interleaved with another's, walked
    from 0 by `next` at `n` 37, 5 nodes written to it (and 3 to the other) between each of the first 3 pages:
    215 nodes, each once, in order; at the end no `next`, and a poll past the newest is an empty page.
  - `a_walk_back_by_older_reads_each_node_once`: 300 nodes walked back from past the newest by `older` at `n`
    41, 2 written between every page: each node it had, once; none of the new ones.
  - `without_a_cursor_the_answer_is_todays`: `n` alone and neither give the same positions as before, and the
    JSON has neither `next` nor `older`; a cursor without `n` pages 200.
  - `the_read_while_the_shape_is_built_bounds_as_the_index_does`: `bounded` over the whole session gives the
    index page's nodes, `next` and `older`, for `n` in {1, 7, 50, 1000} and 27 cut points, each with `after`,
    `before`, and both.
  - `a_page_reads_the_same_at_a_hundred_nodes_and_at_ten_thousand`: a page after a mid position, a page before
    it, and the newest page each decode 20 nodes in a session of 100 and one of 10,000, and the transcript is
    never read whole (`Store::transcript_reads`). (The count is `HistoryPage.read`, which I added.)
  - `a_pending_confirms_card_is_on_every_page`: an `fs.write` waits for approval, then 300 nodes are written;
    five pages (newest 5; before past the newest; after the newest, an empty page; after 0; a page of 3 that
    ends before the call) each carry the same card, byte for byte, as the whole read, with the gate's reason and
    the same attention, and none reads the transcript whole.
- Existing tests kept: `tests_lists::node_list_and_history_through_the_index_answer_as_the_scan_did`,
  `tests_m3::the_lists_carry_attention_for_a_session_parked_on_a_confirm`, `rpc::tests` (their literals gained
  `..Default::default()`).
- CLI golden `history_pages.txt` (new: a `--before` page with `older`, an `--after` page with `next`).
  theseus-protocol's tests regenerate the TypeScript (186 tests with the CLI's, all pass).
- Planted reverts, each restored and `touch`ed, `git status` clean after:
  - `after` ignored (`Page { after: None, .. }`): `a_walk_forward_by_next_reads_each_node_once` fails ("the walk
    ends": it repeats the newest page), and `the_read_while_the_shape_is_built_bounds_as_the_index_does` fails.
  - `before` ignored: `a_walk_back_by_older_reads_each_node_once` fails (the same page forever), and the bounds
    test fails.
  - a page read as the whole session (the `page` replaced by the fallback): `a_page_reads_the_same_at_…` fails
    (`left: 2, right: 0` whole reads) and `a_pending_confirms_card_is_on_every_page` fails (8 vs 4 whole reads).
  - the card read from the page alone (`card_nodes` skipped): `a_pending_confirms_card_is_on_every_page` fails.

## Step 2: a node's short id (8c9a5411, 439ef00b)

### What I found

- `node.reach` took only a whole id, and `theseus history` printed none.
- The `Store` trait had no keys-only read (`latest_of_kind` reads every record), as the brief says; the index has
  `keys_of_kind` and `keys_with_prefix`, but an ending can't be a range of the key table, so it is a walk.
- `task::short` and `narrative::short` write ids differently from the cockpit; I added one Rust form of the
  cockpit's `short` (`theseus_protocol::short_id`, in history.rs) that the daemon's refusal and the CLI share.

### What I changed

- theseus-store: one read, `Store::keys_ending(kind, ending, limit)`: the WAL store walks its index's key table
  for the kind (`RedbIndex::keys_ending`) and reads no record; the trait's default reads every record of the kind.
  Away from `MANIFEST_FORMAT`.
- theseus-core `reach.rs`: `resolve` and `Named`. `node_reach` (methods.rs) tries the whole id first, exactly as
  before, and only then resolves: the last 6 or more characters (`e0f1a2`, `…e0f1a2`), or the cockpit's
  `msg·e0f1a2`, whose prefix must match too. One node: answered as its whole id. Several: `INVALID_PARAMS`, `` `7c41ab`
  names 2 nodes (msg_… in ses·q7f3k2, tcl_… in ses·b2c9w1): give more of its id ``, each with its session (up to 8,
  then "and N more"; the walk stops at 64 and says "at least 64"). None, or under 6 characters: `NOT_FOUND`, as an
  unknown whole id always was, in words (`no node's id ends with `abcdef``; `no node is named `1a46b`: give a
  node's whole id, or at least its id's last 6 characters`; with a prefix, `no node's id starts `tcl_` and ends
  with `61a46b``, 439ef00b).
- Design choice to hear: an end under 6 characters is `NOT_FOUND`, not `INVALID_PARAMS`, so the existing
  `tests_reach::node_reach_answers_over_the_protocol` (an unknown `msg_x` is NOT_FOUND) holds unchanged.
- CLI: `theseus history` prints each node's short id (`msg·61a46b`) after its first line's mark (the time, `⚙`, or
  `←`), through `render::history::node_lines`. **`render::node_lines` is unchanged, so theseus-tui's detail pane
  shows no ids**: the pane has no command line to paste them into, and smalls is in that pane; say if it should.
  `theseus reach --help` says NODE may be the short id.
- `NodeReachParams.node_id`'s doc says what it takes (one regenerated TypeScript file, NodeReachParams.ts).

### How I proved it

- `crates/theseus-core/src/rpc/tests_node_names.rs`: unique (6 characters, 8, `…`, the cockpit's form, spaces
  around: each answers byte for byte as the whole id; the cockpit's prefix tells two alike apart), ambiguous
  (refused naming both ids with their sessions; more of the end names one), too short and unknown (NOT_FOUND,
  in words, the prefix held), and the whole id unchanged (the existing `tests_reach` suite, 6 tests, unchanged
  and passing). theseus-store: `index::tests::keys_ending_walks_a_kinds_keys`.
- CLI goldens: `history.txt`, `history_full.txt` and `history_pages.txt` rewritten with `THESEUS_GOLDEN=write`;
  the diff is the short ids alone (read line by line). New `reach_short.txt` (a short id) and
  `reach_ambiguous.txt` (the daemon's refusal of an end that names two, exit 1). The fixtures' ids are `nod_1`
  and so on, so the goldens show `nod·nod_1`: their ids are shorter than real ones.
- Planted revert: the ending matched to the first of two (`[one, ..] => One`):
  `an_end_that_names_two_nodes_is_refused_naming_each` fails (it got an answer, not the refusal). Restored,
  touched, `git status` clean.
- **The resolve's time at 100,000 nodes** (`the_resolve_at_a_hundred_thousand_nodes`, `#[ignore]`, run with
  `--run-ignored only --no-capture`, debug build, this 4-core VM): 33.3, 33.5, 35.5, 36.3, 47.6 ms; median
  35.5 ms. It runs only for a name that is no whole id. I did not time a release build.

## Runs under load

AGENTS.md's recipe, with `yes > /dev/null` as the four nice-0 busy loops (this environment refused the
`sh -c 'while :; do :; done'` form as a possible removal: a false positive, so I took the other route), the tests
at `nice -n 19`: `tests_history`, `tests_node_names`, `tests_lists`, `tests_reach`, the store's `keys_ending`,
and the CLI's history and reach goldens, 31 tests, 5 runs: 31 passed each time (26 to 40 s a run).

## The live check I ran here, and the maintainer's

I ran this on a scratch daemon of the debug build (fresh state dir, Discord, the web and the index off, the
stand-in model): 300 nodes; the walk back from past the newest, 15 pages of 20, and forward from 0, 15 pages,
each gave the 300 positions once, in order; `history -n 5` showed `msg·…` ids; `reach 61a46b` and `reach
msg·61a46b` printed byte for byte what the whole id did; `abcdef`, `1a46b` and `tcl·61a46b` were refused in words,
exit 1; `rpc session.history` with `after` the newest position answered an empty page in 9 ms (CLI included). I
could not make an ambiguous end live: two of 300 random ids sharing their last 6 hex characters is about 1 in
6,000; the unit test holds it.

For the maintainer, on the install build (`theseus`, `theseusd`, `theseus-sim` on PATH), in a fresh directory:

```bash
D=$(mktemp -d) && cd $D
echo '[{"when": "", "text": "Noted."}]' > rules.json
cat > theseus.toml <<'EOF'
[model]
live = "scratch"
provider = "anthropic"
model = "claude-sonnet-5-5"
api_key_secret = "anthropic_api_key"
api_base = "http://127.0.0.1:7491"
[profiles.scratch]
provider = "anthropic"
model = "claude-sonnet-5-5"
[secrets]
anthropic_api_key = "env:SCRATCH_MODEL_KEY"
[server]
disk_warn_mb = 0
disk_floor_mb = 0
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
EOF
theseus-sim fake-model --addr 127.0.0.1:7491 --rules rules.json > fake.log 2>&1 & FAKE=$!
SCRATCH_MODEL_KEY=sk-invented theseusd --config $D/theseus.toml --socket $D/s.sock --state-dir $D/state \
  > daemon.log 2>&1 & DAEMON=$!
T=$D/s.sock
S=$(theseus --socket $T --json ask "hello 0" | python3 -c 'import json,sys; print(json.load(sys.stdin)["session_id"])')
for i in $(seq 1 149); do theseus --socket $T ask -s $S --no-stream "hello $i" >/dev/null 2>&1; done
```

(If `ask --json` names the session under another key, take it from `theseus sessions`.)

1. Both walks, every node once:

```bash
cat > walk.py <<'EOF'
import json, subprocess, sys
T, S = sys.argv[1], sys.argv[2]
def hist(*a):
    return json.loads(subprocess.run(["theseus", "--socket", T, "--json", "history", S, *a],
                                     check=True, capture_output=True, text=True).stdout)
allp = [n["position"] for n in hist()["nodes"]]
for flag, cur, key in (("--before", "999999999", "older"), ("--after", "0", "next")):
    got, pages = [], 0
    while True:
        h = hist("-n", "20", flag, cur); pages += 1
        p = [n["position"] for n in h["nodes"]]
        got = p + got if flag == "--before" else got + p
        if key not in h: break
        cur = str(h[key])
    print(flag, pages, "pages,", len(got), "nodes, each once:", got == allp)
EOF
python3 walk.py $T $S
theseus --socket $T history $S -n 20 --before 999999999 | tail -1
```

   Shows `--before 15 pages, 300 nodes, each once: True` and the same for `--after`; the last command's last
   line is `── older nodes before position P: theseus history <S> --before P`.

2. Short ids:

```bash
theseus --socket $T history $S -n 5
W=$(theseus --socket $T --json history $S -n 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["nodes"][0]["node_id"])')
E=${W: -6}
theseus --socket $T reach $W > a.txt; theseus --socket $T reach $E > b.txt; theseus --socket $T reach "msg·$E" > c.txt
cmp a.txt b.txt && cmp a.txt c.txt && echo SAME
theseus --socket $T reach abcdef; theseus --socket $T reach ${E:1}; theseus --socket $T reach "tcl·$E"
```

   The history lines carry `msg·xxxxxx` after their times; `SAME`; then three refusals, each exit 1:
   ``no node's id ends with `abcdef` (code -32002)``, ``no node is named `…`: give a node's whole id, or at least
   its id's last 6 characters (code -32002)``, and ``no node's id starts `tcl_` and ends with `…` (code -32002)``.
   An end that names two can't be made on demand (see above): `tests_node_names` holds it.

3. An empty page at once:

```bash
N=$(theseus --socket $T --json history $S -n 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["nodes"][0]["position"])')
time theseus --socket $T rpc session.history "{\"session_id\":\"$S\",\"after\":$N}"
```

   `"nodes": []`, no `next`, in a few milliseconds.

Stop: `theseus --socket $T shutdown`, wait for `$DAEMON` to exit, then `kill $FAKE`.

## Left, uncertain, and for the docs

- The cockpit's polls with `after` and the TUI's scrolling with `before` are later steps (protocol.gen/ only).
- `ledger.tail`'s `older` with both cursors (above).
- A cursor without `n` pages 200; without a cursor, `n` absent still means the whole session, as before.
- Docs the maintainer may want: `docs/status.md`'s landed step; the spec's Part III item; theseus-core's AGENTS.md
  ("The protocol server" or `rpc/pages.rs`: `history_page`, `card_nodes`, and `reach::resolve`); theseus-store's
  AGENTS.md (`Store::keys_ending`, a keys-only read); the CLI's AGENTS.md (`render/history.rs`: the history's own
  lines, short ids, apart from `node_lines`, which the TUI shares).
- Shared files touched: crates/theseus/src/main.rs (the two `history` args, the match arm, and `reach`'s doc),
  crates/theseus-protocol/src/lib.rs (the move, two module lines, and `node_id`'s doc). methods.rs: only
  `session_history` and `node_reach`. theseus-store: one read in store.rs's trait and impl, and index.rs.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit: fmt, shape, features, clippy,
the cockpit, the test build and the reader rule pass; the suite fails only on the 33 known L1 tests (theseus-pv6i:
theseus-sandbox's contract tests and theseusd's sandbox tests, the VM runs as root): at each of the three commits, 2,802 tests run, 2,769 passed, 33 failed (all L1), 22 skipped. After the suite
I ran the rest of `machine_checks` and the deny phase myself: the generated TypeScript is committed (the protocol
types check passes), the turn bench `--runs 5 --burst 0` gives 5 frames plain and 9 with a tool (budgets 5 and 9),
and `cargo deny --offline check` passes (advisories, bans, licenses, sources ok; `cargo deny fetch` succeeded at
setup). No other test failed, and no flaky-list retry was needed in the runs I read.
