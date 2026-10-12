//! Admission through the whole core (theseus-4g0l): the queue's class order
//! as turns meet it, and a foreground job's wait lending its slot. The
//! queue's own rules are `turn/tests_admission.rs`'s. A child of `tests_m3`
//! for its rig and helpers.

use super::*;

/// The marker each request's own session holds, in the order the model was
/// asked: one session's request holds no other's words.
fn asked(r: &Rig, markers: &[&str]) -> Vec<String> {
    r.fake
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|q| {
            let text = format!("{q:?}");
            markers
                .iter()
                .find(|m| text.contains(**m))
                .map(|m| m.to_string())
        })
        .collect()
}

/// A turn of a session of `kind`, its input `input`, spawned now.
fn spawn_turn(
    core: &Arc<Core>,
    kind: SessionKind,
    input: &str,
) -> tokio::task::JoinHandle<TurnSubmitResult> {
    let rec = SessionRecord::new(kind, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (core, input) = (core.clone(), input.to_string());
    tokio::spawn(async move { turn(&core, Some(&rec.session_id), &input).await })
}

/// Two task turns wait for the one slot, then a person's typed message
/// arrives: when the slot frees, the person's turn is admitted first, and
/// the tasks follow in their arrival order. Before, every waiter polled each
/// 50 ms and the first to look took the slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_typed_turn_is_admitted_before_queued_task_turns() {
    let (held, mut there, go) = Held::new();
    let r = rig_parts(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["touch", "holder-ran"]}))],
            ),
            Scripted::text("The holder is done."),
            Scripted::text("One answer."),
            Scripted::text("Another answer."),
            Scripted::text("A third answer."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.server.disk_floor_mb = 1024;
            cfg.kernel.admission_ceiling = 1;
        },
        |_| {},
    );
    r.core.tools.disk.set_probe(Arc::new(HeldDiskCheck {
        held,
        once: std::sync::Once::new(),
    }));
    // The holder's turn takes the one slot, its job held before its launch.
    let holder = spawn_turn(&r.core, SessionKind::Conversation, "holder: touch it");
    there.recv().await.unwrap();
    let mut waiting = Vec::new();
    for (kind, input) in [
        (SessionKind::Task, "task-one-marker"),
        (SessionKind::Task, "task-two-marker"),
        (SessionKind::Conversation, "typed-marker"),
    ] {
        waiting.push(spawn_turn(&r.core, kind, input));
        // Each joins the queue before the next arrives.
        for _ in 0..50 {
            if r.core.tools.admission.load().1 == waiting.len() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(r.core.tools.admission.load(), (1, waiting.len()));
    }
    go.send(()).unwrap();
    let holder = tokio::time::timeout(Duration::from_secs(20), holder)
        .await
        .expect("the holder's turn ended")
        .unwrap();
    assert_eq!(holder.output, "The holder is done.");
    for w in waiting {
        tokio::time::timeout(Duration::from_secs(20), w)
            .await
            .expect("every waiting turn ran")
            .unwrap();
    }
    assert_eq!(
        asked(
            &r,
            &[
                "holder",
                "task-one-marker",
                "task-two-marker",
                "typed-marker"
            ]
        ),
        [
            "holder",
            "holder",
            "typed-marker",
            "task-one-marker",
            "task-two-marker"
        ],
        "the typed turn first, then the tasks in their order"
    );
    assert_eq!(r.core.tools.admission.load(), (0, 0));
}

/// A turn whose foreground job runs past `LEND_AFTER` gives its slot to the
/// next turn, which runs while the job does, and takes a slot back when the
/// job ends: its result is read in the same turn. Before, the slot was held
/// for the whole job, and the next turn waited for it. The job runs until
/// the test lets it go, so no step here leans on a clock but the lend's own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreground_jobs_wait_lends_its_slot_and_takes_it_back() {
    /// Lets the job go when the test ends, passed or failed, so a failure
    /// leaves no job behind.
    struct LetGo(std::path::PathBuf);
    impl Drop for LetGo {
        fn drop(&mut self) {
            let _ = std::fs::write(&self.0, b"");
        }
    }
    let release = tempfile::tempdir().unwrap();
    let gate = release.path().join("go");
    let _let_go = LetGo(gate.clone());
    let script = format!("while [ ! -e {} ]; do sleep 0.05; done", gate.display());
    let r = rig_parts(
        vec![
            Scripted::tools(
                "",
                &[("t1", "proc_run", json!({"argv": ["sh", "-c", script]}))],
            ),
            Scripted::text("Answered while the job ran."),
            Scripted::text("The job is done."),
        ],
        |cfg| {
            cfg.policy.tools.insert("proc.run".into(), Posture::Open);
            cfg.tools.proc_sync_secs = 120;
            cfg.kernel.admission_ceiling = 1;
        },
        |_| {},
    );
    let jobbed = spawn_turn(&r.core, SessionKind::Conversation, "run the long job");
    // The job's turn lends its slot once its job has run `LEND_AFTER`.
    let lent = async {
        while !(r.core.tools.admission.admitted() >= 1 && r.core.tools.admission.load() == (0, 0)) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(60), lent)
        .await
        .expect("the job's turn lent its slot");
    let other = tokio::time::timeout(
        Duration::from_secs(60),
        turn(&r.core, None, "a quick question"),
    )
    .await
    .expect("the next turn ran on the lent slot");
    assert_eq!(other.output, "Answered while the job ran.");
    assert!(
        !jobbed.is_finished(),
        "it ended while the job still ran, its turn still waiting on it"
    );
    std::fs::write(&gate, b"").unwrap();
    let jobbed = tokio::time::timeout(Duration::from_secs(60), jobbed)
        .await
        .expect("the job's turn ended")
        .unwrap();
    assert_eq!(jobbed.output, "The job is done.");
    assert_eq!(jobbed.tool_calls, 1, "its result read in its own turn");
    assert_eq!(r.core.tools.admission.load(), (0, 0), "nothing held after");
}
