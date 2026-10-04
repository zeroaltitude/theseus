# theseus-protocol

The wire types (spec §3.18): JSON-RPC 2.0 over newline-delimited JSON. Types only: no runtime, no I/O, no clock.
Every client and the core read it, and the cockpit's TypeScript is generated from it.

Key modules: `lib.rs` (the `method`, `notify`, `error_code` tables), `events.rs`, `push.rs`, `gate.rs`, `ts.rs`. Read by: every crate on the wire, and the cockpit (generated).

## What's here

- `lib.rs`: the methods' params and results, `VERSION`, and three tables: `method` and `notify`, each one list
  from which its `ALL` is built, and `error_code`.
- `events.rs`: one struct per notification, and the `events!` table, from which `Event::VARIANTS` is built.
- `cancel.rs` (M4 18a): a cancel's verdict on the wire (`CancelVerdict`), health's `CancelCount`, and `words`, the
  one wording of a verdict every surface shows.
- `gate.rs`: the gate's record of a tool call (`GateRecord`), written through `canonical` (sorted keys), so stored
  records keep their bytes.
- `push.rs`: what every surface shows of an execution: `ExecutionView` and `attention()`, the design's rules,
  first match wins. The caller passes how to write a time of day, since this crate reads no clock.
- `ledger.rs`: the ledger's kinds, `LedgerKind` (theseus-j6qn): every kind a row is written under, by any crate,
  one line each. Writers take a variant, so a new kind is a new line here, and core's `tests_registry` fails a
  variant nothing writes. A row stores the kind's name, so old kinds (`LedgerKind::RENAMED`) and unknown ones still
  read; renaming a kind is a store version change (P5b).
- `places.rs` (the place rule, theseus-nbsh): a place's `PlaceClass` (private or shared), and health's
  `PlacesHealth`, each place with its class. The rule itself is the core's (`places.rs`). 19a's labels went with it:
  a stored node's `label` and a manifest's audience fields read and are left unread (NODE 7, COMPILATION 5), and a
  row of the five `label.*` kinds reads as an unknown kind.
- `sandbox.rs`: health's `sandbox` block (17b, with 18c's egress counts), and `reach` and `egress_in`, the one
  wording of an L1 job's reach (`no network`, `egress: github.com:443`) and the list a proposal binds.
- `cred.rs` (theseus-gh7): `HarnessOnly`, health's and `theseusd check`'s line of the secrets a job may be handed
  and the keys that stay the harness's own. 18d's credential requests, `secret.requested` among them, went in
  theseus-w5op: a stored row of their kinds still reads, as an unknown kind.
- `arrangement.rs` (M5 27): a task's arrangement as `TaskInfo` carries it, its pieces by reference.
- `check.rs` (M5 28a): a check task's basis (`TaskCheck`) as `TaskInfo` carries it, and `TaskCheck::line`, the one
  wording every surface shows (`cockpit/src/lib/check.ts` mirrors it).
- `ts.rs`: the TypeScript export.

## Invariants

- **One definition per wire shape** (Item 30). Senders build the typed struct; nothing sends `json!` for a shape
  this crate defines, and nothing reads one back by string keys.
- **Old rows and older peers still decode.** A new field is optional, with serde's default, and absent from the
  bytes when unset, so every fixture keeps its bytes. On the TypeScript side it is `#[ts(optional)]`, which writes
  `field?: T`.
- **The reader rule** (P0, rule 3). A new method goes into the `method` table and gets its dispatch arm in
  `crates/theseus-core/src/rpc/server.rs` on the same commit. A new notification goes into `notify`, gets its
  `Event` in `events!`, and a sender in theseus-core. Otherwise core's `tests_registry` fails, naming the fix, or
  it takes a marker in that test's `RESERVED` table naming the row that brings its reader.
- **The generated TypeScript is part of the change.** Any test run of this crate rewrites
  `cockpit/src/protocol.gen/` (`the_cockpits_types_are_generated_from_the_rust_ones`). `git add` it, or the gate
  fails. A `serde_json::Value` field names its TypeScript type with `#[ts(type = "...")]`. Large integers are
  `number`.

## Tests

- `tests/wire.rs` compares each shape with its fixture in `tests/wire/`, byte for byte, both ways.
- `THESEUS_GOLDEN=write` rewrites the fixtures. Use it only for a change you mean to make to the wire, and read the
  diff: a fixture rewritten to pass hides exactly the change these tests exist to catch.

## Traps

- ts-rs can't flatten an optional struct: give the outer struct optional fields instead (as `ToolStarted` does).
- Stored floats re-parse one ULP off without serde_json's `float_roundtrip` (theseus-k52m), so a re-encoded node
  can differ from its stored bytes.
