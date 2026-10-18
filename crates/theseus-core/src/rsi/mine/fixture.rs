//! A synthetic ledger for the miner's tests: invented repositories, sessions
//! and days, scripted as the transcript would show them, folded into runs by
//! the same `call_of` the night uses. The owner's example is in it: a sync
//! stretch (fetch, pull, test baseline) and a land stretch (fetch, test,
//! merge, a conflict that rerere resolves or leaves, test, push) with the
//! model's edits between them.

use serde_json::json;

use super::read::{call_of, Env};
use super::shape::{normalize, result_class, Where};
use super::{Loop, Run, Vocab};

pub const TEST: &[&str] = &["cargo", "nextest", "run", "-p", "harbord"];
pub const REPO: &str = "r:0000000000feed01";

/// How a land ends.
#[derive(Clone, Copy, Debug)]
pub enum Land {
    Clean,
    RerereResolved,
    RerereLeft,
}

pub struct Ledger {
    pub vocab: Vocab,
    pub runs: Vec<Run>,
    pos: u64,
    constants: usize,
}

pub fn env<'a>(
    clean: &'a dyn Fn(&str) -> bool,
    session: &'a dyn Fn(&str) -> Option<super::read::SessionInfo>,
) -> Env<'a> {
    Env {
        key: b"fixture-key".to_vec(),
        clean,
        session,
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            vocab: Vocab::default(),
            runs: vec![],
            pos: 100,
            constants: 0,
        }
    }

    /// A new run (one turn), and its index.
    pub fn run(&mut self, session: &str, day: &str, scope: &str) -> usize {
        self.runs.push(Run {
            session: session.into(),
            turn: format!("t-{session}-{day}-{}", self.runs.len()),
            task: String::new(),
            day: day.into(),
            scope: scope.into(),
            place: "cli".into(),
            calls: vec![],
            loops: vec![],
            last_ms: 1_000 + self.runs.len() as u64,
        });
        self.runs.len() - 1
    }

    /// The model answers: a loop with a cost, which the next calls belong to.
    pub fn model(&mut self, run: usize) {
        self.runs[run].loops.push(Loop {
            tokens: [1_000, 50_000, 2_000, 300],
            usd_micros: 21_000,
            model_ms: 4_000,
            calls: 0,
        });
    }

    /// A tool call and its result.
    pub fn call(&mut self, run: usize, tool: &str, input: serde_json::Value, exit: i64, out: &str) {
        let clean = |_: &str| true;
        let none = |_: &str| None;
        let env = env(&clean, &none);
        let class = result_class("ok", exit != 0, Some(exit), out);
        let l = self.runs[run].loops.len().saturating_sub(1) as u32;
        for n in normalize(tool, &input, &Where::default()) {
            let mut c = call_of(&env, &mut self.vocab, &n, &mut self.constants);
            c.res = self.vocab.add_result(&class);
            c.loop_ix = l;
            c.pos = self.pos;
            c.ms = 1_500;
            self.pos += 2;
            self.runs[run].calls.push(c);
            if let Some(lp) = self.runs[run].loops.last_mut() {
                lp.calls += 1;
            }
        }
    }

    pub fn argv(&mut self, run: usize, argv: &[&str], exit: i64, out: &str) {
        self.call(run, "proc.run", json!({ "argv": argv }), exit, out);
    }

    /// The model's own edit: it cuts the stretch.
    pub fn edit(&mut self, run: usize, path: &str) {
        self.model(run);
        self.call(
            run,
            "fs.edit",
            json!({"path": path, "old_string": "a", "new_string": "b"}),
            0,
            "edited",
        );
    }

    /// The sync stretch.
    pub fn sync(&mut self, run: usize) {
        self.model(run);
        self.argv(run, &["git", "fetch", "origin"], 0, "");
        self.argv(run, &["git", "pull", "--ff-only"], 0, "Already up to date.");
        self.argv(run, TEST, 0, "test result: ok. 12 passed");
    }

    /// The land stretch, ending as `how` says.
    pub fn land(&mut self, run: usize, branch: &str, how: Land) {
        self.model(run);
        self.argv(run, &["git", "fetch", "origin"], 0, "");
        self.argv(run, TEST, 0, "test result: ok. 12 passed");
        self.model(run);
        match how {
            Land::Clean => {
                self.argv(
                    run,
                    &["git", "merge", "--no-ff", branch],
                    0,
                    "Merge made by the 'ort' strategy.",
                );
                self.argv(run, TEST, 0, "test result: ok. 13 passed");
                self.argv(run, &["git", "push", "origin", "main"], 0, "main -> main");
            }
            Land::RerereResolved => {
                self.argv(
                    run,
                    &["git", "merge", "--no-ff", branch],
                    1,
                    "Resolved 'src/a.rs' using previous resolution.\nAutomatic merge failed; fix conflicts and then commit the result.",
                );
                self.model(run);
                self.argv(
                    run,
                    &["git", "commit", "--no-edit"],
                    0,
                    "[main abc1234] Merge",
                );
                self.argv(run, TEST, 0, "test result: ok. 13 passed");
                self.argv(run, &["git", "push", "origin", "main"], 0, "main -> main");
            }
            Land::RerereLeft => {
                self.argv(
                    run,
                    &["git", "merge", "--no-ff", branch],
                    1,
                    "Recorded preimage for 'src/b.rs'\nCONFLICT (content): Merge conflict in src/b.rs\nAutomatic merge failed; fix conflicts and then commit the result.",
                );
            }
        }
    }

    /// Noise: what a session does between the flows, in four kinds.
    pub fn noise(&mut self, run: usize, k: usize) {
        self.model(run);
        match k % 4 {
            0 => {
                for i in 0..2 + k % 3 {
                    self.call(
                        run,
                        "fs.read",
                        json!({"path": format!("crates/x{}/src/lib.rs", i + k)}),
                        0,
                        "text",
                    );
                }
                self.call(
                    run,
                    "fs.grep",
                    json!({"pattern": format!("needle{k}"), "path": "crates"}),
                    0,
                    "hit",
                );
            }
            1 => self.argv(run, &["git", "status", "--short"], 0, ""),
            2 => self.call(run, "fs.list", json!({"path": "crates"}), 0, "a b"),
            _ => {}
        }
    }

    /// The owner's example across 6 sessions on 4 days.
    pub fn owners_example(&mut self, scope: &str) {
        let plan = [
            ("s-ash", "2026-10-10", Land::Clean),
            ("s-birch", "2026-10-10", Land::Clean),
            ("s-cedar", "2026-10-11", Land::RerereResolved),
            ("s-dune", "2026-10-12", Land::Clean),
            ("s-elm", "2026-10-12", Land::RerereLeft),
            ("s-fern", "2026-10-13", Land::RerereResolved),
        ];
        for (i, (s, d, how)) in plan.into_iter().enumerate() {
            let r = self.run(s, d, scope);
            self.noise(r, i);
            self.sync(r);
            self.edit(r, &format!("crates/harbord/src/m{i}.rs"));
            self.noise(r, i + 1);
            self.land(r, &format!("feat/job-{i}"), how);
            if matches!(how, Land::RerereLeft) {
                self.edit(r, "crates/harbord/src/b.rs");
            }
        }
    }
}
