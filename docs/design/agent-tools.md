# An agent Theseus invokes has tools (issue 28, `profile.run`)

The owner, 2026-11-19, asked Theseus to use another model profile to list a folder's files and show the results. Theseus
ran the listing itself in a shell, then called `profile.ask`, which gives the asked model one question and no tools, so the
other model only formatted the output. The owner: "when theseus invokes an agent, it is enabled with tools".

An agent with tools is a **task**, and Theseus already has tasks: a session of its own, the task's tools, its own spend
limit carved from the conversation's, its approvals and the floor as for any task, and a report. So `profile.run` is built
ON the task machinery and is no second agent loop. `profile.ask` is unchanged: one question, no tools, cheap. Each tool's
description says in a clause which is which.

## What the tool does (`profile_tool/run.rs`, family `profile`, deferred)

`profile.run {profile, work}` in the owner's own turn, in a private place (the wall of `profile.ask`, unchanged):

1. **Opens a task now** through `task::create`, the one path `task.create` and `task.fan_out` use, with the profile as the
   task's model. `create` takes the model as an argument (`on`) and not as a field of its input, so a model's own
   `task.create` still cannot name a profile (only a check may). The task is pinned to the profile: no brief verdict picks
   its model. Its objective is the owner's message that started the turn (quoted by node id, as `task.fan_out` does); the
   call's `work` is its brief.
2. **Waits** for the task's report with `task_wait::wait_on_tasks`, the wait `task.wait` runs, for at most `WAIT` (five
   minutes), never longer than the turn's own time allows. In time, the report is the tool's result (and a message of the
   turn, read once, as `task.wait`'s is). Not in time, the tool says the agent is still working and the turn goes on.
3. **Speaks in the conversation**, in both cases and once. The frame that opens the task writes a marker
   (`task.run.<session>`: the profile, the model, the profile of the turn that handed the work over, and the place that
   asked). The frame that ENDS the task, which already stages the task's report post, stages one more post when the marker
   is there: a `subagent` post, the message and footer `profile.ask`'s answer has (the model part is the profile and the
   model that ran, then "subagent answer, asked by <profile>", then the task's spend and time). It is staged when the task
   ends, so it comes before the asking turn's reply when the agent was waited for, and when the agent was late it comes
   when it ends. The task keeps its own post to the forum or channel its reports go to; where that IS the place the agent
   spoke in, the one message stands for both and the report post is not made twice.

## No new powers

Everything this tool can set in motion was already reachable by the same turn.

| What it can cause | Where that was already reachable | The test |
|---|---|---|
| A task, started at once, on a profile, with tools | `task.create` from the same conversation (the task's tools, postures, labels, working directory); `task.start --profile` for the model | `work_handed_to_a_profile_is_done_by_a_task_that_has_tools` |
| The profile used for a conversation's data | a turn on that profile (`session.profile`, `profile.hold`): the floor's refusal is `profile_refusal`, asked before anything opens | `a_profile_the_labels_forbid_is_refused_before_a_task_opens`, `an_unknown_profile_opens_nothing` |
| The task's limit | a task's default: a quarter of what the turn has left, carved from it and counted in it; there is no budget argument | the same, `nothing_but_the_profile_and_the_work_can_be_named` |
| The task's calls | judged by `policy.rs` and `runs_program` exactly as any task's are (both untouched); its cards go where a task's go and name the task, and an agent cannot answer one | `policy.rs` untouched; the task's turns are the existing ones |
| Starting a task with no card where `task.create` asks | the hand-over asks when `task.create` is on `approve` (`ask_if_task_asks`), and after outside text as the other profile tools do | `where_a_task_asks_first_so_does_the_hand_over`, `the_hook_touches_only_the_hand_over_and_only_where_a_task_asks` |
| Who may call it | the wall (`config_change::wall`): refused to a shared place, a participant who is not the owner, a task, a routine, the self loop and an MCP session; it is also in `routine::grant::NEVER` | `a_task_may_not_hand_work_to_a_profile`, the wall's own tests (`every_other_caller_is_refused_in_its_own_words`) |

## Outside text

A task's report is already treated as carrying a child session's text (`content.rs`, `ItemKind::TaskReport`); the report
that comes back through this tool is that same node and keeps that marking. The `subagent` post is the owner's display of
it, not text any model reads.

## What it costs a request

The `profile` family is deferred: it adds no tool definition to a request that does not load it. Loaded, the tool's
description and schema cost the tokens the test `the_profile_family_is_deferred_and_adds_nothing_to_a_request` prints.

## What it does not do

No agent loop of its own; no budget, wake, start time or profile argument beyond the two it takes; no change to the floor;
no answer to a card by the asking turn or model; no change to `profile.ask`'s limits.
