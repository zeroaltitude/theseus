//! `[google]`'s loader checks and the crown jewels (the Google design §2.1).

use crate::broker::{harness_only, Broker};
use crate::secrets::SecretBoard;
use crate::Config;

fn load(google: &str) -> Result<Config, String> {
    let mut text = Config::EXAMPLE_TOML.to_string();
    text.push_str("\n[google]\n");
    text.push_str(google);
    let cfg = Config::from_toml(&text).map_err(|e| e.to_string())?;
    // The template's own secrets carry no Google entries until they are un-commented.
    let mut cfg = cfg;
    cfg.secrets
        .entry("google_client".into())
        .or_insert_with(|| "op://v/g/n".into());
    cfg.secrets
        .entry("google_refresh_token".into())
        .or_insert_with(|| "file:/x/rt".into());
    cfg.validate().map(|()| cfg).map_err(|e| format!("{e:#}"))
}

#[test]
fn config_rejects_unknown_google_keys() {
    let e = load("account = \"owner@example.test\"\nclient_id = \"x\"\n").unwrap_err();
    assert!(e.contains("unknown field `client_id`"), "{e}");
    // The names the owner decided on are `client` and `refresh_token`: the
    // design's separate id and secret names are not keys.
    let e = load("account = \"owner@example.test\"\ncredentials = { client_id = \"a\", client_secret = \"b\", refresh_token = \"c\" }\n").unwrap_err();
    assert!(e.contains("unknown field `client_id`"), "{e}");
    let e = load("account = \"owner@example.test\"\nscopes = [\"x\"]\n").unwrap_err();
    assert!(e.contains("unknown field `scopes`"), "{e}");
    let ok = load("account = \"owner@example.test\"\n").unwrap();
    assert_eq!(ok.google.services.len(), 6, "all six by default");
    assert_eq!(ok.google.max_sends_per_day, 25);
    assert_eq!(ok.google.credentials.client, "google_client");
    assert_eq!(ok.google.credentials.refresh_token, "google_refresh_token");
}

#[test]
fn the_loader_checks_the_account_the_services_the_names_and_the_endpoint() {
    for (google, says) in [
        ("account = \"not-an-address\"\n", "is not an email address"),
        (
            "account = \"owner@example.test\"\nservices = [\"gmail\", \"gopher\"]\n",
            "not a service",
        ),
        (
            "account = \"owner@example.test\"\nservices = [\"gmail\", \"gmail\"]\n",
            "twice",
        ),
        (
            "account = \"owner@example.test\"\ncredentials = { client = \"nowhere\" }\n",
            "no matching entry under [secrets]",
        ),
        (
            "account = \"owner@example.test\"\nendpoint = \"https://google.example\"\n",
            "not a stand-in on this machine",
        ),
    ] {
        let e = load(google).unwrap_err();
        assert!(e.contains(says), "{google}: {e}");
    }
    assert!(load("account = \"owner@example.test\"\nendpoint = \"http://127.0.0.1:7499\"\nservices = [\"gmail\", \"calendar\"]\nmax_sends_per_day = 0\n").is_ok());
}

#[test]
fn the_template_has_a_google_section_and_markers_and_an_empty_one_serializes_to_nothing() {
    assert!(Config::EXAMPLE_TOML.contains("# [google]"));
    for marker in [
        "# gw-mail: the gmail.* lines",
        "# gw-drive: the drive.* lines",
        "# gw-sheets-slides: the sheets.* and slides.* lines",
        "# gw-calendar: the calendar.* lines",
    ] {
        assert!(Config::EXAMPLE_TOML.contains(marker), "{marker}");
    }
    assert!(!Config::example().google.account.is_empty() || Config::example().google.is_empty());
    let text = toml::to_string(&Config::example()).unwrap();
    assert!(!text.contains("[google]"), "no account: no section");
}

#[test]
fn google_secrets_never_reach_l1() {
    let both = ["google_client", "google_refresh_token"];
    let mut cfg = Config::example();
    cfg.google = crate::config::GoogleConfig {
        account: "owner@example.test".into(),
        ..Default::default()
    };
    for n in both {
        cfg.secrets.insert(n.into(), "op://v/g/n".into());
    }
    cfg.validate().unwrap();
    // The broker says so by name, and hands none.
    let broker = Broker::new(&cfg.broker, SecretBoard::empty(), None);
    let h = harness_only(&cfg, &broker);
    assert_eq!(h.google, both);
    for n in both {
        assert!(!broker.may_hand_out(n), "a job may be handed {n}");
    }
    assert!(
        h.line()
            .contains("Google's secrets (google_client, google_refresh_token)"),
        "{}",
        h.line()
    );
    // A grant that names one is refused at load, whichever way it names it.
    for grant in [
        "[broker.programs.curl]\nenv = { TOKEN = \"google_refresh_token\" }\n",
        "[broker.secrets.google_client]\nposture = \"approve\"\n",
    ] {
        let mut text = Config::EXAMPLE_TOML.replace("[broker.programs.gh]", "[broker.programs.zz]");
        text.push('\n');
        text.push_str(grant);
        let mut c = Config::from_toml(&text).unwrap_or_else(|e| panic!("{grant}: {e}"));
        c.google = cfg.google.clone();
        for n in both {
            c.secrets.insert(n.into(), "op://v/g/n".into());
        }
        let e = format!("{:#}", c.validate().unwrap_err());
        assert!(
            e.contains("no job is ever handed a Google secret"),
            "{grant}: {e}"
        );
    }
    // No Google: the line says nothing of it.
    let none = harness_only(
        &Config::example(),
        &Broker::new(&Config::example().broker, SecretBoard::empty(), None),
    );
    assert!(none.google.is_empty() && !none.line().contains("Google"));
}

/// The template's `[google]` section and its `google.call` line, un-commented,
/// are real: they load, and say what the template says (the Google design §2.1).
#[test]
fn the_templates_google_section_uncommented_parses() {
    let section: Vec<&str> = Config::EXAMPLE_TOML
        .lines()
        .skip_while(|l| *l != "# [google]")
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.strip_prefix("# ").unwrap_or(l))
        .collect();
    assert_eq!(section.len(), 5, "{section:?}");
    let cfg = load(&section.join("\n")).unwrap();
    assert_eq!(cfg.google.account, "owner@example.test");
    assert_eq!(cfg.google.services.len(), 6);
    assert_eq!(
        cfg.google.endpoint.as_deref(),
        Some("http://127.0.0.1:7499")
    );
    let line = Config::EXAMPLE_TOML
        .lines()
        .find(|l| l.contains("\"google.call\" ="))
        .unwrap();
    let cfg = Config::from_toml(&format!(
        "[policy.tools]\n{}\n",
        line.trim_start_matches("# ")
    ))
    .unwrap();
    assert_eq!(
        cfg.policy.tools["google.call"],
        crate::policy::Posture::Notify
    );
}
