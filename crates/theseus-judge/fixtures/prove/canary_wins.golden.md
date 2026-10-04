# Prove report: JUDGE_STOP canary against control

## Verdict: canary_better

- canary: 40 tasks, 40 labeled, 30 successes, $20.00 spent
- control: 40 tasks, 40 labeled, 20 successes, $20.00 spent
- completions per dollar, canary minus control: +0.500 [+0.085, +0.915]
- completion per task, canary minus control: +0.250 [+0.038, +0.433]
- canary is better per dollar, and not worse per task

Minimum: 30 labeled tasks per arm; 30 labeled items per precision, recall, false-completion, or nudge rate. Intervals are 95%.

## Cohorts

| | canary | control |
|---|---|---|
| tasks | 40 | 40 |
| labeled tasks | 40 | 40 |
| outcome unknown | 0 | 0 |
| successes | 30 | 20 |
| total spend (judge included) | $20.00 | $20.00 |
| of which the judge's | $0.80 | $0.00 |
| turns | 230 | 300 |
| nudges sent | 36 | 0 |

Canary spend over control: 1.00 (equal total spend).

## Per task

| metric | canary | control |
|---|---|---|
| completion | 75.0% [59.8, 85.8] (n=40) | 50.0% [35.2, 64.8] (n=40) |
| spend per task (USD) | 0.500 [0.500, 0.500] (n=40) | 0.500 [0.500, 0.500] (n=40) |
| turns per task | 5.75 [5.34, 6.16] (n=40) | 7.50 [7.03, 7.97] (n=40) |
| false completion (of tasks called complete) | 8.6% [3.0, 22.4] (n=35) | 42.9% [28.0, 59.1] (n=35) |
| unnecessary nudges (of nudges sent) | 16.7% [7.9, 31.9] (n=36) | insufficient (nudges sent: 0 of 30) |
| stop precision (labeled stops) | 87.5% [71.9, 95.0] (n=32) | 50.0% [35.2, 64.8] (n=40) |
| stop recall (labeled should-stop) | 93.3% [78.7, 98.2] (n=30) | insufficient (labeled should-stop tasks: 20 of 30) |

Completion per task, canary minus control: +0.250 [+0.038, +0.433]

## Per dollar

| metric | canary | control |
|---|---|---|
| completions per USD | 1.500 [1.228, 1.772] (n=40) | 1.000 [0.686, 1.314] (n=40) |
| turns per USD | 11.50 [10.68, 12.32] (n=40) | 15.00 [14.06, 15.94] (n=40) |

Completions per USD, canary minus control: +0.500 [+0.085, +0.915]

