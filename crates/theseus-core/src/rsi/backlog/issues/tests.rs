//! The GitHub issue source's own tests (theseus-dl4pu). Every name here is
//! invented: the fixture's repository, its issue titles and its bodies are
//! made up for this test and describe no real issue.
//!
//! The four bars of the card, each with its own test below:
//!
//! - a fixture of issue JSON becomes candidate items **with their origin
//!   recorded** (`a_fixture_of_issue_json_becomes_candidates_naming_their_issue`);
//! - **a source that fails is not a source that is empty**
//!   (`a_failed_read_says_so_and_an_empty_listing_says_nothing`);
//! - **no write verb is ever constructed** (`no_write_verb_can_be_constructed`);
//! - the version tag picks the release's buckets (`the_version_label_picks_this_releases_buckets`).

use super::*;

/// An invented listing, in the shape GitHub's issues endpoint returns:
/// three open issues, one closed, and one pull request (which that endpoint
/// also returns and which is not a bucket of work).
pub(super) const FIXTURE: &str = r#"[
  {"number": 41, "title": "The lantern keeper's log rolls at the wrong hour",
   "body": "The roll happens at 00:00 UTC and should follow the local day.\n\nSeen three nights running.",
   "state": "open", "updated_at": "2026-11-11T09:00:00Z",
   "labels": [{"name": "1.0.6"}, {"name": "bug"}]},
  {"number": 42, "title": "Tide table imports lose their footnotes",
   "body": "", "state": "open", "updated_at": "2026-11-12T10:30:00Z",
   "labels": ["1.0.6"]},
  {"number": 43, "title": "A harbour map for the quiet berths",
   "body": "An idea, not a defect.", "state": "open",
   "updated_at": "2026-11-10T08:00:00Z", "labels": [{"name": "1.0.7"}]},
  {"number": 44, "title": "Closed: the beacon already blinks", "state": "closed",
   "updated_at": "2026-11-12T11:00:00Z", "labels": [{"name": "1.0.6"}]},
  {"number": 45, "title": "Pull request: blink the beacon", "state": "open",
   "updated_at": "2026-11-12T12:00:00Z", "pull_request": {"url": "x"},
   "labels": [{"name": "1.0.6"}]}
]"#;

const REPO: &str = "harbourworks/lanterns";
const NOW: u64 = 1_795_000_000_000;

#[test]
fn a_fixture_of_issue_json_becomes_candidates_naming_their_issue() {
    let issues = parse(FIXTURE.as_bytes()).expect("the fixture parses");
    assert_eq!(
        issues.len(),
        5,
        "every entry is read, pull request included"
    );
    let got = obs(&issues, REPO, "", 20, NOW);
    // The positive count first, so an empty input cannot pass this test: the
    // closed issue and the pull request are the two that must be left out.
    assert_eq!(got.len(), 3, "{got:#?}");
    let numbers: Vec<&str> = got.iter().map(|o| o.key.as_str()).collect();
    assert_eq!(
        numbers,
        vec!["github:issue:42", "github:issue:41", "github:issue:43"],
        "the most recently updated first"
    );
    // The origin is recorded on every one: the item NAMES the issue it came
    // from, number and title, which is the card's own bar.
    for o in &got {
        assert_eq!(o.source, "github");
        assert!(o.id.starts_with("github:harbourworks/lanterns#"), "{o:?}");
        // The identity carries the issue's own `updated_at`, so an edit is
        // new evidence and an unchanged issue is not.
        assert!(o.id.contains("@2026-11-1"), "{o:?}");
        assert!(
            o.title.starts_with('#'),
            "the number leads the title: {o:?}"
        );
        assert!(o.note.contains("harbourworks/lanterns issue #"), "{o:?}");
        assert_eq!(o.at_ms, NOW);
        assert_eq!(o.usd, 0.0);
        assert!(o.person, "an issue is a person's words");
        assert!(!o.task_failed);
    }
    let first = got.iter().find(|o| o.key == key_of(41)).expect("#41");
    assert_eq!(
        first.title,
        "#41: The lantern keeper's log rolls at the wrong hour"
    );
    assert_eq!(
        first.note,
        "harbourworks/lanterns issue #41 [1.0.6, bug]: \
         The roll happens at 00:00 UTC and should follow the local day."
    );
    // They group into candidates by their keys, one candidate an issue, and
    // the candidate carries the issue's own words.
    let cands = super::super::read::group_with(got, &|_| false);
    assert_eq!(cands.len(), 3, "{cands:#?}");
    let c = cands.iter().find(|c| c.key == key_of(42)).expect("#42");
    assert_eq!(c.source, "github");
    assert_eq!(c.title, "#42: Tide table imports lose their footnotes");
    assert_eq!(c.obs.len(), 1);
}

#[test]
fn the_version_label_picks_this_releases_buckets() {
    let issues = parse(FIXTURE.as_bytes()).unwrap();
    let got = obs(&issues, REPO, "1.0.6", 20, NOW);
    assert_eq!(got.len(), 2, "{got:#?}");
    assert!(
        got.iter().all(|o| o.key != key_of(43)),
        "1.0.7 is not this release's"
    );
    // A label that nothing carries keeps nothing, and that is an EMPTY read,
    // not a failed one: the caller tells them apart, not this function.
    assert!(obs(&issues, REPO, "2.0.0", 20, NOW).is_empty());
    // And the bare string form of a label counts as the object form does.
    assert!(got.iter().any(|o| o.key == key_of(42)), "{got:#?}");
}

#[test]
fn at_most_max_issues_are_taken_newest_first() {
    let issues = parse(FIXTURE.as_bytes()).unwrap();
    let got = obs(&issues, REPO, "", 2, NOW);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].key, key_of(42), "the newest");
    assert_eq!(got[1].key, key_of(41));
}

#[test]
fn a_failed_read_says_so_and_an_empty_listing_says_nothing() {
    // (1) Not JSON at all.
    let why = parse(b"<html>404 not found</html>").expect_err("not a listing");
    assert!(why.contains("did not parse"), "{why}");
    // (2) An object that is not a listing: the shape a 404 body has.
    let why = parse(br#"{"message": "Not Found"}"#).expect_err("not a listing");
    assert!(why.contains("no `items` array"), "{why}");
    // (3) A file that is not there.
    let missing = std::path::Path::new("/var/empty/theseus-test-no-such-issues.json");
    let why = read(missing).expect_err("no file");
    assert!(why.contains("could not be read"), "{why}");
    // (4) An EMPTY listing is a success with nothing in it, and it must not
    // be confused with any of the three above: that is the whole point.
    assert_eq!(parse(b"[]").expect("an empty listing reads"), vec![]);
    assert!(obs(&[], REPO, "", 20, NOW).is_empty());
    // And the search endpoint's shape reads too, so a listing fetched that
    // way is not mistaken for a failure.
    let items = parse(br#"{"items": [{"number": 7, "title": "A berth light", "state": "open"}]}"#)
        .expect("the search shape reads");
    assert_eq!(items.len(), 1);
    assert_eq!(obs(&items, REPO, "", 20, NOW).len(), 1);
}

#[test]
fn no_write_verb_can_be_constructed() {
    // The type has exactly one variant, so there is no value of `Verb` that
    // is not a read. An exhaustive match is the proof the compiler checks:
    // adding `Post` or `Delete` to the enum fails this match to compile.
    let every = [Verb::Get];
    assert_eq!(every.len(), 1, "one verb, and it is a read");
    for v in every {
        match v {
            Verb::Get => assert_eq!(v.method(), "GET"),
        }
    }
    // And the one request this source builds asks for open issues and
    // nothing else: no verb to change, no body to send.
    let r = Request::open_issues("harbourworks/lanterns", 20).expect("a repository");
    assert_eq!(r.verb, Verb::Get);
    assert_eq!(
        r.path(),
        "/repos/harbourworks/lanterns/issues?state=open&per_page=20"
    );
    assert!(!r.path().contains("comments"), "{}", r.path());
    assert!(!r.path().contains("labels"), "{}", r.path());
}

#[test]
fn a_misshapen_repository_is_refused_before_anything_is_read() {
    for bad in ["", "theseus", "a/b/c", "owner/", "/name", "own er/name"] {
        let why = Request::open_issues(bad, 20).expect_err(bad);
        assert!(why.contains("owner/name"), "{bad}: {why}");
    }
    let r = Request::open_issues(" zeroaltitude/theseus ", 1000).expect("trimmed and clamped");
    assert_eq!(
        (r.owner.as_str(), r.repo.as_str()),
        ("zeroaltitude", "theseus")
    );
    assert_eq!(r.per_page, 100, "GitHub's own page ceiling");
    assert_eq!(Request::open_issues("a/b", 0).unwrap().per_page, 1);
}

#[test]
fn an_issues_key_holds_its_number_and_never_its_title() {
    // The keel mark is read from a key by word, so a title must not decide an
    // item's class: an issue called "loosen the policy" is no more a security
    // item than any other, and the keel guard still fails the branch.
    let issues = parse(
        br#"[{"number": 9, "title": "Loosen the policy for sealed secret ceilings",
             "state": "open", "updated_at": "2026-11-12T00:00:00Z"}]"#,
    )
    .unwrap();
    let got = obs(&issues, REPO, "", 20, NOW);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].key, "github:issue:9");
    assert_eq!(
        super::super::keel::class_of(&got[0].key),
        None,
        "{:?}",
        got[0]
    );
}

#[test]
fn a_long_body_line_is_cut_and_a_titleless_issue_still_names_its_number() {
    let long = "x".repeat(400);
    let json =
        format!(r#"[{{"number": 11, "title": "   ", "body": "\n\n{long}", "state": "open"}}]"#);
    let issues = parse(json.as_bytes()).unwrap();
    let got = obs(&issues, REPO, "", 20, NOW);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].title, "issue #11 of harbourworks/lanterns");
    assert!(got[0].note.ends_with('…'), "{:?}", got[0].note);
    assert_eq!(
        got[0].note.chars().count(),
        "harbourworks/lanterns issue #11: ".chars().count() + NOTE_BODY_CHARS + 1
    );
}
