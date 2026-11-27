//! The root helper's install is "the loop is installing" from its build on (issue 41, round 3): the pipeline's view
//! names it while a stand-in build is held, before any record of its stages exists.

use crate::rsi::join::tests_publish::PassYes as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use theseus_protocol::rsi::SelfPipelineParams;

use super::tests::rooted;
use super::Seams;
use crate::config::self_improve::SelfInstall;
use crate::rsi::join::install;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_view_names_the_root_helpers_build_while_it_runs() {
    let t = rooted(SelfInstall::Auto, None);
    let held = Arc::new((Mutex::new(true), Condvar::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let (h, e) = (held.clone(), entered.clone());
    t.r.core.tools.root_install.stand_in(Seams {
        helper: t.helper.clone(),
        units: t.units.clone(),
        build: Arc::new(move |_, tree, _| {
            e.store(true, Ordering::SeqCst);
            let (lock, cv) = &*h;
            let mut still = lock.lock().unwrap();
            while *still {
                still = cv.wait(still).unwrap();
            }
            let out = tree.target.join("release");
            std::fs::create_dir_all(&out)?;
            for b in install::BINARIES {
                std::fs::write(out.join(b), format!("fake {b}"))?;
            }
            Ok(())
        }),
    });
    let core = t.r.core.clone();
    let seams = t.r.seams();
    t.r.seed(
        "trim-the-sails",
        crate::rsi::join::tests::PROVED,
        crate::rsi::join::tests::SONNET,
    );
    let pass = tokio::spawn(async move { core.self_join_pass_yes(&seams).await });
    for _ in 0..600 {
        if entered.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(entered.load(Ordering::SeqCst), "the build began");
    assert_eq!(t.r.core.root_pending(), None, "no record of its stages yet");
    let v =
        t.r.core
            .self_pipeline(&SelfPipelineParams::default())
            .unwrap();
    assert_eq!(v.installing.len(), 1, "{v:?}");
    let why = v.installing[0].why.clone().unwrap();
    assert!(why.contains("through the root helper"), "{why}");
    {
        let (lock, cv) = &*held;
        *lock.lock().unwrap() = false;
        cv.notify_all();
    }
    pass.await.unwrap().unwrap();
}
