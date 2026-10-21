//! What the turn-end facts keep in memory goes when its job's output does
//! (theseus-53ujg.5, F3): the spool's sweep removes a job's raw output while
//! its wrapper still lived at the result's cleanup, and the job's listing and
//! line in `beside`'s book go with it.

use std::time::{Duration, SystemTime};

use crate::provider::FakeProvider;
use crate::store::Store;
use crate::toolrun::job::beside;
use crate::Core;

#[test]
fn a_swept_jobs_book_entries_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let parts =
        crate::rpc::Parts::for_tests(cfg, std::sync::Arc::new(FakeProvider::default()), store);
    let core = Core::build(parts).unwrap();

    // A job's listing at its launch, and a stale output of its in the spool.
    let id = "act_swept_leak";
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    beside::before("exec_leak", id, &work);
    assert!(beside::holds(id));
    let out = core.spool.result_path(id);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, "output\n").unwrap();
    let old = SystemTime::now() - Duration::from_secs(3 * 24 * 3600);
    std::fs::File::options()
        .write(true)
        .open(&out)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let swept = crate::sweep::sweep(&core.kernel, &core.store, &core.spool, SystemTime::now());
    assert!(!out.exists(), "the sweep left the output: {swept:?}");
    assert!(
        !beside::holds(id),
        "the book still holds the job: {swept:?}"
    );
}
