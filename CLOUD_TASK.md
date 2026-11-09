<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/ with a web cockpit under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, re-signs your commits, runs the full gate on the owner's machine, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents.

## Four facts about THIS VM. Believe this section over anything else you read.

Each of these was wrong in an earlier brief and each cost a session real time. Facts 1 to 3 are the last wave's text unchanged — that wave returned seven landable branches of seven. **Fact 4 is corrected again for this wave**, from two independent measurements made last night: the count is forty, not thirty-five, and it has two causes rather than one.

1. **`CARGO_INCREMENTAL=0` from your FIRST cargo command, exported before you run anything.** Not "for the cargo commands you remember". Cargo folds the flag into its unit hashes, so flipping it part-way through a session invalidates every artefact built before the flip and builds a complete second set — about nine minutes of cold build, thrown away. Three sessions in one wave lost a build to exactly that. Do this once, first:
   ```bash
   export CARGO_INCREMENTAL=0
   ```
2. **There is no `web/` directory in this repository.** An older brief said "web apps under web/ and cockpit/" and told sessions to run `npm ci` in both; `cd web` fails and the whole setup chain dies on it. The only `package.json` in the tree is `cockpit/package.json`. Run `npm ci` in `cockpit/` and nowhere else.
3. **The cockpit's node is pinned to exactly v24.18.0, and this image ships v22.23.4.** The pin is `NODE_PIN` at `crates/theseus-core/src/sandbox/self_build.rs`. The cockpit phase of the gate will fail on the image's node, and it hides its own errors, so you will read a bare non-zero exit with nothing useful under it. Install the pinned version before your first gate rather than diagnosing it afterwards:
   ```bash
   node --version    # expect v22.23.4 on the stock image
   # then install v24.18.0 (nvm, or a nodejs distribution tarball) and put it first on PATH
   ```
4. **Forty tests fail on this VM for reasons that are not yours — and the count was WRONG in each of the last three briefs.** They said four, then thirty-five; the measured set is **40, in four places, from two unrelated causes**. Two independent cloud sessions in two different waves enumerated the sandbox family and got the identical names, and a third proved the TZ family, so this is evidence rather than an estimate.

   **Cause A — this VM runs as root and `RLIMIT_NPROC` behaves differently here, so the sandbox refuses to build the view it asserts about. 36 cases in three places:**
   - **`theseus-sandbox::contract` — 20 cases** (`crates/theseus-sandbox/tests/contract.rs`)
   - **`theseus-sandbox::bench spawn_100`** (`crates/theseus-sandbox/tests/bench.rs`)
   - **`theseusd::sandbox` — 15 cases** (`crates/theseusd/tests/sandbox.rs`) — **fifteen, not fourteen.** The name the old fourteen-entry list left out is `a_deadline_ends_a_job_mid_tunnel_and_its_connection_is_still_recorded`.

   **Cause B — this VM's `TZ` is UTC and the gate sets none, while four tests assume the owner's Phoenix zone. 4 cases in one place:**
   - **`theseus-core google::tests_morning` — 4 cases.** Proved, not guessed: `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(google::tests_morning)'` gives **10 run, 10 passed**. If you see those four, re-run them with `TZ=America/Phoenix` and say you did. A branch landing in tonight's departure makes them hermetic, so after a `git merge origin/main` they may simply be gone — which is the better outcome. Say which you saw.

   That is the whole known set. **A failure outside those four places is yours to explain**, and you must still name every failing test by its full test name and say for each one why it is or is not yours. Do not write "all N failures were environmental": a past cloud report did exactly that and one of the N was a real break its own malformed output parse had hidden.

**Setup** (about 15 minutes, once), in this order:
- `export CARGO_INCREMENTAL=0` — first, before any cargo command (fact 1).
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- Install node **v24.18.0** and put it first on `PATH` (fact 3), then `npm ci` in `cockpit/` — and only there (fact 2).
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.

**Budget the gate, in minutes, before you start the work.** Setup plus a cold build plus one gate is most of an hour on this VM and it is front-loaded. Two of the last wave's three NOT-LANDABLE rows reached the end of their budget with **the gate never run** — and one of those never saw the golden that actually blocked it, because only the gate would have shown it. Plan to run the gate at least once, and prefer **one sub-item finished and gated** over three sub-items coded and ungated.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**The gate command, exactly — note the pinned keel base:**

```bash
THESEUS_KEEL_BASE=<the base hash below> THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh
```

**`THESEUS_KEEL_BASE` is not optional and it is not cosmetic.** See the next section for what happens without it. When the gate's suite phase fails only on the 35 cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`.

The speed budgets in the gate are measured on the owner's machine: leave them alone.

## The keel guard has TWO halves, and both of them bite

`scripts/keel-guard.py` runs as an early phase of the gate. **It fails in about one second, before a single test runs**, so a branch can burn a nine-minute build and then die to it with nothing proved.

**(a) The guarded FILE list — about *where* you edit.** `KEEL_FILES` in `scripts/keel-guard.py` names specific files; editing any one of them without an ack is a `keel-file` finding. **Read the real list in the file.** As it stands on this base it is: `scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`, the whole `crates/theseus-core/src/rsi/sealed*` family (`sealed.rs`, `check.rs`, `filter.rs`, `daemon.rs`, `runner.rs`, `relay.rs`, `tests.rs`, `tests_reach.rs`), `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`, `scripts/scrub-added.sh`, `scripts/test_scrub_added.py`, `crates/theseus-core/src/routing.rs`, `crates/theseus-core/src/turn/data_step.rs`, `crates/theseus-core/src/content/cn.rs`, `crates/theseus-core/src/data_gate.rs`, `crates/theseus-core/src/exits.rs`, `crates/theseus-core/src/peer.rs`, `crates/theseus-core/src/peer_proc.rs`, `crates/theseusd/src/web.rs`. Plus `KEEL_PREFIXES = ("crates/theseus-exam/exam/",)` and `LOCKED_FLOW_PREFIXES = ("crates/theseus-discord/src/golden/",)`.

**Two things an older brief got wrong about that list, corrected here:** `crates/theseus-core/src/turn/route_step.rs` is **NOT** on it (`turn/data_step.rs` is, and `routing.rs` is), and `scripts/gate.sh` is **not in `KEEL_FILES`** either — it is handled by special cases that cover only its lines naming the keel, its door, or its phase list. A builder who trusts a summary instead of the file refuses a legal edit; one did.

**(b) The EROSION rules — about *what your edit does*, on ANY file anywhere in the tree.** These fire wherever the change is: `test-removed`, `test-ignored`, `assert-removed`, `assert-weakened`, `allow-added`, `budget-raised`, `cap-raised`, `ceiling-raised`, `ci-loosened`. **Clearing half (a) says nothing about half (b).** A branch died to this confusion: its change was nowhere near a guarded file, and a test three directories away lost one assertion.

**Probe the guard against your exact edit rather than inferring from a rule name.** This is the single cheapest habit in this brief. A row in the last wave left four tests red and reported "four goldens need an owner call" because it reasoned from the rule names that bumping a test constant would read as erosion. It would not: the triage built the exact change and ran the guard, which said `keel: ok, 0 findings`. `BUDGETS` in `keel-guard.py` holds only four named files' token constants; a golden count in an ordinary test file is not a budget, a cap or a ceiling. **Make the edit, run `python3 scripts/keel-guard.py --base <your base>`, and read what it actually says.** The probe costs seconds; the inference cost that row three of its four "owner calls".

**Pin `--base` to YOUR base, never `origin/main`.** Main moves under you — departures land every two hours — and the guard judged against a moved main reports **every test main ADDED as one your branch REMOVED**: about twenty bogus `test-removed` findings plus a handful of `assert-removed`, which kill the gate before a test runs. Three rows in the first wave died to exactly that. The signature of drift is that the findings name files you never touched, and are byte-identical to what any other branch would produce. **If your keel phase fails, re-judge at your own base and report BOTH results** — a drift finding and a real erosion look identical in the output, and one row blamed drift while carrying two genuine `allow-added` erosions underneath it.

**You cannot ack a finding.** An ack is a `Keel:` trailer on a **GPG-signed commit by a key listed in `scripts/keel-signers` at the base**, and you cannot sign here. The form the guard's `ACK` regex accepts is:

```
Keel: <rule> <path>[, <path>...] — <why>
```

where `<rule>` is one of the rule names above, `keel-file`, or `locked-flow`, and the `<why>` quotes **the owner's own authorising words**. A trailer that opens with anything that is not a rule name is refused by the regex. **Never invent a quote**, and **never reshape a change to dodge the guard.** If your work genuinely needs a guarded edit or an erosion, your item below tells you what to do instead.

## The locked DM golden carries a COST FOOTER, and six tokens move it

`crates/theseus-discord/src/golden/dm_flow.txt` is under `LOCKED_FLOW_PREFIXES`, and it records a **per-reply cost**:

```
-# <model> · N loops · N tool calls · $0.0584 total ($0.0584 this reply) · …
```

That figure is computed from the request's estimated tokens, and the tool table is part of the request. So **any change that grows a request — one new tool, one new schema property, one longer tool description — moves the footer**, and rewriting the golden is a `locked-flow` finding that only the owner can clear. Three rows are blocked on that single sentence as this is written. The chain is real, not hypothetical: a row last wave added two boolean schema properties of about 92 bytes each and that was enough.

**If your change could grow any request in any way, expect this.** Do not treat it as a surprise at the end:

1. Run `cargo nextest run -p theseus-discord -E 'test(dm_flow)'` **early**, before you have built anything on top of the change.
2. If the footer moved: **leave the regenerated golden STAGED and uncommitted**, draft the trailer as `Keel: locked-flow crates/theseus-discord/src/golden/dm_flow.txt — <THE OWNER'S WORDS GO HERE>` with the owner's words left as an explicit placeholder, and put the before/after footer values and a **one-line question** in `CLOUD_REPORT.md`.
3. **Land everything else.** A row blocked on one sentence should still deliver every commit that is not.

Also check `crates/theseus-core/tests/golden/private_requests.txt`, `crates/theseus-core/src/own_bench/first_request.golden.json` and `crates/theseus-kernel/tests/golden/kernel_frames.txt` for the same movement. **None of those three is locked** — they regenerate with `THESEUS_GOLDEN=write` and the guard says `ok` on bumping them. Only the `theseus-discord/src/golden/` directory is locked.

## Two hazards that cost a join gate this week

**1. A function at exactly 100 lines reddens clippy when you add one line.** `clippy.toml` sets `too-many-lines-threshold = 100` and `Cargo.toml` sets `too_many_lines = "warn"`, which the gate runs as `-D warnings`. **Thirty-two functions on main sit at or just under that line.** Before you grow a function, count it. If your addition would cross 100, **extract a helper** — do not add `#[allow(clippy::too_many_lines)]` or `#[expect(...)]` to buy the room, because `allow-added` is a keel erosion rule and it fires wherever you put it. (The `#[expect(clippy::too_many_lines, reason = "shape budget: split it")]` attributes already in the tree were added deliberately by a sweep; adding a new one is not the same act.)

**2. A whole-tree invariant goes stale when a sibling lands.** Several tests are completeness censuses: they count the call sites of an operator notice, the entries in a kind list, the files under a swept directory, and assert an exact number. **If your change adds a call site, a new file under a swept directory, or a new member of an enumerated family, it must update that census in the same commit.** Find it before you are surprised by it: `grep -rn "\.len() == " crates/ --include=*.rs | grep -i 'RULES\|KINDS\|ALL\|census'` is a cheap first pass, and the test that breaks will name itself. A row that lands without its census update turns main red for everybody at the next departure.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` (one form only; the harvester keeps it verbatim).
- You can't sign here: commit without `-S`. The maintainer re-signs: every fix commit is re-signed by cherry-picking its CONTENT onto current main, so keep each fix in its own commit with a clean message, and end it with that trailer.
- Never fold `CLOUD_TASK.md`, `CLOUD_REPORT.md` or `KEEL-PATCH.md` into a fix commit: they are dropped at the merge, and a fix commit that carries them cannot be picked clean.
- Push only your branch, `git push origin HEAD:refs/heads/cloud/p5-b7-gate-truth`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies in any Cargo.toml or package.json. If the right fix needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, or anyone else.
- Don't edit the spec, docs/status.md, or the README: the maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a fix. If time runs out mid-issue, leave it out and report what you found.

**Merge main in before you gate, if your row can afford it.** Of the ten rows in the first wave, the only one that escaped the base-drift failure was the one that had merged `origin/main` into its branch before gating. It also makes the maintainer's job smaller. If you do, say so in the report and still pin `THESEUS_KEEL_BASE` to your original base.

**Departures, and the one landing as you start:** main lands by fixed departures every two hours, and your branch rides the next one after a maintainer review. Finish and push rather than polish: a complete, pushed branch rides; a polished one still on the VM waits for the next.

**About twelve branches land on `origin/main` at roughly the hour you begin, and your base is the commit before them.** So `git fetch origin && git merge origin/main --no-edit` early is worth more in this wave than in any previous one: it is how you see code your item may depend on, and it is what saved the only first-wave row that escaped the base-drift failure. **Do it, say in your report what the merge brought in, and still pin `THESEUS_KEEL_BASE` to your original base** — the guard judged against a moved main reports every test main added as one your branch removed. **Do it twice: once after setup, and again immediately before your gate run** — the departure's gate takes a quarter of an hour, so a merge at minute 15 of your session may well pre-date it.

**The report:** when you are done, or after about 2.5 hours of work, whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. If the gate's nextest profile supports JUnit output, commit the JUnit file beside CLOUD_REPORT.md in that same report commit, on your branch only, never in a fix commit. (**As this is written, the `default` profile in .config/nextest.toml has no `junit` section**, so there will be no file unless a sibling branch has added one — one row of this wave is adding it. Check rather than assume, either way.) The maintainer reads it from the branch and drops that commit at the merge.

## Your row: the gate tells the truth: cargo deny, the keel guard, a stale spec line

**Branch:** `cloud/p5-b7-gate-truth` — push only this branch.
**Base:** `4683adb8a24e182c6123f1207f164a9486c1a1fa` — `origin/main` as of 2026-11-09 ~01:00 MST, the commit your clone starts from.

Pin the keel guard to that base, never to `origin/main`:

```bash
python3 scripts/keel-guard.py --base 4683adb8a24e182c6123f1207f164a9486c1a1fa
```

and run the gate with it pinned too:

```bash
export CARGO_INCREMENTAL=0
THESEUS_KEEL_BASE=4683adb8a24e182c6123f1207f164a9486c1a1fa THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh
```

**This row is a bundle of 4 independent items.** One commit each, worked in the order below, reported one by
one. See "This row is a BUNDLE" at the end of this brief for exactly what that means.

Four items about **checks that mislead the person reading them**. Two of them touch `scripts/keel-guard.py`, which is itself a keel-guarded file — so read the keel section of this brief twice, and expect to land those two as a drafted `KEEL-PATCH.md` with the owner's words left blank rather than as a commit. That is the designed outcome for a guarded edit here, not a failure, and a row last wave did exactly this correctly. **Items 1 and 4 are not guarded and should land as ordinary commits — do those first so the row lands something whatever happens to items 2 and 3.**

### The spec — four checklist lines of bucket row `theseus-4vsrz` (1.0.5 B7), verbatim

> - [ ] (P2) **cargo deny on main goes RED at the next advisory refresh, independently of any branch - and the two cloud reports that found it disagree about which id is which.** (reading review 2026-11-08, cloud-rows-review.md B1). Four of seven VMs ran `cargo deny` online against a fresh database and all four found the same two advisories missing from deny.toml; mm4a+zbttu and q202f+lv2u give OPPOSITE id-to-advisory mappings for RUSTSEC-2026-0429 and -0441 (zip 8.6.0 ZipArchive::extract vs pin-utils unmaintained). Two of the four are wrong and three recommend writing a line that depends on which. CONFIRM THE MAPPING AGAINST THE DATABASE BEFORE WRITING EITHER. deny.toml on main has neither id; the local advisory-db was last fetched 2026-10-24 and holds neither file, which is the only reason the gate is green (gate.sh:435-445 already prints 'the advisory database is 15 days old' every run). Both ignores are the owner's call and all four reports correctly declined to make it.
>
> - [ ] (P3) **A stale 'Known gaps' line in the spec produced a wrong cloud brief.** (reading review 2026-11-08, cloud-rows-review.md mm4a). docs/spec/19-part3-item-118.md:1274 still lists theseus-mm4a under Known gaps though Item 141 closed it, which is why mm4a's brief sent it to write a test that already existed in a fourth file. Strike the line or the next brief written from it repeats the session.
>
> - [ ] (P1) **BATCHING CAN UNACK A PROPERLY ACKED KEEL EROSION — and the obvious "fix" is to write a false owner authorisation.** Found assembling batch 7, 2026-11-07. `p5/nomatch-pack-default` deletes `the_daemons_no_match_is_the_packs` from `routing/tests_effort_open.rs` and `turn/delay_step.rs`, and **acks it correctly**: on its own branch `keel-guard.py` exits **0**, classifying it `test-removed` in those two files, covered by a signed trailer that explains the constants were hand-copied duplicates of each pack's declared value, are deleted because the live path now reads the pack, and are replaced by `every_choice_question_declares_a_no_match` — a wider bar. That is a model ack.
>   **Merge that branch together with `p5/k2sw-mute` and the guard exits 1.** Its rename detection pairs the deleted routing test with two unrelated NEW voice tests in `crates/theseus-voice/tests/mute.rs` ("renamed and rewritten"), reclassifies the finding from **`test-removed`** to **`assert-removed`**, and reattributes it to **`mute.rs:224` and `:231`** — a file that has no erosion at all. The existing trailer names the two real files, so it no longer covers the finding, and the batch fails.
>   **The trap this sets is the dangerous part.** The guard's own remedy text asks for `Keel: assert-removed crates/theseus-voice/tests/mute.rs — <why>`. A conductor in a hurry would add it, and that trailer would be **a false statement that the owner authorised an erosion in a file where none exists** — laundering a real ack into a wrong one. **Do not ack across a batch boundary. Prove where the erosion actually is first** (`git grep` the symbol at the base and at the merged head; here it exists at the base in two files and is absent at the head, which is what proved the erosion real and the *attribution* false).
>   **Workaround used:** split the batch so the two branches land separately. 7a (seven branches) keel exit 0, 5 findings acked; the voice pair alone keel exit 0, 0 findings. **Real fix: the rename pairing must not cross files whose contents share nothing**, or `test-removed` must not be silently reclassified as `assert-removed` when the pairing is cross-crate. `scripts/keel-guard.py` is itself keel-class, so that change needs the owner's words.
>
> - [ ] (P2) **A brief told a builder 'no `Keel:` trailer is needed' because the file it named is not keel-class - and that is not how the guard works.** 2026-11-08 01:4x, mine, caught by the fixer on `p5/6f8t0-card-observed`. `card/tool.rs` is indeed not in `KEEL_FILES`, but **`assert-removed` is a rule about a CLASS OF CHANGE, not a list of files**: it fires on any test anywhere in the tree that loses an assertion, and it killed the gate in **0 s before a single test ran** (`tests_route_cliff.rs:117: assert-removed: 3 assertions -> 2`). **Two different things are called 'keel' and a brief must say which it means:** (a) the guarded FILE list, which is about where you edit, and (b) the EROSION rules - assert-removed, assert-weakened, test-removed, budget and ceiling - which are about what your edit does, anywhere. A brief that clears (a) has said nothing about (b). **Wanted: the builder brief's keel section states both halves**, and the authoring note for briefs says a ruling that deletes or weakens an assertion must come with the owner's quote or be marked as needing one.

### Item 1 — `cargo deny`'s real bar: main green with no branch in flight (P1, do this first)

**Read this paragraph before you touch anything, because the last session on this line got a good result and
still missed the bar.** A previous cloud row landed a diagnostic message and an `AGENTS.md` paragraph for this
line and reported it DONE; its reviewer's verdict is that *the line's bar — main must not go red with no branch
in flight — is unmet*, and the line stays open. The id-to-advisory mapping is now settled and **confirmed from
the fetched database**: `RUSTSEC-2026-0429` is `zip` (`ZipArchive::extract` aborts on a symlink entry with a large
declared size; patched at >= 9.0.0, and `Cargo.lock` holds both 8.6.0 and 9.0.5, so 8.6.0 is the vulnerable copy) and
`RUSTSEC-2026-0441` is `pin-utils` 0.1.1, unmaintained, no patch. Two of four earlier reports had that backwards. **Do not re-derive it from memory;
confirm it from the database you fetch, and say in your report what you confirmed it against.**

What is actually wrong: `cargo deny` reads an advisory database that is fetched at run time, so the check's
result depends on **when it runs**, not on what the tree holds. Main is green today and red tomorrow with no
commit in between. That makes it useless as a gate signal and alarming as a daily signal.

The deliverable is the **decoupling**, not an ignore:

- **Do not add an `ignore` entry for either advisory.** Both are the owner's call, and all four prior reports
  correctly declined to make it. Adding one is the single thing this line forbids.
- Find where `cargo deny` runs (`scripts/deny-daily.sh`, and whatever the gate does) and make the two runs
  **different checks with different contracts**: the gate's run judges the tree against a pinned or vendored
  advisory set and so is reproducible; the daily run fetches and is allowed to report new advisories without
  reddening anything that blocks a merge. Implement whichever half is cleanly within reach and say exactly what
  the other half would need.
- Write the two owner questions out, in one place, in the form of a question: whether to ignore, upgrade, or
  vendor around each of the two advisories, with the cost of each option as you measured it (does the `zip`
  bump compile? is `pin-utils` reachable at all, or is it a transitive dev-dependency? `cargo tree -i` answers
  the second in seconds).

### Item 2 — the stale `Known gaps` line in the spec (P3, cheap, do it second)

`docs/spec/19-part3-item-118.md` around `:1274` still lists `theseus-mm4a` under **Known gaps** although Item
141 closed it — and that stale sentence is the direct cause of a wrong cloud brief, which sent a session to
write a test that already existed in a fourth file. Verify both halves in your clone (the stale line, and that
Item 141 does close it) and strike or correct the sentence.

**The standing rule in this brief says not to edit the spec** — this item is the exception and it is explicit:
this one sentence, in this one file, because a brief was written from it. Change nothing else in that document.
If on reading it you find Item 141 does **not** close `theseus-mm4a`, that is the more valuable finding: say so
and leave the line alone.

### Item 3 — batching can un-ack a properly acked keel erosion (P1, GUARDED)

Read the archived line above: it is the spec, and it names the trap that follows from it — *the obvious "fix"
is to write a false owner authorisation*. **You must not write one.** An ack quotes the owner's own words and
you cannot obtain them.

The mechanism to establish first, from `scripts/keel-guard.py` itself: acks are read from the commits in
`base..HEAD`, and the range is resolved by `resolve_range`. Work out, and **prove with a scratch repository or
a throwaway branch rather than by reading**, what happens to an ack when the acked commit is merged as part of
a batch — whether the ack commit is still in the judged range, and whether the finding it answers is still
raised. Your report's value is in that demonstration.

`scripts/keel-guard.py` and `scripts/test_keel_guard.py` are both in `KEEL_FILES`. So the landing shape is:

- **prove the correct behaviour outside the guarded files** — a reproduction script under a path that is not
  guarded, or a test in your report that anyone can re-run;
- put the one-line (or few-line) patch to `keel-guard.py` in **`KEEL-PATCH.md`** at the repository root, as a
  diff, with the drafted trailer `Keel: keel-file scripts/keel-guard.py — <THE OWNER'S WORDS GO HERE>` and the
  owner's words left as that explicit placeholder;
- **commit `KEEL-PATCH.md` on its own**, never folded into a fix commit;
- probe rather than infer: `python3 scripts/keel-guard.py --base 4683adb8` against your exact working tree, and
  quote what it actually says. An inference about what the guard "would" say is not evidence.

### Item 4 — the keel guard's own message: a brief's wrong claim about trailers (P2, GUARDED)

The fourth line above records a brief telling a builder *no `Keel:` trailer is needed* because the file it named
is not keel-class — **and that is not how the guard works**: the erosion rules (b) fire on any file anywhere,
independently of the guarded file list (a). Two builders have now been misled by the same confusion, and one
blamed base drift for a failure that carried two genuine `allow-added` erosions underneath it.

The fix is **in the guard's own output**, which is where the next person reads it:

- when the guard reports a finding, its message should make the two halves distinguishable at a glance — which
  rule fired, and whether it fired because of *where* the edit is (`keel-file`, `locked-flow`) or because of
  *what it does* (the erosion rules);
- when findings look like base drift — many `test-removed` findings naming files the range does not touch —
  the guard should **say so**, and name the cheap check (`--base <the branch's own base>`), because a drift
  finding and a real erosion are currently byte-identical in the output and that has cost gates in both
  directions.

Same guarded landing shape as item 3: prove it outside the guarded file, put the patch in `KEEL-PATCH.md` (one
file, both patches, clearly separated by item), owner's words blank. **If `scripts/AGENTS.md` is not guarded on
your base — check, do not assume — the documentation half of this item can land as an ordinary commit**, and it
should: write the two halves of the guard down where a builder reads them, with the correction that a non-keel
file is not exempt from the erosion rules.

## How this work is proved

1. **Red test first.** Write the test, run it, and watch it fail for the stated reason, *then* make the change. A test that never failed proves nothing. Quote the exact failure in your report.
2. **`nextest` filters need the module path.** `test(=bare_fn)` matches nothing, runs 0 tests and **exits 0** — a fake green. Run `cargo nextest list -E '<your filter>'` first and check it names your test, then read the positive count in the run, not only the exit code.
3. **A "zero X" bar is met by an empty set.** Whenever you assert that something is absent, also assert the positive count of what *should* be there, so an empty input cannot pass.
4. **The gate, before you claim green**, with the keel base pinned:
   ```bash
   THESEUS_KEEL_BASE=<the base hash in your item> THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh
   ```
   Run it here, in this VM. There is no lane host and no `arm.sh` here: the gate is that script. Install nextest first (`cargo install cargo-nextest --locked`, about 3.5 minutes); a cold workspace build is about 9 minutes. **Budget for it.** Two rows last wave ended with the gate never run, and both said so honestly — which was right, and also meant neither could be landed without someone else's gate.
5. **`CARGO_INCREMENTAL=0`, exported before your first cargo command** and never changed afterwards. See fact 1 at the top: flipping it mid-session rebuilds the whole workspace from scratch.
6. **Do not classify a failure as environmental without evidence.** The known-environmental set on this VM is **40 tests in four places** — 36 in the `RLIMIT_NPROC` family (`theseus-sandbox::contract` 20, `theseus-sandbox::bench spawn_100` 1, `theseusd::sandbox` **15**) and 4 in `theseus-core google::tests_morning`, which are the VM's `TZ` and pass under `TZ=America/Phoenix`. Both are listed in full at the top, and both were measured independently by more than one session. Name every failing test by its full test name, and say for each one why it is or is not yours. A past cloud report called all 35 of its gate failures environmental, and one of them was a real break that its own malformed output parse had hidden.
7. **Probe the keel guard against the exact edit, early, instead of reasoning about it.** `python3 scripts/keel-guard.py --base <your base>`. It is an early gate phase and fails in about one second — but only *after* the build, so finding it yourself is free and finding it through the gate is not. Both halves apply: the guarded file list, and the erosion rules on any file anywhere. **An inference about what the guard "would" say is not evidence; its output is.**
8. **Count a function's lines before you grow it.** The threshold is 100 and thirty-two functions on main are at or near it. Extract a helper rather than adding an `#[allow]`, which is itself a keel finding.
9. **If you add a call site, a file under a swept directory, or a member of an enumerated family, update the census that counts them, in the same commit.** A stale whole-tree census turns main red at the next departure, for everybody.

## Rules that are not negotiable

- **No person's name anywhere.** This repository is public. Write "the owner"; use the handle `zeroaltitude` only where an identity string is genuinely needed, and "collaborator" for anyone else. That covers code, comments, docs, fixtures, goldens, test data, branch names and commit messages.
- **Each fix is its own commit**, ending with the trailer `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>` exactly as written above. Commit without `-S`; the maintainer re-signs by cherry-picking each fix's content onto current main, so a commit that mixes a fix with `CLOUD_TASK.md`, `CLOUD_REPORT.md` or `KEEL-PATCH.md` cannot be picked clean. Push at every green point so nothing is lost if the session ends.
- **No new dependencies** in any `Cargo.toml` or `package.json`. If the right fix needs one, say so in the report instead.
- **Do not edit** the spec, `docs/status.md`, or the README. Where a doc should change, say what and where in the report.
- **Never erode a test to get green.** Deleting a test, adding `#[ignore]`, dropping an assertion, loosening an equality, adding `#[allow(...)]`, or raising a budget, cap or ceiling is a keel finding wherever in the tree it happens, and you cannot ack one. If a test is genuinely wrong, say so in the report and leave it standing.
- Chain commands with `&&`, never `;`.

## Your report — your last action

When you are done, or after about 2.5 hours of work, write `CLOUD_REPORT.md` at the repository root, commit it as the branch's **last** commit (subject `cloud report (not for main)`), and push. Open the file with a **one-word verdict heading** — `# LANDABLE` or `# NOT LANDABLE` — then give:

- **Row** and which bucket-row checklist item this is.
- **Branch**, the head hash you pushed, and the **base** hash you actually cloned.
- **Files** changed with `+`/`−`, and the commit count.
- **The defect, confirmed in the code**, with `file:line` — what is actually wrong in *your* clone, not what this brief assumed. **Verify every path and line number in this brief yourself: they were read on this base and main moves every two hours.** If the premise turns out to be false — the mechanism is gone, already fixed, or materially different — **stop, write that up with the evidence, and return it as an owner call rather than forcing the task.** A cheap, correct "this no longer exists" is a good outcome, not a failure.
- **The fix and why that shape**, including the alternative you rejected.
- **The proof**: the red test and its exact failure, the gate's numbers (test counts and `exit=`), and each planted revert and what it caught.
- **The keel guard's result** on your range — the exact command you ran, with `--base` — and anything you left undone because the guard would have refused it. If you left a golden staged, say which, with the before and after values and the one-line question for the owner.
- **Whether anything you changed could grow a model request**, and what `-p theseus-discord -E 'test(dm_flow)'` said when you ran it.
- **What is left, uncertain, or that this brief got wrong.** A session that corrects the brief is doing the job. Say it plainly rather than quietly working around it.

**A verdict is about this branch's own state, not about the whole item.** `# LANDABLE` with one of three sub-items finished, gated and honestly scoped is a better outcome than `# NOT LANDABLE` covering three half-done ones. If part of your work is blocked and part is not, say `# LANDABLE` and name precisely which commits land and which are held, with the reason. A row last wave was marked NOT LANDABLE when it was the cleanest branch of its wave; another was marked NOT LANDABLE over four "owner calls" that were not owner calls at all.

Your final message is short (under 1,200 characters) and starts with the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line on the item, and the gate's result.

## This row is a BUNDLE, and a bundle may land partial

Your row carries several independent items. They were bundled because each is small, not because they are one
change. **They do not depend on each other and they must not be committed together.**

- **One commit per item**, each with its own clean message and the `Co-Authored-By` trailer, so the maintainer can
  cherry-pick any subset onto current main. An item you did not finish costs the others nothing; an item folded
  into a neighbour's commit costs both.
- **Work them in the order they are listed**, which is the order of value. If you run out of budget, stop at a
  green point and report — do not start a fourth item with twenty minutes left.
- **Report per item.** `CLOUD_REPORT.md` gets a section per item, each naming its own state in one word —
  **DONE**, **PARTIAL**, **NOT STARTED**, or **PREMISE FALSE** — followed by its commit, its proof, and what is
  left. A bundle whose report says only "three of six done" without saying *which three and how proved* cannot be
  landed by anyone.
- The verdict heading is still **one word for the branch**: `# LANDABLE` if any commit on it is landable, with the
  per-item sections saying which. Do not mark a branch NOT LANDABLE because one of its six items is blocked.
- **A `~/reports/…` path inside a quoted checklist line is on the owner's machine and unreachable from here.**
  Several of the lines quoted in this brief cite review files by path. You cannot read them and you are not
  expected to: everything they establish that bears on your work has been copied into the item's own text.
  Their absence is not a blocker and is not worth a minute of searching.

- **An item whose premise has moved is a finished item, not a failure.** Write up the evidence, say PREMISE FALSE,
  and move to the next one. Several items below are follow-ups to work that landed days ago; one or more of them
  may already be fixed on your base. Check before you build.
