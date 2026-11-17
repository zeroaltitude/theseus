use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn a_write_is_whole_and_keeps_the_old_permissions() {
    let d = tmp();
    let f = d.path().join("theseus.toml");
    std::fs::write(&f, "a = 1\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
    write_config(&f, "a = 2\n").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "a = 2\n");
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o7777,
        0o640
    );
    let left: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left.len(), 1, "no temporary file is left: {left:?}");
}

#[test]
fn the_last_ten_previous_copies_are_kept_numbered() {
    let d = tmp();
    for i in 1..=13 {
        let (n, p) = keep_previous(d.path(), &format!("v = {i}\n")).unwrap();
        assert_eq!(n, i);
        assert_eq!(std::fs::read_to_string(p).unwrap(), format!("v = {i}\n"));
    }
    let kept: Vec<u32> = previous(d.path()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(kept, (4..=13).collect::<Vec<u32>>());
    assert_eq!(
        std::fs::metadata(&previous(d.path())[0].1)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn a_candidate_is_read_back_only_if_it_is_the_one_kept() {
    let d = tmp();
    let sha = keep_candidate(d.path(), "x = 1\n").unwrap();
    assert_eq!(read_candidate(d.path(), &sha).unwrap(), "x = 1\n");
    std::fs::write(candidate_path(d.path(), &sha), "x = 2\n").unwrap();
    assert!(read_candidate(d.path(), &sha)
        .unwrap_err()
        .contains("not the one"));
    drop_candidate(d.path(), &sha);
    assert!(read_candidate(d.path(), &sha).is_err());
}
