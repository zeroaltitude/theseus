# Cloud report: step 43b, an acked extension loaded, restarted, and revoked (theseus-ext.8)

Branch `cloud/20261004-extensions-load`, from main at d3cad45. Three commits on top of the task commit:

| Commit | What |
|---|---|
| 70b6a83 | extend: an acked extension loads, restarts with the daemon, and is revoked |
| 6a5eff6 | discord: /extensions lists the loaded extensions, each with a Revoke button |
| 29a80d8 | cockpit: an Extensions card in Systems, with each loaded extension and Revoke |

No new dependency. No store format bump: the new record is a META key of its own (`extensions`), as 36b read the
rule for `mcp.tools.<server>`. No existing stored record gained a field.

## Step 1: load on ack, restart, revoke, a new version, the CLI and health (70b6a83)

### What I found

- 43a's ack (`extend/answer.rs`) binds the question's confirm in one `Kernel::frame` and loads nothing. The board's
  `servers` is a map fixed at build, so `rebuild` can't offer a server added later. A trial (`mcp/trial.rs`) is a
  `Server` kept off that map.
- A turn's tools are fixed once per turn: `TurnRunner::request_spec` calls `ToolRuntime::definitions_for` once, before
  the loop (turn.rs, about line 1553). So anything that changes the catalog applies from the next turn without more
  work. The model also can't call a tool its spec never offered.
- `ToolPolicy::posture` checks `[policy.mcp]` "server/tool", then "server", then falls back to the enforcement. The
  `[policy.mcp]` key check accepts `ext-wordcount` already.

### What I changed

- **The ack's frame** (`extend/answer.rs`, with `Core::load_of` in `extend/load.rs`) now also stages:
  - the META record `extensions`: name to `Loaded` (digest, description, command, frozen copy, source, tools,
    capabilities, who acked and through what, when, the question, the proposing session, that session's place
    name and its ceiling at the ack, and the digest it replaced);
  - the `extend.loaded` row (it names `replaced`);
  - the server's stored list `mcp.tools.ext-<name>`, built from the trial's tools. Otherwise a new version would
    offer the old version's list until it listed again;
  - a replaced version's manifest, now in state `replaced`.

  All of this is written **in the ack's own frame**. After the frame, `Core::board_load` puts the server on the board.
  `answer_extension` holds `Extensions::writes`, a mutex, so a load and a revoke never interleave their
  read-modify-write of `extensions`.
- **The board** (`mcp/ext.rs`, plus about ten lines in `mcp/mod.rs`): `extensions: Mutex<BTreeMap<String, Loaded>>`
  sits beside `servers`.
  - `rebuild`, `status`, `stop`, `restart`, and `start` include the loaded servers.
  - `load_extension` builds the `Server`. If the board has started, it spawns 36b's own `tend` for it, under the
    list's lock, so a load racing `start` is tended exactly once.
  - Loading over an older version replaces it, then ends the old one.
  - `unload_extension` takes the server off the board, rebuilds the catalog, aborts its tending, and sends SIGTERM
    through its client to its process group.
- **What runs** (`Loaded::server_cfg`): 43a's `trial_cfg`, meaning the frozen copy, L1, the acked network as its
  egress, and no secret. On top of that, `external = !network.is_empty()` (Q22) and the configured default of
  `call_timeout_secs = 110`.
- **The gate**:
  - `ToolPolicy::posture` gives an `ext-` server's tool `notify` unless `[policy.mcp]` says otherwise. The setting is
    "an extension's default". I take the stricter of notify and the enforcement, so an `approve` enforcement still
    asks.
  - Class `Run` comes from `McpTool` itself, since an extension has no `read` list.
  - A configured server named `ext-…` is refused at the config's check (`config/mcp.rs`).
- **Never wider.** The proposing session's place and ceiling at the ack are recorded in `Loaded`. That ceiling's
  **floor** holds every call of the extension's tools, wherever the call comes from: `Floors::floor`, one line in
  `toolrun.rs` right after the place's own floor.
  - The recorded ceiling's `tools` list is recorded but not applied. The proposing place may list `extend` without
    `mcp:ext-<name>`, which can't exist before the ack. Applying it would make the extension unusable in the place
    that asked for it.
  - Where the tools are offered is where MCP tools are: private places only, and within each place's own ceiling
    (`mcp:ext-<name>`). Its process gets only what was asked and acked.
- **Restart** (`Core::seed_extensions`, called in `Core::build`): reads `extensions` (one key) and each extension's
  stored list (one key each), and loads each without tending, as 36b's configured servers are loaded. `start`, after
  serving, tends them. A record that does not read is logged and skipped; it does not fail the start.
- **Revoke** (`extend/revoke.rs`; the RPC wrapper is in `rpc/mcp.rs`, since `Conn` is private to `rpc`):
  - `extension.revoke { name, author?, discord? }` names no loaded extension: refused, saying so.
  - Otherwise it goes through `judge_act(Act::Revoke { name })`. A refusal is ledgered as `approval.refused` with
    `act: extension.revoke` and answered `REFUSED`.
  - Then one frame: the record without it, its manifest `revoked`, and `extend.revoked`. Then the floor is cleared
    and the board unloads it. The frozen copy stays on disk.
  - The method is in the CLI's `OPERATORS` as `theseus extend revoke`.
- **Surfaces**:
  - `extend.list` gains `loaded: Vec<ExtendLoadedInfo>`, each with its board state, calls, errors, and last error.
  - `theseus extend list` prints the loaded ones first, each with a `revoke:` line. `theseus extend revoke <name>`
    is new.
  - Health's `extensions.loaded` counts them, and health's `mcp[]` shows `ext-` servers.
  - Narrative lines for `extend.loaded` and `extend.revoked`. `extend.acked` no longer says "nothing loads in this
    build".
  - MCP telemetry applies unchanged: `span_attrs` keys on the `mcp:` prefix.
- **Protocol**: the `extension.revoke` method; `ExtendLoadedInfo`, `ExtensionRevokeParams`,
  `ExtensionRevokeResult`; the ledger kinds `extend.loaded` and `extend.revoked`. The TypeScript is regenerated.
  `theseus-protocol/src/lib.rs` goes from 2,636 to 2,638 lines, so I raised its ceiling in `scripts/long-files.txt`
  with that reason.
- **43a's tests changed where the behaviour did.** `no_tool_is_offered_before_the_ack_nor_after_it` is now
  `no_tool_is_offered_before_the_ack`. theseusd's 43a test now expects `mcp.list` to list the 5 tools after the ack.
  `FromDir` and `proposal` became `pub(super)`, and `FromDir` keeps each fake's serving task.

### How I proved it

- `cargo nextest run --workspace -E 'package(theseus-core) and test(/extend::/)'`: **16 of 16 pass** (6 new in
  `extend/tests_load.rs`, 10 from 43a). The new ones:
  - `an_acked_extension_is_offered_from_the_next_turn_never_mid_turn`:
    - The ack lands from inside a turn's first model call. Both of that turn's requests offer no `mcp__` tool,
      though the catalog has them after the first.
    - The next turn is offered `mcp__ext-wordcount__echo`, and its call returns the text. It ran from the frozen
      copy, in L1, with no egress, no env, and `external = false`.
    - The posture is notify ("an extension's default") though the config's enforcement is open. No T1 hold.
    - `extend.list` and health show it, and `calls == 1`.
  - `a_restart_starts_it_after_serving_from_the_frozen_copy`:
    - The extension is acked before the board starts, then the workspace's `mode` is edited to `error`.
    - A new core on the same store offers the tool at once with no start before `start`, and its state is
      `stopped`.
    - After `start` it starts exactly once, from the frozen path, and its call succeeds as frozen.
  - `a_revoke_drops_its_tools_and_stops_its_server`:
    - A revoke from a shared guild channel is refused with `approval.refused` (`act: extension.revoke`), and the
      extension stays loaded and ready.
    - The CLI's revoke writes `extend.revoked`, empties the catalog, and takes it off the board. The fake that
      served the load ends, so its connection closed.
    - The frozen copy stays, the manifest is `revoked`, and the next turn is not offered the tool. A second revoke
      says "no extension named".
  - `a_second_version_replaces_the_first_only_once_acked`: v2 proposed but not acked leaves v1 running and listed.
    Acking v2 writes `extend.loaded` with `replaced = v1`, v2 starts from its own frozen copy, v1's manifest is
    `replaced`, and one server of the name remains.
  - `a_shared_places_turn_is_never_offered_an_extensions_tool`: a shared place's turn is offered no `mcp__` tool, and
    the model's call of one there does not succeed. A private session's next turn is offered it.
  - `its_posture_is_the_policy_lines_and_its_loads_ceiling_floors_it`: `[policy.mcp] "ext-wordcount" = "open"` wins.
    A load under `#pier`'s `posture_floor = "approve"` makes its calls approve, the reason names `#pier`, other
    servers are untouched, and the floor goes once it is cleared.
- **Planted reverts** (each restored, `touch`ed, and checked with `git status`):
  - *A load applied to the running turn's tools.* I added `spec.tools = self.tools.definitions_for(t.tc.place());`
    before each loop's `compile_step` in turn.rs. `an_acked_extension_is_offered_from_the_next_turn_never_mid_turn`
    failed "offered mid-turn": the second loop listed the five `mcp__ext-wordcount__*` tools.
  - *Restart from the workspace's dir.* In `seed_extensions` I loaded with `frozen = source`.
    `a_restart_starts_it_after_serving_from_the_frozen_copy` failed: the start ran from `…/work/tools/wc`, not the
    frozen path.
- **theseusd's `tests/extend.rs`, real processes.** The new test is
  `an_acked_extension_loads_survives_a_restart_and_is_revoked`, and the 43a test is updated. As root, 2 of 2 pass:
  L1 refuses the trial, and the new test checks the refusal and returns. As uid 65534, run as 43a ran it
  (`setpriv --reuid=65534 --regid=65534 --clear-groups env HOME=/tmp TMPDIR=/tmp target/debug/deps/extend-<hash>`
  from `/tmp`), **2 of 2 pass**. The new test checks:
  - After the ack: `extend.loaded`, and `mcp.list` offers the tool at once.
  - `ext-wordcount` is ready with its process tree `mcp-sandbox`, then `job-sandbox`, then
    `theseus-sim fake-mcp`. A temporary trace printed it; I removed it after.
  - `theseus extend list` shows it loaded and ready.
  - A turn's `mcp__ext-wordcount__echo` returns its text.
  - `shutdown` ends all three processes.
  - A new daemon on the same state offers the tool at once. `mcp.started` for `ext-wordcount` comes after the
    start's `server.serving` row, and the next turn calls it again.
  - The daemon's `kill -9` leaves none of its processes. After a new start, `theseus extend revoke` from a job's shell
    (`THESEUS_SESSION` set) is refused with "theseus extend revoke refused" and no `extend.revoked`.
  - From the operator's shell it is revoked: its processes are gone, `mcp.list` has no tools, health's `mcp[]` has
    no `ext-wordcount`, and the list says `revoked`.
- **Under load** (four `while :; do :; done` loops at nice 0, killed by their pids; tests at nice 19):
  - theseus-core's `extend::` filter: **5 of 5 runs, 16 of 16 each**, about 16–19 s a run.
  - theseusd's `tests/extend.rs` as uid 65534: **5 of 5 runs, 2 of 2 each**, about 26–31 s a run.
- Also: the CLI's `the_operators_methods_are_refused_inside_a_job` covers the new `OPERATORS` entry, the config's
  refusal of `mcp.servers."ext-wordcount"` is in `unknown_keys_and_bad_servers_are_refused`, and the protocol's
  tests regenerate the TypeScript.

## Step 2: Discord's `/extensions` (6a5eff6)

- **What I changed.** `runtime/extensions.rs` is a module of its own; runtime.rs gains 15 lines (now 3,447, ceiling
  3,500).
  - `/extensions` is registered globally. It answers the presser alone (ephemeral) with each loaded extension: name,
    short digest, state, network, tools, who acked it, and when (`<t:…:R>`).
  - Each extension gets a Revoke button (`ext-revoke:<name>`, five to a row, 25 at most).
  - The text and a press go through the place (`Control::Extensions`, `Control::Revoke(name, origin)`), so a press is
    `extension.revoke` with the presser's ids, and the core judges it. After a press the message is updated with what
    happened above the list as it is now.
  - The hook runs in `on_interaction` after the presser's ids are known and before the confirm buttons' branch.
- **How I proved it.** theseus-discord: **99 of 99 pass**.
  - `the_list_says_each_loaded_extension_with_a_revoke_button`: the text, 7 buttons in 2 rows, and the id parsing.
  - `revoke_goes_to_the_core_as_the_presser`, through `place_for_tests`: the list from the core. A press from a
    shared guild channel answers "⚠️ wordcount was not revoked: … shared …", and the extension is still listed. The
    owner's press from their DM answers "🧩 Revoked **wordcount** `3f2a1c`", and the list is then empty.
- **One edit outside my area.** The voice test `join_names_a_voice_channel_and_leave_nothing` counts the binding's own
  commands before `/join`. It now counts ten (`/extensions` is the tenth) and 12 in all. The edit is one assertion and
  its comment.

## Step 3: the cockpit's Extensions card (29a80d8)

- **What I changed.** `components/Extensions.tsx` is a card in the Systems view, shown once health counts any
  proposal. I added no route, because `main.tsx` is a join file. The card shows:
  - each loaded extension: digest, state, server, network, tools, who acked it and when, the manifest's tests and
    description, files and bytes, the command, the frozen copy, calls and errors, the replaced digest, and its last
    error;
  - a Revoke button, confirmed first, which calls `extension.revoke`;
  - then the proposals that are not what runs, with the `theseus confirm` id for a waiting one.
  - Its pure parts are `lib/extensions.ts`.
- `ExtendInfo` gains `files` and `bytes`, absent when zero.
- **How I proved it.** `npm run lint` (no new warning), `npm test` (**34 pass**, 2 new in
  `test/extensions.test.ts`), and `npm run build`, all inside the gate. I did not look at it in a browser: no daemon
  here has a loaded extension, since L1 refuses root.

## The live check (the maintainer's, on the owner's machine)

This goes on from 43a's check, on its scratch daemon, where `wordcount` was acked. Let `T="theseus --socket <scratch
socket>"`. That ack was made by 43a's build, so under 43b's build nothing is loaded yet. Propose and ack the counter
again under this build (43a's steps), or confirm a fresh proposal. Then:

1. **The ack loads it.** `$T rpc ledger.tail '{"n":200}' | grep extend.loaded` shows the row with `"server":
   "ext-wordcount"`. `$T extend list` starts with `wordcount <digest>  loaded as ext-wordcount  ready  no network`
   and a `revoke: theseus extend revoke wordcount` line. `$T mcp` lists `ext-wordcount` ready, with its tools at
   posture notify ("an extension's default").
2. **A turn calls it.** Ask a turn for the word count of a scratch file. `mcp__ext-wordcount__…` runs, the operator
   gets its notify notice, and the ledger has its call. `ps -o pid,cmd --ppid <role pid>` shows `job-sandbox`, then
   the server below it.
3. **Restart.** `$T shutdown`, and check that none of the extension's processes are left (`pgrep -f fake-mcp`, or
   the server's own name). Start the scratch daemon again. `$T mcp` shows `ext-wordcount` ready after serving; in
   `ledger.tail`, its `mcp.started` comes after the start's `server.serving`. The next turn calls it again.
4. **Revoke.** `THESEUS_SESSION=ses_x $T extend revoke wordcount` is refused ("theseus extend revoke refused …").
   Then `$T extend revoke wordcount` prints `Revoked wordcount <digest>: its server stopped …`. Check:
   - `ledger.tail` has `extend.revoked`;
   - `$T mcp` no longer lists the server or its tools;
   - `pgrep` finds none of its processes;
   - the next turn is not offered the tool;
   - `<state>/extensions/wordcount/<digest>/` still exists.
5. **For the owner's own press:** `/extensions` in their DM lists it with a Revoke button, and the button revokes
   it. The same press in a shared channel answers with the refusal. The cockpit's Systems view shows the Extensions
   card.

## What is left, or uncertain, and design choices for the owner

- **The proposing ceiling's floor applies everywhere; its `tools` list does not** (see Step 1, "Never wider"). If the
  owner reads "its ceiling is the proposing execution's at the ack" more strictly, `Floors` is where the `tools` list
  would be applied too.
- **Notify is never looser than the enforcement.** An `approve` enforcement makes extensions ask, and a
  `[policy.mcp] "ext-<name>"` line still overrides either way.
- **The latest ack wins.** A new version replaces the old one at its ack, not once it is up: the old one stops when
  the new one is put in its place. If v1 is acked after v2, v1 loads in v2's place, because the latest ack wins.
- **An extension's `list_changed` is honoured as any MCP server's is,** with a notice. An extension can therefore
  change its own tool list at run time from its frozen code, behind a notice and `mcp.tools_changed`. A stricter
  rule, offering only the acked tool names, would be a filter in `rebuild`.
- **A narrow race in revoke:** an abort that lands while its tending task is between awaits on another worker can
  leave a client the task just set. Dropping that client still SIGTERMs the group once the last clone goes, but not
  at once.
- **Docs for the maintainer to write:**
  - Part III's item for 43b;
  - `docs/status.md` (43b landed; M7 row 43 done);
  - `docs/design/m7-surface.md` §2.7 could note:
    - the `extensions` record holds the place and ceiling;
    - Q22 is applied (outside text only with a network);
    - the floor-only reading of "never wider";
  - the spec's §3.21 could name `extension.revoke` and `/extensions`.
  - The AGENTS.md files of theseus-core, theseus-discord, and the cockpit are updated in these commits.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the last commit's tree:
- **Passed:** fmt, shape, features, clippy, the cockpit, the test build, and the reader rule.
- **The suite failed on 34 tests, none of them this step's.** I ran the phases after it myself (`machine_checks`'
  rest: the protocol types are clean, the turn bench's frames are 5 and 9 against budgets of 5 and 9, and `cargo deny
  --offline check` is ok), so I count each commit green. The failing tests, the same list at each of the three
  commits:
  - **33 L1 tests that need a non-root user** (theseus-pv6i). Each fails with "the daemon runs as root, and Linux
    exempts root from RLIMIT_NPROC, so an L1 job would have no process limit". They are:
    - 19 of `theseus-sandbox::contract` (`clause_01`…`clause_12`, the three `egress_18b_*`,
      `a_job_that_cannot_start_says_why`, `exit_status_and_signals`, `scratch_is_reported_and_discarded`,
      `sigterm_is_forwarded_to_the_command`), and `theseus-sandbox::bench spawn_100`;
    - 13 of `theseusd::sandbox`: `a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace`,
      `a_deadline_ends_a_job_mid_tunnel_…`, `a_granted_program_in_l1_…`, `a_job_with_no_list_has_no_proxy`,
      `a_listed_host_is_reached_through_the_proxy_…`, `a_running_l1_jobs_command_…`, `a_probe_script_in_l1_…`,
      `health_reports_the_last_real_l1_launch`, `an_aws_granted_l1_job_…`, `the_jobs_bench_l1_row`,
      `l1_argv_routes_a_call_to_l1`, `unlisted_and_private_hosts_are_refused_…`, and
      `a_host_beyond_the_list_once_approved_is_outside_text`.
  - **`theseus-core tests_output::the_cores_output_matches_its_golden`: this VM's time zone.** The only difference is
    a wake's time printed with `+#:#` (this VM is UTC) where the golden has `-#:#`. With
    `TZ=America/Los_Angeles` the test passes. The golden is unchanged.
- The VM's disk filled once mid-gate (`target/debug/incremental` at 18 GB). I deleted that cache and the gate ran
  again.
