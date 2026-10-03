# theseus-protocol

The wire types (spec §3.18): JSON-RPC 2.0 over newline-delimited JSON. Types only: no runtime, no I/O, no clock.
Every client and the core read it, and the web apps' TypeScript is generated from it.

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
- `label.rs` (M4 19a): a node's `Label` (integrity, readers, and an untrusted node's source), a session's
  `Audience`, and the manifest's `Withheld` and `InPlay`. The rules that combine them are the core's (`labels.rs`).
- `sandbox.rs`: health's `sandbox` block (17b, with 18c's egress counts), and `reach` and `egress_in`, the one
  wording of an L1 job's reach (`no network`, `egress: github.com:443`) and the list a proposal binds.
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
- **The generated TypeScript is part of the change.** Any test run of this crate rewrites `web/src/protocol.gen/`
  (`the_web_apps_types_are_generated_from_the_rust_ones`). `git add` it, or the gate fails. A `serde_json::Value`
  field names its TypeScript type with `#[ts(type = "...")]`. Large integers are `number`.

## Tests

- `tests/wire.rs` compares each shape with its fixture in `tests/wire/`, byte for byte, both ways.
- `THESEUS_GOLDEN=write` rewrites the fixtures. Use it only for a change you mean to make to the wire, and read the
  diff: a fixture rewritten to pass hides exactly the change these tests exist to catch.

## Traps

- ts-rs can't flatten an optional struct: give the outer struct optional fields instead (as `ToolStarted` does).
- Stored floats re-parse one ULP off without serde_json's `float_roundtrip` (theseus-k52m), so a re-encoded node
  can differ from its stored bytes.
