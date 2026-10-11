# Cloud report: stack-tags (theseus-mpk2s)

Branch `cloud/20261011-stack-tags`. Code commit: fd346ab.

## What I found
- `aws.stack.plan` and the bootstrap set `theseus:execution` as a stack tag, so a plan from any later session changed every resource's tags.
- The budget reconcile sent `Tags: []` on its change set. CloudFormation reads an empty list as "remove all tags", so the $100 reconcile's change set modified every resource. That is the 23-resource refusal, and it would have happened with or without a changed execution id.

## What I changed (fd346ab)
- New `aws/stack_tags.rs`:
  - `stable_tags`: a new stack gets owner, stack and deployment. An existing stack keeps its own tags less `theseus:execution`.
  - `migration`: true only when the stack holds `theseus:execution` and the tags sent differ from its tags by that removal alone.
  - `tag_only` / `tag_only_gaps`: the classifier.
- Plan (`stack.rs`): tags come from `stable_tags`. A change set that is `migration` and `tag_only` is labelled "A tag-only migration" and omits the change-set verdict's floor lines. The template scan's own lines stay. `Shown.tag_migration`, and `tag_migration` in the plan's meta, carry it to apply. Apply marks its ExecuteChangeSet `aws.called` row with `tag_migration: "theseus:execution"`, using the new `AwsBinding::mark_last` in theseus-tools.
- Description: the plan's was already `theseus <execution> from <path>` and the bootstrap's `theseus aws bootstrap (<execution>)`. They are unchanged and now tested.
- Bootstrap (`bootstrap.rs`): stable tags, one extra `DescribeStacks` on updates, and `differs` skips the floor lines for a migration.
- Reconcile (`tend.rs`): sends the stack's tags less execution. The budget's own change is as before. The rest of the change set must be a migration of that one tag, else it stops with "touches more than the budget (...)". New `Reconciled::Migrated`; the `aws.budget.reconciled` row gets `tag_migration`.
- Fake CloudFormation (`tests_c2.rs`) now models stack tags, change-set tags, and Scope/Details/CausingEntity.
- 7 new tests in `tests_c2/stack_tags.rs`.

## What passes the floor now that didn't before
Only a change set that meets all of these:
- the stack holds `theseus:execution`;
- the tags sent equal the stack's tags minus that one;
- every change is a `Modify` with Scope exactly `[Tags]`, replacement False, every detail targeting `Tags` and never recreating; or
- a `Modify` with Scope exactly `[Properties]` and replacement False or Conditional, every detail `Dynamic` and `ResourceAttribute`, whose `CausingEntity` is a resource that is itself a tag change in the set, other than a custom resource.

These are not accepted: an Add, Remove, Import or `Replacement: True`, a mixed Properties+Tags change, a `Static` property edit, a cause outside the set, and an empty set.

## Design choices for the owner
- **Knock-ons are accepted.** The brief says "every Scope is Tags". Real change sets also list dependents (`ReaperInvokePermission` Conditional; likely `TheseusBucketPolicy`) as Dynamic Properties changes. Without accepting them the migration would still fail the floor and the reconcile. The reasoning is that a tag cannot change an attribute. Narrow this in `is_knock_on` if you disagree.
- **Existing stacks keep their tags.** An existing stack's tags are carried over, not rewritten to a fixed set, so a stack with no `theseus:deployment` is not given one. I did not verify that CloudFormation's actual Details shapes match the fake's; the live check will show it.
- **No new ledger kind.** `toolrun.rs` is at 2488 of 2500 lines. The apply's own row is the existing `aws.called` row, labelled. The reconcile uses `aws.budget.reconciled` with a field.

## Proof
- `cargo nextest run -p theseus-core -p theseus-tools`: 1784 passed.
- Full workspace without core: 33 failures, all the known L1 sandbox tests (theseus-sandbox contract and `spawn_100`, theseusd sandbox).
- fmt, shape and clippy `-D warnings` are clean.
- Planted reverts, each file restored and touched:

| Revert | Caught by |
|---|---|
| Plan sets the execution tag again | `a_plan_from_a_later_session_lists_only_its_real_change` |
| Floor bypass without the classifier | `a_plan_that_edits_a_guard_beside_the_migration_keeps_the_floor` |
| `tag_only` always true | `..._keeps_the_floor` and `a_pure_tag_change_is_tag_only_with_its_knock_on_and_nothing_else_is` |
| Reconcile ignores the rest of the change set | `the_reconcile_refuses_the_migration_with_any_other_change` |
| Description drops the execution id | `a_plan_from_a_later_session_lists_only_its_real_change` |
| Apply's row mark removed | `a_plan_from_a_later_session_lists_only_its_real_change` |

## Live check for the maintainer
After the join and an install, start a scratch daemon. Health's reconcile line should read "the stack holds the config's $100 a month" after one tag-only migration on theseus-foundation. `aws_stack_status` should show the tags are owner, stack and deployment. A plan of theseus-hands from another session should list only its real change. The first plan of each old stack should say "A tag-only migration" with no floor lines. If CloudFormation lists something the classifier refuses, the plan keeps today's floor and the reconcile stops with the names.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` failed in its keel phase. The findings are in the range `merge-base..HEAD` (the merge-base is a9ad950, the tip of the base, so these come from main's history, not my diff):
- tui test `a_finished_task_is_done_counted_and_rung_once` lost an assertion;
- `#[allow]`/`#[expect]` added in `theseus-index/src/vectors.rs` and `theseus-protocol/src/tests_work_join.rs`;
- long-files ceilings raised for `crates/theseus-protocol/src/lib.rs` and `crates/theseus/tests/golden.rs`;
- ack signatures do not verify here.

My diff adds none of these. I ran the later phases by hand. `cargo deny` could not run offline, so the deny phase was skipped. The cockpit and lifecycle/jobs/turn benches were not run, and neither were the registry, protocol-types or cockpit-build phases; I ran no protocol types change, but the plan meta gained a `tag_migration` key.

## Keel findings expected
None from this row.

## Docs to update (maintainer)
- `docs/design/aws-toolset.md` lines ~487 and ~554, which describe stacks carrying `theseus:execution`.
- The spec's Part III item for this row.
