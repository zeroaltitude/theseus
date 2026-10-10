# theseus-tools

Toollets (spec §3.23, §3.24): small, typed, in-process tools behind one contract (`Tool`, in `src/lib.rs`). Read by
theseus-core, whose `toolrun.rs` runs every call as a kernel action, and by theseus-sandbox.

Key modules: `fs.rs`, `git.rs`, `proc.rs`, `paths.rs`. Read by: core.

## What's here

- `src/lib.rs`: the contract. A toollet parses and checks its input (`plan`), names the paths or argv it will touch
  so the gate can read intent, and runs in process (`run`), or describes a job for the wrapper (`proc.run` only). Its
  output is text for the model plus a structured `meta`. `Backend` says where a call runs.
- `src/fs.rs` (`fs.read`, `write`, `edit`, `patch`, `glob`, `grep`, `list`), `src/git.rs` (`git.diff`, `git.log`,
  through gitoxide, no git CLI), `src/text.rs` (`text.diff`), `src/proc.rs` (`proc.run`, the escape hatch), and
  `src/image.rs`.
- **What `fs.read` reads**: text with line numbers; an image as an image (`Media::Image`); a PDF as its pages,
  `pages` (`3`, `3-5`, `21-`), up to 20 a read and the first 20 without one, cut out as a PDF of their own with
  their text (`Media::Pdf`, through theseus-files' capped converter; `pdf_pages` in `src/docs.rs`, which
  `http.fetch` and `file.read` share; theseus-c9l6); a document (Word, Excel, PowerPoint, OpenDocument, EPUB, RTF, a
  notebook) as its text by section, and an archive as its list, `pages` counting sections (`docs::read_doc`). A
  PDF may be 32 MiB, any other file 16, whole. A text file over 16 MiB is read by window when the call gives
  `offset` or `limit` (`src/fs_window.rs`, theseus-ywdd): lines streamed from the start, the scan bounded at 256 MiB
  (`MAX_SCAN_BYTES`, about 0.2 s warm), the window's bytes at `max_read_bytes`, each line kept to 8 KB while it is
  scanned. The scan bound ends a line too, so a file of one huge line costs the bound, not the file. A head with a
  NUL, an image, or a kind `theseus_files::kind::sniff` names (archive, document, notebook, RTF) gets the old
  refusal, worded for whether the call asked for a window. The runtime stores what a tool returns in the blobs (`run_with_media`, and
  `run_async_with_media` for an async tool).
- **`proc.run`'s `steps`** (theseus-7gir.3): a batch in place of `argv`, exactly one of the two, at most
  `MAX_STEPS` (16). `Tool::steps` gives each step as the call it would be alone (the gate judges each), and
  `Tool::jobs` each step's job, every directory checked before the first starts; the plan's `steps` holds each argv.
- **A job's handle** (theseus-n8gk): `proc.run`'s `background: true` (one program, never `steps`), and `src/jobs.rs`,
  the schemas and plans of `job.read`, `job.wait` (reads) and `job.stop` (a run). Their plans read the input alone;
  the core finds the job in the calling session, makes `job.stop`'s plan the job's own run, and runs all three.
- `src/recount.rs`: `fs.patch` rewrites each hunk header's lengths from its body before diffy parses it
  (theseus-inw), keeping its starts, so diffy still checks the context; an empty line inside a hunk is blank context,
  and the empty lines that end it are its end unless its header counts them.
- `src/paths.rs`: path resolution the gate can trust: lexical normalization first, then the symlinks of the longest
  prefix that exists. `src/net.rs`: which addresses are not public (the one classification: the core's resolver
  and L1's proxy both read it, since 18c), and an egress list's entry, `Allow` (`host:port`, a glob on the host).

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
