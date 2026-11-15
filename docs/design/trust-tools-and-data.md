# Issue 13: trust, tools and data (1.0.7)

Notes for the core crate that the core's own guide has no room for.

## Chat acts: seal, move, and a project's items

No act on what a person says is reached by matching the words of a message. `seal_by_word` and `moved_by_words`
(and `is_seal_word`, `moved_words`, `words_may_move`) are gone; each act is a tool the model calls, offered only where
the act is possible, and each has a command.

- `chat.seal_thread` (`chat_acts/threads.rs`): offered in the session of an unsealed thread, takes that thread's slug
  and nothing else, seals through `thread.seal` as the owner who spoke. Commands: `/thread seal`, `theseus thread seal`.
- `chat.move_to_thread` (`chat_acts/threads.rs`): offered where the router placed a message before this one and another
  live thread, or home, exists. It stages the move; `Core::carry_out_move` runs it when the turn ends, because a move
  stops a turn running in the wrong thread, which can be the turn making the call. Commands: `/move` and
  `theseus thread move`, which act at once. (The owner's answer 7: A.)
- `project.item_add|close|list` (`chat_acts/items.rs`): offered in a conversation attached to a project. An item is a
  task record of kind `item` under the project (what a plan step already was), so there is no new record kind and no
  store format change. Wire: `project.item.add|close|list` (`rpc/project_items.rs`). Commands: `theseus project item`,
  `/project item`.
- Thread routing still reads words (`thread_route::address`, `pre_route`) and chooses a thread for a message; it moves
  nothing already written. Two judges read words for hints. None of it acts on a match.

## A refusal is a call that never ran

`toolrun::invalid_input` writes a refusal by a place, a mode or a glide as `Declined`, as a hold already was, so the
repeat cap (`turn/repeat_step.rs`) bounds a model that asks for the same refused call again. Invalid input stays `Error`.

## Sandbox residue

- A `sandbox.limit_hit` row for each limit an L1 job's end shows (`output_mb` by `SIGXFSZ`; `scratch_mb` when what the job
  wrote to scratch occupies the whole limit, from `Scratch.allocated`). Never `pids` (no cgroup), and not a full `/tmp`,
  `/dev/shm` or HOME. Counted as `theseus.sandbox.limit_hits{limit}`; a job's start as `theseus.job.start_ms{class}`,
  both once per result (`Sandbox::counted`).
- A job a cancel stops writes no completion, so its wrapper leaves its `detail` in the spool (`results/<id>.detail`)
  before its verdict; the stopped job's result reads it, and its `sandbox.egress` rows ride in the frame the cancel's
  answer writes (theseus-zupk). Its limits are still not read.
- `theseusd example-config --for-this-host` is the template with `[sandbox] default` chosen by L1's own self-test
  (`sandbox/new_install.rs`); `scripts/setup.sh` writes a new config from it. The built-in default stays `l0`.
- `proc.run` stops offering `sandbox` for a root daemon (`Run::without_sandbox`); a container that cannot make user
  namespaces is found at the first call.

## TLS roots

Written down and tested in `crates/theseusd/tests/tls_roots.rs`: HTTPS calls trust the bundled set only; the Discord
gateway trusts the bundled set and the machine's; the voice gateway's socket the machine's alone.
