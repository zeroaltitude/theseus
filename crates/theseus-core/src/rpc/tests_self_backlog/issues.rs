//! The GitHub issue source through the whole night (theseus-dl4pu): an issue
//! becomes an item on the backlog that names the issue it came from, and a
//! source that cannot read leaves the rest of the backlog standing.
//!
//! Every name here is invented. The repository, the issue numbers and the
//! issue titles are made up for this test and describe no real issue.

use super::*;

/// The invented listing, in the shape GitHub's issues endpoint returns.
const LISTING: &str = r#"[
  {"number": 118, "title": "The berth lights flicker after a reboot",
   "body": "They come back dim and settle after a minute.",
   "state": "open", "updated_at": "2026-11-12T10:00:00Z",
   "labels": [{"name": "1.0.6"}]},
  {"number": 119, "title": "A chart of the shallow approaches",
   "body": "An idea, not a defect.", "state": "open",
   "updated_at": "2026-11-11T10:00:00Z", "labels": [{"name": "1.0.6"}]}
]"#;

const REPO: &str = "harbourworks/lanterns";

/// A core that acts, reading its issues from `file`.
fn with_issues(file: &str, repo: &str) -> Arc<Core> {
    let base = test_core("the tide is out");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = SelfMode::Act;
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.backlog_starts = Some(0);
    cfg.self_improve.backlog_max_open = 20;
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.self_issues = crate::config::self_issues::SelfIssuesConfig {
        enabled: true,
        repo: repo.into(),
        file: file.into(),
        version_label: String::new(),
        max: 20,
    };
    let repo_fixture = crate::rsi::tests_tree::fixture();
    cfg.self_improve.repo = Some(repo_fixture.place.repo.display().to_string());
    std::mem::forget(repo_fixture);
    let core = Core::build(Parts::for_tests(
        cfg,
        Arc::new(FakeProvider::default()),
        base.store.clone(),
    ))
    .unwrap();
    core.tools
        .self_pr
        .set_env(crate::rsi::tests_tree::hermetic());
    core.self_resume(&SelfResumeParams::default(), &cli())
        .unwrap();
    core
}

/// A file holding `text`, in a directory of this test's own.
fn listing_file(tag: &str, text: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("theseus-self-issues-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("issues.json");
    std::fs::write(&p, text).unwrap();
    p
}

/// The card's first bar: the candidate list holds an item that came from a
/// GitHub issue, and the item NAMES the issue (number and title), beside the
/// items the night's own record gave it.
#[test]
fn an_issue_becomes_a_backlog_item_that_names_the_issue_it_came_from() {
    let file = listing_file("ok", LISTING);
    let c = with_issues(&file.display().to_string(), REPO);
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let night = c.backlog_night(tick()).unwrap();
    assert_eq!(night.gate, "allowed");
    assert_eq!(night.issues, 2, "both open issues read: {night:?}");
    assert_eq!(night.issues_failed, None);

    let items = listed(&c);
    // The positive counts first, so an empty backlog cannot pass: the ledger's
    // own three causes are still there, and the two issues are added to them.
    let from_record: Vec<&str> = items
        .iter()
        .filter(|i| !i.cause.starts_with("github:"))
        .map(|i| i.cause.as_str())
        .collect();
    assert_eq!(from_record.len(), 3, "{from_record:?}");
    let from_issues: Vec<&theseus_protocol::rsi::SelfBacklogItem> = items
        .iter()
        .filter(|i| i.cause.starts_with("github:"))
        .collect();
    assert_eq!(from_issues.len(), 2, "{items:#?}");

    let one = from_issues
        .iter()
        .find(|i| i.cause == "github:issue:118")
        .expect("issue #118 is on the backlog");
    assert_eq!(one.title, "#118: The berth lights flicker after a reboot");
    // Its origin is in its evidence, so a person reading the backlog has the
    // repository and the number and can go and look.
    let said = format!("{:?}", one);
    assert!(said.contains("harbourworks/lanterns"), "{said}");
    assert!(said.contains("118"), "{said}");
    // The owner writes the issues, so an issue is nobody's keel class and the
    // item is proposed rather than held: the keel guard is still the guard on
    // what a branch for it may do.
    assert_eq!(one.state, "proposed", "{one:?}");
    assert!(
        !night.waiting.iter().any(|w| w.contains("118")),
        "{night:?}"
    );
}

/// The card's second bar, and it is not negotiable: **a source that FAILS
/// must not look like a source that is EMPTY.** The night keeps every item
/// its own record gave it, says the issue source failed, and says why.
#[test]
fn a_source_that_cannot_read_leaves_the_rest_of_the_backlog_standing() {
    // Three ways to fail: a file that is not there, a body that is not JSON
    // (what a 404 page is), and a repository name that is not one.
    let missing = std::env::temp_dir()
        .join(format!("theseus-self-issues-gone-{}", std::process::id()))
        .join("issues.json");
    let not_json = listing_file("bad", "<html>404: Not Found</html>");
    let not_a_listing = listing_file("msg", r#"{"message": "Not Found"}"#);
    let cases: [(&str, &str, &str); 4] = [
        ("an unreadable file", &missing.display().to_string(), REPO),
        (
            "a body that is not JSON",
            &not_json.display().to_string(),
            REPO,
        ),
        (
            "an object that is not a listing",
            &not_a_listing.display().to_string(),
            REPO,
        ),
        (
            "a repository that is not owner/name",
            &not_json.display().to_string(),
            "not-a-repository",
        ),
    ];
    for (what, file, repo) in cases {
        let c = with_issues(file, repo);
        let t0 = theseus_protocol::now_unix_ms();
        seed_day(&c, t0);
        let night = c.backlog_night(tick()).unwrap();
        assert_eq!(night.gate, "allowed", "{what}");
        assert_eq!(night.issues, 0, "{what}: {night:?}");
        assert!(
            night.issues_failed.is_some(),
            "{what}: a failed source says so: {night:?}"
        );
        // The rest of the backlog is UNCHANGED, not empty: the same three
        // causes the night's own record gave it, as with no issue source at
        // all. This is the assertion the bar is about.
        let items = listed(&c);
        assert_eq!(items.len(), 3, "{what}: {items:#?}");
        assert!(
            items.iter().all(|i| !i.cause.starts_with("github:")),
            "{what}: {items:#?}"
        );
    }
}

/// Off by default: a core whose `[self_issues]` says nothing reads nothing,
/// says nothing failed, and files exactly what it filed before this row.
#[test]
fn the_source_is_off_by_default_and_the_night_is_as_it_was() {
    let c = acting(0, 20);
    assert!(
        !c.cfg.self_issues.enabled,
        "off by default: {:?}",
        c.cfg.self_issues
    );
    let t0 = theseus_protocol::now_unix_ms();
    seed_day(&c, t0);
    let night = c.backlog_night(tick()).unwrap();
    assert_eq!((night.issues, night.issues_failed), (0, None));
    assert_eq!(listed(&c).len(), 3);
}

/// An issue read twice is one cause: the second night files no second item,
/// and an UNCHANGED issue is no new evidence either — the identity an
/// observation carries holds the issue's own `updated_at`, which is what
/// makes an edit news and a re-read quiet.
#[test]
fn the_same_issue_two_nights_running_is_one_item() {
    let file = listing_file("twice", LISTING);
    let c = with_issues(&file.display().to_string(), REPO);
    let n1 = tick();
    let first = c.backlog_night(n1).unwrap();
    assert_eq!(first.filed.len(), 2, "{first:?}");
    assert_eq!(listed(&c).len(), 2);

    let second = c.backlog_night(n1 + DAY).unwrap();
    assert_eq!(second.issues, 2, "the source read both again: {second:?}");
    assert!(second.filed.is_empty(), "nothing new is filed: {second:?}");
    assert!(
        second.updated.is_empty(),
        "an unchanged issue is not new evidence: {second:?}"
    );
    assert_eq!(listed(&c).len(), 2);
    let counts: Vec<u64> = listed(&c).iter().map(|i| i.count).collect();
    assert_eq!(counts, vec![1, 1], "one sighting each, not two");

    // Edit one issue (a later `updated_at`) and the third night adds its
    // evidence to the item it already has, and still files nothing new.
    std::fs::write(
        &file,
        LISTING.replace("2026-11-12T10:00:00Z", "2026-11-13T08:00:00Z"),
    )
    .unwrap();
    let third = c.backlog_night(n1 + 2 * DAY).unwrap();
    assert!(third.filed.is_empty(), "{third:?}");
    assert_eq!(third.updated.len(), 1, "the edited issue alone: {third:?}");
    assert_eq!(listed(&c).len(), 2);
    let edited = listed(&c)
        .into_iter()
        .find(|i| i.cause == "github:issue:118")
        .expect("#118");
    assert_eq!(edited.count, 2, "{edited:?}");
}
