//! The seed by the store's terms (theseus-id8d): the board seeded from every
//! execution and the actions not settled holds the same views, questions,
//! model calls and counts as one seeded from every action ever, on mixed
//! stores made from a run of seeds; and the counts the board keeps as frames
//! apply equal a recount of its entries.

use std::sync::Arc;
use std::time::Instant;

use theseus_kernel::{
    ActionState, ExecState, Execution, SessionKind, Wake, BUDGET_TOOL, PROVIDER_TOOL,
};
use theseus_store::{kinds, NewRecord, Store as _};

use super::super::{seed, seed_from, seed_recent, Board, Frame, Push};
use super::{action, execution};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::{Config, Core};
use theseus_kernel::Action;
use theseus_protocol::{ExecutionView, Level};

fn core(dir: &tempfile::TempDir) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::scripted(vec![])),
        store,
    ))
    .unwrap()
}

/// A small generator: the same seed, the same store.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const STATES: [ExecState; 6] = [
    ExecState::Waiting,
    ExecState::Waiting,
    ExecState::Running,
    ExecState::Queued,
    ExecState::Complete,
    ExecState::Failed,
];

/// An execution of the run's `i`th session, in a state the seed picks.
fn an_execution(r: &mut Rng, i: u64, calls: &[String]) -> Execution {
    let mut e = execution(STATES[r.below(STATES.len() as u64) as usize], None);
    e.id = format!("exe_{i:06}");
    e.session_id = format!("ses_{i:06}");
    e.kind = if r.below(3) == 0 {
        SessionKind::Task
    } else {
        SessionKind::Conversation
    };
    e.parent = (e.kind == SessionKind::Task && i > 0).then(|| format!("exe_{:06}", r.below(i)));
    e.turns = r.below(9);
    e.updated_at_ms = 1_000 + r.below(1_000_000);
    e.wake = (e.state == ExecState::Waiting).then(|| match calls.first() {
        Some(c) if r.below(2) == 0 => Wake::Confirm {
            confirm_id: c.clone(),
        },
        _ => Wake::Input,
    });
    e.outstanding = calls.iter().filter(|_| r.below(2) == 0).cloned().collect();
    e
}

/// An action of `e`, its records as its life wrote them: planned, then on to
/// where the seed stops it (a question left asking, a call in flight, or a
/// settled end).
fn an_action(r: &mut Rng, e: &str, n: u64) -> Vec<Action> {
    let tools = ["proc.run", PROVIDER_TOOL, BUDGET_TOOL, "fs.write"];
    let mut a = action(tools[r.below(tools.len() as u64) as usize]);
    a.correlation_id = format!("cor_{e}_{n}");
    a.execution_id = e.to_string();
    a.session_id = e.replacen("exe_", "ses_", 1);
    a.planned_at_ms = 1_000 + r.below(100_000);
    let mut life = vec![a.clone()];
    let steps = [
        ActionState::Authorized,
        ActionState::Dispatched,
        ActionState::Succeeded,
        ActionState::Failed,
        ActionState::OutcomeUnknown,
        ActionState::Cancelled,
    ];
    for _ in 0..r.below(3) {
        a.state = steps[r.below(steps.len() as u64) as usize];
        life.push(a.clone());
        if a.state.is_settled() {
            break;
        }
    }
    life
}

/// A mixed store from `seed`: executions in every state, tasks under
/// parents, and actions whose records are spread over frames, many settled.
fn mixed_store(core: &Core, seed: u64) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let store = core.store.inner();
    let mut frames: Vec<Vec<NewRecord>> = Vec::new();
    // Each record is written at a later time than the one before, as the
    // kernel's clock writes them.
    let mut clock = 1_000;
    for i in 0..(5 + r.below(30)) {
        let id = format!("exe_{i:06}");
        let lives: Vec<_> = (0..r.below(5)).map(|n| an_action(&mut r, &id, n)).collect();
        let calls: Vec<String> = lives
            .iter()
            .filter(|l| !l.last().unwrap().state.is_settled())
            .map(|l| l[0].correlation_id.clone())
            .collect();
        for k in 0..1 + r.below(3) {
            let mut e = an_execution(&mut r, i, &calls);
            e.turns += k;
            clock += 1 + r.below(5);
            e.updated_at_ms = clock;
            frames.push(vec![
                NewRecord::json(kinds::EXECUTION, Some(&id), &e).unwrap()
            ]);
        }
        for life in lives {
            // A life's records in order: each in the frame of the one before
            // or a later one.
            let mut at = r.below(frames.len() as u64) as usize;
            for a in life {
                let rec = NewRecord::json(kinds::ACTION, Some(&a.correlation_id), &a).unwrap();
                frames[at].push(rec);
                at = (at + r.below(3) as usize).min(frames.len() - 1);
            }
        }
    }
    for f in frames {
        store.append(&f).unwrap();
    }
}

/// A view as a surface shows it, its position apart.
fn without_position(v: &ExecutionView) -> String {
    let mut v = serde_json::to_value(v).unwrap();
    v["position"] = 0.into();
    v.to_string()
}

/// A board left cold past its 6 most recent executions: every view it holds
/// is the full read's, every view that needs you or works is among them,
/// `executions.watch`'s answer is the full read's for each limit the recent
/// ones cover, and a cold session's wait loads its view, the full read's.
#[test]
fn a_board_left_cold_answers_as_the_full_read() {
    let mut cold_loaded = 0;
    for s in 1..=40u64 {
        let dir = tempfile::tempdir().unwrap();
        let core = core(&dir);
        mixed_store(&core, s);
        let recent = 6;
        let lazy = seed_recent(&core, recent).unwrap();
        let full = seed_from(
            &core,
            core.kernel.executions_at().unwrap(),
            core.kernel.actions_at().unwrap(),
        )
        .unwrap();
        for (id, e) in &lazy.entries {
            let (Some(v), Some(w)) = (&e.view, full.entries.get(id).and_then(|f| f.view.as_ref()))
            else {
                assert!(e.view.is_none(), "seed {s}: {id} is no execution");
                continue;
            };
            assert_eq!(without_position(v), without_position(w), "seed {s}");
            assert_eq!(e.pending, full.entries[id].pending, "seed {s}");
        }
        let active =
            |v: &ExecutionView| matches!(v.attention.level, Level::NeedsYou | Level::Working);
        let mut shown = 0;
        for (id, f) in &full.entries {
            if f.view.as_ref().is_some_and(active) {
                shown += 1;
                assert!(
                    lazy.entries[id].view.is_some(),
                    "seed {s}: {id} needs you or works"
                );
            }
        }
        assert_eq!((lazy.views, lazy.questions), lazy.recount(), "seed {s}");
        assert_eq!(lazy.questions, full.questions, "seed {s}");
        let total = core.kernel.count_executions().unwrap();
        assert_eq!(
            total, full.views,
            "seed {s}: the terms count every execution"
        );
        let lazy_push = Push::default();
        *lazy_push.board.lock().unwrap() = lazy;
        let full_push = Push::default();
        *full_push.board.lock().unwrap() = full;
        for limit in 0..=recent.saturating_sub(shown) {
            let (_, a, held) = lazy_push.snapshot(limit);
            let (_, b, all) = full_push.snapshot(limit);
            let a: Vec<String> = a.iter().map(without_position).collect();
            let b: Vec<String> = b.iter().map(without_position).collect();
            assert_eq!(a, b, "seed {s}, limit {limit}");
            assert_eq!(held.max(total), all, "seed {s}");
        }
        let views: Vec<ExecutionView> = full_push
            .board
            .lock()
            .unwrap()
            .entries
            .values()
            .filter_map(|e| e.view.clone())
            .collect();
        for w in views {
            let cold = lazy_push.view_of_session(&w.session_id).is_none();
            if cold {
                let mut rec =
                    crate::session::SessionRecord::with_id(w.session_id.clone(), w.kind, None);
                rec.execution_id = Some(w.execution_id.clone());
                core.store.put_session(&w.session_id, &rec).unwrap();
                cold_loaded += 1;
            }
            let v = lazy_push.view_or_load(&core, &w.session_id).unwrap();
            assert_eq!(without_position(&v), without_position(&w), "seed {s}");
        }
        let b = lazy_push.board.lock().unwrap();
        assert_eq!(
            (b.views, b.questions),
            b.recount(),
            "seed {s}: after the loads"
        );
    }
    assert!(cold_loaded > 0, "the run left some cold");
}

/// What a board shows of each execution, with its view's position apart:
/// the seed from open actions may know an older position for a view whose
/// newest action record is settled, never a newer one.
fn shown(b: &Board) -> Vec<(String, String, Option<u64>)> {
    let mut out: Vec<_> = b
        .entries
        .iter()
        .filter(|(_, e)| e.view.is_some() || !e.pending.is_empty())
        .map(|(id, e)| {
            let mut v = e.view.clone().map(|v| serde_json::to_value(v).unwrap());
            let at = v.as_mut().map(|v| {
                let p = v["position"].as_u64().unwrap();
                v["position"] = 0.into();
                p
            });
            let pending = serde_json::to_string(&e.pending).unwrap();
            (id.clone(), format!("{v:?} {pending} {:?}", e.models), at)
        })
        .collect();
    out.sort();
    out
}

/// Over forty mixed stores: the seed by terms holds what the full read
/// holds, its counts are a recount, its session index finds each view, and
/// its position is the store's newest execution or action.
#[test]
fn the_seed_by_terms_holds_what_every_action_ever_gives() {
    let mut asked = 0;
    for s in 1..=40u64 {
        let dir = tempfile::tempdir().unwrap();
        let core = core(&dir);
        mixed_store(&core, s);
        let by_terms = seed(&core).unwrap();
        let full = seed_from(
            &core,
            core.kernel.executions_at().unwrap(),
            core.kernel.actions_at().unwrap(),
        )
        .unwrap();
        let (a, b) = (shown(&by_terms), shown(&full));
        assert_eq!(a.len(), b.len(), "seed {s}");
        for (x, y) in a.iter().zip(&b) {
            assert_eq!((&x.0, &x.1), (&y.0, &y.1), "seed {s}");
            assert!(x.2 <= y.2, "seed {s}: a view's position is never newer");
        }
        assert_eq!(by_terms.position, full.position, "seed {s}");
        assert_eq!(
            (by_terms.views, by_terms.questions),
            full.recount(),
            "seed {s}"
        );
        assert_eq!((by_terms.views, by_terms.questions), by_terms.recount());
        for (id, e) in &by_terms.entries {
            if let Some(v) = &e.view {
                assert_eq!(by_terms.by_session.get(&v.session_id), Some(id), "seed {s}");
            }
        }
        asked += full.questions;
    }
    assert!(asked > 0, "the run's stores ask questions");
}

/// The counts the board keeps as frames apply (a view's first, a question
/// asked, answered, a settled call, an execution that ends) equal a recount.
#[test]
fn the_kept_counts_equal_a_recount_after_mixed_frames() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let mut b = Board::default();
    let mut r = Rng(0x5eed);
    let mut pos = 10;
    let frame = |records: Vec<(u16, Vec<u8>)>, pos: &mut u64| {
        let records = records
            .into_iter()
            .map(|(k, p)| {
                *pos += 1;
                (k, *pos, p)
            })
            .collect();
        Frame {
            at_ms: *pos,
            committed: Instant::now(),
            position: *pos,
            records,
            nodes: vec![],
        }
    };
    let mut asked = 0;
    for step in 0..400u64 {
        let i = r.below(12);
        let calls = vec![format!("cor_exe_{i:06}_{}", r.below(3))];
        let e = an_execution(&mut r, i, &calls);
        let mut records = vec![(kinds::EXECUTION, serde_json::to_vec(&e).unwrap())];
        for a in an_action(&mut r, &e.id, step % 3) {
            asked += usize::from(a.awaits_confirm());
            records.push((kinds::ACTION, serde_json::to_vec(&a).unwrap()));
        }
        let f = frame(records, &mut pos);
        b.apply(&core, &f);
        assert_eq!((b.views, b.questions), b.recount(), "step {step}");
    }
    assert!(asked > 0 && b.views > 0, "the run asked and viewed");
}

/// The seed reads no settled action and no cold execution: over a store of
/// 400 settled calls and 300 conversations parked on input, with 10 recent
/// executions asked for, it reads tens of records, not hundreds.
#[test]
fn the_seed_reads_no_settled_action_and_no_cold_execution() {
    let dir = tempfile::tempdir().unwrap();
    let core = core(&dir);
    let store = core.store.inner();
    let mut frame = Vec::new();
    for i in 0..300u64 {
        let mut e = execution(ExecState::Waiting, Some(Wake::Input));
        e.id = format!("exe_{i:06}");
        e.session_id = format!("ses_{i:06}");
        e.kind = SessionKind::Conversation;
        e.parent = None;
        frame.push(NewRecord::json(kinds::EXECUTION, Some(&e.id), &e).unwrap());
    }
    for i in 0..400u64 {
        let mut a = action("proc.run");
        a.correlation_id = format!("cor_{i:06}");
        a.execution_id = format!("exe_{:06}", i % 300);
        a.state = ActionState::Succeeded;
        frame.push(NewRecord::json(kinds::ACTION, Some(&a.correlation_id), &a).unwrap());
    }
    store.append(&frame).unwrap();
    let before = theseus_store::records_read_here();
    let b = seed_recent(&core, 10).unwrap();
    let read = theseus_store::records_read_here() - before;
    assert_eq!(b.views, 10, "the recent ones");
    // The walk for the recent ones reads 4 records a recent one, then the
    // newest action: 41, where reading every one is 700.
    assert!(read <= 60, "the seed read {read} records");
}
