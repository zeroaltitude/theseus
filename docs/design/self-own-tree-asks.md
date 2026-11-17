# A self builder is not stopped at the owner for working in its own folder, and a question does not outlive its build (issue 20)

Two lines of issue 20, found on 2026-11-17 from one unattended build. Its code is `rsi/own_tree.rs`
(`l1_of`, `l1_instead_of_floor`), `policy.rs` (`floor_is_only_the_daemons_binary`), `rsi/pipeline/parked.rs`
with `queues.rs`, `rpc/question_waits.rs` and one check in `rpc/confirms.rs`. The earlier, related line is
`self-own-removals.md`: the same three places (a self session's working copy, build folder and review
scratch), a different hold.

## What happened

A builder, whose working copy is `<state>/self/trees/<task>`, ran
`bash -c "cd <its tree>; rustfmt ...; cargo test -p theseusd --test <its test> ..."` with no `sandbox` key.
The call was held for the owner (`floor: true`, "it touches Theseus's own state or a secret, which always
asks") and the build's turn ended waiting on the answer. The owner was reminded every fifteen minutes for an
hour and a half. Meanwhile the pipeline read the waiting execution as a build that had ended with no
branch, failed the item and removed its working copy; the daemon was restarted and another item took the
one slot. When the owner approved, 98 minutes after the question, the call failed ("working directory does
not exist"), the session woke, made five more model calls for nothing and went back to waiting.

## Why the floor held it (it was not the path)

The first reading of this was that the floor asks about any command that names a path under the state
directory. It does not. The floor's paths (`toolrun/guarded.rs`) are the stores and spools, the bindings
file, the owner's instruction file, the config and its kept copy, the credentials' token and the sealed sets;
the self trees are workspace roots, never floor. The ledger's own reason for the held call ends "floor:
`theseusd` is Theseus's own binary or the 1Password CLI", and that is the cause: the floor also holds a call
that runs `theseusd` or `op`, and it reads a command line at every position a program could stand in, with
no program name consulted (`policy::runs_program`). `-p` is not one of the flags that hide the word after
it, so in `cargo test -p theseusd` the package name is read as the program. Any session's `cargo ... -p
theseusd` was held. Tests: `policy::tests::the_package_name_after_dash_p_is_what_puts_a_builders_test_run_on_the_floor`,
and `toolrun::tests_undo::self_floor::naming_its_own_tree_alone_is_no_tie_to_the_floor` (the tree's path alone
is no tie).

It ran at L0. A call is bound for L1 only by `[sandbox] default = "l1"`, an `l1_argv` entry, or `sandbox:
true`; a self builder's brief asks for `sandbox: true` (`rsi/backlog/brief.rs`) and this one omitted it. An L1
call is never read by the floor (`sandbox::l1_decision`: notify, `floor: false`), because its view hides
every floor path, the socket and the credentials, and only the item's build folder persists.

## Line 1: what a self builder's call does now

A self builder's `proc.run` with no `sandbox` key, whose ONLY tie to the floor is the program `theseusd`
(`ToolPolicy::floor_is_only_the_daemons_binary`: no floored path as a resource or in any word of any line it
runs, no other floored program, no line the floor ran out of reading), is bound for L1 instead of asked
(`own_tree::l1_instead_of_floor`). It is a move, not a lowering: the floor still holds at L0 as it did. The
class is read through one function (`own_tree::l1_of`) by the gate (`sandbox::unbrokered`), the reversibility
layer (`toolrun/reversible.rs`), a batch (`toolrun/batch.rs`) and the removal rule (`rsi/own_removals.rs`),
so they agree on it. The move needs: the tool is `proc.run`; the call carries no `sandbox` key at all (`false`
included); L1 is not refused on this machine (a root daemon); the session is of the self principal; and its
item's tree is open (a builder: `own_tree::builder_path`), the same test that gives it the build folder.

Still asked: a person's, a task's and a job's call; a self call that also runs `op`; any word of any line that
names the store, the spool, the bindings file, the instruction file, the config, the token or the sealed sets
(as a word or as an option's value); a call the floor ran out of reading; `sandbox: false`; a self session
with no open tree. A moved call runs in the builder's view like any sandboxed call of it: the workspace is an
overlay whose writes are thrown away, only the build folder persists, and its network is the package
registries; a shell line that edits the working copy by itself does not keep the edit (the file tools do).

## Line 2: a question does not outlive its build

`rsi/pipeline/parked.rs`.

- **A parked build is building.** A build whose turn ended on a question has its execution `Waiting`. The
  queues read that as ended; now an in-progress item whose execution has an open question, while its working
  copy is open, is in the building queue, holds its slot, and shows `waiting on you: <what> (answer it with
  `theseus answer <id>`)` in `theseus self pipeline`. A parked build whose working copy is gone is a build
  that ended, as before.
- **A question is withdrawn when its build is not building.** Every tick, after the pipeline's moves, each
  open question of a session that belongs to a self item and is not the question of an entry of the building
  queue is withdrawn: the item failed, was set aside, dropped, retried or done, its working copy was closed
  or removed, or a newer start of the same item superseded the session. The call is declined by `withdrawn`
  in the kernel's frame (`declined by withdrawn: no longer needed: the build it belonged to has ended`,
  which `outbox::Closed::of` reads as a withdrawn card), its clients hear `confirm.resolved` with
  `withdrawn`, its card is edited to say so, its reminder count ends with it (`forget_noticed`), and the
  execution it parked is stopped, so nothing waits on it. Questions of any other session are never read.
- **A late answer is refused and recorded.** `Core::confirm_action_with` refuses an answer to a withdrawn
  question with the words "that question was withdrawn (...), so this answer is late: nothing was run, nothing
  was spent and nothing was woken", recorded as an `approval.refused` row. It binds nothing, runs nothing,
  spends nothing and wakes nothing.
- **A restart** changes none of it. The question, the item and the working copy are on disk; a parked build is
  read as parked again; a question whose working copy went while the daemon was down is withdrawn by the new
  daemon's first tick. Never a live question for a dead build.
- **Reminders.** A waiting question is told at one life (`[kernel] confirm_ttl_secs`, 15 minutes) and at two
  (`question_waits`), then once an hour, for a self build's question only; the others keep one a life. The
  words say which change waits, what it wants, how long it has waited, how to answer and how often they come.

## What is not done

- The tick that withdraws is the pipeline's, which runs while self-improvement is `act` and not halted. A
  question of a self build left while the loop is off is withdrawn at the next tick after it is on again.
- A review session's questions (a review's scratch is not an item's working copy) are not read.
- A self builder's L0 command that does not run `theseusd` or `op` is not held by the floor and runs at L0 at
  notify, as it always did: its working copy is a workspace root. Moving every builder command into L1 would
  discard a shell line's edits to the working copy; that is the owner's call, not this change's.
