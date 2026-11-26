//! Issue 57, A4: which settings cards take the owner's authenticator code, through
//! a real `config.propose` on a real settings file (the rig of `tests.rs`), and
//! the yes judged by the same rule the card was asked by.
//!
//! Every sensitive setting keeps its card. The code is dropped for prompt text,
//! for tightening one of the five listed kernel limits (compared in the kernel's
//! own units) and, by the owner's word, for group G3 (self-improvement, the
//! accounts, the vault, folders, agents); groups G1 and G2 keep it by his word.

use serde_json::{json, Value};

use super::tests::{change, rig, Rig, FILE};
use crate::node::ResultStatus;
use crate::Config;

/// The settings file with `extra` after it.
fn file_with(r: &Rig, extra: &str) {
    std::fs::write(&r.file, format!("{FILE}\n{extra}")).unwrap();
}

/// Propose `call`, and the card it asked.
async fn card_of(r: &Rig, call: Value) -> theseus_protocol::ConfirmRequest {
    let sid = r.session();
    r.turn(&sid, call.clone()).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{call}: {text}");
    r.asked()
        .into_iter()
        .find(|q| q.session_id == sid)
        .expect("a card")
}

/// The card is declined, so the next may ask.
fn decline(r: &Rig, q: &theseus_protocol::ConfirmRequest) {
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
}

/// What the kernel would hold for the settings file `text`.
fn kernel_of(text: &str) -> theseus_kernel::KernelConfig {
    Config::parse(text).unwrap().0.kernel.to_kernel_config()
}

/// (D) Prompt text asks with its card alone, and the owner's yes alone writes
/// it: the card's mark and the yes's check agree.
#[tokio::test]
async fn prompt_text_takes_the_card_alone_and_the_yes_alone_writes_it() {
    let r = rig();
    let q = card_of(&r, change("model.system", json!("be brief"))).await;
    assert!(!q.needs_proof && !q.needs_fresh_proof, "{q:?}");
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(r.file_text().contains("be brief"), "{}", r.file_text());
}

/// (G1, G2) An address, a provider and a gate keep the code by the owner's
/// word: the card is marked, and the yes alone (no app paired) is refused, so
/// nothing is written.
#[tokio::test]
async fn the_kept_groups_keep_the_code_at_the_card_and_at_the_yes() {
    let r = rig();
    for call in [
        change("discord.rest_proxy", json!("proxy.example:80")),
        change("model.provider", json!("anthropic")),
        change("judge.enabled", json!(false)),
    ] {
        let q = card_of(&r, call.clone()).await;
        assert!(q.needs_proof, "{call}: {}", q.reason);
        let e = r
            .core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap_err();
        assert!(
            e.to_string().contains("theseus owner proof enroll"),
            "{call}: {e:#}"
        );
        decline(&r, &q);
    }
    assert_eq!(r.file_text(), FILE);
}

/// (G3) A folder takes the card alone by the owner's word, and the owner's yes
/// alone writes it: the card's mark and the yes's check agree.
#[tokio::test]
async fn a_folder_takes_the_card_alone_and_the_yes_alone_writes_it() {
    let r = rig();
    let q = card_of(&r, change("tools.roots", json!(["/tmp/elsewhere"]))).await;
    assert!(!q.needs_proof && !q.needs_fresh_proof, "{q:?}");
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(
        r.file_text().contains("/tmp/elsewhere"),
        "{}",
        r.file_text()
    );
}

/// Finding 1: each of the four dollar limits, from a stop of $50, to a positive
/// value below half a micro-dollar, which the kernel holds as zero (no limit):
/// the code. At the rounding boundary, one that converts to one micro-dollar is
/// a tighter stop: the card alone, and the yes alone writes it.
#[tokio::test]
async fn a_dollar_limit_that_converts_to_zero_micro_dollars_needs_the_code() {
    for key in [
        "spend_limit_usd",
        "daily_spend_ceiling_usd",
        "daily_runaway_usd",
        "runaway_usd_per_10min",
    ] {
        let r = rig();
        file_with(&r, &format!("[kernel]\n{key} = 50.0\n"));
        let path = format!("kernel.{key}");
        let field = |k: theseus_kernel::KernelConfig| match key {
            "spend_limit_usd" => k.spend_limit_micros,
            "daily_spend_ceiling_usd" => k.daily_ceiling_micros,
            "daily_runaway_usd" => k.daily_runaway_micros,
            _ => k.burn_line_micros,
        };
        let held = |v: f64| field(kernel_of(&format!("{FILE}\n[kernel]\n{key} = {v}\n")));
        assert_eq!(held(0.0000004), 0, "{key}: the kernel holds no limit");
        assert_eq!(held(0.0000006), 1, "{key}: one micro-dollar");
        let q = card_of(&r, change(&path, json!(0.0000004))).await;
        assert!(q.needs_proof, "{key} to 0.0000004: {}", q.reason);
        decline(&r, &q);
        let q = card_of(&r, change(&path, json!(0.0000006))).await;
        assert!(!q.needs_proof, "{key} to 0.0000006: {}", q.reason);
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        assert_eq!(
            field(kernel_of(&r.file_text())),
            1,
            "{key}: written, one micro-dollar"
        );
    }
}

/// The fifth limit, the hard multiple, is a whole number: lowering it is a
/// tighter stop; zero (never stop) needs the code.
#[tokio::test]
async fn the_hard_multiple_is_its_own_whole_number_case() {
    let r = rig();
    file_with(&r, "[kernel]\nspend_limit_hard_multiple = 10\n");
    let q = card_of(&r, change("kernel.spend_limit_hard_multiple", json!(0))).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert_eq!(
        kernel_of(&format!(
            "{FILE}\n[kernel]\nspend_limit_hard_multiple = 0\n"
        ))
        .spend_hard_multiple,
        0,
        "the kernel holds no stop"
    );
    decline(&r, &q);
    let q = card_of(&r, change("kernel.spend_limit_hard_multiple", json!(5))).await;
    assert!(!q.needs_proof, "{}", q.reason);
    decline(&r, &q);
}

/// A removal, and a value set where the file had none, cannot be read as a
/// tightening (the default may be higher), so they keep the code.
#[tokio::test]
async fn a_removed_limit_and_one_set_where_there_was_none_need_the_code() {
    let r = rig();
    file_with(&r, "[kernel]\ndaily_spend_ceiling_usd = 300.0\n");
    let removed = json!({
        "changes": [{"path": "kernel.daily_spend_ceiling_usd", "remove": true}],
        "reason": "the owner asked for it",
    });
    let q = card_of(&r, removed).await;
    assert!(q.needs_proof, "removed: {}", q.reason);
    decline(&r, &q);
    let r = rig();
    let q = card_of(&r, change("kernel.daily_spend_ceiling_usd", json!(150.0))).await;
    assert!(q.needs_proof, "set from the default: {}", q.reason);
}

/// A catalog price decides what a call is charged and reserved: set to zero, or
/// raised, it needs the code.
#[tokio::test]
async fn a_catalog_price_set_to_zero_needs_the_code() {
    let r = rig();
    let path = "catalog.claude-sonnet-5-5.input_per_mtok";
    let q = card_of(&r, change(path, json!(0.0))).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason
            .contains("a price that decides what a model call is charged or reserved"),
        "{}",
        q.reason
    );
    decline(&r, &q);
    let q = card_of(&r, change(path, json!(90.0))).await;
    assert!(q.needs_proof, "raised: {}", q.reason);
}

/// One change of several: a limit made tighter, prompt text, and a stop that
/// converts to zero micro-dollars. The stricter rule wins; without the last,
/// the card alone.
#[tokio::test]
async fn in_a_change_of_several_the_stricter_rule_wins() {
    let r = rig();
    file_with(
        &r,
        "[kernel]\ndaily_spend_ceiling_usd = 300.0\ndaily_runaway_usd = 2000.0\n",
    );
    let both = |runaway: f64| {
        json!({
            "changes": [
                {"path": "kernel.daily_spend_ceiling_usd", "value": 250.0},
                {"path": "model.system", "value": "be brief"},
                {"path": "kernel.daily_runaway_usd", "value": runaway},
            ],
            "reason": "the owner asked for it",
        })
    };
    let q = card_of(&r, both(0.0000001)).await;
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason.contains("kernel.daily_runaway_usd"),
        "{}",
        q.reason
    );
    decline(&r, &q);
    let q = card_of(&r, both(1000.0)).await;
    assert!(!q.needs_proof, "{}", q.reason);
}

/// What a real proposal of `call`, on the file `FILE` with `extra` after it, asks at the card
/// and at the yes: an error when the card or the yes takes less than the code.
async fn code_at_card_and_yes(tool: &str, extra: &str, call: Value) -> Result<(), String> {
    let r = rig();
    if !extra.is_empty() {
        file_with(&r, extra);
    }
    let before = r.file_text();
    let sid = r.session();
    r.turn_with(tool, &sid, call.clone()).await;
    let (status, text) = r.result(&sid);
    if status != ResultStatus::Ok {
        return Err(format!("{call}: no card: {text}"));
    }
    let Some(q) = r.asked().into_iter().find(|q| q.session_id == sid) else {
        return Err(format!("{call}: no card: {text}"));
    };
    if !q.needs_proof && !q.needs_fresh_proof {
        return Err(format!("{call}: the card alone"));
    }
    let yes = r.core.confirm_action(&q.correlation_id, true, None, "cli");
    match yes {
        Ok(_) => return Err(format!("{call}: the yes alone was taken")),
        Err(e) if !e.to_string().contains("theseus owner proof enroll") => {
            return Err(format!("{call}: the yes was refused for another reason: {e:#}"))
        }
        Err(_) => {}
    }
    if r.file_text() != before {
        return Err(format!("{call}: written"));
    }
    Ok(())
}

/// Issue 57, round 4 (the review's counterexamples): every setting just outside the two closed
/// lists keeps main's code, at the card and at the yes, through a real proposal on a real
/// file, optional and non-default fields included. Each case runs on its own rig.
#[tokio::test]
async fn the_settings_just_outside_the_closed_lists_keep_mains_code() {
    let remove = |path: &str| {
        json!({"changes": [{"path": path, "remove": true}], "reason": "the owner asked for it"})
    };
    let cases: Vec<(&str, Value)> = vec![
        ("", change("self.install", json!(true))),
        ("", change("self", json!({"install": true, "mode": "act"}))),
        ("[mcp.servers.docs]\ncommand = [\"docs-server\"]\n", change("mcp.servers.docs.read", json!(["delete_everything"]))),
        (
            "[mcp.servers.docs]\ncommand = [\"docs-server\"]\n",
            change("mcp.servers.docs", json!({"command": ["docs-server"], "read": ["delete_everything"]})),
        ),
        ("[google]\nmax_sends_per_day = 5\n", change("google.max_sends_per_day", json!(500))),
        ("[google]\nmax_sends_per_day = 5\n", remove("google.max_sends_per_day")),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", change("self_fix.lines", json!(5000))),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", change("self_fix.files", json!(500))),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", change("self_fix.turn_minutes", json!(600))),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", remove("self_fix.lines")),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", remove("self_fix.files")),
        ("[self_fix]\nlines = 50\nfiles = 5\nturn_minutes = 10\n", remove("self_fix.turn_minutes")),
        ("[self.sealed]\nevery_branch = true\n", change("self.sealed.every_branch", json!(false))),
        ("", change("self.sealed.dir", json!("/tmp/sealed"))),
        ("", change("import.openclaw.state_dir", json!("/tmp/openclaw"))),
        ("", change("import.openclaw.agents", json!(["main"]))),
        ("", change("personas.theseus.files", json!(["/tmp/persona.md"]))),
        ("", change("context.files", json!(["/tmp/context.md"]))),
        ("", change("self_issues.enabled", json!(true))),
        ("", change("self.remote", json!("elsewhere"))),
        ("", change("self.reviewer_profile", json!("opus"))),
        ("", change("self.revert_window_hours", json!(0))),
        ("", change("model.end_note", json!(false))),
        ("", change("notify.unprompted_pace_secs", json!(0))),
    ];
    let mut wrong = Vec::new();
    for (extra, call) in cases {
        if let Err(why) = code_at_card_and_yes("config_propose", extra, call).await {
            wrong.push(why);
        }
    }
    assert!(wrong.is_empty(), "less than main's code:\n{}", wrong.join("\n"));
}

/// An AWS account's hands network keeps main's code, at the card and at the yes.
#[tokio::test]
async fn an_aws_accounts_hands_network_keeps_mains_code() {
    let secrets = FILE.replace(
        "anthropic_api_key = \"op://v/i/f\"\n",
        "anthropic_api_key = \"op://v/i/f\"\naws_access_key_id = \"op://v/i/a\"\n\
         aws_secret_access_key = \"op://v/i/b\"\n",
    );
    let r = rig();
    std::fs::write(
        &r.file,
        format!(
            "{secrets}\n[aws.accounts.111122223333]\nregion = \"us-west-2\"\n\n\
             [aws.accounts.111122223333.hands_network]\nvpc = \"vpc-0a1b2c3d4e5f60718\"\n\
             subnets = [\"subnet-0a1b2c3d4e5f60711\"]\n"
        ),
    )
    .unwrap();
    let call = change(
        "aws.accounts.111122223333.hands_network.security_group",
        json!("sg-0a1b2c3d4e5f60718"),
    );
    let q = card_of(&r, call.clone()).await;
    assert!(q.needs_proof, "{call}: {}", q.reason);
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap_err();
    assert!(e.to_string().contains("theseus owner proof enroll"), "{e:#}");
}

/// `self.install = true` put back by an undo keeps main's code too.
#[tokio::test]
async fn an_undo_that_puts_back_self_install_true_keeps_the_code() {
    let r = rig();
    let before = format!("{FILE}\n[self]\ninstall = true\n");
    r.seed(&before, FILE, &["self.install"]);
    let n = r.rows("config.changed")[0]["previous_copy"].as_u64().unwrap();
    let sid = r.session();
    r.turn_with("config_undo", &sid, json!({"change": n})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().into_iter().find(|q| q.session_id == sid).expect("a card");
    assert!(q.needs_proof, "{}", q.reason);
    let e = r
        .core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap_err();
    assert!(e.to_string().contains("theseus owner proof enroll"), "{e:#}");
    assert_eq!(r.file_text(), FILE);
}

/// Each setting of the closed list takes the card alone through a real proposal, and the yes
/// alone writes it.
#[tokio::test]
async fn each_setting_of_the_closed_list_takes_the_card_alone() {
    for call in [
        change("self.mode", json!("act")),
        change("google.services", json!(["gmail"])),
        change("vault.keep_by_discord", json!(true)),
        change("tools.projects_dir", json!("/tmp/projects")),
        change("index.weights_dir", json!("/tmp/weights")),
        change("lsp.servers.rust-analyzer.roots", json!(["Cargo.toml"])),
        change(
            "profiles.opus",
            json!({"model": "claude-sonnet-5-5", "system": "be brief"}),
        ),
        change("personas.theseus.card", json!("a calm voice")),
    ] {
        let r = rig();
        let q = card_of(&r, call.clone()).await;
        assert!(!q.needs_proof && !q.needs_fresh_proof, "{call}: {q:?}");
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        assert_ne!(r.file_text(), FILE, "{call}: written");
    }
}

/// Finding 6: a use of a vault secret keeps MAIN's requirement exactly. With the reference
/// already identical and a folder beside it, the code (main: the proposal names the secret);
/// with a changed reference, the code; a plain use, the card alone, and its yes writes.
#[tokio::test]
async fn a_vault_use_keeps_mains_requirement_exactly() {
    use std::sync::Arc;
    let rig_vault = |file: &str| {
        let r = rig();
        std::fs::write(&r.file, file).unwrap();
        let dir = tempfile::tempdir().unwrap();
        r.core
            .tools
            .look
            .set_vault(Arc::new(crate::secrets::look::tests::stub(dir.path(), "ok")));
        (r, dir)
    };
    let call = |also: Value| {
        json!({"vault": "Harbor", "item": "Lantern key", "field": "credential",
               "name": "lantern_key", "also": also,
               "reason": "the key is in the vault as Lantern key"})
    };
    let with_ref = |reference: &str| {
        FILE.replace(
            "anthropic_api_key = \"op://v/i/f\"\n",
            &format!("anthropic_api_key = \"op://v/i/f\"\nlantern_key = \"{reference}\"\n"),
        )
    };
    let roots = json!([{"path": "tools.roots", "value": ["/tmp/elsewhere"]}]);
    for (file, what) in [
        (with_ref("op://Harbor/Lantern key/credential"), "the same reference"),
        (with_ref("op://Harbor/Other key/credential"), "a changed reference"),
    ] {
        let (r, _d) = rig_vault(&file);
        let sid = r.session();
        r.turn_with("vault_use", &sid, call(roots.clone())).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{what}: {text}");
        let q = r.asked().into_iter().find(|q| q.session_id == sid).expect("a card");
        assert!(q.needs_proof, "{what}: as on main, the code: {}", q.reason);
        let e = r
            .core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap_err();
        assert!(e.to_string().contains("theseus owner proof enroll"), "{what}: {e:#}");
        assert_eq!(r.file_text(), file, "{what}");
    }
    let (r, _d) = rig_vault(FILE);
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let q = r.asked().into_iter().find(|q| q.session_id == sid).expect("a card");
    assert!(!q.needs_proof && !q.needs_fresh_proof, "a plain use: {q:?}");
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(r.file_text().contains("lantern_key = \"op://Harbor/Lantern key/credential\""));
}
