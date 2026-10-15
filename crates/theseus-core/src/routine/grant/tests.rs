//! A routine's grant, read and checked on its own (theseus-52tut); the gate's
//! use of it is `tests_routine_trust.rs`.

use super::*;

fn known(t: &str) -> bool {
    matches!(
        t,
        "gog.gmail.search" | "web.fetch" | "channel.post" | "fs.write" | "proc.run" | "aws.call"
    )
}

fn w(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_grant_is_tools_and_a_posting_tool_with_its_place() {
    let g = check(
        &w(&[
            "gog.gmail.search",
            " web.fetch ",
            "channel.post   #digest",
            "web.fetch",
        ]),
        &known,
    )
    .unwrap();
    assert_eq!(
        g,
        w(&["gog.gmail.search", "web.fetch", "channel.post #digest"])
    );
}

#[test]
fn a_grant_naming_what_no_grant_may_is_refused_naming_it() {
    for (entry, says) in [
        ("proc.run", "`proc.run` can't be in a grant"),
        ("aws.call", "`aws.call` can't be in a grant"),
        ("nope.tool", "`nope.tool` is not a tool you have"),
        ("channel.post", "granted to a place"),
        ("web.fetch #digest", "takes no place"),
        ("mcp:ext-x/run", "proposed extension"),
        ("  ", "names a tool"),
    ] {
        let e = check(&w(&[entry]), &known).expect_err(entry);
        assert!(e.contains(says), "{entry}: {e}");
    }
    // One bad entry refuses the whole grant: nothing partly granted.
    assert!(check(&w(&["web.fetch", "proc.run"]), &known).is_err());
}

#[test]
fn a_tool_is_covered_by_its_name_and_a_post_by_its_place_in_any_case() {
    let g = Grant::of(&w(&["web.fetch", "channel.post #Digest"]));
    assert!(g.covers("web.fetch", &[]));
    assert!(!g.covers("web.search", &[]));
    assert!(g.covers("channel.post", &["#digest"]));
    assert!(g.covers("channel.post", &["nowhere", "  #DIGEST "]));
    assert!(!g.covers("channel.post", &["#general"]));
    assert!(!g.covers("channel.post", &[]));
    // A bare posting entry never covers (the check refuses it; this is the
    // gate's own belt).
    assert!(!Grant::of(&w(&["channel.post"])).covers("channel.post", &["#digest"]));
}

#[test]
fn entries_join_once() {
    let have = w(&["web.fetch"]);
    assert_eq!(
        joined(&have, &w(&["fs.write", "web.fetch"])),
        w(&["web.fetch", "fs.write"])
    );
}
