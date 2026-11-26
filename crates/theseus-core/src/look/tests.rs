//! The looks' wall and gate (issue 26), on the facts alone: who is refused, in
//! which words, and which turn is asked first. The played turns, through the
//! whole core, are `config_change/tests_look.rs`.

use super::*;
use crate::config_change::wall;

fn started_from_a_file() -> crate::Config {
    let mut cfg = crate::Config::example();
    cfg.config_file = Some("/etc/theseus/theseus.toml".into());
    cfg
}

fn plan() -> Plan {
    Plan {
        summary: "look".into(),
        ..Default::default()
    }
}

/// A clean turn is told and not asked; one that read text from outside, or was
/// steered, is asked, each in its own words; and only the looks are touched.
#[test]
fn a_look_is_free_when_clean_and_asked_after_outside_text() {
    use crate::config_change::tests_hold::{each_cause_is_said, ways};
    for tool in NAMES {
        each_cause_is_said(tool, |o, s, way| held(tool, o, s, way, plan()).authority);
    }
    for other in ["config.show", "proc.run", "look.other", "fs.read"] {
        assert!(
            held(other, true, true, &ways()[1], plan())
                .authority
                .is_none(),
            "{other}"
        );
    }
}

/// Every refusal of the wall is said in the look's own name, and none of them
/// says the look changes a setting.
#[test]
fn every_refusal_is_in_the_looks_own_words() {
    let walls = [
        wall::NOT_PRIVATE,
        wall::NOT_ALONE,
        wall::NOT_A_TASK,
        wall::NOT_THE_OWNER,
    ];
    for tool in NAMES {
        for w in walls {
            let said = worded(Some(w), tool).unwrap();
            assert!(said.starts_with(tool), "{said}");
            assert!(!said.contains("config.propose"), "{said}");
            assert!(!said.contains("change Theseus's"), "{said}");
        }
        assert_eq!(worded(None, tool), None);
    }
}

/// The two tools are in the names the config's `[policy.tools]` accepts, the
/// registry, and a family of their own.
#[test]
fn the_looks_are_registered_and_in_the_look_family() {
    let mut r = Registry::default();
    register(&started_from_a_file(), &mut r);
    for tool in NAMES {
        let t = r.get(tool).expect(tool);
        assert_eq!(t.family(), FAMILY);
        assert_eq!(t.class(), ToolClass::Read);
    }
}

/// The descriptions tell the model what to do, and each thing they forbid is
/// something the tool really refuses: the vault's look never gives a value, and
/// says the shell has no session; the other says its command line is not the
/// way.
#[test]
fn the_descriptions_say_what_the_shell_is_not() {
    let (t, v) = (LookTheseus.description(), LookVault.description());
    assert!(t.contains("command line in a shell is not the way"), "{t}");
    assert!(v.contains("no session by design"), "{v}");
    assert!(v.contains("Never a value, a field or a note"), "{v}");
    assert!(v.contains("the cap is said"), "{v}");
}

/// Only a daemon that was started from a config has the tools: a file's, or the
/// copy of the config the vault gave. A default config, which no start made, has neither, and
/// carries nothing a test would have to be tuned to.
#[test]
fn a_daemon_started_from_a_config_has_the_looks_and_a_bare_config_has_none() {
    let count = |cfg: &crate::Config| {
        let mut r = Registry::default();
        register(cfg, &mut r);
        r.len()
    };
    assert_eq!(count(&crate::Config::example()), 0);
    assert_eq!(count(&started_from_a_file()), 3);
    let mut vault = crate::Config::example();
    vault.config_copy = Some("/var/lib/theseus/config.copy".into());
    assert_eq!(count(&vault), 3);
}
