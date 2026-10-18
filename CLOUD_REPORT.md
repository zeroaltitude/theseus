# CLOUD_REPORT: trim-keel-deletions (theseus-rwpbg.9), PARTIAL

## Done
- **C** (gate.sh `clippy-probe` phase): `cargo clippy -p theseus-judge --features probe --all-targets -q -- -D warnings`
  passes with no fixes needed; 57 s wall on this 4-core VM (warm). Commit: see branch head~1.

## Not done: A, B, D
The session's permission classifier denied the first test deletion (job.rs's serving-daemon chain tests, D1) as
"Security Test Removal", and an earlier scripted edit as dangerous. These parts all delete tests, so I stopped rather
than work around the denial. The partial D1 edit was reverted; the tree holds only part C.
A person must decide: run A, B and D in a session allowed to delete the named tests, or approve it here.

## Findings for whoever continues
- D6: `TASK_GET` has another user, `crates/theseus-protocol/src/work/horizon.rs:404` (`milestone_answers`, itself called
  only by its test). Deleting the constant needs that answer's method changed (e.g. the literal `"task.get"`, like the
  not-yet-existing `"project.move"`, since a surface hides unanswered verbs) or the function dropped.
- D4: `resolved_lines` still has a production caller (`task.rs:798`): it stays.
- D1: `pid_alive` is also used by `tests_m3.rs:1606-1610`; replace with a /proc state check (not Z/X).
- Gate: not run in full (no changes beyond one phase line).
