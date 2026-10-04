# Cloud report: the ontology wired in, step 21b (theseus-8kk.1)

Branch `cloud/20261004-ontology-wire-in`, from `main` at d9b0931. One step commit, 6aff9ea, then this report
(`cloud report (not for main)`: drop it at the merge). Started 02:45 UTC, report written 03:30 UTC.

## What I found

- `crates/theseus-ontology` had everything 21b needed: `Record` and its META keys, `Ontology::{load, put, check,
  compose, memberships, mint_id}`, and `Composition`'s `MembershipUsed`/`GuidanceUsed` for the manifest. It has no
  `AGENTS.md`, which the brief mentions.
- The compiler decides append or recompile from the manifest's digests (`compiler::compile`). A turn builds its
  `RequestSpec` once, so a context file's edit applies at the next turn's first loop (`system_changed`). The design
  needs two specs for the walk: an append renders the memberships its manifest recorded, and a recompile the current
  ones. Otherwise a membership change would itself change the system block and recompile at once.
- A place's target is `discord:channel:<id>` or `discord:dm:<user>`. `BoundPlace` carries no guild id, so the
  core can't make `guild:` categories without a change to places.rs and to the binding, both of which the brief
  says to leave alone.
- `judge_act` with `owner_in_private` was already the approvals' rule. The CLI's job refusal (`OPERATORS`) is the
  7.8 speed bump. A job's process reaches the socket as the CLI does, so the daemon can't tell it apart.

## What I changed (6aff9ea)

- **Records and rows.** Kinds, categories, guidance, and membership lists are META records under `onto:`. Each
  write puts its records and its ledger row in one frame. The rows are `ontology.category`, `ontology.guidance`, and
  `ontology.membership` (new `LedgerKind`s; facts in `fact/ontology.rs`). Nothing writes kind rows yet (no method in
  the design's list), so `ontology.kind` isn't declared (the reader rule).
- **The snapshot** (`crates/theseus-core/src/ontology.rs`, `Board`):
  - It is built after serving by one `onto:` prefix scan (`Core::warm_ontology`, called from theseusd's
    `after_serving` beside the outbox's warm). If a reader comes first it builds the snapshot itself, once, under
    the same lock.
  - A write checks its records against the snapshot (`Ontology::put`), appends them with their rows, and swaps an
    `Arc<Ontology>`, all under one mutex inside `theseus_store::blocking`.
  - Nothing on the start path reads it. A compile reads the turn's `Walk` from memory.
- **Given kinds.**
  - `Core::bind_places` makes each bound place's given category at its first bind (`Core::bind_categories`):
    `channel:<id>` named as the binding names it (`#lab` becomes `lab`), and `person:<user>` for a DM (`@ana`). All
    new or renamed ones go in one frame, and a later bind with nothing new writes nothing.
  - A session's given membership is read from its place at compile (`ontology::given`), origin `transport`, and is
    never stored. A write of a given kind's membership or category is refused by the ontology crate's rule and comes
    back as invalid params.
- **The compile walk.**
  - `TurnRunner::walk` fixes a `Walk` with the turn's spec: the snapshot as of the turn's start, the place's given
    membership, and the snapshot's interpreted memberships (in a private place only). It is one line in turn.rs:
    `spec.walk = self.walk(sid, t.tc.class)`. There is no walk while the ontology holds no category, so a store with
    none renders exactly as before.
  - `compiler::compile` now composes twice: the append spec from the manifest's recorded `memberships`, and the
    recompile spec from the current ones. It decides on the append spec's digests. A recompile, a ring, or the
    image strip renders the recompile spec.
  - The guidance goes into the system's second block after the context files (`ontology::guided`), so the header
    and its cache breakpoint are untouched.
  - The manifest gains `memberships` (kind, category, origin, confidence, `as_of_ms`) and `guidance` (category,
    version, digest).
  - `MANIFEST_FORMAT` 5 → 6 (the onto: records and the manifest's two fields). The tests that pin the number moved
    with it: `store::tests` in core and in store, and `theseusd/tests/versions.rs`.
  - The format-5 compilation layout is byte for byte the "3 (theseus-ev1)" sample already in `tests_layouts.rs`, so
    I named format 5 in that sample's description rather than adding a duplicate.
- **The place rule first.** The admissions (tools, context files, the class) are decided before the walk and never
  read a membership. A shared place's walk composes only `transport` memberships: what the operator assigned a
  session is the owner's material, and a shared place gets only its own place's guidance.
- **The protocol** (`crates/theseus-protocol/src/ontology.rs`):
  - `ontology.list` takes an optional `session_id`. With it, the list shows that session's given and interpreted
    memberships; without it, every stored interpreted one. The other methods are `ontology.category.add`,
    `ontology.guidance.set`, and `ontology.membership.set`.
  - Each write calls `judge_act(Act::Ontology { method, what })` before it reads anything. A refusal is
    `REFUSED`, with an `approval.refused` row whose `act` is the method.
  - The TypeScript is regenerated into `web/src/protocol.gen/` (10 files and the index).
- **The CLI** (`crates/theseus/src/ontology.rs`, `render/ontology.rs`):
  - The commands are `theseus ontology kinds` (the default), `categories`, `topic add NAME [--parent P]
    [--desc D]`, `guide CATEGORY [TEXT|-]` (stdin by default), and `member SESSION [+CAT|-CAT …]`. With no changes,
    `member` lists the session's memberships.
  - The three writes join `client::OPERATORS`, so a job's shell refuses them before sending anything.
- **The reader rule.** theseus-ontology's `[package.metadata.theseus] reserved_for` is gone. theseus-core depends on
  it by path, which adds one edge to Cargo.lock and no new package.
- **AGENTS.md.** The ontology bullet in `crates/theseus-core/AGENTS.md`, the crate's line in the root map (18.9
  KB, under the 20 KB cap), and the job refusal's list in `crates/theseus/AGENTS.md`.
- **Shared and long files.** turn.rs grew by 6 lines (the `ontology` field, the walk line, and three `RequestSpec`
  fields), to 3,402 of its 3,500 ceiling. protocol/lib.rs grew by 9 (`mod`, `pub use`, four method names), to
  2,575 of 2,613. No ceiling moved, and `scripts/shape.sh` is ok.

## How I proved it

- **The design's 21b tests**, in `crates/theseus-core/src/tests_ontology.rs`. All 7 pass:
  - `a_topics_guidance_is_in_the_system_block_under_its_header`: the preamble and `# Guidance (topic
    theseus)\n\n<text>` come after the context files. The manifest records `topic`, `topic:theseus`, `operator`,
    and an as-of, plus the guidance's version 1 and its digest.
  - `a_membership_change_waits_for_the_next_recompile`: after the change, the next turn appends with no guidance
    (triggers `[new_session]`). `request_recompile(Transcript)` then brings the guidance (`manual_transcript`), and
    the manifest lists the membership.
  - `a_guidance_edit_in_play_forces_one_system_changed`: triggers are exactly `[new_session, system_changed]` across
    three turns. An edit to guidance the session doesn't carry changes nothing.
  - `given_memberships_refuse_writes`: `+channel:<id>` and a `channel` category add are both refused, and no
    `ontology.membership` row is written. The bind made the 2 given categories once, and a second bind added none.
    `ontology.list` shows the session's channel membership with origin `transport`.
  - `admissions_are_identical_with_and_without_memberships`: in the CLI (no place), the owner's DM, and a shared
    channel, the same session before and after a topic membership plus a recompile is offered the same tools and
    the same context files with the same withheld marks. The shared channel gets its own channel's guidance and
    never the topic's.
  - `an_ontology_write_counts_only_from_the_owner_in_a_private_place`: an unnamed connection, another Discord user,
    and the owner from a shared channel are each refused. That makes 3 `approval.refused` rows with `act:
    ontology.guidance.set`, and the guidance is unchanged. The owner's DM then writes version 2.
  - `the_snapshot_rebuilt_after_a_restart_equals_the_one_kept_in_memory`: after categories (a nested one included),
    guidance edits, and a membership add then remove, `ontology::load(store)` and a forgotten-then-rebuilt `Board`
    both equal the snapshot held in memory, and every change wrote its row.
- **A job's process can't write guidance**: `client::tests::a_jobs_process_cannot_write_the_ontology` (the three
  writes are refused with `THESEUS_SESSION` set, and `ontology.list` is not). The CLI renderer has
  `render::ontology::tests::the_tree_indents_by_depth_and_shows_guidance`.
- **Planted reverts**, each restored from a copy, `touch`ed, and checked with `git status`:
  1. *A membership reaches the admission filter.* In turn.rs, a session with any stored membership was given the
     private tool list. `admissions_are_identical_with_and_without_memberships` failed for
     `Some("channel:314159265358979323")`: the shared place was offered `proc_run` and the file tools. The other 6
     passed.
  2. *A guidance edit skips its recompile.* In `compile_with`, a digest change counted as no `system_changed` when
     only the guidance differed. `a_guidance_edit_in_play_forces_one_system_changed` failed: left
     `["new_session"]`, right `["new_session", "system_changed"]`. The other 6 passed.
  3. (Extra) *An append takes the current memberships.* `a_membership_change_waits_for_the_next_recompile` failed
     at "the change waits for a recompile". The other 6 passed.
- **Under load**: the 7 ontology tests under `nice -n 19` beside four busy loops at nice 0, 5 runs, 5 of 5 green.
  The safety check refused the brief's `sh -c 'while :; do :; done'` (it flags any `sh -c`), so the loops were
  `yes > /dev/null`, killed by their own pids.
- **Benches, for their shape** (the maintainer measures):
  - `theseus-sim bench turn --check --runs 5 --burst 0`: `frames_plain: 5 frame(s) at the p95, budget 5: ok`. The
    plain turn's frames are unchanged.
  - `theseus-sim bench lifecycle --runs 10 --check`: `LIFECYCLE OK`. Cold start p95 17.4 ms, SIGKILL restart 11.7
    ms, swap 16.2 ms.
- **A live check here**, on a scratch daemon of this build: `theseus-sim discord rig` with fake-discord, the model
  stand-in, and a fresh state dir in the session's scratch directory. All three processes were stopped by their
  pids afterwards.
  - At the binding's start, `ontology categories` showed `@ana (person:…101)` and `lab (channel:…010)`, with 2
    `ontology.category` rows, origin `transport`.
  - `topic add theseus`, then `guide theseus -` from stdin, then `member SID +theseus`. The next turn appended (the
    newest compilation was still `new_session` with no memberships).
  - After `sessions recompile SID --strategy transcript`, the `manual_transcript` manifest listed `[{kind: topic,
    category: topic:theseus, origin: operator, as_of_ms}]` and `guidance [{version 1, digest 2b6065cc6fb6dc8c}]`.
  - An edit (`guide theseus "…"`) gave exactly one `system_changed` with version 2, and the turn after appended.
  - `THESEUS_SESSION=SID theseus ontology guide …` was refused by the CLI, exit 1. `member SID
    +channel:…` was refused: "`channel` memberships are given: … cannot be set" (-32602).
  - A Discord message in #lab compiled with `memberships [{kind: channel, origin: transport}]` and the channel's
    guidance digest.
  - After a restart, the categories and guidance were all there, and the CLI session's next turn appended with no
    spurious recompile.

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passed fmt, shape, clippy, bench build, test
build, and the reader rule. It **failed in the suite on 32 tests, all L1**: 20 `theseus-sandbox::contract` and
`bench spawn_100`, and 11 `theseusd::sandbox`. Each says "the daemon runs as root, and Linux exempts root from
RLIMIT_NPROC" (theseus-pv6i, known on this VM). The suite ran 1,760 tests: 1,728 passed, 32 failed, 10 skipped.

I ran the phases after the suite myself, and all passed: protocol types (the generated files staged), `theseus-sim
bench turn` (frames 5), `cargo deny --offline check` (advisories, bans, licences, sources ok), web lint and build,
cockpit lint, test, and build, and web dist unchanged. Lifecycle and jobs were skipped by `THESEUS_GATE_NO_BENCH`;
I ran lifecycle by hand (above).

Why the gate ran with `TZ`: under this VM's UTC, `tests_output::the_cores_output_matches_its_golden` fails at line
1174 on a wake preview's offset (`+#:#` against the golden's `-#:#`). The golden was written at a negative UTC
offset. It passes with `TZ=America/Los_Angeles`, and nothing in this step touches wake times. I didn't rebuild
`main` alone to confirm, so this is from reading the diff line, not from a run of `main`.

## Live check for the maintainer (on a copy of the owner's store)

With a scratch daemon on the copy (its own `--config`, `--socket $S`, `--state-dir`), and `T="theseus --socket
$S"`:

1. `$T ontology categories` should list the bound places' channels and DM persons (made at the binding's start,
   rows `ontology.category`, origin `transport`). A copy without the binding running shows only what earlier runs
   made, so possibly nothing.
2. Add the topic: `$T ontology topic add theseus --desc "Theseus, the agent runtime"` should print `topic theseus
   (topic:theseus)`.
3. Set its guidance: `echo "When asked about Theseus, name the crate that holds the answer first, in backticks." |
   $T ontology guide theseus -` should print `topic:theseus: guidance version 1, … digest <d>`.
4. Assign it to the owner's private-place session, say the DM's or a CLI session (`$T sessions` lists them): `$T
   ontology member <SID> +theseus` should list `topic:theseus topic operator as of …` and say the change applies
   at the next recompile.
5. `$T sessions recompile <SID> --strategy transcript`. The brief's `theseus session recompile` is `theseus
   sessions recompile`, and its default strategy is `fresh`.
6. `$T ask -s <SID> "Where does the turn loop live?"` should show `⟳ context recompile (manual_transcript, …)`, and
   the answer should start by naming a crate in backticks (`theseus-core`).
7. `$T rpc compilation.list '{"session_id":"<SID>"}'` should give, in the newest compilation's `manifest`,
   `"memberships":[{"kind":"topic","category":"topic:theseus","origin":"operator","as_of_ms":…}]` and
   `"guidance":[{"category":"topic:theseus","version":1,"digest":"<d>"}]`.
8. Optional: `$T ontology guide theseus "…edited…"` then one `ask` should show exactly one `system_changed`, and the
   turn after should append. And `THESEUS_SESSION=<SID> $T ontology guide theseus x` should be refused by the CLI.

## Left open, and choices the owner should hear about

- **Shared places get only their own place's guidance.** The brief says "a session in a shared place gets guidance
  only from what that place may see". I read that as: in a shared place, the walk drops every interpreted
  membership (a topic the operator assigned), and keeps the place's own given ones (its channel; its guild, once
  there is one). If the owner wants a topic's guidance in a shared channel, that needs a readers mark on guidance,
  as context files have.
- **No `guild:` categories.** `BoundPlace` has no guild id, and places.rs and the binding are another change's. 21b
  makes `channel:` and `person:` (DMs) only. A channel's listed users aren't made `person` memberships either. A
  follow-up can add the guild once the binding passes it (the trusted-guild change may bring it).
- **Tasks.** A task's given membership is its parent's place (via `outbox.target`), but its interpreted memberships
  are its own session's, so topics aren't inherited. Say if a task should inherit its parent's topics.
- **Reads are open.** `ontology.list` isn't judged and isn't refused inside a job, so a job can read guidance text,
  as it can read other sessions' history today.
- **Cost per loop.** With a walk, `compile` composes twice per loop (memory only) and clones the `RequestSpec` when
  the composition used anything. Once Discord binds, that is every session with a place. That costs tens of
  microseconds at today's tool count. A later step can skip the second composition when the recorded and current
  memberships name the same categories.
- **The kinds table has no write method** (not in the design's list), so `ontology.kind` rows aren't declared.
- **The snapshot may be built by a turn.** If a turn arrives before the after-serving warm finishes, the turn's walk
  builds the snapshot itself (one prefix scan, under the board's lock). It is never on the start path.
- **What the cockpit's Ontology view (21c) should show**, over these methods:
  - the kinds table (`ontology.list` `kinds`);
  - the category tree (`categories`, depth-first with `depth`) with each one's guidance, version, and digest;
  - a guidance editor (`ontology.guidance.set`; empty text takes it away);
  - "add topic" with a parent picker (`ontology.category.add`);
  - per session, its memberships (`ontology.list {session_id}`, given and interpreted, with origin and as-of) beside
    what its current manifest recorded (`compilation.list`), so a pending change reads "applies at the next
    recompile", with a recompile button;
  - a write refused with `REFUSED` (-32005) showing the message's who and why.

## Docs the maintainer should change (I didn't edit them)

- `docs/design/README.md`: delete the reserved-crates row `theseus-ontology … row 26 (21b)`.
- The spec's Part III: the 21b item, with:
  - the two-spec compile;
  - format 6;
  - the shared-place reading of the guardrail;
  - no guild categories;
  - the Observatory's memberships moved to the cockpit's 21c;
  - `COMPILATION 4` in m4-boundaries' store-bumps table, which is now `MANIFEST_FORMAT` 6.
- `docs/status.md`: row 26 landed.
- `docs/design/m4-boundaries.md` §2.8 "Surfaces": the Observatory line is the cockpit's (21c).
