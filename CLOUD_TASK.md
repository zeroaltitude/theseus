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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-personas`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

Your branch starts from `main` at `23328545` (2026-10-23: release 1.0.1, installed). That is your base. Line numbers
quoted below are at `23328545` but may be off by a few: search by name. The release preamble above is older than this
note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2).** Several rows start on `23328545` at about the same time, each in an
integration lane its task names. Rows in the same lane are told each other's files so they stay additive: one helper,
not two; adjacent lines, not rewrites.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
where it fits. If your row adds a field or a variant to a stored record, bump it to 50, add the old layout's literal
sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say so in your report's first section.
Several rows may bump; the maintainer renumbers at the join. Never write the number as a literal anywhere but the
constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733. Put every new type in a module file of its own and add to
  `lib.rs` only method constants, the `mod`/`pub use` line and field lines. Your task names your row's line budget.
- `crates/theseus-core/src/turn.rs` 3,469 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing
  (a `mod` line at most, and say so).

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's standing rules for every row:**
1. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. If your
   fix would change any message of that flow (wording, layout, order, timing), STOP that part and write
   "OWNER CALL: <what and why>" in your report instead.
2. Keel: no removed, ignored or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or
   its tests. A cloud row cannot carry a `Keel:` trailer. Run `python3 scripts/keel-guard.py --base origin/main --head
   HEAD --quiet` before your last commit and paste its output in the report.
3. The repository is public: no person's name, no real address or id anywhere. Fixtures use invented names and ids
   under `example.test`. Write "the owner".
4. Nothing private goes to a model of China origin. Right over compatible: fix the cause, one source of truth.
5. Red first: each test is seen failing on the base code before the fix, and each behaviour change gets a planted
   revert named in the report.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

---

## Your task: p2-personas, personas move into the ontology: a seeded `persona` kind, one category per persona with its soul in a small file, Jev's per-message pick over them plus `none`, a per-persona size cap, the owner asked for a new persona's soul, and `[personas]` read once then retired (theseus-utfnl)

Branch: `cloud/20261023-p2-personas`. Every commit's subject carries `theseus-utfnl`. Deadline for the report: 6 hours
after you start.

**Why.** Today a persona is a `[personas.<key>]` table in the owner's config: its files, name, emoji, card, voice,
avatar, initiative and `shareable`. The owner, 2026-10-23: "I do think the personas should be an ontology -- config
begs the question of how we manage them -- why not just in ontology?"; "Let's not have any logic to bind personas to
locations -- that is overkill. Lets make sure all the personas are small ... The budget here should be like 1000-2000
tokens, not much more than that."; "Then we need a mechanism for noting that a persona has been added and requesting
the user provide some SOUL information for it -- that gets mapped to a file."; and "I'm ok with the place logic" (keep
the shared-place `shareable` filter). What set it off: a dev persona whose files included a 19k-token runbook was added
to the config, and because every persona's files sit in the DM's cached persona layer, the DM's first request went
from 14,086 to 39,925 tokens. It was backed out. Nothing caps a persona today.

The owner's install has five personas, each one small file (323 to 931 tokens; the persona layer is 3,348 tokens in
all), one of them the default with no card. After this row they must keep working with **no change to any prefix
byte**.

**Risk: medium-high.** It touches the persona layer of the cached prefix, the ontology's writer rules (a keel matter:
Jev, the sweep and the dream must never write a persona), the store format, and an owner-facing message. Read
"The design check" below in full before writing code: it was done against your base and names the exact code.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins the lane `integ/p2-onto`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Size |
|---|---|---|---|
| `p2-personas` | theseus-utfnl | the BUILD SPEC below, steps 1-11 | L |

## The other row in your lane (stay additive)
`p2-onto-mint` (theseus-f61px) is built at the same time on another host (branch `lane/p2-onto-mint`, not visible to
you) and lands in `integ/p2-onto` with you. It fixes Jev's minting: a kept "same kind" answer that blocks an unlisted
name from minting for up to 30 days. Its files are Jev's mint and judgment code under
`crates/theseus-core/src/judge/onto/` (`decide.rs`, `stage.rs`, `mint.rs`, `judged.rs`, `renamed.rs`) and their
tests, possibly the sweep (`crates/theseus-ontology/src/sweep.rs`). **Do not edit those files.** Your ontology edits are
in `crates/theseus-ontology/src/` `kind.rs` (one seed row, the pinned row), `machine.rs` (one refusal in
`check_guidance_writer`), `ontology.rs` (one refusal in `check`'s `Record::Members` arm), `category.rs` (one field),
`lib.rs` (a `mod` line), and new files `persona.rs` and `tests_persona.rs`. Keep each edit small and on lines of its
own so the integrator's merge is mechanical. Other patch2 rows may add rows to `docs/messages.md`'s catalog: add yours
as one table line at the end of the table.

## The design check (done against your base; follow it)
Every point below was checked in the code at `23328545`. Where it says "decided", do not reopen it.

1. **The kind.** Seeds are code (`crates/theseus-ontology/src/kind.rs`, `interpreted_seeds()`); `Ontology::seeded()`
   and `Ontology::load` start from them, so a new seed needs no stored record. Add `persona`: `Basis::Interpreted`,
   `assigned_by = [Operator, Import]`, `per_session = AtMost(1)`, no parent, `Rule::IntentLine` (a built rule is
   required; nothing composes it, see 2), **precedence 100** (unique; the seeds use 10 to 90; `check_table` requires
   uniqueness), description "A persona: who answers a message. Its soul is a file; Jev picks one per message. Only the
   owner writes one." **Pin the row** like the given kinds: in `Kind::check_basis`, the persona row's `basis`,
   `assigned_by` and `per_session` stay as the seed has them for every writer, the operator included (so no kind
   edit can ever add `jev`, `sweep` or `dream` to it); precedence and description stay changeable.
2. **No memberships.** Engagement is per message (`crates/theseus-core/src/engage.rs`), never a session's membership.
   In `Ontology::check`'s `Record::Members` arm, refuse a list of kind `persona` from every origin ("a persona is picked
   per message, never a membership"). Then the compile (`ontology::Walk::compose`) never meets a persona, and nothing
   of the persona kind reaches the category guidance block.
3. **No guidance.** A persona's text is its soul file, so its guidance record stays empty. Today
   `machine.rs` `check_guidance_writer` lets the dream write the FIRST guidance of ANY category: that is a way in.
   Refuse a guidance record on a `persona:` category from every origin, the operator included (one source of truth;
   the refusal names `theseus persona soul`).
4. **Who writes.** With `assigned_by = [Operator, Import]`, `interprets()` refuses jev/sweep/dream category writes, and
   `check_machine_merge` refuses a sweep merge of a category a machine did not write. Jev's filing never sees the kind
   (`judge/onto/mod.rs` keeps kinds whose `assigned_by` names jev). Close 1-3 above and the persona is the owner's
   alone in the ontology.
5. **The fields: store format 49 -> 50.** The design hoped serde-default fields need no bump; the version rule in
   `crates/theseus-store/src/record.rs` says a field added to a stored record bumps `MANIFEST_FORMAT`, with serde
   defaults as the reader and an old-layout sample in `crates/theseus-core/src/tests_layouts.rs` (person handles did
   this at format 26). Decided: **bump to 50**, name it in your report's first section. Put the persona's fields in ONE
   nested field, `Category.persona: Option<PersonaCard>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`),
   with `PersonaCard` in a new `crates/theseus-ontology/src/persona.rs`:
   - `soul: Option<String>`: the soul file's path as the config spelled it; none = awaiting its soul;
   - `soul_public: bool`: the file entry's `readers = "public"` (the shared-place rule, `context_files::withhold_shared`);
   - `soul_digest: Option<String>`: sha16 of the text when the owner's words were written;
   - `emoji`, `card`, `tts_voice`, `avatar`: `Option<String>`, validated as `PersonaConfig::validate` does (`CARD_MAX`);
   - `initiative: Initiative` (default normal), `shareable: bool`, `default: bool` (at most one category holds it).
   `check_fields` checks these only on a `persona:` category and refuses `persona` on any other kind. Category id
   `persona:<key>` with local = the config key exactly (`CategoryId::new("persona", key)`, not `mint_id`); the
   category's `name` = the display name (`PersonaConfig::name`, else `capitalised(key)`). Keys `none` and `prior` are
   reserved (route option ids) and refused.
6. **Jev's pick and `none`.** Today `turn/persona_engage.rs` builds `Choice`s from `cfg.personas`
   (`engage::choices`), `persona_ctx` sends route.v7's persona question only the personas **with a card**
   (`PersonaInput` needs one), and `engage::decide` maps the answer back by key. A default persona with no card can
   therefore never be picked; only `prior` keeps it. The persona question (`crates/theseus-judge/packs/route.v7.toml`)
   is `options_from = "personas"` plus one static option `prior`, and a pack's static options never change in place.
   Decided: **do not touch any pack, golden request or the `theseus-judge` crate.** Instead `persona_ctx` adds one
   dynamic item `{id: "none", name: <the default persona's name>, card: <its card, else "No specialist: the general
   assistant, for anything the other cards do not cover.">}` whenever the question is asked, and the default
   persona's own item is not listed beside it. `engage::decide` maps `none` to the default persona: the category
   marked `default`, else the built-in (`PersonaView::built_in`). The bands are unchanged (explicit address wins at 1;
   at `act` silent; `confirm` to `act` shown; below `confirm`, late or absent, the prior stands).
7. **The cached prefix must not move.** Today, where two or more personas can answer (DM, channel),
   `Config::context_paths(persona, engaging = true)` carries every persona's files in the persona layer, each headed
   `# Context file (persona <key>): <expanded path>` (`context_files::header`), in key order; the header block is the
   built-in default; the engaged persona is one line in the tail (`persona_line`). After the move, build the persona
   layer from the live persona categories in id order (= key order), each
   `ContextPath { path: soul, persona: Some(key), public: soul_public }`: same keys, same expanded paths, same bytes,
   so the **same prefix bytes**: the migration causes no recompile. What does change the prefix is inherent and as
   today (a new live persona, a retired one, an edited soul: one `system_changed` recompile each). An awaiting-soul
   persona changes nothing. `persona::Board::observe`'s field digest must see the same `PersonaView` values after the
   migration, so no spurious `persona.changed` row.
8. **Every reader of `cfg.personas` moves to one book.** core: `config.rs` (`context_paths`, `persona_engaging`,
   `persona_for`, `persona_key_for`, `persona_names`, the validate and parse-warning code), `turn/persona_engage.rs`,
   `turn/persona_step.rs`, `persona_recall.rs`, `places.rs`, `rpc/persona.rs`, `rpc/persona_view.rs`,
   `rpc/persona_import.rs`, `rpc/info.rs`, `rpc/voice_pick.rs`, `rpc/threads.rs`, `rpc/thread_import.rs`,
   `rpc/standing.rs`, `correction/mod.rs`, `correction/words.rs`, `judge/inbound.rs`, `judge/people/mod.rs` (persona
   names kept out of people), `threads_import/candidates.rs`, `census.rs`, `keep_warm.rs`, `context_parts.rs`,
   `turn/spec_step.rs`; the CLI's `crates/theseus/src/persona_cmd.rs` (its `edit` and `import` write the config file
   today); Discord `runtime/route.rs`, `bindings.rs`; cockpit `views/Ontology.tsx`, `lib/ontology.ts`. Search for
   `personas` yourself too; the list may be short by a few. Mechanics: a `personas::Book` (key -> `PersonaView`, plus
   the default key) built from the ontology snapshot, held by the core and swapped on every persona write (the
   ontology cache already swaps snapshots); `Config::context_paths` and friends take `&Book`.
9. **Migration: read once, then ignored.** The config file is the owner's; the daemon does not rewrite it, and a
   startup refusal of `[personas]` would stop his daemon the moment the install lands. Decided: at startup, before
   serving, if the ontology holds no persona written by the migration and the config has `[personas]`, write each as a
   category (origin Operator, `added_by` `config-migration`) in one frame, with one ledger row `persona.migrated`
   (keys, paths, digests). From then on `[personas]` and `[context] default_persona` are ignored: a startup warning
   (the `Config::parse` warnings path) and one `ℹ️ FYI · Personas moved into the ontology` note (via
   `crate::owner_note`) say so and how to delete them. A second start writes nothing. A persona with more than one file:
   the first is its soul; the others are named in the warning as no longer carried (a dev runbook is not a persona).
   `default_persona` becomes `default: true` on that category. The bindings file's per-place `persona = "<key>"` and a
   session's own choice (`persona.session.<id>`) keep resolving by key. **No file moves**: the pointer is the path the
   config spelled.
10. **No place binding** (the owner): nothing new binds a persona to a place. `engage::choices` (a shared place: its own
    and the `shareable` ones) and `context_files::withhold_shared` stay as they are, reading the book.
11. **The cap.** Nothing caps a persona file today (only `context_files::MAX_BYTES`, 64 KiB per file). Add
    `[context] persona_max_tokens` (default 2000; validated 200 to 8000) in `config/context.rs`, estimated as bytes /
    `task_graph::view::BYTES_PER_TOKEN` (say so in its doc comment). Refused at every soul write with the count and the
    cap ("trim it to N tokens"); never cut. At compile (a hand edit, or a migrated file over it) the soul is still
    carried whole, and `turn/persona_step.rs` sends one `⚠️ Alert · Persona <name>'s soul is over its cap` note per
    digest (beside `missing_text`). `census.rs`: each persona part gets an `over_cap` flag, shown in `theseus census`
    and the cockpit census. Startup and `theseusd check`: a warning line per over-cap persona (never a refusal).
12. **A new persona and its soul.** A persona added without a soul (`theseus persona add KEY` with no `--soul`, or the
    cockpit's add form) is created **awaiting its soul**: listed, never offered to Jev, never addressable, no prompt
    tokens, not counted by `persona_engaging` (two or more LIVE personas). One message, never repeated:
    `crate::owner_note::note(Kind::Action, "New persona: <name>", <trigger: "you added it from the CLI" / "the
    cockpit">, <body: a few sentences on its voice, temperament, what it cares about, what it is for, and an emoji>)`,
    with the action last: the command `theseus persona soul <key>`, and on Discord two buttons, "Write its soul" (a
    modal with one paragraph field: the pattern of `crates/theseus-discord/src/runtime/answer_cmd.rs`
    `open_answer_modal` / `answer_modal`) and "Decline" (retires the category, undo-able). It goes through
    `stage_to_operator` with a new kind `persona_soul`, its handler beside `runtime/onto_card.rs` in a new file. Add its
    catalog line to `docs/messages.md`; `tests_owner_note::every_unasked_message_starts_with_a_kind_and_a_title` (core)
    and its Discord twin must cover it. This is NOT the locked DM task flow and touches none of it. The soul goes live
    when written: `~/.theseus/personas/<key>/PERSONA.md` (file 0600, dir 0700), the category's `soul` and
    `soul_digest` set. A later hand edit is noticed by the existing `persona::Board::observe` (`persona.changed` row)
    and the cap re-checked; the category is not rewritten.
13. **Only the owner writes a soul.** One function, `persona::soul::write` (new, theseus-core), reached only from the
    `persona.add` / `persona.soul` / `persona.import` RPCs with an owner `Answerer` from a private place (the pattern of
    `rpc/ontology.rs` `ontology_guidance_set`: "Only you, from a private place"), refused inside a job. Atomic write,
    the replaced file kept as `PERSONA.md.<ms>.bak` beside it, a ledger row `persona.soul` (key, path, old and new
    digest). No model tool writes a persona or its file.

## BUILD SPEC (in this order; red first where there is a test)
1. **Ontology: the kind and the card** (design 1-5). Tests in a new `crates/theseus-ontology/src/tests_persona.rs`:
   `no_machine_origin_writes_a_persona` (jev, sweep and dream each try a new persona category, an edit of one, a merge,
   a retire, guidance, a membership, and a kind row adding themselves to `assigned_by`: every one refused; this is the
   keel test, name it in the report), `a_persona_has_no_memberships`, `a_persona_has_no_guidance`,
   `one_default_persona`, `the_persona_kind_row_is_pinned` (an operator kind write changing its assigned_by is
   refused), `none_and_prior_are_reserved`; `tests_kinds.rs` updated for the new seed.
2. **Store format 49 -> 50** for `Category.persona`; a category of the old layout reads with `persona: None` in
   `tests_layouts.rs`.
3. **The book** (design 8): `crates/theseus-core/src/personas.rs`; every `cfg.personas` reader moved; `persona_engaging`
   = two or more live personas. Every existing persona test still passes unchanged in what it asserts (move its setup
   from `[personas]` config to persona categories where it must; never weaken an assertion).
4. **Migration** (design 9). Tests: `migration_round_trip` (five invented fixture personas, the existing ones under
   `crates/theseus-core/fixtures/personas/`, one with `readers = "public"`, one `shareable`, one default with no card:
   the book's `PersonaView`s equal the config's), `migration_is_idempotent`, `a_second_file_is_named_not_carried`,
   `migration_changes_no_prefix_byte` (the persona layer and the project layer of a fresh DM's first request are
   byte-equal built from the config before and from the ontology after; model it on
   `crates/theseus-core/src/tests_persona_layer.rs`, whose whole suite must also pass with personas from the ontology),
   `no_persona_changed_row_after_migration`.
5. **The cap** (design 11). Tests: `an_oversized_soul_is_refused_at_write`, `census_flags_an_oversized_persona` (a
   fixture soul over 2,000 tokens), `census_dm_persona_layer_unchanged` (the five fixtures' persona layer the same
   size before and after the move), `an_over_cap_soul_alerts_once`.
6. **`none`** (design 6). Tests in `engage.rs`: `none_engages_the_default`, `none_with_no_default_is_the_built_in`,
   `the_default_is_not_offered_twice`. No pack, golden or `theseus-judge` change (check `git diff --stat origin/main
   -- crates/theseus-judge` is empty).
7. **Writers** (design 12-13). RPCs `persona.add`, `persona.soul`, `persona.set` (short fields), `persona.retire`,
   `persona.default`; `persona.import` writes a category (origin Import) instead of the config. CLI
   `theseus persona add|soul|set|retire|default` (`soul` reads `--file`, stdin, or `$EDITOR`; `edit` stays as an alias
   of `set`, or of `soul` with no field flags); no config-file write left in `persona_cmd.rs`. Protocol types in a new
   module (not in `lib.rs`); `lib.rs` gets at most 8 lines (method constants, the `mod`/`pub use` line). Test
   `only_the_owner_writes_a_soul` (a job, a shared place and a non-owner are refused; the file is untouched).
8. **The new-persona note** (design 12). Tests: the owner_note family tests (core and Discord) include it; a fake-Discord
   test that the "Write its soul" modal's text becomes the file and goes live, and that "Decline" retires it; the DM
   task flow golden: `git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` is empty and
   its test passes. `crates/theseus-discord/src/runtime.rs` is at 3,494 / 3,500: put the handler in a new file, at
   most the `mod` line and one dispatch arm there.
9. **Cockpit `/ontology`**: the persona kind's categories with name, emoji, card, the default mark and the soul's state
   (live / awaiting its soul / over its cap, with its tokens); an add form; a soul editor (a textarea ->
   `persona.soul`); retire. `cockpit/src/protocol.gen/` regenerated. A test in `cockpit/test/ontology.test.ts`.
10. **Docs**: the config reference's `[personas]` marked "read once at the first start of 1.0.2, then the ontology";
    `crates/theseus-core/config/theseus.example.toml` without `[personas]` (a comment pointing at `theseus persona
    add`); `crates/theseus-core/AGENTS.md` persona section; a `docs/status.md` sentence in your report for the
    integrator.
11. **Gates**: `cargo nextest run -p theseus-ontology`, and theseus-core's persona, engage, census, layouts and
    owner_note tests, theseus-discord's owner_note and golden tests, the cockpit tests; `cargo clippy --workspace
    --all-targets -- -D warnings`; `cargo fmt --check`; the keel guard (expected empty).

## Open questions: do not decide them; build the default
1. Jev proposing a persona when a message addresses an unknown name: **leave as no**. Personas are added by the owner
   (CLI, cockpit, import) only.
2. A soul over the cap: **leave as refused with the count**. No "condensed draft" offer (that would be a model call).
3. Adding the two keel tests to `scripts/keel-guard.py`'s protected list: **leave it unedited** (the owner's call).

## Not in this row
- Any route pack, golden request or the `theseus-judge` crate.
- Jev's mint and judgment code (`judge/onto/*`): p2-onto-mint's.
- The Discord DM task flow (task reply, started and thread lines, board, forum, needs-you cards, budget card, task
  reports) and `golden/dm_flow.txt`.
- Place binding of personas (the owner said no), and the owner's private runbook (not in the repository).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before your last commit and paste its output.
- **Ceilings** (the note above): `turn.rs`, `render.rs` and Discord `runtime.rs` get nothing but a `mod` line or one
  call each; say which in the report.
- **Dependencies.** None new.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Names.** No person's name anywhere: write "the owner". Persona fixtures use the invented names already under
  `crates/theseus-core/fixtures/personas/`; any address uses `example.test`.
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (personas, the ontology, the context files, the place
  rule, owner notes), `crates/theseus-ontology/AGENTS.md` if present, `crates/theseus-discord/AGENTS.md` and
  `docs/messages.md` first.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds, first: the store format bump (49 -> 50) and any ceiling line added. Then:
- what you built, BUILD SPEC step by step, with the file and function for each and the wire shapes;
- the keel: the test `no_machine_origin_writes_a_persona` and `only_the_owner_writes_a_soul`, what each pins, and their
  plants;
- the prefix: `migration_changes_no_prefix_byte`'s result and how the persona layer is built now;
- the migration as built, and what the owner's install will see at its first start (the warning, the FYI note);
- the Discord check: the new message's head, and the golden's `git diff --exit-code` result;
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output;
- a `docs/status.md` sentence for the integrator;
- the files you touched in `crates/theseus-ontology/` (for the merge with p2-onto-mint) and in `docs/messages.md`;
- follow-ups, and anything you were unsure of.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
