# The stale edit: a read record is noted when it exists, and checked when it exists

_Written 2026-11-09 from the owner's decision of that morning, which closed the open product question of
`theseus-w2cs` sub-item 2. Two shapes were on the table; the cheap one was chosen, and the other was built once,
measured and deliberately not committed. This note is the rule, so the next agent to touch `fs.edit` does not
rebuild the rejected one._

## 1. The rule

A tool that hands a model a file's bytes **notes** what it handed over. `fs.edit` and `fs.patch`, before they
write, **check** that note — and only when a note for that path exists. A path with no note is written exactly as
it is written today.

> Refuse only when a read record exists and the file changed since; the refusal is an ordinary tool error that
> says read it again, then write.

The absence of a read is **never** a refusal. This is a guard against one specific race — *I read it, something
else changed it, I write over that change* — and not a permission system, not an approval gate, and not a policy
denial. A model that learned a file's contents from `proc.run cat`, from `git.diff`, from an earlier turn or from
an earlier session writes as it always did.

That is what makes this cheap: **every existing test that edits a file without reading it first stays green by
construction**, because nothing about its path changed. The guard adds a refusal only on a path that was read in
this session *and* moved underneath the reader.

## 2. The rejected alternative, and why

The other shape was **a prior read required**: `fs.edit` refuses a path this session has not read. It was built
in an earlier attempt, measured, and not committed.

| | required prior read | noted-and-checked-when-exists |
|---|---|---|
| rig tests reddened | **27** | 0 by construction |
| changes what a model may do | in every session | only on a path it read and that then changed |
| `rsi` self-sessions | held with no fail-open path | safe (§3 below) |

The 27-test figure is **carried from the wave-3 measurement and is unverified here**. It would be reproduced by
building the required-read variant and running `cargo nextest run --workspace`; it is recorded because the number
is the cheap half of the argument, not the whole of it.

The real reason is the second row. A model legitimately knows a file's contents by routes that are not `fs.read`:
`proc.run cat`, `git.diff`, a result still in this turn's context, a summary of an earlier session. A required
read calls every one of those a mistake, and it does so in every session on the machine.

**`rsi` self-sessions are the case that made it unacceptable.** A self session's calls fail open where the
*policy* layer would hold them for approval (`ToolPolicy::fails_open`, `crates/theseus-core/src/policy.rs`), so self-improvement is
never stuck waiting for an owner who is asleep. A read-record refusal is not an approval: it is a `ToolFailure`,
an ordinary tool error, and there is no fail-open path for one and no reason to invent one. Under *required*, a
self session that knew a file by any other route would be stopped dead with nothing to answer. Under the rule in
§1 there is nothing to fail open from: a self session that reads and then writes with nothing changing underneath
sees no refusal at all, and one that genuinely lost a race should be told it lost. **This is the single most
likely thing for a later reader to get wrong.**

## 3. The shape

**Where the note lives.** Off the session, dying with it. The first attempt kept one process-wide `HashMap` with
one insert and one get: no eviction, no cap, no clear at a session's end. That is the finding to fix, not to
copy. The shapes to match are `JobWaits` (`crates/theseus-core/src/toolrun/waits.rs:32`, reaching a call as
`pub job_waits: Arc<JobWaits>`, `crates/theseus-core/src/toolrun.rs:301`) and `Terms`
(`crates/theseus-core/src/term/mod.rs:239`): one board on the runtime, keyed by session, cleared when the session
ends.

**How it reaches a tool.** `ToolCtx` (`crates/theseus-tools/src/lib.rs:276`) already carries two optional
capabilities the runtime injects per call — `cores: Option<Arc<dyn Cores>>` (the trait at `:199`) and
`secrets: Option<Arc<dyn Secrets>>` (at `:208`) — and `ToolCtx::for_tests` (`:316`) leaves both `None`. The read
record arrives the same way: one more `Option<Arc<dyn …>>`, bound to one session per call, `None` everywhere a
test builds a context. A `None` board notes nothing and checks nothing, so every tool test keeps today's
behaviour without being edited.

**The cap.** A per-session cap on the paths noted. At the cap the oldest note is evicted, and that path is then
simply unguarded — the quiet choice, not the noisy one. It is the right default under §1: an evicted note means
the guard goes back to doing nothing, which is what it does for every unread path anyway. *This is a choice, and
an open question for the owner: evict quietly, or refuse to record and say so.*

**What identifies a file's state.** The file's own **observable state — its byte length and its modification
time** — read from `fs::metadata`, never a hash of its contents.

> **A `DefaultHasher` value is not stable across releases and must never be stored or compared across processes.**

The first attempt identified a file by `DefaultHasher`. `std::hash::DefaultHasher`'s algorithm is explicitly not
guaranteed between Rust releases, so such a value is meaningful only inside one running process. Keeping the rule
and the hash together is fragile; length and mtime avoid the question entirely, cost one `stat` instead of a read
of up to 16 MiB on every edit, and are sound here for a precise reason: **Theseus's own file writes go through
`write_atomic`**, which writes a new file and renames it over the target, so a write by another tool, another
session or the operator's editor gives a fresh mtime. Two states that a change could leave identical would need
the same byte length *and* the same mtime to nanosecond resolution.

**What is noted.** Only a tool that hands over a file's bytes **as a file** — `fs.read`. Not `fs.grep`, which
returns matching lines; not `proc.run cat`, whose argv the runtime does not read as a file read. *This is a
choice, and an open question for the owner.* It is the narrow end of the range, and widening it later only ever
adds guarded paths; narrowing it later would silently drop a guard a model had come to rely on.

**A write refreshes a note it finds, and creates none.** After `fs.edit`, `fs.write` or `fs.patch` writes a path,
the file has changed — by the model's own hand. If the note were left as it was, the *next* edit of that file
would be refused, and a model making two edits after one read would be stopped by its own first edit. So a
successful write sets the note for each path it wrote to the file's new state, **and only where a note for that
path already exists**. A write to an unnoted path still notes nothing, so §1's "no read, no refusal" holds
exactly: a write never brings a path under the guard.

## 4. The refusal's words

Quoted as the code says them:

```
{path} changed since you read it, so this change was made against contents that are
no longer there. Nothing was written. Read the file again, then write.
```

(`fs_stale::stale_words`, `crates/theseus-tools/src/fs_stale.rs`. It is one string in the source and is
wrapped here only to fit the page. The commit that wrote this note quoted a draft of it; this is what the
code says.)

The recovery it names is the owner's own assumption, under test:

> *"i'm assuming the model can respond to the refuse with a new read, then write -- if so, your recommendation"*

So the text must name the path, name the re-read, and name the write, in that order and in so many words. If a
model cannot recover from this message by reading the file again and writing, the feature is wrong and the
message is where it is wrong. It is an ordinary `ToolFailure` (`crates/theseus-tools/src/lib.rs:368-370` shows
the shape): it goes back to the model as a tool error result, and the turn goes on.

## 5. What this does not cover

Three things, deliberately, and they are acceptable under §1 because the guard is a courtesy against one race and
not a guarantee of anything.

- **A file changed by something that never went through a tool** — the operator's editor, a build, a `git
  checkout` the model did not run. It *is* caught whenever the path was read through `fs.read` in this session,
  since the note is of the file's observable state and not of who moved it; it is not caught otherwise. There is
  no watch and no inotify here, and there should not be.
- **Two sessions racing on one path.** Each session has its own board, so session A's read does not guard against
  session B's write, and B's write does not refresh A's note. A's next edit of that path is refused, correctly, if
  A had read it; if A had not, A writes. Cross-session serialisation is the execution lock's job, not this one's.
- **A model that read the file by another tool.** `fs.grep`, `proc.run cat`, an attached file, a result from an
  earlier turn. No note, so no guard. This is the §1 rule working as written, not a gap in it: the alternative is
  §2, which was rejected.

A fourth, smaller one: a note is per path as the call resolved it (`ToolCtx::resolve`), so two names for one file
— a symlink and its target, two mount points — are two notes. The guard under-fires there; it never over-fires.
