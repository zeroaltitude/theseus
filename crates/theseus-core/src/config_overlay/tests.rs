//! The overlay's render (theseus-dxgb): the template's lines kept, an
//! overlay's values in place, and the result a config that loads.

use super::render;
use crate::Config;

/// An operator's overlay as the template expects one: the eight references
/// in [secrets] and the [approval] section switched on (invented values).
const OVERLAY: &str = r#"
[secrets]
anthropic_api_key = "op://Ops Vault/anthropic key/notesPlain"
zai_api_key = "op://Ops Vault/z.ai key/notesPlain#api key value"
jev_api_key = "op://Ops Vault/jev key/notesPlain"
github_token = "op://Ops Vault/github token/notesPlain"
aws_access_key_id = "op://Ops Vault/aws key/notesPlain#AWS_ACCESS_KEY_ID"
aws_secret_access_key = "op://Ops Vault/aws key/notesPlain#AWS_SECRET_ACCESS_KEY"
discord_bot_token = "op://Ops Vault/discord bot/notesPlain#bot_token"
brave_api_key = "op://Ops Vault/q2w3e4r5t6y7u8i9o0p1a2s3d4/notesPlain#value"

[approval]
trusted_users = ["discord:314159265358979323"]
channels = ["cli", "web", "discord:dm"]
"#;

/// The lines that differ between `a` and `b`, which have as many lines.
fn changed<'a>(a: &'a str, b: &'a str) -> Vec<(&'a str, &'a str)> {
    assert_eq!(
        a.lines().count(),
        b.lines().count(),
        "the render added or lost a line"
    );
    a.lines().zip(b.lines()).filter(|(x, y)| x != y).collect()
}

#[test]
fn an_empty_overlay_prints_the_template_as_it_is() {
    assert_eq!(
        render(Config::EXAMPLE_TOML, "").unwrap(),
        Config::EXAMPLE_TOML
    );
}

#[test]
fn the_template_with_an_operators_overlay_is_his_deployment_and_changes_only_those_lines() {
    let out = render(Config::EXAMPLE_TOML, OVERLAY).unwrap();
    let (cfg, _) = Config::parse(&out).unwrap();
    assert_eq!(
        cfg.secrets.get("anthropic_api_key").map(String::as_str),
        Some("op://Ops Vault/anthropic key/notesPlain")
    );
    assert_eq!(
        cfg.secrets.get("aws_secret_access_key").map(String::as_str),
        Some("op://Ops Vault/aws key/notesPlain#AWS_SECRET_ACCESS_KEY")
    );
    let changed = changed(Config::EXAMPLE_TOML, &out);
    // Eight references, the [approval] header, and its two keys; no other line.
    assert_eq!(changed.len(), 11, "{changed:#?}");
    assert!(changed
        .iter()
        .any(|(was, now)| *was == "# [approval]" && *now == "[approval]"));
    let users = out
        .lines()
        .find(|l| l.starts_with("trusted_users = "))
        .unwrap();
    assert!(
        users.starts_with(r#"trusted_users = ["discord:314159265358979323"]"#),
        "{users}"
    );
    // Its trailing comment stays, at the column it had.
    let was = Config::EXAMPLE_TOML
        .lines()
        .find(|l| l.starts_with("# trusted_users = "))
        .unwrap();
    assert_eq!(
        users.find("# who may answer"),
        was.find("# who may answer"),
        "{users}"
    );
    let brave = out
        .lines()
        .find(|l| l.starts_with("brave_api_key = "))
        .unwrap();
    assert!(brave.ends_with("# web.search's key"), "{brave}");
    assert!(
        !out.contains("<your vault>/<Anthropic"),
        "the placeholder is replaced"
    );
}

#[test]
fn a_key_the_table_lacks_goes_after_its_last_key_and_a_table_it_lacks_at_the_end() {
    let template = "# head\n[server]\nsocket = \"/run/x.sock\"   # where\n# web_port = 7433\n\n# -- next\n[kernel]\nmax = 1\n";
    let overlay = "[server]\nextra = 2\n[telemetry]\nkept = true\n";
    let out = super::render_lines(template, overlay).unwrap();
    assert_eq!(
        out,
        "# head\n[server]\nsocket = \"/run/x.sock\"   # where\n# web_port = 7433\nextra = 2\n\n# -- next\n[kernel]\nmax = 1\n\n[telemetry]\nkept = true\n"
    );
}

#[test]
fn a_live_line_wins_over_a_commented_one_and_a_value_with_a_hash_keeps_no_false_comment() {
    let template = "[secrets]\n# token = \"op://<vault>/<item>/notesPlain\"   # an example\ntoken = \"op://<vault>/<item>/notesPlain#label\"\n";
    let overlay = "[secrets]\ntoken = \"op://V/I/notesPlain#other\"\n";
    let out = super::render_lines(template, overlay).unwrap();
    assert_eq!(
        out,
        "[secrets]\n# token = \"op://<vault>/<item>/notesPlain\"   # an example\ntoken = \"op://V/I/notesPlain#other\"\n"
    );
}

#[test]
fn prose_that_looks_like_a_header_or_a_key_is_left_alone() {
    let template = "[tools]\n#      [tools].approve_paths, and a read = outside the roots\n# The value = what it is.\nroots = []\n";
    let overlay = "[tools]\nroots = [\"~/w\"]\n";
    let out = super::render_lines(template, overlay).unwrap();
    assert_eq!(
        out,
        "[tools]\n#      [tools].approve_paths, and a read = outside the roots\n# The value = what it is.\nroots = [\"~/w\"]\n"
    );
}

#[test]
fn a_misspelt_key_fails_with_its_name_and_the_template_is_not_printed() {
    let e = render(
        Config::EXAMPLE_TOML,
        "[approval]\ntrusted_user = [\"discord:1\"]\n",
    )
    .unwrap_err();
    let msg = format!("{e:#}");
    assert!(msg.contains("trusted_user"), "{msg}");
    assert!(msg.contains("does not load as a config"), "{msg}");
}

#[test]
fn an_array_of_tables_is_refused_by_name() {
    let e = render(Config::EXAMPLE_TOML, "[[profiles.x]]\nmodel = \"m\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("[[profiles.x]]"), "{e:#}");
}

#[test]
fn an_overlay_that_is_not_toml_says_so() {
    let e = render(Config::EXAMPLE_TOML, "[secrets\n").unwrap_err();
    assert!(
        format!("{e:#}").contains("parsing the overlay as TOML"),
        "{e:#}"
    );
}
