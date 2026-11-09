# NOT LANDABLE

Row: p5-b4-recall-followups (bucket row theseus-ectzg, 1.0.5 B4). Branch `cloud/p5-b4-recall-followups`. Base `4683adb8a24e182c6123f1207f164a9486c1a1fa`. No fix commits; 0 files changed besides this report.

## Merge result
`git fetch origin && git merge origin/main --no-edit` said "Already up to date": `origin/main` is still 4683adb8a. It brought in nothing.

## Evidence the code is absent
- `grep -rn "would_drop\|mark_would_drop"` over `crates/` and `cockpit/src` finds nothing. The only `low_effort` hit is an unrelated test name in `tests_learn_loop.rs`.
- `crates/theseus-index/src/files.rs` has no "until that cost" text, no dedup limit and no provenance limit. `tests_files.rs` has no `bench_a_backfill_from_the_wals_start` and no `MAX_BLOB`.
- `origin/p5/lv2u-followups` and `origin/p5/fpm2-smalltalk-bar` exist as branches but are not ancestors of `origin/main`.

## Per item
- Item 1 (would_drop reader and paused inflation): PREMISE FALSE / NOT APPLICABLE, branch not yet on main (`p5/fpm2-smalltalk-bar`).
- Item 2 (index header limits F2/F3): NOT APPLICABLE, branch not yet on main (`p5/lv2u-followups`).
- Item 3 (corrupted-blob doc, backfill rate comment): NOT APPLICABLE, same reason.
- Item 4 (third fpm2 limit, bar-efficacy caveat, 64 MiB fixture): NOT APPLICABLE, same reason.

I did not reconstruct the code, as the brief instructs. No gate was run and no keel guard was run, since there is no diff. Nothing here can grow a model request, so `dm_flow` was not run.

## Next step
Re-run this row after the two branches land on main. The departure that was meant to carry them had not landed when this ran.
