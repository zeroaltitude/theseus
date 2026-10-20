//! `[ontology]` (theseus-xzxqv): its defaults, what it refuses, and that it
//! stays out of a sparse note when it is the default.

use super::{ontology::Request, Config};

fn doc(rest: &str) -> String {
    format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n{rest}\n")
}

#[test]
fn the_defaults_are_the_documented_ones() {
    let (cfg, _) = Config::parse(&doc("")).unwrap();
    let c = &cfg.ontology;
    assert_eq!(
        (
            c.judge,
            c.act,
            c.confirm,
            c.name_profile.as_str(),
            c.request
        ),
        (true, 0.80, 0.55, "haiku", Request::Batch)
    );
    assert!(c.is_default());
}

#[test]
fn a_key_is_read_and_a_bad_one_is_refused_with_its_name() {
    let (cfg, _) = Config::parse(&doc(
        "[ontology]\njudge = false\nact = 0.9\nconfirm = 0.6\nname_profile = \"sonnet\"\nrequest = \"own\"\n",
    ))
    .unwrap();
    let c = &cfg.ontology;
    assert_eq!(
        (
            c.judge,
            c.act,
            c.confirm,
            c.name_profile.as_str(),
            c.request
        ),
        (false, 0.9, 0.6, "sonnet", Request::Own)
    );
    for (bad, word) in [
        ("confirm = 0.9\nact = 0.8", "confirm"),
        ("act = 1.5", "act"),
        ("name_profile = \" \"", "name_profile"),
        ("request = \"third\"", "third"),
        ("nightly = true", "unknown field"),
    ] {
        let e = Config::parse(&doc(&format!("[ontology]\n{bad}\n"))).unwrap_err();
        assert!(format!("{e:#}").contains(word), "{bad}: {e:#}");
    }
}
