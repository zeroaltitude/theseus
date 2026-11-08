<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/ with web apps under web/ and cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, re-signs your commits, runs the full gate on the owner's machine, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents.

**Setup** (about 15 minutes, once):
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in web/ and in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.

**Known on this VM, and not yours to fix** (another session works on them): four cases of the sandbox's contract test fail here:
- `theseus-sandbox::contract clause_02_no_network_by_default`
- `clause_03_no_metadata_service`
- `clause_08_proc_and_sys_masked`
- `clause_09_limits`

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**The gate, before every commit:** `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. When its suite phase fails only on the four cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` (one form only, since 2026-11-08; the harvester keeps it verbatim).
- You can't sign here: commit without `-S`. The maintainer re-signs: every fix commit is re-signed by cherry-picking its CONTENT onto current main, so keep each fix in its own commit with a clean message, and end it with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` (use this form; it is the one that reaches main).
- Never fold `CLOUD_TASK.md` or `CLOUD_REPORT.md` into a fix commit: they are dropped at the merge, and a fix commit that carries them cannot be picked clean.
- Push only your branch, `git push origin HEAD:refs/heads/cloud/p5-nsp9l2-prompt-latency`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your branch and base

- **Branch:** `cloud/p5-nsp9l2-prompt-latency`. Push with `git push origin HEAD:refs/heads/cloud/p5-nsp9l2-prompt-latency`.
- **Base:** `origin/main`, which was `2e6aa047` when this brief was written. Record in your report the hash you actually cloned.

## Your item: theseus-nsp9l.2 — find the 20 ms on the typed-prompt path

Bucket row `theseus-la5oc` ("1.0.5 B2: Work that runs by itself"), checklist item `theseus-nsp9l.2` (d, P1).

**The checklist line, whole and verbatim, is your spec:**

> - [ ] `theseus-nsp9l.2` (d, P1): The typed-prompt-to-request path is 49.5 ms against the 30 ms target; find the 20 ms in the stock path.

That is the whole line. It names a measurement, a target and a gap, and it asks you to **find** the difference — not to guess at it and not to optimise by instinct.

**This row is a measurement row, and its deliverable is numbers before anything else.** The order is not negotiable:

1. **Measure the stock path first, on your clone, before you change a line.** Record the figure you actually get, not the one in the row. The 49.5 ms was measured on the owner's 16-core machine; this VM is a 4-core Firecracker guest and your absolute number will differ. **The gap is the subject, not the absolute.** Say in the report what your stock figure is, how many iterations it came from, and its spread — a single sample is not a measurement.
2. **Attribute the time.** Break the path from a typed prompt to a built request into its parts and give each part a figure. This is the actual work of the row. A report that says "it got faster" without saying *where the 20 ms was* has not done this row, even if the number improves.
3. **Then, and only then, change something** — and re-measure the same way, so the before and after are comparable. One commit per change, each with its own before/after pair.

**Where to look for the harness.** There is an existing benchmark and timing-budget apparatus in this workspace, and the speed budgets in `scripts/gate.sh` are measured on the owner's machine. **Find the existing harness and use it rather than writing a new one** — search for the existing budget assertions and the bench targets before you build anything. If there genuinely is no harness for this path, write the smallest one that produces a repeatable figure, and say in the report that you wrote it and why.

**A warning that applies directly to this row.** The gate's speed budgets are calibrated to the owner's 16-core machine. **Leave them alone.** If your change makes a budgeted path faster, do not tighten its budget here; say in the report what the new figure is and let the maintainer re-calibrate on the real machine. If a budget assertion fails on this VM for reasons of the VM, name the test and say so — but read the instruction above about not calling a failure environmental without evidence.

**What makes this item done.**

- A table in the report: the stock path broken into parts, each with a measured figure, summing to the stock total.
- The 20 ms (or whatever your clone's gap turns out to be) **located** — named as a specific cost in a specific place, with the evidence.
- Whatever of it you could remove, removed, each in its own commit, with a before/after pair measured the same way.
- Behaviour unchanged: the gate is green, and you say explicitly which tests cover the path you touched.
- If the gap turns out to be irreducible without a change the owner should decide on — a cache that trades memory, a lazily-built block, a dropped check — **stop at the measurement and return it as an owner call.** A correct, well-evidenced "here is where the 20 ms is and here is the trade to remove it" is a complete answer to this row.

**Do not report an improvement you cannot reproduce.** Run each measurement enough times to separate the signal from this VM's noise, and report the spread alongside the median. A timing claim with one sample behind it will be rejected at review.

## How this work is proved

1. **Red test first.** Write the test, run it, and watch it fail for the stated reason, *then* make the change. A test that never failed proves nothing. Quote the exact failure in your report.
2. **`nextest` filters need the module path.** `test(=bare_fn)` matches nothing, runs 0 tests and **exits 0** — a fake green. Run `cargo nextest list -E '<your filter>'` first and check it names your test, then read the positive count in the run, not only the exit code.
3. **A "zero X" bar is met by an empty set.** Whenever you assert that something is absent, also assert the positive count of what *should* be there, so an empty input cannot pass.
4. **The gate, before you claim green:** `THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run locally here in this VM. There is no lane host and no `arm.sh` here: the gate is that script. Install nextest first (`cargo install cargo-nextest --locked`, about 3.5 minutes); a cold workspace build is about 9 minutes.
5. **`CARGO_INCREMENTAL=0`** for every cargo command you run here. Incremental artefacts are worth nothing on a VM that is discarded and they cost disk you need.
6. **Do not classify a failure as environmental without evidence.** A past cloud report called all 35 of its gate failures environmental, and one of them was a real break that its own malformed output parse had hidden. Name every failing test by its full test name, and say for each one why it is or is not yours.

## Rules that are not negotiable

- **No person's name anywhere.** This repository is public. Write "the owner"; use the handle `zeroaltitude` only where an identity string is genuinely needed, and "collaborator" for anyone else. That covers code, comments, docs, fixtures, goldens, test data, branch names and commit messages.
- **Each fix is its own commit**, ending with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` exactly as written above. Commit without `-S`; the maintainer re-signs by cherry-picking each fix's content onto current main, so a commit that mixes a fix with `CLOUD_TASK.md` or `CLOUD_REPORT.md` cannot be picked clean. Push at every green point so nothing is lost if the session ends.
- **No new dependencies** in any `Cargo.toml` or `package.json`. If the right fix needs one, say so in the report instead.
- **Do not edit** the spec, `docs/status.md`, or the README. Where a doc should change, say what and where in the report.
- Chain commands with `&&`, never `;`.

## Your report — your last action

When you are done, or after about 2.5 hours of work, write `CLOUD_REPORT.md` at the repository root, commit it as the branch's **last** commit (subject `cloud report (not for main)`), and push. Open the file with a **one-word verdict heading** — `# LANDABLE` or `# NOT LANDABLE` — then give:

- **Row** and which bucket-row checklist item this is.
- **Branch**, the head hash you pushed, and the **base** hash you actually cloned.
- **Files** changed with `+`/`−`, and the commit count.
- **The defect, confirmed in the code**, with `file:line` — what is actually wrong in *your* clone, not what this brief assumed. **Verify every path and line number in this brief yourself: they were read on a base that may have moved under you.** If the premise turns out to be false — the mechanism is gone, already fixed, or materially different — **stop, write that up with the evidence, and return it as an owner call rather than forcing the task.** A cheap, correct "this no longer exists" is a good outcome, not a failure.
- **The fix and why that shape**, including the alternative you rejected.
- **The proof**: the red test and its exact failure, the gate's numbers (test counts and `exit=`), and each planted revert and what it caught.
- **What is left, uncertain, or that this brief got wrong.** A session that corrects the brief is doing the job. Say it plainly rather than quietly working around it.

Your final message is short (under 1,200 characters) and starts with the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line on the item, and the gate's result.
