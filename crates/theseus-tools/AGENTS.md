# theseus-tools

Toollets (spec §3.23, §3.24): small, typed, in-process tools behind one contract (`Tool`, in `src/lib.rs`). Read by
theseus-core, whose `toolrun.rs` runs every call as a kernel action, and by theseus-sandbox.

## What's here

- `src/lib.rs`: the contract. A toollet parses and checks its input (`plan`), names the paths or argv it will touch
  so the gate can read intent, and runs in process (`run`), or describes a job for the wrapper (`proc.run` only). Its
  output is text for the model plus a structured `meta`. `Backend` says where a call runs.
- `src/fs.rs` (`fs.read`, `write`, `edit`, `patch`, `glob`, `grep`, `list`), `src/git.rs` (`git.diff`, `git.log`,
  through gitoxide, no git CLI), `src/text.rs` (`text.diff`), `src/proc.rs` (`proc.run`, the escape hatch), and
  `src/image.rs`.
- `src/paths.rs`: path resolution the gate can trust: lexical normalization first, then the symlinks of the longest
  prefix that exists. `src/net.rs`: which addresses are not public.

## Invariants

- **Native first** (§2, NATIVE FIRST). A new capability is a toollet: typed input, structured output, no shell
  parsing, no `PATH` dependence. `proc.run` is the escape hatch, and the ledger counts it as one.
- **The gate judges a path after it resolves**: `..` and symlinks never climb out of a root unnoticed.
- **The git tools stay under the roots** (Item 22). A root inside a larger repository reads only its own part;
  a `core.worktree` outside the roots is refused; every path a diff reads is checked, in the working tree and in
  history. A symlink's content is its target's path, never what it points at (Item 13).
- **Results tell the truth** (Item 21; §3.24). A capped result says what it left out, and `rest` gives the tool's
  own way to get it: a range where the tool takes one, else a narrower call. A listing (`fs.glob`, `fs.grep`,
  `fs.list`) ends with its scope and what it left out.
- **`fs.patch` refuses a deletion that doesn't show every line it removes.**
- **A tool reads only regular files, through a cap** (review 2's R9): `fs::read_regular`, never `fs::read`. A FIFO,
  a socket, or a device is refused by name, since a read of one waits for a writer and holds a pool core and a
  thread for as long as it waits. It is opened without blocking and checked again once open.

## Tests

- Each module's tests. A new reader of outside text also gets property tests, as core's readers have
  (`tests_outside_text.rs`, Item 13).
- A fixture that runs the git CLI sets `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, so it never
  inherits the operator's config: a global `commit.gpgsign` once failed the gate with a locked agent (Item 7).

## Traps

- A tool's wire name replaces each dot with an underscore (`fs.read` is `fs_read`), by the provider's tool-name rule.
- A new tool also takes its line in the config template's `[policy.tools]`, and its posture shows on every surface.
