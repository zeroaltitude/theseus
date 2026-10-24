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
    assert_eq!(section.len(), 8, "{section:?}");
    let cfg = load(&section.join("\n")).unwrap();
    assert_eq!(cfg.google.account, "owner@example.test");
    assert_eq!(
        cfg.google.sensitive_domains.as_deref(),
        Some(&["corp.example.test".to_string()][..])
    );
    assert_eq!(cfg.google.trusted_senders, ["dana@example.test"]);
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

/// The template's `drive.share` line says what the tool does: it asks
/// (theseus-batvg.3; out of `example_template_uncommented_still_parses`,
/// which is at clippy's line budget).
#[test]
fn the_templates_drive_share_line_asks() {
    let line = Config::EXAMPLE_TOML
        .lines()
        .find(|l| l.contains("\"drive.share\" ="))
        .unwrap();
    let cfg = Config::from_toml(&format!(
        "[policy.tools]\n{}\n",
        line.trim_start_matches("# ")
    ))
    .unwrap();
    assert_eq!(
        cfg.policy.tools["drive.share"],
        crate::policy::Posture::Approve
    );
}

const ACCOUNT: &str = "account = \"owner@example.test\"\n";

fn cals(list: &str) -> Result<Vec<crate::config::CalendarSpec>, String> {
    load(&format!("{ACCOUNT}calendars = {list}\n")).map(|c| c.google.calendar_specs())
}

/// `calendars` unset names none (every calendar of the account's list is
/// read, the primary alone until the list is); an entry is an id or a table.
#[test]
fn calendars_default_to_every_calendar_and_take_ids_or_tables() {
    let d = load(ACCOUNT).unwrap();
    assert!(d.google.calendar_specs().is_empty());
    assert_eq!(d.google.sensitive_domains, None);
    let p = crate::config::primary_spec();
    assert_eq!(
        (p.id.as_str(), p.sensitive, &p.label),
        ("primary", false, &None)
    );
    assert_eq!(p.color, "#c9a24a", "brass first");
    assert!(
        cals("[]").unwrap().is_empty(),
        "empty is every calendar too"
    );

    let specs = cals(
        "[\"primary\", { id = \"work.cal.example.test\", label = \"Work\", sensitive = true }, \
         { id = \"club.cal.example.test\", color = \"#112233\" }]",
    )
    .unwrap();
    assert_eq!(specs[0].id, "primary");
    assert_eq!(specs[1].label.as_deref(), Some("Work"));
    assert!(specs[1].sensitive && !specs[0].sensitive);
    assert_eq!(specs[1].color, "#5fa08d", "verdigris by position");
    assert_eq!(specs[2].color, "#112233");
}

#[test]
fn calendars_refuse_a_repeat_a_second_primary_a_bad_colour_and_a_bare_table() {
    let e = cals("[\"a.cal.example.test\", \"A.cal.example.test\"]").unwrap_err();
    assert!(e.contains("twice"), "{e}");
    let e = cals("[\"primary\", { id = \"primary\", label = \"Me\" }]").unwrap_err();
    assert!(e.contains("\"primary\" twice"), "{e}");
    for bad in ["red", "#12345", "#12345g", "123456", "#1234567"] {
        let e = cals(&format!(
            "[{{ id = \"a.cal.example.test\", color = \"{bad}\" }}]"
        ))
        .unwrap_err();
        assert!(e.contains("is not #rrggbb"), "{bad}: {e}");
    }
    let e = cals("[]").unwrap_err();
    assert!(e.contains("empty"), "{e}");
    let e = cals("[{ id = \"a.cal.example.test\", labl = \"x\" }]").unwrap_err();
    assert!(
        e.contains("did not match any variant") || e.contains("unknown"),
        "{e}"
    );
    let e =
        cals("[{ id = \"a\", label = \"Same\" }, { id = \"b\", label = \"same\" }]").unwrap_err();
    assert!(e.contains("names two calendars"), "{e}");
}

/// The default does not serialize, so the owner's sparse config stays as it is.
#[test]
fn the_default_calendars_are_not_written_back() {
    let c = load(ACCOUNT).unwrap();
    let text = toml::to_string(&c.google).unwrap();
    assert!(!text.contains("calendars"), "{text}");
    let c = load(&format!(
        "{ACCOUNT}calendars = [{{ id = \"w.cal.example.test\", sensitive = true }}]\n"
    ))
    .unwrap();
    let text = toml::to_string(&c.google).unwrap();
    assert!(
        text.contains("w.cal.example.test") && text.contains("sensitive"),
        "{text}"
    );
}

/// The template's `calendars` line, un-commented, loads and marks one sensitive.
#[test]
fn the_templates_calendars_line_loads() {
    let line = Config::EXAMPLE_TOML
        .lines()
        .find(|l| l.starts_with("# calendars = "))
        .unwrap();
    let specs = cals(
        line.trim_start_matches("# calendars = ")
            .split("   #")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(specs.len(), 2);
    assert!(specs[1].sensitive && specs[1].label.as_deref() == Some("Team"));
}

/// A calendar of a workplace's domain is sensitive with no config, one of a
/// consumer domain or of Google's own ids is not, and `sensitive_domains`
/// replaces that default (owner change to theseus-jid7t.1, 2026-10-23).
#[test]
fn a_workspace_domain_calendar_is_sensitive_by_default_and_sensitive_domains_narrows() {
    use crate::config::google_calendars::sensitive_by_domain as by;
    /// A consumer domain, kept out of an address literal.
    const CONSUMER: &str = "gmail.com";
    let (consumer, mixed) = (
        format!("someone@{CONSUMER}"),
        format!("someone@{}", "GoogleMail.com"),
    );
    for (id, want) in [
        ("someone@corp.example.test", true),
        ("team@sub.corp.example.test", true),
        (consumer.as_str(), false),
        (mixed.as_str(), false),
        ("abc123@group.calendar.google.com", false),
        ("en.usa#holiday@group.v.calendar.google.com", false),
        ("primary", false),
        ("personal.cal.example.test", false),
    ] {
        assert_eq!(by(id, None), want, "{id}");
    }
    let only = ["corp.example.test".to_string()];
    assert!(by("team@sub.corp.example.test", Some(&only)));
    assert!(!by("someone@other.example.test", Some(&only)));
    assert!(
        !by("someone@corp.example.test", Some(&[])),
        "an empty list: none by domain"
    );
    let specs = cals(&format!(
        "[\"someone@corp.example.test\", \"someone@{CONSUMER}\"]"
    ))
    .unwrap();
    assert_eq!((specs[0].sensitive, specs[1].sensitive), (true, false));
    let c = load(&format!(
        "{ACCOUNT}calendars = [\"someone@corp.example.test\", \"x@other.example.test\"]\n\
         sensitive_domains = [\"other.example.test\"]\n"
    ))
    .unwrap();
    let s: Vec<bool> = c
        .google
        .calendar_specs()
        .iter()
        .map(|s| s.sensitive)
        .collect();
    assert_eq!(s, [false, true]);
    assert!(c
        .google
        .calendar_sensitive_by_domain("primary", Some("x@other.example.test")));
    let e = load(&format!(
        "{ACCOUNT}sensitive_domains = [\"a@b.example.test\"]\n"
    ))
    .unwrap_err();
    assert!(format!("{e:#}").contains("not a bare domain"), "{e:#}");
}
