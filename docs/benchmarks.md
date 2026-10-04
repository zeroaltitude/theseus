# Benchmarks

Theseus's results on public benchmarks are published here first: every run's configuration, model, attempts, score,
and cost. How to run one, and what each trial leaves behind, is in [`bench/README.md`](../bench/README.md).

## Terminal-Bench 2.0

89 tasks in their own containers, through [Harbor](https://github.com/laude-institute/harbor) 0.23, with the bench
profile (`bench/theseus-bench.toml`): no vault, every tool open, L0.

**No full run yet.** The first one fills this table: all 89 tasks, two attempts each, Theseus and another agent on the
same model.

| Date | Theseus build | Agent | Model | Tasks × attempts | Mean reward | Cost |
|---|---|---|---|---|---|---|

## How a result is reported

- The job's `result.json` from Harbor, and each trial's reward, time, and spend.
- Theseus's build (`theseus --version`) and the profile's settings (`THESEUS_BENCH_*`), so a run can be repeated.
- Trials that ended with an error, by Harbor's class for it: a timeout (`AgentTimeoutError`), a spend limit
  (`TheseusSpendLimitError`), a cut turn (`TheseusTurnCutError`), and so on (`bench/README.md`). Their rewards still
  count: the task's tests run after every trial.
