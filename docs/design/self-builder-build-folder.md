# The self builder's build folder

Issue 20 of the self-improvement loop (the line "The builder cannot run the tests of what it writes"). The self
builder is a model session whose `proc.run` commands run in L1, where everything a command writes lands in scratch
(1 GB by default) and is thrown away when the command ends. A test build of the core is several gigabytes, so the
first real `cargo test -p theseus-core` filled scratch (`No space left on device`), and every command would have
started from nothing even had it fit. The builder went ahead on `cargo check` alone and handed over tests it had
never run, and each mistake cost a full check and one of its tries.

The owner's decision (2026-11-15 15:17:49, chat message 1571650461218373648, "A sounds great"): each item gets a
build folder the builder keeps using while it works on that item, about 10 GB of disk per item, removed when the
item ends. That is his yes to that disk use. It is not a yes to raising `[sandbox] scratch_mb`, and nothing here does.

## What the builder gets

While an item's tree is open, each of its builder's sandboxed commands gets ONE directory that is writable and
persists between commands: `<state>/self/builder/<slug>` (`rsi/builder_dir.rs`), with a `target/` directory for
cargo (`CARGO_TARGET_DIR`) and a `tmp/` directory for temporary files (`TMPDIR`); each replaces any value the call set,
and both are made again at each command's start (a `cargo clean` empties `target/`). A second `cargo test` reuses the
first one's build.

- **Whose.** The folder is named by the session's own task (`rsi/own_tree.rs`, `builder_path`), never by the call's
  input. The view that binds it (`sandbox/builder_view.rs`, `Sandbox::builder_view`) is made only for a self
  session's L1 command whose item has an open tree. Another principal's call, a self session's call at L0, and a self
  session with no open tree get the view they always had, and no folder is made for them.
- **What the view is.** An ordinary L1 job's view (`Sandbox::job_view`: the same workspace overlays, read-only
  paths, hidden list less a mask over a directory that holds the folder, and no network beyond the call's own
  egress list) plus that one read-write directory, plus four allocations of its own (see "The
  allocations of the builder's view"): the first real run died of `SIGXFSZ` on the link of the core's tests at the
  64 MiB default. It gets none of the facilities a self build's
  gate is given (`self_build::FACILITIES`). The configured `[sandbox]` caps are as they were.
- **Apart from the gate.** The gate builds in `<state>/self/target/<slug>` (`Tree::target`), in the view
  `Sandbox::self_build_view` makes, which binds the tree and that build tree read-write and nothing under
  `<state>/self/builder`. So what a model compiled is not in the view a verdict is reached in: a gate that wanted
  to read it could not name it. Tests: `tests_jail::the_gates_view_never_binds_what_the_builder_compiled` (the
  view's binds), and theseusd's `self_build::the_gates_view_cannot_see_what_the_builder_left` (a real L1 gate job
  looks for a marker the builder left, and the gate's build tree holds nothing).
- **Not a slot.** The folder is beside `<state>/self/trees`, never under it: a directory there counts as a tree in
  use (`capacity::trees_open`, `join/scratch.rs`).
- **On the disk the capacity check reads.** `capacity::check` is given a path under `<state>/self` and compares its
  free space with `[self] min_free_gb`, so the folder's bytes are bytes that check sees taken. A test holds the
  folder on the same device as the path it reads.
- **Removed with the item.** `tree::close` removes it (`close` and `close_keeping_target` both), `tree::open` removes
  any folder of the same name before the new tree starts (an item never inherits what an earlier one compiled), and
  the stale-item cleanup (`pipeline::clear`, found by `queues::closed`) removes one a closed tree left.
- **Bounded.** `builder_dir::MAX_BYTES` (48 GiB), counted as `du` counts it (blocks). A command is refused when the folder
  has reached it, in words the builder reads (what happened, that nothing was wrong with the command, and to run
  `cargo clean`). A command already running when the folder reaches it is stopped by the daemon
  (`Core::stop_jobs_over_builder_bound`, called from the heartbeat's timer beside `stop_jobs_below_floor`), and its
  result reads "stopped by the build folder's limit". Neither is a silent truncation. The disk floor (`[server]
  disk_floor_mb`, which stops every job) and `[self] min_free_gb` (which holds a start) are still behind it.

## The allocations of the builder's view

The owner, 2026-11-15 17:39:14 (chat message 1571686049867112519): "I think that in all of these, the constraints
should be relaxed to offer more flexibility for the self builder. larger disk allocations, etc. since our current
mode is one-at-a-time self improvements, it means we can have relatively generous disk allocations and file
allocations. so please extend those for flexibility over safety. Removals should be possible, which seems like a
small change. Generally, this sandbox is too stringent, so, I will tend to favor smart relaxing of the standards
here to allow for maximum self improvement power".

Every resource cap that applies to a self builder's sandboxed command, what it was, and what it is. The first four
are the view's own constants (`sandbox/builder_view.rs`), set with `max` so a configured value that is already
larger is kept. None is a configured default: `[sandbox]` is unchanged and no other job's view has them.

**Generous on disk, bounded in memory** (Tabitha/Claude, 2026-11-15 18:35, on this machine's 24 GB). `/tmp`, `/dev/shm`, HOME
and scratch are all tmpfs mounts (`theseus-sandbox` `view.rs`), so what a command writes there is RAM, and nothing
puts a memory limit behind it. Each is therefore 4 GiB (four times the default), 16 GiB in all if every one filled,
and the large temporaries are kept out of them: the builder's `TMPDIR` is the folder's own `tmp/` directory, on disk
under the 48 GiB bound, and cargo's target is its `target/` directory. What is on disk is generous.

| cap | before (builder's view = an ordinary job's) | after | backing | why |
|---|---|---|---|---|
| a single file (`RLIMIT_FSIZE`, `output_mb`) | 64 MiB (4 GiB in the first cut of this branch) | 16 GiB | disk (a file this size can only be made in the build folder) | the link of the core's tests died of `SIGXFSZ` at 64 MiB; equal to the gate's view |
| scratch, the overlays' writes (`scratch_mb`) | 1 GiB | 4 GiB | **memory** (tmpfs) | cargo's registry and cache writes; the build output that filled 1 GiB is in the folder now |
| each of `/tmp`, `/dev/shm`, HOME (`tmp_mb`) | 1 GiB | 4 GiB each | **memory** (tmpfs) | small temporaries; the compiler's and linker's follow `TMPDIR`, on disk |
| `TMPDIR` | `/tmp` (memory) | `<folder>/tmp` | disk, in the folder's bound | large temporaries off the RAM |
| processes (`RLIMIT_NPROC`, `pids`) | 512 | 32,768 | none | parallel compilers and a test run's children on a many-core box (the gate's view: 8,192) |
| the build folder (`rsi::builder_dir::MAX_BYTES`) | none (did not exist) | 48 GiB | disk | measured worst case 37.9 GiB plus a quarter |
| `[tools] job_pids_max` (a cgroup `pids.max`, 4,096) | not applied to an L1 job | unchanged | none | the wrapper applies the cgroup only on its L0 spawn path (`theseus-kernel` `job.rs`, `spawn_l0`); an L1 job is held by `RLIMIT_NPROC` alone, so the builder's 32,768 is its cap |
| open files | none set by L1 | unchanged | none | L1 sets no limit of its own |
| memory | none (L1 has no cgroup) | unchanged | none | no memory limit exists for the view, and none is added here |
| wall time of one call (`[tools] proc_timeout_max_secs`) | 3,600 s | unchanged | none | a configured `[tools]` value for every session, not this view's; a cold whole-workspace build measured 335 s |

Not allocations, and not changed: the network (none beyond the call's own egress list), what is readable, the
hidden list (secrets, the store, the socket, the sealed sets), the facilities the gate's view alone is given, and the
rule that the gate never reads what the builder compiled.

## Removal

A build's code runs with write access to its folder, and a directory it leaves without the owner's write bit made a
plain `remove_dir_all` fail with `EACCES`, so `tree::close` errored after the worktree was gone and the stale-item
cleanup errored every tick. `tree::remove_all` removes a path, and when that is refused makes each directory under it
`0o700` and removes it again. A command of the builder may still run while its folder is removed, so the walk is
race-proof: every directory is opened once (`openat`, `O_NOFOLLOW`, `O_PATH`), and the `chmod`, the listing and the
opening of its children all go through that descriptor, never a path again; a swapped-in link is refused, and the
mode is fixed, never one the builder chose. It is used for the builder's folder, the gate's build
tree (`tree::close`, `pipeline::clear`) and the install's build tree (`install::reap_target`).

## Measured

On a fleet box (32 vCPU, a fresh folder each time, the builder's view with the self trees as a workspace root, no
network), by theseusd's ignored test `a_builders_core_test_build_completes_in_its_folder_and_is_reused`, which
builds twice and then runs tests from the build:

| command, in the builder's view | first run | second run | folder after (`du`, GiB) |
|---|---|---|---|
| `cargo test -p theseus-core --no-run` | 286 s, 344 crates | 0 s, none | 8.08 |
| `cargo test -p theseus-core --lib rsi::capacity` (6 tests, passed) | | 0 s, none compiled | 8.08 |
| `cargo test --workspace --no-run` | 335 s, 661 crates | 3 s, none | 32.62 |
| then `cargo test -p theseus-core --lib rsi::capacity` on that folder | 279 s, 141 crates (another feature set) | | 37.90 |

So one item's folder after a core test build measured 8.1 GiB, which is the owner's "about 10 GB". `MAX_BYTES` is 48
GiB: the worst measured (a workspace build and then a package build on top) is 37.9 GiB, and the bound leaves a
quarter above it. Before the per-file limit was set for the view, the same build died on its first link with
`SIGXFSZ` (signal 25): the core's test binary is far past a job's default 64 MiB file limit.

## What this does NOT do

- It does not meter a command's writes as they happen. The daemon looks at the folder on the heartbeat's timer, so a
  command can write past the bound by what it writes between two looks; the disk floor is behind that.
- It does not make the folder's contents trusted. A model wrote them. Nothing the gate or the join reads comes from
  them, and they are deleted with the item.
- It does not change what a self session may read or write with the file tools, or the approval rules for its
  commands.
