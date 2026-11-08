<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/ with a web cockpit under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, re-signs your commits, runs the full gate on the owner's machine, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents.

## Four facts about THIS VM that the previous wave's brief got wrong

A reading review of five cloud reports from the wave fired at 07:3xZ today found four errors in the brief they were given. Each cost a session real time. They are corrected here; believe this section over anything else you read.

1. **`CARGO_INCREMENTAL=0` from your FIRST cargo command, exported before you run anything.** Not "for the cargo commands you remember". Cargo folds the flag into its unit hashes, so flipping it part-way through a session invalidates every artefact built before the flip and builds a complete second set — about nine minutes of cold build, thrown away. Three sessions in the previous wave lost a build to exactly that. Do this once, first:
   ```bash
   export CARGO_INCREMENTAL=0
   ```
2. **There is no `web/` directory in this repository.** The previous brief said "web apps under web/ and cockpit/" and told sessions to run `npm ci` in both; `cd web` fails and the whole setup chain dies on it. The only `package.json` in the tree is `cockpit/package.json`. Run `npm ci` in `cockpit/` and nowhere else.
3. **The cockpit's node is pinned to exactly v24.18.0, and this image ships v22.23.4.** The pin is `NODE_PIN` at `crates/theseus-core/src/sandbox/self_build.rs:51`. The cockpit phase of the gate will fail on the image's node, and it hides its own errors, so you will read a bare non-zero exit with nothing useful under it. Install the pinned version before your first gate rather than diagnosing it afterwards — one session in the previous wave spent a whole gate run discovering this:
   ```bash
   node --version    # expect v22.23.4 on the stock image
   # then install v24.18.0 (nvm, or a nodejs distribution tarball) and put it first on PATH
   ```
4. **Thirty-five tests fail on this VM for a reason that is not yours — not four.** The previous brief named four and a session that trusts it mis-reads its own gate: it sees 35 reds, cannot account for 31 of them, and either stops or calls real failures environmental. The 35 are all the same root cause — this VM runs as root and `RLIMIT_NPROC` behaves differently here, so the sandbox refuses to build the view it asserts about. They fall in three places:
   - **`theseus-sandbox::contract` — 20 cases** (`crates/theseus-sandbox/tests/contract.rs`)
   - **`theseus-sandbox::bench spawn_100`** (`crates/theseus-sandbox/tests/bench.rs:29`)
   - **`theseusd::sandbox` — 14 cases** (`crates/theseusd/tests/sandbox.rs`)

   That is the whole known set. **A failure outside those three places is yours to explain**, and you must still name every failing test by its full test name and say for each one why it is or is not yours. Do not write "all N failures were environmental": a past cloud report did exactly that and one of the N was a real break its own malformed output parse had hidden.

**Setup** (about 15 minutes, once), in this order:
- `export CARGO_INCREMENTAL=0` — first, before any cargo command (fact 1).
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- Install node **v24.18.0** and put it first on `PATH` (fact 3), then `npm ci` in `cockpit/` — and only there (fact 2).
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**The gate, before every commit:** `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. When its suite phase fails only on the 35 cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

## The keel guard has TWO halves, and both of them bite

`scripts/keel-guard.py` runs as an early phase of the gate. **It fails in about one second, before a single test runs**, so a branch can burn a nine-minute build and then die to it with nothing proved. A branch died on exactly this confusion last night. Read both halves:

**(a) The guarded FILE list — about *where* you edit.** `KEEL_FILES` in `scripts/keel-guard.py` names specific files; editing any one of them without an ack is a `keel-file` finding. The list includes `scripts/keel-guard.py` itself, `scripts/gate.sh`, the `rsi/sealed/*` family, `scripts/scrub-added.sh`, `crates/theseus-core/src/routing.rs`, `turn/data_step.rs`, `data_gate.rs`, `content/cn.rs`, `exits.rs`, `peer.rs`, `peer_proc.rs`, `crates/theseusd/src/web.rs`, `crates/theseus-core/src/config/self_improve.rs` and **`crates/theseus-core/src/toolrun/guarded.rs`**. Read the real list in the file; do not trust this summary.

**(b) The EROSION rules — about *what your edit does*, on ANY file anywhere in the tree.** These fire wherever the change is: `test-removed`, `test-ignored`, `assert-removed`, `assert-weakened`, `allow-added`, `budget-raised`, `cap-raised`, `ceiling-raised`, `ci-loosened`. **Clearing half (a) says nothing about half (b).** The branch that died last night was nowhere near a guarded file: a test three directories from its change lost one assertion, and `assert-removed` killed the gate in 0 seconds.

So, practically: if you delete or rewrite a test, drop an assertion, loosen an equality into a range, add `#[allow(...)]`, raise a numeric budget or a cap, or touch a workflow step — expect the guard, and run it yourself (`python3 scripts/keel-guard.py --base <your base>`) before you spend a build on it. The usual right answer is **don't erode it**: keep the assertion, keep the test, keep the number.

**You cannot ack a finding.** An ack is a `Keel:` trailer on a **GPG-signed commit by a key listed in `scripts/keel-signers` at the base**, and you cannot sign here. The form the guard's `ACK` regex accepts is:

```
Keel: <rule> <path>[, <path>...] — <why>
```

where `<rule>` is one of the rule names above or `keel-file`, and the `<why>` quotes **the owner's own authorising words**. A trailer that opens with anything that is not a rule name is refused by the regex. **Never invent a quote**, and **never reshape a change to dodge the guard**. If your work genuinely needs a guarded edit or an erosion, your item below tells you what to do instead.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` (one form only, since 2026-11-08; the harvester keeps it verbatim).
- You can't sign here: commit without `-S`. The maintainer re-signs: every fix commit is re-signed by cherry-picking its CONTENT onto current main, so keep each fix in its own commit with a clean message, and end it with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` (use this form; it is the one that reaches main).
- Never fold `CLOUD_TASK.md` or `CLOUD_REPORT.md` into a fix commit: they are dropped at the merge, and a fix commit that carries them cannot be picked clean.
- Push only your branch, `git push origin HEAD:refs/heads/cloud/p5-trust-three-hours`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies in any Cargo.toml or package.json. If the right fix needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, or anyone else.
- Don't edit the spec, docs/status.md, or the README: the maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a fix. If time runs out mid-issue, leave it out and report what you found.

**Departures:** main lands by fixed departures every four hours, and your branch rides the next one after a maintainer review. Finish and push rather than polish: a complete, pushed branch rides; a polished one still on the VM waits for the next.

**The report:** when you are done, or after about 2.5 hours of work, whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. If the gate's nextest profile supports JUnit output (a `junit` section for that profile in .config/nextest.toml), commit the JUnit file beside CLOUD_REPORT.md in that same report commit, on your branch only, never in a fix commit. The maintainer reads it from the branch and drops that commit at the merge. For each issue it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- what is left or uncertain.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per issue, and the gate's result.

---

# Your item: three trust and reach defects — a wordless refusal, an imported session on every surface, a correction layer with too much reach

Branch: **`cloud/p5-trust-three-hours`**. Base: **`a2b44fd1`** (`a2b44fd1a4cac9f5eb96afbb716bb28b7b02df8a`). Model: Sonnet 5.5. Budget: about 2.5 hours.

These are **three** checklist items inside the 1.0.5 bucket row **B6 `theseus-3i6d1`, "Trust, tools and data"**. Here are all three lines, quoted whole and exactly as they stand in the row today:

> - [ ] `theseus-2gtyi` (h, P2): The directory rule is silent from a directory above every root, such as HOME: a refusal with no words.

> - [ ] `theseus-a7pr` (h, P2): session.history hands an imported session's messages to every surface, the MCP server's included.

> - [ ] `theseus-c2rv` (h, P2): The correction layer must steer only the owner's private places, never a shared place's turns.

Three items, three commits, 2.5 hours. **They are not equal in size and you will probably not finish all three.** Read all three first, then put them in the order you can actually deliver, and say in your report which you did, which you left, and what the next session needs for each. `2gtyi` is the smallest and the most self-contained; `a7pr` and `c2rv` are both about reach and are the ones with real security weight.

## `theseus-2gtyi` — a refusal with no words

The directory rule is silent when the call is made from a directory **above every root**, such as HOME. So the call is refused and the model is told nothing useful — which means it cannot correct itself, and the owner sees a dead end instead of an explanation.

Start at **`crates/theseus-core/src/policy.rs`**, around the outside-roots machinery: `outside_roots` is at `:683`, *"The label for one path the call reaches outside the roots"* at `:659`, *"The labels of what a call reaches outside the roots: its resources…"* at `:680`, and the rule that raises `open` to `notify` but **never past it** at `:485-497` (*"the roots label and never block (theseus-m9fdg)"*). There is also a comment at `:736-740` about what is **not** listed: *"Nothing outside the roots is listed either: a read, a… whichever tool makes it, and `outside_roots` names the path outside the roots"* — read that whole passage, because the silence may be deliberate there and the defect may be at a different layer.

**The question to answer first: where exactly is the refusal that has no words?** The passage above describes labelling, not refusing, and `policy.rs:12-13` says plainly *"Nothing the gate decides refuses a call (theseus-8az): the strongest answer is to wait for the operator."* So either the refusal comes from somewhere else entirely (a working-directory check, a root resolution that returns nothing), or the "refusal" is a wait the model reads as a refusal. **Find the real site before you write the words.** Then give it words that say what happened and what would fix it — which root, what the cwd was, and what the caller could do instead.

## `theseus-a7pr` — an imported session reaches every surface

`session.history` hands an imported session's messages to every surface, **the MCP server's included**. An MCP server is reachable by things the owner's private places are not, so this is a reach defect, not a cosmetic one.

What a search of the base shows:

- **`crates/theseus-core/src/rpc/methods.rs:1257`** is `session_history`, routed from `crates/theseus-core/src/rpc/server.rs:326` (`method::SESSION_HISTORY`).
- **`crates/theseus-core/src/memory_lookup/mcp.rs:181`** reads: *"A session's nodes, as `session.history` shows them, if it is a private…"* — **that is the sentence to read in full.** There is already a privacy condition on the MCP path; find out what it actually tests and why an imported session slips past it.
- **`crates/theseus-core/src/import/mod.rs:139`**: *"Nodes as a listing shows them (`session.history`, `node.list`): an…"* — the import side's own view of the same surface.
- **`crates/theseus-core/src/books/mod.rs:19-22` and `:46-48`** carry a closely related rule already built and worth copying rather than reinventing: *"**A shared place reads no episode's text.** An imported session is…"*, with the refusal's words at `:48` (*"place rule: its text is not shown in a shared place"*). **`books/` has already solved the shape of this problem for episodes.** Read it, and say in your report whether `session.history` should be using the same mechanism — if it should, that is a much better fix than a new one, and `books/tests.rs` (`:503`, `:548`, `:572`, `:602`) shows how it is tested.

## `theseus-c2rv` — the correction layer's reach

The correction layer **must steer only the owner's private places, never a shared place's turns.** Same class of defect as `a7pr`: something scoped to the owner is reaching further than it should.

The correction machinery is under **`crates/theseus-core/src/capture/`** (`act.rs`, `expect.rs`, `mod.rs`, `state.rs`, `tests.rs`) and touches `card/live.rs` and `card/observe/`. The private-versus-shared distinction already exists in the codebase — `books/mod.rs` above, and `card/mod.rs:30` mentions *"standing's privacy rule shows a shared place that its preferences and…"*. **Find the predicate that already answers "is this place shared", use it, and do not write a second one.** A second answer to the same question is how these rules drift apart; that is the explicit complaint of two other lines in this same bucket row.

## How to prove each

- **Red test first**, per item. For `a7pr` and `c2rv` the shape is the same and it is the important one: drive the surface **as the untrusted caller**, assert the private content does not arrive, **and assert that the same call from the owner's private place still returns it.** A fix that hides the content from everyone passes a one-sided test and breaks the feature.
- **A "zero X" bar is met by an empty set**, and on these two items that failure mode is acute: `assert!(messages.is_empty())` passes when the session simply did not load. **Always assert the positive count beside it.** `books/tests.rs:572` does exactly this (*"a shared place is told the counts"*) — copy that discipline.
- **`nextest` filters need the module path**: `cargo nextest list -E '<your filter>'` first; a filter that matches nothing runs 0 tests and exits 0.
- **Plant a revert** per fix and confirm the test goes red.
- For `2gtyi`, the proof is the words: quote the refusal's old text and its new text in your report, and assert the new wording in a test so it cannot silently regress to nothing.

## The keel guard

**Half (a):** `crates/theseus-core/src/peer.rs` and `peer_proc.rs` **are** on `KEEL_FILES` — they are *"who is on the other end of a socket or a CLI answer (`judge_act`): the guarantee that a job, or anything under a serving daemon, cannot answer its own approval"*. Both `a7pr` and `c2rv` are "who is asking, and what may they see" questions, so **it is entirely plausible your fix wants to touch one of them.** You cannot: you cannot sign, and an ack needs a GPG-signed commit by a key in `scripts/keel-signers` plus the owner's authorising words, which we do not have for these rows.

If a fix requires editing a `KEEL_FILES` file: **do the work, stop before committing that file**, leave the full diff as `KEEL-PATCH.md` at the branch root with the trailer drafted as `Keel: keel-file <the path> — <THE OWNER'S WORDS GO HERE>`, and say in `CLOUD_REPORT.md` exactly what question the owner must answer, costed to one line. **Everything outside the guarded file still lands.** Commit `KEEL-PATCH.md` on its own, subject `keel patch (not for main)`. **Never invent a quote and never reshape the change to dodge the guard.**

**Half (b)** applies on every file in the tree: do not delete a test, drop an assertion, loosen an equality or add `#[allow(...)]` to get green. On these three items the tempting erosion is relaxing an existing test that asserts a surface *does* return something, because your fix now narrows it — **that is `assert-weakened`, it fires wherever you do it, and it killed a branch's gate in 0 seconds last night before a single test ran.** If an old assertion is genuinely wrong under the new rule, leave it standing and say so in your report. Run `python3 scripts/keel-guard.py --base a2b44fd1` before you spend a gate.

## How this work is proved

1. **Red test first.** Write the test, run it, and watch it fail for the stated reason, *then* make the change. A test that never failed proves nothing. Quote the exact failure in your report.
2. **`nextest` filters need the module path.** `test(=bare_fn)` matches nothing, runs 0 tests and **exits 0** — a fake green. Run `cargo nextest list -E '<your filter>'` first and check it names your test, then read the positive count in the run, not only the exit code.
3. **A "zero X" bar is met by an empty set.** Whenever you assert that something is absent, also assert the positive count of what *should* be there, so an empty input cannot pass.
4. **The gate, before you claim green:** `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run locally here in this VM. There is no lane host and no `arm.sh` here: the gate is that script. Install nextest first (`cargo install cargo-nextest --locked`, about 3.5 minutes); a cold workspace build is about 9 minutes.
5. **`CARGO_INCREMENTAL=0`, exported before your first cargo command** and never changed afterwards. See fact 1 at the top: flipping it mid-session rebuilds the whole workspace from scratch.
6. **Do not classify a failure as environmental without evidence.** The known-environmental set on this VM is **35 tests in three places** (`theseus-sandbox::contract`, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox`), listed at the top. Name every failing test by its full test name, and say for each one why it is or is not yours. A past cloud report called all 35 of its gate failures environmental, and one of them was a real break that its own malformed output parse had hidden.
7. **Run the keel guard before you spend a build on a change that erodes something.** `python3 scripts/keel-guard.py --base <the base hash below>`. It is an early gate phase and fails in about one second — but only *after* the build, so finding it yourself is free and finding it through the gate is not. Both halves apply: the guarded file list, and the erosion rules on any file anywhere.

## Rules that are not negotiable

- **No person's name anywhere.** This repository is public. Write "the owner"; use the handle `zeroaltitude` only where an identity string is genuinely needed, and "collaborator" for anyone else. That covers code, comments, docs, fixtures, goldens, test data, branch names and commit messages.
- **Each fix is its own commit**, ending with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` exactly as written above. Commit without `-S`; the maintainer re-signs by cherry-picking each fix's content onto current main, so a commit that mixes a fix with `CLOUD_TASK.md` or `CLOUD_REPORT.md` cannot be picked clean. Push at every green point so nothing is lost if the session ends.
- **No new dependencies** in any `Cargo.toml` or `package.json`. If the right fix needs one, say so in the report instead.
- **Do not edit** the spec, `docs/status.md`, or the README. Where a doc should change, say what and where in the report.
- **Never erode a test to get green.** Deleting a test, adding `#[ignore]`, dropping an assertion, loosening an equality, adding `#[allow(...)]`, or raising a budget, cap or ceiling is a keel finding wherever in the tree it happens, and you cannot ack one. If a test is genuinely wrong, say so in the report and leave it standing.
- Chain commands with `&&`, never `;`.

## Your report — your last action

When you are done, or after about 2.5 hours of work, write `CLOUD_REPORT.md` at the repository root, commit it as the branch's **last** commit (subject `cloud report (not for main)`), and push. Open the file with a **one-word verdict heading** — `# LANDABLE` or `# NOT LANDABLE` — then give:

- **Row** and which bucket-row checklist item this is.
- **Branch**, the head hash you pushed, and the **base** hash you actually cloned.
- **Files** changed with `+`/`−`, and the commit count.
- **The defect, confirmed in the code**, with `file:line` — what is actually wrong in *your* clone, not what this brief assumed. **Verify every path and line number in this brief yourself: they were read on a base that may have moved under you, and several of them are known to have drifted by one or two lines already.** If the premise turns out to be false — the mechanism is gone, already fixed, or materially different — **stop, write that up with the evidence, and return it as an owner call rather than forcing the task.** A cheap, correct "this no longer exists" is a good outcome, not a failure.
- **The fix and why that shape**, including the alternative you rejected.
- **The proof**: the red test and its exact failure, the gate's numbers (test counts and `exit=`), and each planted revert and what it caught.
- **The keel guard's result** on your range, and anything you left undone because the guard would have refused it.
- **What is left, uncertain, or that this brief got wrong.** A session that corrects the brief is doing the job. Say it plainly rather than quietly working around it.

Your final message is short (under 1,200 characters) and starts with the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line on the item, and the gate's result.
