//! The spool's sweep with the real `theseusd` (theseus-2ij): it runs after
//! serving, as a tender, never on the start path. A job's raw output that no
//! job in the store owns goes once it is a day old (as a file from before H3
//! would, 0644); a younger one stays. One `spool.swept` row says so, and
//! health shows the sweep. So does the blobs' sweep of the temporary files a
//! dead writer left (theseus-vipg).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

#[test]
fn the_sweep_runs_after_serving_and_removes_a_stale_output_no_job_owns() {
    let served = common::Served::start(
        |dir| {
            let results = dir.join("state/spool/results");
            std::fs::create_dir_all(&results).unwrap();
            let stale = results.join("act_invented_stale.out");
            std::fs::write(&stale, "stale\n").unwrap();
            std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&stale)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(3 * 24 * 3600))
                .unwrap();
            std::fs::write(results.join("act_invented_new.out"), "new\n").unwrap();
        },
        |_| {},
    );
    let results = served.path("state/spool/results");
    let deadline = Instant::now() + Duration::from_secs(15);
    let sweep = loop {
        let h = served.call("health", Value::Null).unwrap();
        if let Some(s) = h["spool"].get("last_sweep").filter(|s| !s.is_null()) {
            break s.clone();
        }
        assert!(
            Instant::now() < deadline,
            "no sweep in 15 s:\n{}",
            served.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(sweep["removed"], 1, "{sweep}");
    assert_eq!(sweep["removed_by"]["unknown"], 1, "{sweep}");
    assert_eq!(sweep["kept_by"]["young"], 1, "{sweep}");
    assert!(!results.join("act_invented_stale.out").exists());
    assert!(results.join("act_invented_new.out").exists());
    let rows = served
        .call("ledger.tail", json!({"n": 50, "kind": "spool.swept"}))
        .unwrap();
    let rows = rows["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["data"]["removed_bytes"], "stale\n".len());
}

/// The temporary files a crash inside a write leaves in the store's blobs
/// (theseus-vipg) go once the daemon serves: one of another process's, older
/// than the start, is removed; a blob, and another process's temporary
/// newer than the start, stay.
#[test]
fn a_dead_writers_temporary_blob_files_go_after_serving() {
    use theseus_core::blobs::digest;
    // A pid no process can have (past Linux's largest), so never the daemon's.
    const DEAD: u32 = 4_000_000_000;
    let blob: &[u8] = b"a blob its writer finished";
    let stale = format!(".{}.tmp-{DEAD}-7", digest(b"a state the crash cut"));
    let newer = format!(".{}.tmp-{DEAD}-8", digest(b"a state still written"));
    let served = common::Served::start(
        |dir| {
            let blobs = dir.join("state/store/blobs");
            std::fs::create_dir_all(&blobs).unwrap();
            std::fs::write(blobs.join(digest(blob)), blob).unwrap();
            let hour = Duration::from_secs(3600);
            for (name, at) in [
                (&stale, SystemTime::now() - hour),
                (&newer, SystemTime::now() + hour),
            ] {
                std::fs::File::create(blobs.join(name))
                    .unwrap()
                    .set_modified(at)
                    .unwrap();
            }
        },
        |_| {},
    );
    let blobs = served.path("state/store/blobs");
    let deadline = Instant::now() + Duration::from_secs(15);
    while blobs.join(&stale).exists() {
        assert!(
            Instant::now() < deadline,
            "the dead writer's temporary is still there after 15 s:\n{}",
            served.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(blobs.join(&newer).exists(), "newer than the start: kept");
    assert_eq!(std::fs::read(blobs.join(digest(blob))).unwrap(), blob);
}
