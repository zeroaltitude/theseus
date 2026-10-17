# CLOUD_REPORT: cockpit-task-page (theseus-jm46q)

## Step 1: a task's page in the cockpit (ca3258f1)

**Found.** `main` had no `joinable` or `task_redirect_line` in `theseus-protocol/src/work.rs`, so I added both exactly
as the contract gives them (`NeedsYou` is the state the contract calls "blocked"). The deck already showed a `task`
pill and "task of …", but no task-specific header, and its Composer always posted to the page's own session.

**Changed.**
- `crates/theseus-protocol/src/work.rs`: `joinable`, `task_redirect_line`. Tests in `tests_work.rs`.
- `cockpit/src/lib/taskpage.ts` (new, pure): `taskContext`, `viewOfTask`, `composerPlan`, plus TypeScript copies of
  `joinable` and the redirect line, with the same words. The short id is the last 6 characters, as Discord's
  `board_v2::short` has it.
- `cockpit/src/components/TaskHeader.tsx` (new): title, state, doing, forecast, cost / limit, Stop, back to the parent, inspector.
  Shown only for `kind == task` with a parent.
- `Composer.tsx`: a `task` prop. A running task is joined (a `turn.submit` to the task's session) with the new placeholder. An ended
  one posts to the parent, behind the shared line.
- `SessionDeck.tsx`: wires them in. The task's work view comes from `useThreadBoard` (the page's one `work.watch`),
  and the parent's title from `session.history`.
- `cockpit/AGENTS.md`: a bullet for the page.
- `lib/rpc.ts` needed no change.

**Proved.**
- `npm test` in `cockpit/`: 448 pass, 0 fail (9 new in `test/taskpage.test.ts`). `npx tsc -b` is clean. `npm run build` ok. `npm run lint` has no warnings in the new files.
- `cargo test -p theseus-protocol --lib`: 130 pass, including the two new tests. `cargo fmt --all --check` ok, `scripts/shape.sh` ok, `cargo clippy --workspace --all-targets -D warnings` clean, `tests_registry` 15/15.
- Planted reverts:
  - The ended-task Composer posts to the task instead of the parent: `an ended task's composer posts to the parent with the shared line` fails. Restored and touched.
  - `taskContext` drops its `kind === 'task'` check: `the task header is for a task session only` fails. The first version of the test missed this because its conversation fixture had no parent. I added a fixture with a parent, and it now catches it. Restored and touched.

**Live check for the maintainer** (needs a daemon with a model):
1. Start a scratch daemon, open a conversation, and start a task from it. Open `/session/<task id>` in the cockpit. The header should show the title, a running state, the doing label, cost and Stop. "back to <parent title>" should land on the conversation, and "inspector" on `/session/<task>/turn/<turn>`.
2. Type in the Composer while the task runs. The placeholder should read "Message this task…". The message goes to the task (stays queued unless task-join-core has landed).
3. After the task reports, the Composer should read "Send to main about this task…". A send should appear in the parent's transcript starting `[Written to the task “…” (…xxxxxx), and the task has reported, so this goes to main]`.
4. A conversation's page should show no TaskHeader.

**Uncertain.**
- "Ended" comes from the work view's state (not joinable), with the session's `execution_state` as a fallback. I did not check which strings `execution_state` takes for a finished task.
- Stop is `task.cancel` (confirmed first). The deck's own Header still has its Stop and Cancel.
- The cockpit has no DOM test harness (`npm test` runs pure modules), so "the header renders only for a task" is tested through `taskContext`, which gates it, not by rendering.
- The forecast pill uses `min_p50` and `usd_p50` and says "usually ~N min".
- The ended-task wording "the task has reported, so this goes to main" is mine. Discord's is "the task is over", and the maintainer may want one shared string.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in `keel` (and so `door`)**, not in anything this change touches. The keel guard found 3 erosions, in commits that are not mine, and said it refused three owner acks on d424da19e4, 238ecead06 and 1c2beb2747 because "its signature does not verify (%G? is E)". The VM has no signing keys, so no ack can verify.

I ran the other phases by hand instead: fmt, shape, workspace clippy with `-D warnings`, `tests_registry`, the protocol tests, and the cockpit's lint, test and build. All passed. I did not run the full nextest suite, the L1 tests, `cargo deny` (no dependency changed; the fetch ran but the check did not), or any bench. No test failed that I know of.
