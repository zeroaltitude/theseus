use std::time::Duration;

use super::*;

fn pending(dir: &Path, text_before: &str, text_after: &str) -> (PathBuf, Pending) {
    let file = dir.join("theseus.toml");
    std::fs::write(&file, text_after).unwrap();
    let (_, previous) = write::keep_previous(dir, text_before).unwrap();
    let p = Pending {
        id: "act_1".into(),
        file,
        previous,
        previous_sha256: sha256(text_before),
        digest_before: sha256(text_before),
        digest_after: sha256(text_after),
        paths: vec!["import.notes.enabled".into()],
        session_id: "ses_1".into(),
        place: None,
        approved_by: "cli".into(),
        via: "cli".into(),
        requested_ms: 1,
        stage: Stage::Restarting,
        restart_only: false,
        took_ms: None,
        why: None,
    };
    let marker = marker_path(dir);
    save(&marker, &p).unwrap();
    (marker, p)
}

#[test]
fn a_first_start_watches_and_a_second_that_found_it_unconfirmed_goes_back() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        begin(&marker_path(d.path())),
        Begin::Normal,
        "nothing pending"
    );
    let (marker, p) = pending(d.path(), "a = 1\n", "a = 2\n");
    match begin(&marker) {
        Begin::Watch(w) => assert_eq!((w.stage, w.id.as_str()), (Stage::Started, "act_1")),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        load(&marker).unwrap().stage,
        Stage::Started,
        "the mark is kept"
    );
    // The start died before it confirmed, and was started again.
    assert!(matches!(begin(&marker), Begin::RollBackNow(_)));
    let _ = p;
}

#[test]
fn after_a_rollback_one_start_tells_and_no_start_restarts_again() {
    let d = tempfile::tempdir().unwrap();
    let (marker, mut p) = pending(d.path(), "a = 1\n", "a = 2\n");
    let own = p.file.clone();
    roll_back(&marker, &mut p, "it did not load", &own).unwrap();
    assert_eq!(std::fs::read_to_string(&p.file).unwrap(), "a = 1\n");
    assert!(
        matches!(begin(&marker), Begin::Tell(t) if t.why.as_deref() == Some("it did not load"))
    );
    // Told: the marker is put away, so the next start finds nothing.
    finish(&marker, &p);
    assert_eq!(begin(&marker), Begin::Normal);
    assert!(write::dir(d.path()).join("done-act_1.json").exists());
    // And a rolled-back marker is never a reason to roll back or restart.
    let (marker, mut p) = pending(d.path(), "a = 1\n", "a = 2\n");
    let own = p.file.clone();
    roll_back(&marker, &mut p, "x", &own).unwrap();
    for _ in 0..3 {
        assert!(matches!(begin(&marker), Begin::Tell(_)));
    }
}

#[test]
fn a_rollback_never_overwrites_a_later_edit_or_restores_a_replaced_copy() {
    let d = tempfile::tempdir().unwrap();
    let (marker, mut p) = pending(d.path(), "a = 1\n", "a = 2\n");
    std::fs::write(&p.file, "a = 3\n").unwrap();
    let own = p.file.clone();
    let e = roll_back(&marker, &mut p, "x", &own).unwrap_err();
    assert!(e.contains("no longer the one the change wrote"), "{e}");
    assert_eq!(std::fs::read_to_string(&p.file).unwrap(), "a = 3\n");
    std::fs::write(&p.file, "a = 2\n").unwrap();
    std::fs::write(&p.previous, "a = 666\n").unwrap();
    let e = roll_back(&marker, &mut p, "x", &own).unwrap_err();
    assert!(e.contains("not the one that was kept"), "{e}");
    assert_eq!(std::fs::read_to_string(&p.file).unwrap(), "a = 2\n");
    assert_eq!(
        load(&marker).unwrap().stage,
        Stage::Restarting,
        "nothing was marked"
    );
}

#[tokio::test(start_paused = true)]
async fn health_is_waited_for_and_given_up_on_after_the_stated_time() {
    let calls = std::sync::atomic::AtomicU32::new(0);
    let later = || {
        let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            match n {
                0..=2 => Err(format!("not yet ({n})")),
                _ => Ok(()),
            }
        }
    };
    let took = watch_health(later, Duration::from_secs(120), Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(took, Duration::from_secs(6));
    let never = || async { Err::<(), _>("the index tender is down".to_string()) };
    let e = watch_health(never, Duration::from_secs(120), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(e, "the index tender is down");
}

/// A marker is only a marker: one that names another file to overwrite, or a
/// copy that is not one this daemon kept beside it, is refused and nothing is
/// written, whatever its digests say.
#[test]
fn a_marker_cannot_name_a_file_to_overwrite_or_a_copy_the_daemon_did_not_keep() {
    let d = tempfile::tempdir().unwrap();
    let (marker, mut p) = pending(d.path(), "a = 1\n", "a = 2\n");
    let other = d.path().join("elsewhere.toml");
    std::fs::write(&other, "a = 2\n").unwrap();
    let e = roll_back(&marker, &mut p, "x", &other).unwrap_err();
    assert!(e.contains("not this daemon's own"), "{e}");
    // A copy outside the directory the marker is in, with the right digest.
    let own = p.file.clone();
    let planted = d.path().join("previous-0001.toml");
    std::fs::write(&planted, "a = 666\n").unwrap();
    p.previous = planted;
    p.previous_sha256 = sha256("a = 666\n");
    let e = roll_back(&marker, &mut p, "x", &own).unwrap_err();
    assert!(e.contains("not one this daemon kept"), "{e}");
    assert_eq!(std::fs::read_to_string(&own).unwrap(), "a = 2\n");
}
