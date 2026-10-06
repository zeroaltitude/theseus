# Cloud report: telemetry-tests (theseus-qqhd, theseus-fk0g)

Tests only; no bug found, no production code changed (plants only, each restored and `touch`ed; `git status` clean after each).

## 1. qqhd: the cancel on the daemon's path: ae6cb54
- Found: `Core::build_telemetry`'s `stops.export_to` was held by no test.
- Changed: `telemetry/tests_cancel.rs::a_daemons_exporter_counts_the_cancels_health_counts`: parts with `telemetry: None`, config endpoint = receiver, `install_telemetry`, a background `proc_run` cancelled, flush. The `theseus.cancel` points equal health's `cancels` (here `inproc`/`unsupported` 1; no fixed labels asserted).
- Plant: `self.runner.tools.stops.export_to(t.clone());` removed from `build_telemetry`. Fails: `the daemon's metric is health's count  left: []  right: [("inproc", "unsupported", 1)]`. Passes restored.

## 2. qqhd: judgment and retention feeds: 261877f
- New `telemetry/tests_daemon_path.rs` (registered in telemetry.rs). `tests_retention.rs`: `t0`, `labeled`, `frame`, `built` made `pub(crate)` for it.
- `a_judgment_on_the_daemons_path_is_counted`: fake Jev, one judged turn; `theseus.judge.calls` for loop.v1/shadow/act/reply is 1. Plant (`judge.export_to` removed): `no the judge's calls: []`.
- `the_retention_gauge_on_the_daemons_path_is_the_projections_size`: `+retention` live, projection built (2 nodes) before `install_telemetry`, gauge reads 2; a third node via `retention_written` makes it 3. Plant (`memory.export_to` removed): `no the retention gauge: []`.
- Answer to the brief: the gauge is recorded both by `export_to` and by `retention_measured` when the build finishes and on each `retention_written`, so a projection built after serving shows its size too. That is right. (The plant fails at the first read; the growth half is checked passing, not by its own plant.)
- `Core::build` should hand memory its handle too: yes, one line (`runner.memory.export_to(t.clone())` beside the judge's). Not added: no test core needs it.

## 3. fk0g: a gauge moves between samples: 7a9575e
- `tests_tender.rs`: `stand_in_with(state, hang, Arc<Mutex<IndexStatus>>)`; `stand_in` calls it with today's answer.
- `tests_index.rs::the_gauges_move_with_the_tenders_answer_between_samples`: sample (4096/250/3/48 MiB/0), change to 0/0/7/64 MiB, ask `health(STATUS_DEADLINE)` once, sample; restarts stay 0.
- Plant in `Metrics::index` (`if p.int == 0 { p.int = v; }`): fails `left: [Some(4096), Some(250), Some(3), Some(50331648), Some(0)]  right: [Some(0), Some(0), Some(7), Some(67108864), Some(0)]`.

## Proof
- New tests alone: 4 passed. Five runs under load (`nice -n 19` test binary, four `yes` loops at nice 0; `kill` of pids was refused by the sandbox, so loops were `timeout`-bounded): 5/5 passed (4.7 to 5.2 s each).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): fmt (fixed once), shape, features, clippy, cockpit, test build, reader rule pass. Suite: 2844 run, 2811 passed, 33 failed, all the known L1 ones (theseus-sandbox contract tests, theseusd sandbox tests). Every `telemetry::`, tests_tender and rpc telemetry test passed, the old-exporter conformance test included (unchanged). I then ran by hand: protocol types (no diff), `cargo deny --offline check` (ok), `theseus-sim bench turn --check` (frames 5/5 and 9/9 ok). Lifecycle and jobs benches skipped (NO_BENCH).

## Live check (maintainer)
1. Scratch daemon, fresh state dir, Discord and web off, `[telemetry] otlp_endpoint` at a loopback OTLP sink, `metrics_interval_secs = 5`, `[tools] proc_sync_secs = 1`, a `theseus-sim fake-model --rules` rule calling `proc_run` with `sleep 60`; `theseus executions cancel <id>`. Expect the sink's `theseus.cancel` points to equal the `cancels since the start:` line of `theseus health`.
2. `[index] enabled = true` with theseus-index beside theseusd; a turn writing new words; once `theseus index status` shows more documents, the next post's `theseus.index.documents` shows the same number.

## Left
Nothing; no docs to change.
