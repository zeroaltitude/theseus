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
   view a `proc.run` is given (`sandbox::for_job`, from `Sandbox::job_view`) binds none: the workspace roots are
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
   operand its own. An `argv` has no shell, so an operand is the literal path; a call that sets `env` could make `rm`
   a program of its own, so it is not read. A shell line (`bash -c`), a batch of `steps`, `git worktree remove`, `mv`,
   `git clean` and every other program are held exactly as before: the code cannot say what they remove.

**Its own** is: strictly inside its working copy (`<state>/self/trees/<its task>`, never the copy itself, which the
pipeline holds); its review scratch (`<state>/self/review/<slug>`, the directory and all inside it, registered for the
session that runs the review by `CheckTurn` and removed from the registry when the review answers; a call can never
name it, and an entry outside `<state>/self/review` is ignored); or strictly inside the system temporary directory,
except the state dir, the self repo, the sealed set, the workspace and the roots, and anything that holds one of them
(a sweep over a parent). Every path is resolved before it is compared (`..` and links are judged where they land).

## What a self session still cannot remove without a hold

Anything outside those places: the owner's checkouts and home, the store and the rest of the state dir, another
item's working copy, the self repo, the trees root and the reviews' root. And, anywhere, a removal written as a shell
line, a batch, or a program other than `rm`; and a sandboxed call with a network list.

## Not yet: the builder's build folder

`p8/sl-builder-tests` binds an item's build folder read-write for its session's `proc.run` in L1. It is the
session's own by construction, and the base view stays read-write-free, so the L1 rule above still holds for it. A
removal by `rm` there, at L0, is not in the places above: when that branch lands, `own_removals::Own::of` takes one
more place (`own_tree::builder_path`).
