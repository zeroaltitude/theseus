# CLOUD_REPORT: theseus-0zraw, plain words in the cockpit's explainers

## 1. For the maintainer
- Store format: untouched. No Rust file changed. No ceiling raised. No OWNER CALL or STOP.
- `ops.words` was kept, as its own entry, with its own button (a second button, `Words`, beside `Graph`).
- Not run: the full `scripts/gate.sh` could not judge this tree. See section 6.

## 2. The glossary sweep (`cockpit/src/lib/explainers.ts`)
- `threads.list`: "open tasks" is now plans, steps, questions and wakes; "session" is "conversation".
- `threads.work`: plans, steps, questions and wakes; a failed step or task stays until looked at.
- `fleet.graph`: nodes are a conversation or a task (an agent Theseus started); "session" is "conversation".
- `fleet.list`: "place" glossed in place ("where it came from: the CLI, a direct message, a channel").
- `actions.tasks`: plans and steps.
- `actions.taskgraph`: plans and steps, as a tree and a graph.
- `ops.board`: cards are a plan, a step, a task, a conversation, a job or a wake. The Scheduled column says its counter reads "scheduled today". A step not yet started is "to do" and has no column.
- `ops.queue`: "a task change" is "a change to a step's goal".
- `ops.inspector`: names plan, step, task, job and conversation.
- `tasks.new`: a task is an agent that works on its own, with its own budget, and reports back.
- New `ops.graph`: lanes, cards and forms (thread as swimlane, plan as fold and "n of m", step as checkbox glyph, task as badge strip, job as pill), plus the seven state words.
- New `ops.words`: the six nouns in a person's words. Its "why" part also gives turn, ask and wake. Project and Home are not defined there (see 10).
- `judgment.ladder`: the owner's own text, verbatim.
- `judgment.main`, `judgment.log`, `judgment.versions`, `judgment.learning`, `ontology.minted`, `ontology.growth`, `ontology.categories`: "pack" is "check" and "mint" is plain words.
- `bench.yourwork`: "rung" is "step down".
- `speed.*` (two spots): "frame" is plain words.

## 3. The jargon sweep
| word | at base | now |
|---|---|---|
| pack | 12 | 0 |
| rung | 4 | 0 |
| task record, task graph, plan item | 0 as owner-facing text (the old nouns were "task", "tasks", "task records") | 0 |
| still today | 0 | 0 |
| mint / minted | 6 | 0 (the help id `ontology.minted` stays; it is a key, not text) |
| frame / frames | 2 | 0 |
| lens | 0 | 0 (no entry uses it) |

- Kept, with a gloss: **shadow** (the badge says it; glossed as "Jev answers but nothing acts on it"), **place** (a real column and chip; glossed once in `fleet.list`), **live** (glossed).
- The allow-list in `test/opswords.test.ts` is **empty**: no banned word remains anywhere.

## 4. `ops.graph` and its button
`cockpit/src/ops/CommandBar.tsx:77`, right after the Board's line at :76:
```
{lens === 'graph' && <><HelpButton id="ops.graph" title="Graph" /><HelpButton id="ops.words" title="Words" /></>}
```
This is one line and carries both buttons. `explainers.test.ts` passes in both directions with them.

## 5. Tests
- `npm test` ran 792 tests at the base and 798 after (6 new, all in `test/opswords.test.ts`). 0 fail.
- Red at the base: with `src` stashed, `node --test test/opswords.test.ts` gave pass 0, fail 6.
- P1: "check" put back to "pack" in `judgment.ladder.read` failed "no explainer uses a word a person has never met" (797 pass, 1 fail).
- P2: heading restored to "Task graph ·" in `TaskGraph.tsx` failed "the Actions panel of stored work is headed Plans and steps" (797 pass, 1 fail).
- Both restores were `touch`ed. `git status` showed only my intended files, and `npm test` was back to 798 pass.
- `explainers.test.ts` is unchanged and green, including "every panel's help id has an explainer" and "no explainer is left without a panel".
- `npm run lint`: warnings only, none in my files and none new from this change. `npx tsc -b`: clean.

## 6. Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` **failed in the door/keel phase, before any build**:
- The door runs main's current `gate.sh` against this tree. Main has moved past `0fda4d57`, so the keel guard compared against a newer main.
- It reported erosions that none of my commits touch: `crates/theseus/src/persona_cmd.rs` tests, `scripts/keel-guard.py` and `scripts/test_keel_guard.py`. Several `Keel:` acks were also refused because their signatures do not verify here.
- My diff touches only `cockpit/` files and `CLOUD_REPORT.md`.
- The suite and later phases did not run. I ran the cockpit checks myself: `npm test` 798/798, `npm run lint` (warnings only), `npx tsc -b` clean.

## 7. Keel
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` printed nothing and exited 0.

## 8. DM golden
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` exited 0, so the file is byte-identical.

## 9. Join notes
- `cockpit/src/lib/explainers.ts`: entries `threads.*`, `fleet.*`, `actions.*`, `ops.*`, `tasks.new`, `judgment.*`, `ontology.*`, `bench.yourwork` and two `speed.*` entries (text only), plus the new `ops.graph` and `ops.words` before `ops.queue`.
- `cockpit/src/ops/CommandBar.tsx`: one line added at :77.
- `cockpit/src/components/TaskGraph.tsx`: both panel titles, at :29 and :100. They read "Plans and steps · N open of M" and "Plans and steps · N in all".
- `cockpit/test/opswords.test.ts`: new.
- No overlap with hl0r1, gnhnk, brcs4 or 99l9z.
- Design choice: the second heading used to read "N tasks". It now reads "N in all", because "tasks" no longer names stored records.

## 10. Follow-ups
- `docs/glossary.md` and `crates/theseus/src/render/task_graph.rs` still use "task record" and "task graph" (the CLI/TUI/docs row).
- The explainers define turn, ask and wake in `ops.words`. Project and Home are not defined there; add them to `ops.words` or to a Timeline entry if you want them.
- `ops.graph` names the seven state words, but the Graph's legend wording belongs to hl0r1.

## 11. Data floor
Nothing new is sent to a model or routed anywhere.
