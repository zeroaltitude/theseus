# A self session removes what is its own (issue 20)

The reversibility layer (`undo.v1`; `docs/design/undo-v1.md`) holds a call that would lose something nothing
holds a copy of, and its one-way floor forces that hold on a plain file write. It was built for a person's files.
For the self loop's own scratch it protects nothing: on 2026-11-14 the pipeline's reviewer was not allowed to
remove the checkout it had made, and that checkout held the pipeline's only slot for thirteen hours.

The owner, 2026-11-15 17:39:14 (chat message 1571686049867112519): "Removals should be possible, which seems like a
small change. Generally, this sandbox is too stringent, so, I will tend to favor smart relaxing of the standards
here to allow for maximum self improvement power".

The code is `crates/theseus-core/src/rsi/own_removals.rs`, called from `ToolRuntime::judge_reversibility` right
after 0d (`toolrun/reversible.rs`). Its tests are `toolrun/tests_undo/self_own.rs` (through a whole core) and
`rsi/own_removals/tests.rs` (the `rm` grammar).

## What it does, and what it cannot do

It lowers a **hold** (`irreversible_reshapeable`, whether the model chose it or the floor forced it) to
`reversible`, for a session of the self principal only, when every place the call can remove from is the session's
own. It runs after the refusals (the sealed set, the place rule, plan mode, the injection freeze), which are
untouched, and it never touches `irreversible_final`, which still raises the call to the owner. Another principal's
calls (a person's session, a task, a job) are not read by it. `[judge] reversibility` and every config default are
unchanged. The model is still asked; only what the gate does with a hold changes. A lowered hold is said in the
daemon's log (`a self session's own removal is not held`) and nowhere in the ledger: the judgment row is the
model's own answer, unchanged, so a row that says "reshapeable" beside a call that ran is how to find one.

Three kinds of call are covered, each because the code can say what it removes:

1. **A program call in L1 with no network.** What persists from an L1 call is what its view binds read-write. The
   view a `proc.run` is given (`sandbox::for_job`, from `Sandbox::job_view`) binds none, and a self builder's command is then
   given exactly one more read-write bind, its own item's build folder (`Sandbox::with_builder_folder`, which
   `own_tree::builder_folder` offers only to a self session; `docs/design/self-builder-build-folder.md`), which is the
   only thing that persists from it and is the session's own by construction. Otherwise the workspace roots are
   overlays whose writes go to scratch and are thrown away, `/tmp` is the job's own, and the floor (the store, the
   spool, the bindings, the credentials, the socket, the sealed sets) is hidden. The only things that stay are the
   job's output, which the daemon spools in its own record, and nothing else. `own_removals::sandbox_keeps_nothing`
   reads the live view, so a read-write bind added to the base view turns this off by itself. The view the pipeline's
   own gate runs get (`sandbox::self_build_view`) is not a `proc.run` view and is not read here. A call with an
   egress list (its own, or `[sandbox] egress`'s) is **left to the model**: a job with a network and a granted
   credential could remove something that is not on this disk.
2. **`fs.write`, `fs.edit` and `fs.patch`**, the floor's own closed table of tools whose whole effect is the paths
   their plan names, when every path they write is the session's own.
3. **`rm` as a `proc.run` `argv`**, with no `env`, flags from `-r -R -f -v -d` and their long forms, and every
   operand its own. An `argv` has no shell, so an operand is the literal path. The program must be the system's
   `rm`: `/bin/rm`, `/usr/bin/rm`, or a bare `rm` that is the first on the daemon's own `PATH` and resolves to one of
   them (the `PATH` starts in the owner's own directories on some hosts, and a call that sets `env` could make `rm`
   a program of its own, so it is not read). A shell line (`bash -c`), a batch of `steps`, `git worktree remove`, `mv`,
   `git clean` and every other program are held exactly as before: the code cannot say what they remove.

**Its own** is: strictly inside its working copy (`<state>/self/trees/<its task>`, never the copy itself, which the
pipeline holds); strictly inside its item's build folder (`<state>/self/builder/<its task>`, from
`own_tree::builder_path`: named by the session's task and never by a call, and only while the item's tree is open); its review scratch (`<state>/self/review/<slug>`, the directory and all inside it, registered for the
session that runs the review by `CheckTurn` and removed from the registry when the review answers; a call can never
name it, and an entry outside `<state>/self/review` is ignored); or strictly inside the system temporary directory,
except the state dir, the self repo, the sealed set, the workspace and the roots, and anything that holds one of them
(a sweep over a parent).

**How a path is judged.** The file tools act on the path as `paths::canonical_best_effort` resolves it (links
followed, `..` applied lexically), so the judgment and the act agree and a link or a `..` that leaves is judged
where it lands. **`rm` is judged as the kernel acts, which differs**: the kernel follows a link and then applies a
`..` that comes after it, where the resolver drops the `..` first, so `<link>/../x` would be judged beside the
resolver's own place and removed beside the link's target. The first review of this change found it. So an `rm`
operand with a `..` in it is not read (held as before), and an operand is judged as its parent resolved plus its last
part as written, because `rm` removes a link and not what it points at. The exception is an operand that ends in a
`/`: `rm -r` follows a link written that way and empties its target, so such an operand is not read either (the
second review found that the first fix had opened it). A bare `rm` is accepted only when the first `rm` on the
`PATH` a job is given (`job_base_env`) resolves to the system's, and a `PATH` with an empty or relative entry fails
closed.

## What a self session still cannot remove without a hold

Anything outside those places: the owner's checkouts and home, the store and the rest of the state dir, another
item's working copy, the self repo, the trees root and the reviews' root. And, anywhere, a removal written as a shell
line, a batch, or a program other than `rm`; and a sandboxed call with a network list.

## What is wider than "its own", and what is not done

- **The temporary directory is the owner's own scope for this change, and it is wide.** The daemon runs as the owner's
  user over a `/tmp` that every agent and shell on the machine shares, which holds files that are nobody's scratch
  (a shell's history, other lanes' worktrees with uncommitted work). The rule keeps out the state dir, what the
  sandbox hides, the self repo and the workspace roots, and nothing else there. It only matters when a self session
  names a path it did not make. A narrower rule would give the self loop a temporary directory of its own (a
  per-session `TMPDIR` under one directory, removed when the item ends) and take the clause out of `Own::holds_resolved`.
- **The L1 rule never fires where `[sandbox] egress` names a host**, because every L1 call then carries a list.
  That is conservative, and a daemon so configured gets the file-tool and `rm` rules only.
- **A model is not told that the `argv` form of `rm` runs**, and the hold's text for a shell line is unchanged.
- **The gate resolves at one moment and `rm` acts a moment later.** Two coordinated calls of one response could swap
  a directory for a link between them.

## Choices made without asking (the owner's word of 2026-11-15 17:53, decide and notify)

The temporary directory is as wide as the brief says, and the build folder is a place beside the working copy and the
review scratch. Narrowing the first is one clause of `Own::holds_resolved`; a per-session `TMPDIR` under one directory
would let it go.
