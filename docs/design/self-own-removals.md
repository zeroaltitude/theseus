# A self session removes what is its own (issue 20)

The reversibility layer (`undo.v1`; `docs/design/undo-v1.md`) holds a call that would lose something nothing
holds a copy of, and its one-way floor forces that hold on a plain file write. It was built for a person's files.
For the self loop's own scratch it protects nothing: on 2026-11-14 the pipeline's reviewer was not allowed to
remove the checkout it had made, and that checkout held the pipeline's only slot for thirteen hours.

The owner, 2026-11-15 17:39:14 (chat message 1571686049867112519): "Removals should be possible, which seems like a
small change. Generally, this sandbox is too stringent, so, I will tend to favor smart relaxing of the standards
here to allow for maximum self improvement power".

The code is `crates/theseus-core/src/rsi/own_removals.rs`, called from `ToolRuntime::judge_reversibility` right
after 0d (`toolrun/reversible.rs`), with the network list from `rsi/deps/hosts.rs`. Its tests are `toolrun/tests_undo/self_own.rs` (through
a whole core) and `rsi/own_removals/tests.rs` (the `rm` grammar).

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

1. **A program call in L1 with no host beyond the self builder's network.** What persists from an L1 call is what its
   view binds read-write. The view a `proc.run` is given (`sandbox::for_job`, from `Sandbox::job_view`) binds none,
   and a self builder's command is then given exactly one more read-write bind, its own item's build folder
   (`Sandbox::with_builder_folder`, which `own_tree::builder_folder` offers only to a self session;
   `docs/design/self-builder-build-folder.md`), which is the only thing that persists from it and is the session's own
   by construction. Otherwise the workspace roots are overlays whose writes go to scratch and are thrown away, `/tmp`
   is the job's own, and the floor (the store, the spool, the bindings, the credentials, the socket, the sealed sets)
   is hidden. The only things that stay are the job's output, which the daemon spools in its own record, and nothing
   else. `own_removals::sandbox_keeps_nothing` reads the base view, so a read-write bind added to it turns this off by
   itself. **This is the half that covers a shell line**, which nothing else here can read:
   `rm -rf target/x && cargo build` run in the sandbox removes only what the view lets it, and nothing outside the
   build folder stays. The view the pipeline's own gate runs get (`sandbox::self_build_view`) is not a `proc.run` view
   and is not read here.
2. **`fs.write`, `fs.edit` and `fs.patch`**, the floor's own closed table of tools whose whole effect is the paths
   their plan names, when every path they write is the session's own.
3. **`rm` as a `proc.run` `argv`**, with no `env`, flags from `-r -R -f -v -d` and their long forms, and every
   operand its own. An `argv` has no shell, so an operand is the literal path. The program must be the system's
   `rm`: `/bin/rm`, `/usr/bin/rm`, or a bare `rm` that is the first on the `PATH` a job is given (`job_base_env`) and
   resolves to one of them; a `PATH` with an empty or relative entry fails closed, and a call that sets `env` is not
   read. A shell line (`bash -c`) outside the sandbox, a batch of `steps`, `git worktree remove`, `mv`, `git clean` and
   every other program are held exactly as before: the code cannot say what they remove.

**Its own** is: strictly inside its working copy (`<state>/self/trees/<its task>`, never the copy itself, which the
pipeline holds); strictly inside its item's build folder (`<state>/self/builder/<its task>`, from
`own_tree::builder_path`: named by the session's task and never by a call, and only while the item's tree is open);
or its review scratch (`<state>/self/review/<slug>`, the directory and all inside it, registered for the session that
runs the review by `CheckTurn` and removed from the registry when the review answers; a call can never name it, and an
entry outside `<state>/self/review` is ignored). **Nothing under the system temporary directory is its own**: on a
shared machine it holds other lanes' worktrees and the owner's files (the first version of this change had the
temporary directory as a place, and the reviewer showed what that reached; Tabitha, 2026-11-15 20:10, took it out). What
a session needs a temporary directory for has one of its own: a builder's sandboxed commands have
`TMPDIR=<build folder>/tmp` (`rsi::builder_dir::tmp_in`), and a review's commands outside the sandbox are given
`TMPDIR=<its scratch>/tmp` (`own_removals::review_tmp`, set in `toolrun/job.rs`; the review's prompt says so), which
goes with the scratch when the review has answered.

**How a path is judged.** The file tools act on the path as `paths::canonical_best_effort` resolves it (links
followed, `..` applied lexically), so the judgment and the act agree and a link or a `..` that leaves is judged
where it lands. **`rm` is judged as the kernel acts, which differs**: the kernel follows a link and then applies a
`..` that comes after it, where the resolver drops the `..` first, so `<link>/../x` would be judged beside the
resolver's own place and removed beside the link's target. The first review of this change found it. So an `rm`
operand with a `..` in it is not read (held as before), and an operand is judged as its parent resolved plus its last
part as written, because `rm` removes a link and not what it points at. The exception is an operand that ends in a
`/`: `rm -r` follows a link written that way and empties its target, so such an operand is not read either (the
second review found that the first fix had opened it).

## This removes network reach from self builds, and why

A self session's sandboxed command used to carry the operator's `[sandbox] egress` (on the owner's machine,
`*.amazonaws.com:443`), like any session's. **It no longer does.** The gate gives a self session's L1 call the self
builder's network instead: `rsi::deps::hosts::egress()`, the package registries (`index.crates.io`,
`static.crates.io` and `registry.npmjs.org`, port 443) and nothing else; a self session that is not a builder (its
item's tree is not open, a review for one) is given no network at all. That one list in code is also what the
dependencies change appends for a builder and what `builder::operator_list` returns (it is now a replacement, not an
addition to the operator's own entries); a registry added later is added to `HOSTS` there and nowhere else, and the
gate, the view, the outside-text marker and this rule all follow. The reason is the rule above: a call with a network
is one the rule leaves to the model, because a job with a network and a credential could remove something that is not
on this disk, so with the operator's list on every call no sandboxed command of the builder (`rm -rf target/x &&
cargo build` is its ordinary way of working) could be lowered, and "removals should be possible" would have been true
only on a daemon with no `[sandbox] egress`. A self build holds no secret and uses none of that reach, and the gate of
a self branch runs in a view of its own that has no network at all, so what is lost is reach a builder session had and
did not need. What is not changed: a person's, a task's and a job's L1 calls keep the operator's list; a self call
that **names a host of its own** (`sandbox: { egress: [...] }`) still gets it, with the notice any call that names a
host beyond the list gets, and is not lowered (`sandbox_keeps_nothing` reads the list the gate gave and wants every
entry to be one of the registries).

**What the registries' list does and does not restrict.** The egress proxy serves `CONNECT` only: it matches the
tunnel's `host:port` against the list (a glob on the host, the exact port), refuses any name that resolves to a
non-public address, and copies bytes. It never reads what a tunnel carries, so it cannot limit what is done over an
allowed tunnel: it restricts WHERE a command may connect, not which HTTP method it uses or what it sends. The list is
therefore "the package registries", not "fetch-only". What stands between a self command and a write to a registry is
that the command holds no credential for one (a self build has no secret), and that these registries refuse an
unauthenticated write.

## What a self session still cannot remove without a hold

Anything outside those places: the owner's checkouts and home, the store and the rest of the state dir, the system
temporary directory and what other lanes keep in it, another item's working copy or build folder, the self repo, the
trees root, the builders' and the reviews' roots, and the working copy or build folder itself. And, outside the
sandbox, a removal written as a shell line, a batch, or a program other than `rm`; and a sandboxed call that names a
host of its own. A self session's held call says what does run (`own_removals::what_runs`, appended to the hold's
words for a self session only): `rm` as an argv on its own paths, the file tools, any sandboxed command, and that a
review's scratch is removed for it when it has answered.

## What is not done

- **The gate resolves at one moment and `rm` acts a moment later.** Two coordinated calls of one response could swap
  a directory for a link between them.
- **A shell line outside the sandbox stays held** for a self session. The builder no longer needs one (its commands
  run in the sandbox), and a review's scratch is removed for it by the pipeline when it has answered.
