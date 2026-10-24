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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-caps`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/p2-caps` at `9b075d6d` (2026-10-23, gate green). That is your base: **not**
`main`. The release preamble above is older than this note; where they disagree about `main`, the base, ceilings or the
work in flight, this note wins. Line numbers below are at `9b075d6d` and may be off by a few: search by name. Run the
keel guard against `9b075d6d`, not against `origin/main`.

**This wave is patch2 (release 1.0.2), the follow-up rows.** The lane `integ/p2-caps` was cut from `main` `23328545`
(release 1.0.1). At `9b075d6d` it holds four rows, merged with their join fixes:
- **p2-caps-stop** (theseus-2c6iz, theseus-1iqg0, theseus-kf4pf): a summary, audit answer or writer file cut at its cap
  is refused and named. It added `crate::provider::hit_cap(stop_reason)` and `crate::provider::cap_phrase(cap)`
  (`provider.rs` near :735).
- **p2-caps-forced** (theseus-j06y6, theseus-opypg): the exam reviewer (`rsi/exams/review.rs`) asks with
  `tool_choice: auto`, a `"strict": true` tool whose `input_schema` has `"additionalProperties": false`, an instruction
  that names the tool, and a cap of 4,096. The four small forced-tool calls (card ask, thread-import card, people
  extract, onto namer) send `catalog.thinking_off(model)` and check `hit_cap` before they parse. Each `request` takes
  `thinking: Option<Value>` as its second argument. They still force the tool (`{"type":"tool","name":TOOL}`).
- **p2-caps-follow** (theseus-o6ifc, theseus-ohiu8): the learning writer asks for low effort; dream guidance leaves a
  category waiting when its model thinks unasked (`Catalog::thinks_unasked`, `catalog.rs` near :801).
- **p2-card-drafting** (theseus-ly75u): the owner card's drafting rules (`card/evidence.rs` `addressed`, `setup_file`,
  `about_owner`, `one_task`, `kept`; `card/ask.rs` `thin`, `turned`, the tool's `conflicts`; `card/run.rs`
  `settle_conflicts`) and the owner's scope on keep, edit and a kept record (`rpc/card.rs` `card_answer`).

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Nothing in this
row should be stored: `Dreamed` is an in-memory result, and the card changes ride the request and existing records. If
a part truly needs a stored field, stop that part and say so in your report's first section (another lane takes 50).

**Line ceilings.** Every file this row touches is unlisted in `scripts/long-files.txt`, so each stays at 2,500 lines or
fewer. The big ones: `crates/theseus-core/src/provider.rs` 1,972 and `crates/theseus-core/src/catalog.rs` 1,942: add a
`mod` line and a few lines at most; put a new helper in a new file under `crates/theseus-core/src/provider/`. The rest
(`card/evidence.rs` 390, `card/ask.rs` 308, `card/run.rs` 469, `card/tests_job.rs` 1,087, `rpc/card.rs` 216,
`consolidate/guidance.rs` 405) have room. Never raise a ceiling.

**Other rows of lane `integ/p2-follow`, landing at the same time** (each from its own base):
| slug | issues | base | files near yours |
|---|---|---|---|
| p2f-ops-act, -ops-ui, -ops-daemon | Operations page | integ/ops | cockpit, CLI client, calendar: none |
| p2f-turn | turn budget, long call retry | integ/p2-route | leave `provider.rs` retry paths alone |
| p2f-threads | threads cards, families | integ/p2-threads | not `threads_import/card.rs` |
| p2f-onto | the onto mint's name pick | integ/p2-onto | `judge/onto/mint.rs`, maybe `name.rs`'s hints |
| p2f-bench, p2f-fanout | bench config; fan-out | integ/p2-misc; main | none |

## Your task: p2f-caps, the card's drafting follow-ups, a reason for guidance's waiting, and forced-tool calls that work when thinking cannot be off (theseus-0ly2a, theseus-wdzmj, theseus-yym79)

Branch: `cloud/20261024-p2f-caps`.

Every commit's subject carries its Beads id (`theseus-0ly2a`, `theseus-wdzmj` or `theseus-yym79`). One commit per issue
at least. Deadline for the report: about 3 hours after you start.

## Your row

### theseus-0ly2a: card drafting follow-ups (all in `crates/theseus-core/src/card/` and `rpc/card.rs`)

- **L1 IN (`evidence.rs` `about_owner`, near :159).** In an assistant's setup file, `kept` keeps only lines whose
  subject is the owner. `about_owner` looks at the first word only: a pronoun, "the owner", or a word that looks like a
  third-person verb (ends in `s`, not `ss`/`us`, over 3 letters). So a setup line that starts with the owner's *name*
  ("Mara prefers tide tables in metric units", "Mara is vegetarian") is dropped unless the name happens to end in `s`.
  Memory files and dated logs often start lines that way. Fix: also keep a line whose first word is a name and whose
  second word is a third-person verb (the same heuristic, plus `is`, `has`, `was`, `does`). Guard the false keep: an
  imperative plus a plural noun ("Keep notes short", "Check logs every morning") must still be dropped; use a small
  fixed list of setup-file imperatives and determiners for the first word, not a dictionary. If no rule keeps both
  sets of the tests below honest, take the review's other option instead: leave the code, and add a sentence to the card
  bullet of `crates/theseus-core/AGENTS.md` saying such lines are dropped and counted in `setup`. Say which in the report.
- **L2 IN (`evidence.rs` `addressed` and `MACHINERY`, near :103-135).** `MACHINERY.iter().any(|m| l.contains(m))` is a
  substring match: an owner's note line about a "wake update" contains "wake up" and is dropped. Match on word
  boundaries, the way `one_task` already does (near :192, a boundary check after the phrase). Make one small matcher
  both use (one helper, not two), with a boundary before and after the phrase. The bare words "heartbeat" and
  "bootstrap" also drop any owner note line that names them ("Heartbeat agents must claim before working"): in a note
  (not a setup file) match a bare machinery word only when the line is also addressed to the assistant; a setup file's
  lines already need `about_owner`. Keep every existing `addressed` assertion in `tests_pure.rs` (near :295) green.
- **L3 IN (`ask.rs` `turned`, near :286).** `turned(text, s)` drops a statement without "behalf" when *any* line shown
  for its cluster mentions it, even when the statement rests on the cluster's other lines. Fix: look only at the lines
  of the episodes the statement cites (`parse` has the cited labels; `Shown.lines` is `(episode label, text)`). A
  statement citing the "behalf" line and dropping the phrase is still dropped.
- **L4 IN (`rpc/card.rs` `card_answer`, near :128-151).** `scope` is parsed, then ignored for `decline`. Refuse a
  decline that carries a scope, before anything is written (`RpcFailure::invalid`, wording like "a decline takes no
  scope"). This is a protocol refusal; the CLI cannot send it today, and no Discord text is involved.
- **Conflicts across two calls of one run: IN if it fits, with no extra model call.** Today `settle_conflicts`
  (`run.rs` near :395) only sees one call's statements, because the model sees one batch. Fix: the job keeps the
  statements it made earlier in this run; each later call's request lists them after the clusters, labelled `P1`,
  `P2`, ... (scrubbed like every line sent), and the tool's `conflicts` description says it may name those labels too.
  `parse` maps a `P` label to the earlier statement. In `statements`, a new statement that conflicts with an earlier
  one of this run is not offered (the earlier one is already written), and the run's `conflicts` gets the same note
  form `settle_conflicts` writes ("kept '<earlier>' over '<new>': they conflict"). Keep the system text and the tool
  definition the same bytes for every call of a run (the cached prefix); put the list in the user message.
  **Data floor:** the earlier statements come from their clusters' episodes, so the call's privacy routing must cover
  those sessions too: the `sessions` iterator passed to `crate::google::private::over_sessions` in `card_call` (near
  :285) must include the earlier statements' cluster episodes' sessions. A test pins it. The dry run's `at_most_usd`
  stays an upper bound (count the list's worst case, or say in the report why it does not matter). If this does not
  fit cleanly in `ask.rs`/`run.rs`, stop this part and say why in the report.
- **OUT:** the re-draft of the 18 declined proposals (needs the owner's word and a forget path that does not exist);
  the `work:coding` scope (OWNER CALL, would need store 50 and the route verdict before the standing tail); a conflict
  with a record kept in an earlier run; the live check (5 steps on a scratch daemon, for the maintainer).

### theseus-wdzmj: dream guidance's waiting reason and doc nits (`crates/theseus-core/src/consolidate/guidance.rs`)

- **(1) IN: give `waiting` a reason.** `dream_request` (near :235) returns `None` for six causes: the sampled sessions
  last used different profiles (`session_profile` errs), the profile does not resolve, its provider is missing, the
  model thinks unasked (`thinks_unasked`, near :261), fewer than `MIN_SESSIONS` readable sessions remain, or no price
  (`audit_reserve` gives none). `dream_guidance` (near :320) counts each as `out.waiting += 1`, and the nightly tender
  logs only the count (`consolidate/tender.rs` near :62: `waiting = d.waiting`). An owner on an Opus 5.5 or Fable
  session profile sees guidance stop with no reason. Fix: `dream_request` returns `Result<_, Wait>` (a small enum, one
  variant per cause; the thinks-unasked one carries the model), `Dreamed` gains the reasons beside the count (for
  example `waits: BTreeMap<String, u64>` keyed by a short reason, or a `Vec`), `waiting` stays the total, and the
  tender's log line carries the reasons. Nothing is stored and no owner message changes: the morning note
  (`judge/onto/digest.rs`) reads writes, not `Dreamed`; leave it alone.
- **(2) IN: doc nits.** `guidance.rs:17-19`, the module doc lists "(Opus 5.5, Fable 5.x, GLM)": add DeepSeek (the gate
  covers `Implicit`). `guidance.rs:230-234`, `dream_request`'s doc: add "or its model thinks unasked" (or replace the
  list with the `Wait` enum's docs). `Dreamed.waiting`'s doc (near :93) names only two causes: fix it. One clause in the
  guidance item of `crates/theseus-core/AGENTS.md` (near :297) naming the reasons.
- **(3) OUT:** the live check (effort low on the writer's Opus 5.5 call; guidance waiting on an Opus profile).

### theseus-yym79: the four forced-tool calls, robust now

**Why.** Sonnet 5.5, Opus 5.5 and Fable 5.1 refuse a forced `tool_choice` (`{"type":"tool"}` or `"any"`) with a 400;
on Sonnet 5.5, `thinking_off` gives `between_tools` (its `disabled` is a 400). Today the four calls default to Haiku
5.5, where `thinking_off` gives `disabled` and a forced tool is fine. A profile moved to Sonnet 5.5 (or an Opus 5.5 /
Fable profile) would make every one of these calls fail. Make them robust now.

**The rule.** Force the tool only where the model's thinking is off in the plain sense: `thinking_off` gives
`{"type":"disabled"}`, or the row sends no thinking and does not think unasked (`ThinkingMode::None`, `Budget`, or no
catalog row: today's behaviour for an unknown model). Everywhere else (Sonnet 5.5's `between_tools`, `Always`,
`Implicit`): `tool_choice: {"type":"auto"}`, the tool gets `"strict": true`, and every object in its `input_schema`
(nested `items` objects too) gets `"additionalProperties": false`. Keep each tool's `required` lists as they are. Each
of the four `INSTRUCTIONS` already names its tool ("with the state_cards tool", "with the write_card tool", "with the
propose_people tool", "with the name_category tool"); keep that. A text-only answer under `auto` already fails closed
in each reader ("the answer held no ... call"), after `hit_cap`.

**One shared helper, reusing the lane's.** One `Catalog` method (in `catalog.rs`, beside `thinking_off` and
`thinks_unasked`, which it should use) says whether a model may be forced, and one pure function in a new file
`crates/theseus-core/src/provider/` (for example `provider/one_tool.rs`, `pub fn one_tool(tool: Value, force: bool) ->
(Value, Value)` returning the tool and the `tool_choice`) builds the pair, closing the schema when not forced. The five
`request` functions use it: `card/ask.rs` (near :143, forced at :159), `threads_import/card.rs` (near :147, :163),
`judge/people/extract.rs` (near :60, :75), `judge/onto/name.rs` (near :98, :108), and the exam reviewer
`rsi/exams/review.rs` (near :61; Fable 5.1 is `Always`, so the helper gives what it sends today: keep its tests green).
Pass the force flag the cheapest honest way: beside `thinking`, or both in one small struct a single `Catalog` method
returns. Update the callers (`card/run.rs` `Job::scrubbed`, `rpc/thread_import.rs`, `judge/people/run.rs`, `name.rs`'s
Core caller, `review.rs`'s `CoreReviewer`) and the tests that call the `request` functions (signature updates only).
Check how a non-Anthropic provider (GLM, DeepSeek) carries a tool's `strict` field (search the provider code for
`input_schema` / `tools`): if one would refuse it, strip it in that adapter, in one place, and say so.

## Tests (each red at the base, with a planted revert)
Put the card tests in `card/tests_pure.rs` / `card/tests_job.rs`, the guidance test in
`consolidate/tests_guidance.rs`, the helper's in the new module's tests, and each call's beside its existing cap test.
- **L1** `a_setup_line_that_starts_with_the_owners_name_is_kept`: "Mara prefers tide tables in metric units" and "Mara
  is vegetarian" pass `about_owner`; "Keep notes short", "Check logs every morning" and every existing negative
  (`tests_pure.rs` near :321) do not. Red: the base drops the first two. Plant: drop the second-word arm.
- **L2** `a_notes_wake_update_and_heartbeat_line_is_the_owners`: "Post the wake update before nine" and "Heartbeat
  agents must claim before working" are not `addressed` in a note; "Read this file first at every session start" and
  "Keep the daily log under the notes folder" still are. Plant: back to `contains`.
- **L3** `a_statement_on_a_clusters_other_lines_is_not_turned`: a cluster with one "on the owner's behalf" line and two
  others; a statement citing only the others is kept; one citing the behalf line without the phrase is dropped. Plant:
  `turned` over all lines.
- **L4** `a_decline_with_a_scope_is_refused`: invalid, the proposal still open, store `last_position` unchanged (as
  `the_owner_sets_a_kept_records_scope` checks). Plant: ignore the scope again.
- **Cross-call** `a_conflict_across_two_calls_of_one_run_is_not_offered`: a fake provider whose second answer names
  `P1` in `conflicts`; one proposal made, one note in `conflicts`; the second request's user text lists `P1` and its
  system text and tools equal the first's. Plus `the_earlier_statements_sessions_route_the_later_call`: when the
  earlier statement's cluster has a private session and the later call's clusters have none, the later call goes the
  private way. Plants: `parse` ignores `P` labels; the session list leaves the earlier clusters out.
- **wdzmj** `a_waiting_category_says_why`: the Opus profile case (as `a_category_whose_model_always_thinks_waits_unasked`
  near :217) gives a thinks-unasked reason naming the model; a disagreeing-profiles case gives its own reason; the
  total `waiting` is unchanged. Plant: one reason for every cause.
- **yym79** the helper's test: forced gives the tool untouched and `{"type":"tool","name":...}`; not forced gives
  `auto`, `strict: true`, `additionalProperties: false` on the top object and on a nested `items` object. A catalog
  test: Haiku 5.5 may be forced; Sonnet 5.5, Opus 5.5, Fable 5.1 and a GLM/DeepSeek row may not; Haiku 4.5 and an
  unknown model may. Then one test per call (card, thread-import card, people extract, onto namer): on a Sonnet 5.5
  target the request sent has `tool_choice` auto, a strict closed tool and `thinking == {"type":"between_tools"}`; on
  Haiku 5.5 it stays forced with `disabled`. Red: the base always forces. Plants: the helper always forces; the catalog
  method always says yes.
- Run: `cargo nextest run -p theseus-core card standing guidance catalog exams thread_import people onto`, then the full
  `-p theseus-core`, `-p theseus-protocol -p theseus -p theseus-discord`.

## Not in this row
- The other `integ/p2-follow` rows above, and their files. `judge/onto/name.rs` may also be touched by p2f-onto (from
  `integ/p2-onto`, which does not have this lane's `thinking` argument): change only its `request`/`tool` and say so in
  the join notes.
- `cockpit/src/protocol.gen/`: no protocol type should change in this row. If one does, regenerate it once with the
  repository's generator and say so (the maintainer regenerates at the join).
- The owner-visible card text: proposals' words, `card_question`, `card_options`, the CLI's "Left out" and "Conflict"
  lines keep their form. Any change to a Discord card or DM message is an OWNER CALL (none is expected here).

## Rules for this row
- **Locked Discord DM task flow.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical:
  `git diff --exit-code 9b075d6d -- crates/theseus-discord/src/golden/dm_flow.txt` must be empty. A fix that would
  change any message of that flow (wording, layout, order, timing) stops that part: write "OWNER CALL: <what and why>,
  before/after text" in the report instead.
- **Keel.** No removed or weakened test, no raised budget, cap or ceiling (each call's `MAX_TOKENS` stays), no edit of
  `scripts/keel-guard.py` or its tests, no renamed test. A `request` signature update in a test is not weakening. Run
  `python3 scripts/keel-guard.py --base 9b075d6d --head HEAD --quiet` before the report and paste its output (must be
  empty).
- **Data floor never loosened.** Nothing private to a China-origin model; private or sensitive data only to cleared
  models. The cross-call list carries model-written text drawn from private episodes: it routes like them. End the
  report with a `Data-routing review` section: what each request now carries and how its privacy routing covers it.
- **Public repo.** No person's name, no real organisation, address, calendar id or email anywhere. Fixtures use
  invented names (as the card tests do: lantern keeper, harbour ledger, tide tables) and ids under `example.test`.
  Write "the owner".
- **Red first.** Each fix comes with a test red at the base and green after, and a planted revert named in the report
  (break it, see the test fail, restore and `touch`, `git status` clean). Real-world behaviour, never a benchmark case.
- **Right over compatible; one helper, not two.** Reuse `provider::hit_cap`, `cap_phrase`, `catalog.thinking_off`,
  `thinks_unasked`, and `one_task`'s boundary logic.
- **Model roles:** you write the code; keep it minimal. Every line should earn its place.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Beads id in each subject. End each
  message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B 9b075d6d`; do not retype it).
- **Gate.** Read `AGENTS.md` and `crates/theseus-core/AGENTS.md` first (the card, guidance and catalog items). Run the
  tests above, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.
  In `crates/theseus-core/AGENTS.md` add clauses to the existing card, guidance and catalog items, no new section.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools (never a shell heredoc); check it is non-empty after the
commit. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Sections:
1. **For the maintainer first:** store format (49, unchanged, or why not), any file over a ceiling, any OWNER CALL,
   whether L1 took the code rule or the AGENTS.md sentence, whether the cross-call part landed or stopped, any provider
   adapter that strips `strict`.
2. **Per issue and finding** (0ly2a L1-L4 and cross-call; wdzmj 1-2; yym79): done or not and why, the test, the planted
   revert and the line that failed. List the OUT items (the 18 declined re-draft, `work:coding`, earlier-run conflicts,
   both live checks).
3. **The new request shapes:** for each of the five calls, on Haiku 5.5 and on Sonnet 5.5, `tool_choice`, `strict`,
   `thinking`. The cross-call list's worst-case input tokens.
4. **Gate commands and their results.**
5. **Keel output** (empty) and the DM golden `git diff --exit-code` result (empty).
6. **Join notes for `integ/p2-follow`:** files shared with sibling rows (`judge/onto/name.rs` with p2f-onto; core
   `AGENTS.md` with every core row), changed `request` signatures and their callers.
7. **A `docs/status.md` sentence** for the integrator, and follow-ups.
8. **Data-routing review.**
