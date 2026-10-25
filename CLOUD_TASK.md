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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3a-kimi`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
This row needs no store change (catalog entries and config only). Do not bump it.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: `crates/theseus-core/src/catalog.rs` 2,067 (cap 2,500: if your tests would push it past, put them in a new
`catalog/tests_moonshot.rs` or the crate's existing test-module pattern), `crates/theseus-core/config/theseus.example.toml`
(read its size).

**Your lane: `integ/p3-first`** (new: the first 1.0.3 rows). Its other rows, all fired at about the same time on the
same base: `p3a-recall-floor` (theseus-96cyh: `config/memory.rs` defaults, `recall/relevance.rs`, `recall/render.rs`),
`p3a-recall-now` (theseus-mo1vm: `recall/temporal.rs`, `recall/render.rs`, the exam probe), `p3a-mem-spike`
(theseus-xxac7: `crates/theseus-index`). **Shared with you:** only `crates/theseus-core/config/theseus.example.toml`,
where `p3a-recall-floor` may touch the `[memory]` comments: you touch only the `[providers]` section and the
profiles. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3a-kimi, the Kimi (Moonshot AI) models as China-origin catalog and provider rows (theseus-rgbtz)

Branch: `cloud/20261024-p3a-kimi`.

Every commit subject carries `theseus-rgbtz`. Deadline for the report: about 3 hours after you start.

**THE DATA RULE FIRST (non-negotiable).** Moonshot AI is a China-origin provider. The owner's China-origin rule
applies in full and UNCHANGED: a China-origin model gets only open work; drafts it writes are reviewed by a US
Anthropic model; anything private (the `[privacy]` sensitive names, any session that touched the owner's Google
data, any employer data) never reaches it. All of that is enforced today by one chokepoint,
`crates/theseus-core/src/data_gate.rs`, keyed on the catalog's `origin = cn`. This row adds Kimi BEHIND that
chokepoint, exactly as the existing GLM (`zai`) and DeepSeek rows are: you change NOTHING in `data_gate.rs`'s rules,
and a test must prove Kimi is gated the same way GLM is. If anything you find would let a private request reach a
Kimi profile, STOP that part and write `OWNER CALL: <what and why>`.

**No key, no live call.** You have no Moonshot key and must not need one: every test is a fixture test (recorded or
hand-written response bodies, an `example.test` stand-in server if the crate already has that pattern). The live
checks are a LATER LOCAL step done by the maintainer with the owner's key; list them in the report. Never put a key,
or a placeholder that looks like one, in any file.

**What is known (a live probe on the owner's account, and the vendor's docs):**
- Endpoint: the Anthropic Messages API at host `api.moonshot.ai`, base path `/anthropic` (scheme https), so the
  `api_base` is that host and path as the `zai` entry writes its own. Model id `kimi-k3.1-pro`; a faster
  `kimi-k3.1-pro-highspeed` variant at double the price.
- Auth: `x-api-key` works (and so does a Bearer header), so the existing `anthropic_messages` provider kind needs NO
  auth change.
- The response is the standard Anthropic shape. `usage` carries input/output tokens, cache read/creation,
  `cache_creation.ephemeral_5m` / `ephemeral_1h`, `output_tokens_details.thinking_tokens`, `service_tier` and
  `inference_geo`. `stop_reason` may also be `refusal`.
- The model: 1M context (1,048,576), max output 131,072; text, image and video input; it ALWAYS thinks, with effort
  low/high/max in `output_config.effort` (changing effort breaks the prefix cache).
- Prices per Mtok: input 2.00, cached input 0.20, cache write 2.00 (5m) and 4.00 (1h), output 10.00, flat over the
  whole context. Highspeed: 4.00 / 0.20 / 20.00 output.
- Endpoint quirks: custom tools only, names `^[a-zA-Z_][a-zA-Z0-9-_]{0,127}$`; `tool_choice` is auto / any / none
  only (no forcing a tool by name); thinking signatures must be replayed; `cache_control` only at the top level
  (per-block markers ignored; caching is automatic); images base64 only (no URL source); no document/PDF block.
- Data terms: the contracting party is in Singapore, its terms allow training on API content without an enterprise
  deal, and zero retention is enterprise-only: so `origin = cn` (it is China-origin by the owner's rule regardless of
  the server's country).
- Skip: `kimi-k3` and every K2 model (weaker or being retired).

**What to build (read `catalog.rs`'s GLM and DeepSeek code first and mirror it; right over compatible).**
1. **Catalog** (`crates/theseus-core/src/catalog.rs`):
   - Add `moonshot.ai`, `moonshot.cn`, `kimi.ai` and `kimi.com` to `CN_HOSTS` (:124; resize the array).
   - Add `kimi` and `moonshot` to `origin_of` (:133 on) so a model or provider named that way reads `Cn` whatever the
     config says, as `glm`/`zai`/`deepseek` do; and to `TokenRates::of` (or wherever per-family rates live).
   - A `kimi()` builder beside `glm()` (:698): provider `moonshot`, the context and max output above, the prices
     above (say how the 1h write price maps onto the catalog's fields: if the catalog has one cache-write price, use
     the 5m one and note the 1h one), thinking always on (map to the catalog's existing thinking modes: the one that
     means "the model always thinks; effort is the only knob"), effort supported, vision on, PDF off, tool search off,
     origin `Cn`. Entries for `kimi-k3.1-pro` and `kimi-k3.1-pro-highspeed`.
   - If the catalog or the request builder has a capability for forcing a tool by name, mark it off for Kimi and make
     sure no code path forces a named tool on a Kimi model (test it). If there is no such capability and some path
     does force a named tool for every model, do not build a new capability here: name the path in the report as a
     follow-up and an item for the live check.
   - Bump `BUILTIN_VERSION` (:45) to a new dated value; fix any test that pins it the way earlier bumps did (read
     `git log -S BUILTIN_VERSION` for the pattern).
2. **Config** (`crates/theseus-core/config/theseus.example.toml`): a COMMENTED `[providers.moonshot]` block beside the
   DeepSeek one (:302-306), in its exact style: `api_base`, `api_key_secret = "moonshot_api_key"`, `kind =
   "anthropic_messages"`, `origin = "cn"` with the same comment, and "Add its key to the secrets table before
   un-commenting". Two COMMENTED profiles, `kimi` (effort high) and `kimimax` (effort max), in the profiles section's
   style. Do NOT add Kimi to any routing default, `cn_draft`, `cn_profiles` or Jev route pack: routing waits for an
   owner-approved A/B against GLM-5.3 and DeepSeek flash (a later step). Do NOT change
   `crates/theseus-core/fixtures/owner_shape*.toml` or any resolved-profiles snapshot: the owner's config gains
   Kimi in the later local step, and the fixture changes with it.
3. **Usage parsing:** check that a Kimi-shaped response (fixture: the `usage` fields above, `thinking_tokens`,
   `stop_reason: refusal`) parses into the harness's usage and cost with no field lost or mispriced, and that a
   `refusal` stop is handled as the harness already handles a refusal (read how; for a cn draft the existing rule is
   that Claude redoes it). If a field needs a parser change, make it in the provider-generic parser only if it is
   right for every Anthropic-compatible endpoint; say so.

## Tests (each red at the base, with a planted revert)

1. Catalog: a `moonshot` arm in the provider/model listing test (near :1702), the cn host and name cases (near
   :1767) for all four hosts and both names; a pricing test (a fixed usage -> the dollar figure at the prices above,
   including cached input and thinking tokens counted as output). Plant P1: drop `moonshot.ai` from `CN_HOSTS`: red.
2. **The data gate:** a test, beside the existing cn tests (`data_gate.rs`'s tests or the `routing_cn` tests), that
   a private request (a sensitive name, a Google-touched session) is refused or rerouted for a Kimi profile EXACTLY as
   for a GLM profile (parametrise over both if the tests allow). Plant P2: make `origin_of` read `kimi` as US: red.
3. Config: the example toml still parses with the block uncommented in a test fixture copy (if the config tests have
   that pattern; near `config/tests.rs` :1279), and the `origin = "cn"` cannot be overridden to US for the moonshot
   host. Plant P3: remove the host-forces-cn rule for moonshot: red.
4. Usage: the fixture response parses and prices as in item 3.
- Keep green: every existing catalog, config, routing, draft (`tests_draft*.rs`, `draft/first_pass.rs`) and
  data-gate test.

## Not in this row

No change to `data_gate.rs`'s rules, the review rule, routing defaults, Jev packs or the owner_shape fixtures. No key,
no live call, no spend. No Discord change.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. A pinned `BUILTIN_VERSION` value updated for the bump is the one expected
  edit: say so. Before the report run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste
  its output.
- **Data floor:** this row only describes a cn provider; it routes nothing to it. Say so in one line, and say which
  test proves private work cannot reach it.
- **Public repo:** no person's name, no real organisation (other than the vendor's own name and hosts, as the GLM and
  DeepSeek rows have), no address, no email, no key. Fixtures use invented content under `example.test`. Write "the
  owner".
- **Red first;** right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run` (or `cargo test`)
  for `theseus-core`, `theseus` and `theseusd`; then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), the new `BUILTIN_VERSION`, any ceiling,
OWNER CALL or STOP. (2) **The data rule:** how Kimi reaches `Cn` (host and name), and the test that proves a private
request cannot reach it. (3) **Catalog entries:** each field and where its value came from; the thinking mode and the
1h cache price mapping. (4) **Config:** the commented block and profiles, and what the later local step uncomments
(provider, secret name `moonshot_api_key`, profiles) and where the owner_shape fixture changes then. (5) **Forced
tools and refusals:** what you found and did. (6) **Tests** and plants P1-P3, red at the base and green after.
(7) **Gate** commands and results. (8) **Keel** output. (9) **DM golden** diff result. (10) **Join notes:** every file
touched. (11) **The live checks for the later local step:** tool names and schema limits against the real tool set,
thinking-signature replay across tool turns, base64 images, a refusal (Claude redoes the draft), cache hits, whether
temperature or top_p give a 400; then the cn-draft A/B (Kimi vs GLM-5.3 vs DeepSeek flash: cost, solve rate, review
rework) before any routing share. (12) **Follow-ups.**
