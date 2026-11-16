# Libraries for a self build

Issue 20 of the self-improvement loop, the line "The builder cannot try a library the machine does not already hold".
The owner's decision (2026-11-15 17:46:56, chat message 1571687985215308000, "A"): let the builder download libraries so
it can try them, and have any change that adds one stop at his card before it joins.

Before this, a self build's view had no network and cargo was told `--offline`, so a change that needed a crate the
machine's cache lacked failed its gate at once, and its builder (whose own commands run in L1 with the operator's
egress list only) could not even try the library. Four parts, and the line between them is the point. Code:
`crates/theseus-core/src/rsi/deps.rs` and `rsi/deps/`, the join's class in `rsi/join/dependencies.rs`.

## 1. The builder may fetch (`deps/builder.rs`, `deps/hosts.rs`)

A self builder's `proc.run` is given the registries this repository already uses as its egress list, and nothing else
(not the operator's own `[sandbox] egress` too: the gate gives a self session's sandboxed call this list in its place,
`docs/design/self-own-removals.md`, which says what the proxy does and does not restrict):

| Host | What is asked of it |
|---|---|
| `index.crates.io:443` | cargo's sparse index: which versions of a crate exist, and what each needs |
| `static.crates.io:443` | cargo's downloads: the `.crate` file of each version |
| `registry.npmjs.org:443` | the npm registry the cockpit's lock file names: metadata and tarballs |

The list is `hosts::HOSTS`, a constant with a reason for each entry. Nothing reads a config, a branch or a call to
extend it, and a test holds it to exactly these three. It is not `crates.io` itself (its pages and API are not needed
to fetch), nor GitHub (a git dependency is refused, part 3).

What the builder downloads goes to a package cache of its own inside the item's build folder
(`<state>/self/builder/<slug>/.packages`: `CARGO_HOME` and `npm_config_cache` point there), so a second command does not
download again. Nothing else reads it: the gate's view never binds the build folder (`rsi::builder_dir`), and the
pipeline's fetch, below, writes a cache of its own. The builder's cache starts empty, so the first cargo command of an
item downloads the crates it builds with; the operator's `~/.cargo` is not copied in (a copy would share a file with the
operator's cache, or cost a gigabyte an item).

A job that reached only these hosts is not marked as outside text: they are the owner's own decision, so they count as
part of the operator's list for a self builder (`builder::operator_list`).

## 2. The gate never takes a package from the builder (`deps.rs`, `deps/fetch.rs`)

The gate keeps no network. Before the gate of a tree whose `Cargo.lock` or `cockpit/package-lock.json` differs from
main's, the pipeline (the daemon's code, in `tree::gate_with`) calls `deps::provide`, which fetches exactly the packages
those lock files name into `<state>/self/packages/<key>` (a hash of both lock files), from the registries, each checked against the checksum its
lock file records:

- **cargo:** `<static.crates.io>/crates/<name>/<name>-<version>.crate`, its sha256 compared with the lock's `checksum`,
  unpacked as a directory source (`cargo-home/vendor/<name>-<version>`, inside cargo's home so the install's path remap covers it, with a `.cargo-checksum.json` that holds the package checksum
  and no file list) by `theseus_files::archive::unpack_tar_gz`, which writes regular files and directories only and
  refuses a path that climbs or leaves the crate's own directory. `cargo-home/config.toml` sends crates.io to that
  directory; `CARGO_HOME` points at `cargo-home`, which also links the operator's advisory database for `cargo deny`.
- **npm:** the `resolved` tarball of each package, its hash compared with the lock's `integrity` (sha512, sha384 or
  sha256; a lock with only a sha1 is refused), kept whole where npm's own cache looks for it
  (`_cacache/content-v2/<algo>/<hex>`). `npm ci --offline` finds a tarball by its integrity there.

No build script, no code of the branch and no code of a downloaded package runs: it is a download, a hash, and a write.
No cargo or npm runs in the daemon either (`cargo metadata` reads the branch's own config, which can name a compiler).
A redirect is never followed, so no answer can send the fetch to another host. The cache is an overlay in the gate's
view (writes vanish), bound into the view of every tree whose lock files have its key (the branch's gate, the planted
revert's, the merge's, the install's, and a later tree cut from a main that took them): a cache made for other lock
files is not one (`Packages::of`). It stays when a tree closes; each `provide` keeps the cache it made, main's own and
the three newest, and removes the rest.

A package the registry does not have, or serves with other bytes than the lock names, makes the gate red before it
starts, with a message the builder reads in the gate's tail. A registry that does not answer (three tries) is the
machine's, not the change's: the run is marked `unrunnable`, which the loop already counts as infrastructure.

A place with no sandbox (a test's stand-in gate runs at L0) judges but never fetches: only a sandbox's view reads the
cache, and no test reaches a registry.

## 3. A dependency from anywhere else is refused (`deps/policy.rs`, `deps/lock.rs`)

Refused at the build, with the reason, before anything is fetched: a `Cargo.lock` entry from any source but crates.io
(a git URL, another registry) or with no sha256 checksum; a `package-lock.json` entry not served by
`https://registry.npmjs.org/`, a link to a local path, or one with no strong integrity; in a tracked `Cargo.toml`, a
dependency (in any dependencies table, a target's, or the workspace's) that names a `git`, a `registry` or a path that
leaves the tree, a `[patch]` of any source but crates.io, a `[replace]`; and in a tracked `.cargo/config` or
`config.toml`, a `[source]`, `[registries]`, `[patch]` or `[paths]` table. The last matters most: a project's cargo
config beats the gate's `CARGO_HOME` config, so a branch that could replace crates.io could make the lock file's
checksum mean nothing. All of it is read as text.

## 4. A change of dependencies stops at the owner's card (`rsi/join/dependencies.rs`)

The keel check gains a class, `dependency`. It reads the two lock files as the branch left main (the merge base, as the
keel guard does) and as the branch has them, and for each package the change adds, removes, or changes the version,
source or checksum of, one line of the card: `cargo: tokio 1.40.0 -> 1.41.0, from crates.io`,
`cargo: added fresh 2.1.0, from crates.io`, `npm: removed left-pad 1.2.0 (it came from the npm registry)`. A lock file
change that changes no package (a reorder, a workspace member's own entry) is no hit; a lock file that cannot be read,
or an entry from a source that may not be fetched, is one: the check never passes by default. The card lists every
package up to 300, however many, where other classes show their first twenty.

A manifest's dependency line is judged through the lock it must agree with. A feature flipped on a package the lock
already names changes no package and is not held.

## What is proven and what is not

Proven by tests with a registry the test owns (a loopback server): the fetch's checks and refusals, the cache's shape,
the refusals, the gate's red, the jail's view, and the card. Proven once against the real crates.io and npm on a fleet
box: see the issue's report. Not proven: that a model's session, in the real L1 view, reaches the registries through
the egress proxy (the list is the same mechanism an operator's `[sandbox] egress` uses, and is tested as a list).
