<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-review-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: review-fixes, the owner's two decisions of 2026-10-17 night: one sensitive-names list in the config (read by route.v6's data question, by the pack-text check in place of hashed digests, and by a fail-closed floor that keeps a hit off China-origin models), and task channels that close to the operator alone (theseus-oejh3)

Branch: `cloud/20261017-review-fixes`. Every commit's subject carries `theseus-oejh3`. Deadline for the report: 4 hours
after you start.

**Where this comes from.** The owner's Discord DM of 2026-10-17 21:46:39 MST, verbatim:
- (a) "I think you should make a sensitive names config, and instead of basing it on a hashed name, just use the config
  (which is OK to check in as the example config, because it's not that sensitive)";
- (b) "I like the idea that access defaults to being removed for everyone but the operator of theseus".

The owner also pasted an external AI's code review of this repository. The maintainer checked each of its five findings on
`main`, and **none is a defect to fix** (three wrong, two real but with no practical risk here; the verdicts are in "The
external review" below). **So this row has no review fixes. Do not change the WAL reader, the seccomp filter, the sandbox init, the
web UI's peer check or the locks because of that review.**

**Prerequisite:** `main`. You build now, on `main` as it is: `f3a1cb3f` (2026-10-17 20:58 MST) or later. Seven trim
rows are already joined there: trim-binary, -docs, -security-v1, -config-inherit, -effort, -utils, and
trim-keel-deletions' part C. **You join after the trim rows that are still out**: trim-tokens (rwpbg.3), trim-names
(rwpbg.10), trim-tests-dedup (rwpbg.1), and whatever of trim-keel-deletions (rwpbg.9) is still unjoined. trim-names
renames config keys and adds a renamed-key table that turns an old key into a "`<old>` was renamed to `<new>`" error.
Your config move needs one row in that table, and your base does not have the table yet: see "Overlaps".

## Your row
| # | Slug | Beads | Scope | Files | Tests | Size |
|---|---|---|---|---|---|---|
| 1 | `review-fixes` | theseus-oejh3 | A: `[privacy] sensitive_names` and the one matcher; B: route.v6's names and the pack-text check (T28) read it; C: the floor (a hit keeps the turn off restricted-origin models); D: task channels close to the operator alone | `crates/theseus-core/src/config.rs`, a new `config/privacy.rs`, `config/routing_data.rs`, the template; `crates/theseus-judge/src/` (the matcher; `pack.rs`'s T28); `judge/inbound.rs`; the route step's profile choice; the provider chokepoint; theseusd's start (the pack check); `crates/theseus-discord/src/runtime/{task_channels,war_room}.rs`; `theseus-sim`'s fake Discord if its Modify Channel is wrong | the matcher's table test; the config parse and old-key tests; T28 converted; the verdict override; the route skip; the chokepoint refusal; the start check; the close test (a guest and a role lose access, the owner keeps view and history) | M |

Do the parts in this order, one commit (or a few) each, each green before the next: **D, A, B, C**. D stands alone, so a
session that runs out of time still lands it.

## D. Task channels close to the operator alone (theseus-bh3t.1, bh3t.4)
**Check today's behaviour first, and write it in the report before you change anything.** As the maintainer read it:
- `task_channel_close` (`runtime/task_channels.rs`, about :367) PATCHes the channel with `permission_overwrites =
  task_overwrites(guild, true)`. That list is:
  - `@everyone` denied VIEW_CHANNEL;
  - each id from `owner_ids()` allowed VIEW_CHANNEL | READ_MESSAGE_HISTORY | ADD_REACTIONS and denied SEND_MESSAGES;
  - the bot (view, send, manage).
- Discord's Modify Channel **replaces** the whole overwrite set. So a war-room guest's member overwrite (`war_room.rs`
  `set_guest`, about :216) and any role overwrite someone added by hand should already be gone after close.

The owner's rule is the default: on close, nobody but the operator keeps access. Make that true and prove it:
1. **The fake.** Check `theseus_sim::fake_discord`'s Modify Channel (PATCH `/channels/{id}` with
   `permission_overwrites`): it must replace the set, as Discord does, not merge. If it merges, fix the fake first, in
   its own commit, and say so. A test against a merging fake would prove nothing.
2. **The test** `a_closed_task_channel_is_the_operators_alone`. Use the rig of `tests_task_channels.rs` /
   `tests_war_room.rs`, in the file the subject's tests live in today:
   - a task gets its channel; the owner shares it with a guest (a member overwrite, through the real `share:` path);
   - an extra **role** overwrite is put on the channel (allow VIEW_CHANNEL | SEND_MESSAGES for a role id other than
     `@everyone`; through the fake's API, as a person in Discord would);
   - the task ends;
   - assert, on the fake's stored channel:
     - the guest's overwrite and the role's overwrite are gone;
     - `@everyone` is denied VIEW_CHANNEL;
     - the owner is allowed VIEW_CHANNEL and READ_MESSAGE_HISTORY and denied SEND_MESSAGES;
     - nothing else is there but the bot's own.
   - Then compute who can view with the crate's `viewers` helper (`viewers.rs`) over the fake guild's roles and the
     stored overwrites: only the owner and the bot.
   - Plant: make close PATCH only `name` and `parent_id` (no overwrites); the test fails on the guest and the role.
3. **What close must also do**, each with its own test and a plant:
   - **The guest list.** Clear META `discord.task_guests.<task>` at close. Today it still lists the guests after
     close, so Details or a later `unshare:` reads a stale list. Keep the count that the report states (comments and
     guests counted). Clear only the live list.
   - **`share:` after the end.** A `share:` on an ended task's channel or thread is refused with one line: "that task
     has ended; its place is yours alone". Today it would put a guest back.
   - **A task thread** (no channel of its own; a thread or the default forum's post). At close, remove the guests the
     war room added (thread members from that task's guest list) where Discord allows it. Say what Discord allows for
     an archived thread. If a removal can't be done, say why in the report.
4. **The operator.** `owner_ids()` reads the place rule's owners (`place_rule.owners(&cfg)`). Write in the report what
   that set holds on a default config and whether it can hold anyone but the operator. If it can (a configured second
   owner), the close keeps them, and the report says so: that is a config question for the owner, not this row's to
   change.
5. **Documentation.** Update the module comment of `task_channels.rs` ("At the end …") and the crate's AGENTS.md lines
   on task channels: on close, every overwrite but `@everyone`'s deny, the operator's read and the bot's goes. Add one
   sentence: a member with Administrator, and the guild's owner, see every channel whatever the overwrites say. Discord
   allows nothing narrower. (The owner said tonight that the bot is an administrator in the owner's guild. That is
   the bot, which already keeps its access.)

## A. `[privacy] sensitive_names`: one list, one matcher
**The config.** A new top-level section `[privacy]` (`config/privacy.rs`, struct `PrivacyConfig`,
`#[serde(deny_unknown_fields)]`, default empty) with one key:
- `sensitive_names = []`: the names of organisations, clients, projects and people whose matter never goes to a model
  of restricted origin (`catalog::ModelOrigin::restricted()`: `cn` or `unknown`).

It **replaces** `[routing.data] private_topics` (`config/routing_data.rs`): one list for one thing, no alias (the owner,
2026-10-17 17:23:18: "more important to be right than to be historically conservative"). `RoutingData` keeps
`open_topics` and loses `private_topics`. Its `names()` takes the privacy list for `DataNames.private`, or the caller
builds `DataNames` from both: pick the smaller change. `[routing.data]` keeps `deny_unknown_fields`, so a file that
still says `private_topics` fails to load today with serde's unknown-field error. Pin that in a test
(`routing_data_private_topics_is_refused`). At the join after trim-names, the joiner adds the row to trim-names'
renamed-key table, which turns the error into "`[routing.data] private_topics` was renamed to `[privacy]
sensitive_names`". Give that exact table row in the report. The owner's live config has no `[routing.data]` table at
all (the maintainer checked key names only), so nothing live breaks.

**The template** (`crates/theseus-core/config/theseus.example.toml`): a commented `[privacy]` block with **placeholder
values only**. Use made-up names that are obviously not real: `# sensitive_names = ["Examplecorp", "Project
Placeholder"]`. Its comment says, in a few lines:
- what a hit does (it keeps the turn off restricted-origin models, fails closed, and Jev reads the names as private);
- how matching works: case-insensitive, whole words, a multi-word name matched as a run of words, so `Examplecorp`
  matches "examplecorp's plan" and "EXAMPLECORP-internal" but not "examplecorpus". List each spelling you mean,
  `BigCo` and `Big Co` separately;
- that the list lives in your own config, never in the repository.

The owner said a real list would be fine to check in as the example. This public repository carries no person's or
organisation's name, so the template stays placeholders. The maintainer puts the owner's real values in
`/etc/theseus/theseus.toml` at install. Write in the report exactly what to add there: the block, with
`<the owner's names>` in place of values. The `[routing.data]` block in the template loses `private_topics`.
`example_template_uncommented_still_parses` and the section tests must pass.

**The matcher: one function, one place.** It goes in `theseus-judge` (core already depends on it for `DataNames`). For
example, `theseus_judge::names::hits(text: &str, names: &[String]) -> Vec<String>`, returning the configured names that
occur.
- Words are maximal runs of Unicode alphanumerics, compared lower-cased (`char::to_lowercase`). A name of several
  words matches the same words in a row, whatever separators lie between them. A name made of no alphanumerics matches
  nothing.
- Precompute nothing global. If a caller checks many texts, it may build the names' word lists once (a small struct).
- Test `sensitive_name_matching`, a table:
  - case (`ACME`, `acme`);
  - possessive (`Acme's`);
  - hyphen and underscore (`acme-internal`, `acme_internal`);
  - the word inside a longer word, which does not match (`acmeology`);
  - a two-word name across a newline and across punctuation (`Big\nCo`, `Big, Co`);
  - a two-word name with a word between, which does not match;
  - non-ASCII (`Société Générale` vs `société  générale`);
  - an empty list;
  - a name that is only punctuation.
- Plant: drop the lower-casing; the case rows fail.

## B. route.v6 and the pack-text check read the list
1. **Jev's state.** `data_names.private` (route.v6's `data` question, `builders.rs` `data_names_state`) comes from
   `[privacy] sensitive_names`. The state's shape, the golden `route.v6.request.json`, the names fixture
   `fixtures/inputs/inbound.names.json` and `inbound.state.json` stay byte-identical. Only the config path changes.
   The call sites are `turn/inbound_step.rs` (about :107) and `rpc/thread_flow.rs` (about :441). The test
   `tests_inbound.rs` (about :278) sets `private_topics` and moves to the new key.
2. **The verdict, deterministically.** In `judge/inbound.rs` `verdict` (about :746), or where its caller has the
   message text: when the message hits a sensitive name, the `data` answer is `private` at confidence 1.0, band `Act`,
   whatever Jev said, and Jev's own answer is kept beside it if the type allows that cheaply (else say so).
   - Test `a_sensitive_name_makes_the_data_answer_private`: Jev answers `open` at 0.95, the message names a configured
     name, and the answer is `private`.
   - Plant: skip the override.
   - Today nothing acts on the data answer (`routing/data.rs` says so). The later cn rows (cn-gate-floor) will mint a
     pass only on `open`. Forcing `private` here is what makes them fail closed on a hit.
3. **T28, `the_pack_text_names_no_organisation`** (`crates/theseus-judge/src/pack.rs`, about :1690-1722). Today it
   hashes every word and two-word span of `route.v6.toml` and checks them against `DENIED_DIGESTS`. That list holds one
   digest: the planted word `acme-private`'s. No real name was ever added, so the test guards against nothing real. The
   owner's decision replaces the hashes with the config:
   - **The real check runs where the real list is.** At daemon start, after the config loads, check the text of every
     embedded pack (`EMBEDDED`, every version, not only route.v6) with the matcher against `[privacy]
     sensitive_names`. On a hit, write one startup warning and one health problem line that name the pack and version
     and the count of hits, never the name itself (a health line can reach the cockpit and a bug report). The start
     goes on.
     - Test `a_pack_naming_a_sensitive_name_is_reported_at_start`: configure a name that occurs in an embedded pack's
       text, for example a word you find in route.v6. Check the warning and the health line carry the pack and version
       and not the name.
     - Plant: skip the check.
     - Keep it cheap. Run it once at start, off the serving path if it costs more than a millisecond. Measure it and say
       so: the start's "serving at" must not move.
   - **The unit test keeps its name and its job.** Delete `DENIED_DIGESTS`, `digest` and `denied_in`. The test checks
     every embedded pack's text with the matcher against the template's placeholder names, which must not hit. It keeps
     the mechanism check with a planted text ("Examplecorp private plans" hits `Examplecorp`). The test's assertion now
     reads the config's list instead of hashes, which is a change of meaning. Say so in the report and in the commit
     body, and run the keel guard (see Keel).
   - `sha2`/`hex` in theseus-judge: if T28 was their only user, drop the dev-dependency (no new dependency, one fewer).

## C. The floor: a hit keeps the turn off restricted-origin models (fail closed)
The owner's rule (2026-10-17 09:26), paraphrased so that no organisation is named here: the owner's employer's
matter, and any private, personal or confidential information, never goes to China-origin models. A model's origin is `Config::profile_origin(profile, &catalog)` (config.rs, about :1876), and
`ModelOrigin::restricted()` is true for `cn` and `unknown`.
1. **The route.** Where the turn's route step turns a task class's (or a pin's, a hold's, a detour's) profile list
   into the profile it runs on (find it: `route_profiles` / `Profile::runs`, the route step under
   `crates/theseus-core/src/turn/`): when the message hits a sensitive name, every restricted-origin profile is
   unusable for this turn. The next usable profile in the list runs. If none is left, the configured `[model]` default
   runs when its origin is not restricted. Otherwise the turn ends before any model call, with one line: "this message
   names something in `[privacy] sensitive_names`, and no profile of unrestricted origin is configured for it". The
   route row records why (`data_gate`, or a field the row already has).
   - Test `a_sensitive_name_skips_restricted_profiles`: a list of [a restricted profile (a fake provider whose api_base
     is a China-origin host, or a catalog row `origin = "cn"`), an unrestricted one]. A plain message runs on the
     first. A message naming a configured name runs on the second.
   - Test `a_sensitive_name_with_only_restricted_profiles_sends_nothing`.
   - Plants for both.
2. **The chokepoint.** One guard where a request is handed to a provider client, so every caller is covered: the turn,
   compaction, keep-warm, replay, the judge's background calls and anything added later. Find the one place the
   providers are built or chosen. A small `Provider` wrapper on each client, or one check in the code that picks the
   client, is enough. Use whichever is one place.
   - For a request whose model's origin is restricted, walk every JSON **string value** of its system blocks, messages
     (text, tool inputs, tool results) and tool definitions. Walk the unescaped strings, never the serialized body: an
     escaped `\n` would glue words together.
   - On a hit, return a `ProviderError` of its own class (`data_gate`) without sending a byte. The turn reports it like
     any refusal.
   - Do no work at all when the list is empty or the model is not restricted. A plain turn on Claude stays exactly as
     today (5 frames, no extra pass over the request).
   - Test `a_restricted_model_is_never_sent_a_sensitive_name`, through a fake provider that would record a send: a
     request naming a configured name to a restricted model fails with `data_gate` and records nothing. The same
     request to an unrestricted model is sent.
   - Plant: let the guard pass everything.
3. **Documentation.** Two lines in `crates/theseus-core/AGENTS.md` (its routing or data-rule section, wherever
   `[routing.data]` is described): `[privacy] sensitive_names` is the one list; the route skips restricted profiles on
   a hit, and the provider chokepoint refuses one. Name the matcher's function.

## Overlaps
- **The remaining trim rows join first.** trim-names (rwpbg.10) renames `[routing.modes.*]` to
  `[routing.classes.*]`, `max_loops_mode` to `on_max_loops` and the cache keys, and adds the renamed-key table and its
  function. Do not rename anything yourself. Use today's names in your code and tests. The joiner updates them at the
  merge, and your report lists every place you used a name trim-names renames. Leave `config/cache.rs`'s `inherit`
  alone. trim-tokens (rwpbg.3) edits `tools_defer/`, keep-warm and `turn/compile_step.rs`: stay out of them.
  trim-tests-dedup (rwpbg.1) moves test helpers: put your tests in the file their subject's tests live in today.
- **The China-origin rows** (theseus-a1wvq.3-.14) are not launched. cn-gate-floor (.3) will add the full DataPass gate
  at the same chokepoint and the same route step. Keep your guard and your route skip each one small function, named
  for what they do, so its writer builds on them. Their task files still say `[routing.data] private_topics`, and the
  maintainer will update them to `[privacy] sensitive_names`.
- **Discord.** Every Discord row (theseus-bh3t.1-.6, ss36w, i3do6) has joined, so `crates/theseus-discord/` is free
  for D. Keep D to `task_channels.rs`, `war_room.rs`, their tests, and the fake if needed.

## Keel (the joiner's part)
Converting T28 changes what its assertion checks (hashes become the config's list). If the guard reports it (a changed
or removed assertion in `pack.rs`'s test), paste the finding and give the exact `Keel:` trailer it asks for. Quote the
owner's DM of 2026-10-17 21:46:39 MST: "I think you should make a sensitive names config, and instead of basing it on a
hashed name, just use the config". No other keel finding is expected. Do not delete a test, and do not edit
`scripts/keel-guard.py` or its test. If you think one of them needs a change, say so in the report and stop there. The
joiner signs any keel commit with the owner's key.

## Report extras
- **D's before**: what close did before your change, read from the code and shown by your test against the unchanged
  close (or by a short probe), before the fix.
- **The maintainer's install steps**: the `[privacy]` block for `/etc/theseus/theseus.toml` (placeholders in your
  report, never a real name), and the renamed-key row for trim-names' table.
- Every place you used a key name that trim-names renames (file:line).
- The start check's measured cost.

## Proof nothing was lost (report section)
- route.v6's goldens and fixtures are byte-identical (name them).
- A plain turn on an unrestricted model makes the same frames and requests as before (the turn bench, the
  owner-config fixture test).
- Every task-channel and war-room test that passed before still passes.
- T28 still fails on a planted name.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `f3a1cb3f` (2026-10-17
20:58 MST, the first trim batch's join) or later, with **store format 45**. The `file:line` references in this task are
at `0a44e1f1` (16:46) and were rechecked by name on `f3a1cb3f`. A line may have moved a little, so search by name.

Line ceilings at `f3a1cb3f` (`scripts/long-files.txt`, length / ceiling):
- `crates/theseus-protocol/src/lib.rs` **2,729 / 2,733**
- `crates/theseus-kernel/src/kernel.rs` 3,026 / 3,030
- `crates/theseus-discord/src/runtime.rs` **3,497 / 3,500**
- `crates/theseus/src/render.rs` 3,110 / 3,128
- `crates/theseus-core/src/turn.rs` 3,354 / 3,525
- `crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050
- `crates/theseus-discord/src/render.rs` 2,926 / 3,009
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Never raise a ceiling in this row. Put new code in new or small files: `runtime.rs` has 3 lines of room, so D's code
goes in `runtime/task_channels.rs` and `runtime/war_room.rs`.

## The external review (read, not to act on)

An external AI reviewed the repository, and the maintainer checked each claim on `main`. The verdicts are below so that
you neither redo them nor "fix" what isn't broken:
- **F1, the web UI's peer check:** wrong on TOCTOU and on `::ffff:` (`peer.rs` `forms()` handles mapped pairs and is
  tested; a vanished client fails closed). The O(N) `/proc/net/tcp` scan is real (about 2 ms an accept) but not worth
  changing.
- **F2, the WAL's `body_len`:** wrong. Recovery reads the segment, then checks `body_len` against the bytes it has,
  before the CRC (`check_frame`), and treats the frame as torn.
- **F3, seccomp's clone check reads only the low 32 bits:** not worth it. `clone(2)` itself uses only the low 32 bits
  of its flags.
- **F4, the init's `kill(-1)` and inherited fds:** wrong. `init_main` exits unless it is pid 1, and `close_range` runs
  at the start and with CLOEXEC before `execve`.
- **F5, lock poisoning:** not worth it. Release and release-thin are `panic = "abort"`, and the store writer's coupling
  is the K1 design.

None of these is in this row's scope.

## Out of scope for this row

- The external review's findings above.
- Any rename of a config key other than `[routing.data] private_topics` → `[privacy] sensitive_names`. (trim-names
  owns the others.)
- The China-origin rows' DataPass, clearance, open runs, open tasks and draft review (theseus-a1wvq.3-.14).
- route.v6's prompt wording. (`packs/route.v6.toml` is unchanged: only the names its state carries come from a new key.)
- The sealed holdout set.
- `PLAN_NOTE`.
- The LTO profile.

If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- **Model roles.** You write the code. Keep it minimal and elegant: every line you add should earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same unless this task says
  otherwise. Where it says otherwise, pin the old behaviour in a test first, then change it, and show the diff.
- **No new model tool and no always-loaded text.**
  - First-request budgets stay: cli/task 8,000 and dm 15,000 tokens. The turn bench and the owner-config fixture test
    must pass.
  - FAST: nothing new on a plain turn. A plain turn stays 5 frames. With an empty `sensitive_names` or an unrestricted
    model, the floor does no work.
- **Every behaviour change gets a test and a planted revert** (break it, see the named test fail, restore). List them
  in the report.
- **Keel.**
  - Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling.
  - Before your last commit, run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its
    output in the report.
  - The output must be empty, except for the T28 finding this task's Keel section allows. For that finding, the report
    gives the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- **No store format change. No new dependency.** Cargo.lock and the package-lock.json files gain no package. Dropping a
  dev-dependency nothing uses any more is fine: say so. Never loosen a test, budget or ceiling.
- **No person's or organisation's name** anywhere in the repo, in commit messages, or in the report:
  - write "the owner";
  - use obviously made-up names in tests and the template (Examplecorp, Acme, Widgetco);
  - if you edit a file that already carries a person's name, replace it with "the owner" in the lines you touch.
- **Read the AGENTS.md/CLAUDE.md of each crate you touch, first.**
- **Keep CLOUD_REPORT.md honest:** what you ran, what passed, what you could not run in the cloud. Add a section
  **"Proof nothing was lost"**.
