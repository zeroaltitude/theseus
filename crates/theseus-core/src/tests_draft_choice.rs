//! Jev chooses which coding turns a China-origin draft is made for
//! (theseus-kqeih; the owner, 2026-10-27 23:05 and 23:15: "let it be routed by
//! jev with decent standing orders"). `[routing] draft_by_jev` is off by
//! default: a class with a `cn_draft` then drafts whenever it is eligible, as
//! before, and Jev is not asked. On, the drafting turn needs Jev's `draft`
//! answer to be `cheap` and sure; the labels and the other conditions of
//! `draft_step` still come first, so a China-origin model's data rule is not
//! Jev's to waive. The rig is `tests_draft`'s.

use serde_json::Value;
use theseus_judge::fake::Scripted as Jev;

use crate::tests_dm_rig::say;
use crate::tests_draft::{draft_edit, load, rig_with, ASK};
use crate::tests_route::decided;

/// `tweak` for a rig whose operator lets Jev choose. Through the config's own
/// serde shape, so a build without the key fails here and not at the compile.
fn letting_jev_choose(cfg: &mut crate::Config) {
    let mut v = serde_json::to_value(&cfg.routing).unwrap();
    v["draft_by_jev"] = Value::Bool(true);
    cfg.routing = serde_json::from_value(v).expect("[routing] draft_by_jev parses");
}

fn jev_says(option: &str, confidence: f64) -> impl FnOnce(&theseus_judge::fake::FakeJev) + '_ {
    move |jev| {
        jev.script(
            "draft",
            Jev::Choice {
                option: option.into(),
                confidence,
            },
        )
    }
}

fn last_row(d: &crate::tests_draft::Draft) -> Value {
    decided(&d.r.core.store).pop().expect("a route row")
}

/// Off, nothing changes: the eligible coding turn drafts, Jev is never asked
/// `draft`, and the route row carries no `draft_choice`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_a_coding_turn_drafts_as_before_and_jev_is_not_asked() {
    let d = rig_with(|_| {}, |_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert!(
        res.output.starts_with("Drafting on dsflash ("),
        "{}",
        res.output
    );
    assert!(
        last_row(&d).get("draft_choice").is_none(),
        "{}",
        last_row(&d)
    );
}

/// On, Jev's sure `cheap` drafts, and the row says Jev chose it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sure_cheap_answer_drafts_and_the_row_says_jev_chose() {
    let d = rig_with(letting_jev_choose, jev_says("cheap", 0.95));
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert!(
        res.output.starts_with("Drafting on dsflash ("),
        "{}",
        res.output
    );
    let row = last_row(&d);
    assert_eq!(row["draft_choice"], "cheap", "{row}");
    assert!(row.get("draft_skip").is_none(), "{row}");
}

/// On, `strong` runs the turn on the class's own profile, no draft opens, and
/// the row says why, in the word the bench report counts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_strong_answer_runs_the_class_profile_and_says_so() {
    let d = rig_with(letting_jev_choose, jev_says("strong", 0.95));
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    assert!(d.tasks(&sid).is_empty(), "no draft");
    assert!(
        d.zai.sent().is_empty(),
        "the China-origin model saw nothing"
    );
    let row = last_row(&d);
    assert_eq!(row["draft_choice"], "strong", "{row}");
    assert_eq!(row["draft_skip"], "jev_declined", "{row}");
}

/// On, a `cheap` that is not sure (under the pack's confirm bar) fails to the
/// stronger model, as does an answer that never came.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_cheap_answer_fails_to_the_stronger_model() {
    let d = rig_with(letting_jev_choose, jev_says("cheap", 0.30));
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    assert!(d.tasks(&sid).is_empty(), "no draft");
    let row = last_row(&d);
    assert_eq!(row["draft_skip"], "jev_unsure", "{row}");
}

/// Jev's say does not waive the data rule: a `cheap` answer on a session whose
/// labels keep a China-origin model off still does not draft, and the row's
/// word is the label's, as it was before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cheap_answer_does_not_waive_the_labels() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use crate::session::SessionRecord;
    let d = rig_with(letting_jev_choose, jev_says("cheap", 0.95));
    let sid = d.dm(&d.repo);
    let mut rec: SessionRecord = d.r.core.store.get_session(&sid).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: None,
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.9,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    rec.data = Some(Box::new(flags));
    d.r.core.store.put_session(&sid, &rec).unwrap();
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    assert!(d.zai.sent().is_empty());
    assert_eq!(last_row(&d)["draft_skip"], "labelled");
}

/// The template with `keys` added to its `[routing]` table (a second `[routing]`
/// header is a duplicate key), and `routing` appended, loaded as the daemon does.
fn load_with_keys(keys: &str, routing: &str) -> Result<crate::Config, String> {
    let extra = "[providers.deepseek]\napi_base = \"https://api.deepseek.com/anthropic\"\n\
        api_key_secret = \"zai_api_key\"\n[profiles.dsflash]\nprovider = \"deepseek\"\n\
        model = \"deepseek-flash\"\n";
    let base =
        crate::Config::EXAMPLE_TOML.replacen("\n[routing]\n", &format!("\n[routing]\n{keys}\n"), 1);
    crate::Config::parse(&format!("{base}\n{extra}\n{routing}\n"))
        .map(|(cfg, _)| cfg)
        .map_err(|e| format!("{e:#}"))
}

/// The key's own checks at load: it chooses among drafts, so a config in which
/// no class drafts is a mistake that says so; it is off by default, and the
/// template documents it where an operator reads it, commented.
#[test]
fn the_switch_ships_off_and_needs_a_class_that_drafts() {
    let off = load("").unwrap();
    let shown = toml::to_string(&off.routing).unwrap();
    assert!(!shown.contains("draft_by_jev"), "{shown}");
    let e = load_with_keys("draft_by_jev = true", "").unwrap_err();
    assert!(
        e.contains("routing.draft_by_jev") && e.contains("cn_draft"),
        "{e}"
    );
    let ok = load_with_keys(
        "draft_by_jev = true",
        "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]",
    )
    .unwrap();
    assert!(toml::to_string(&ok.routing)
        .unwrap()
        .contains("draft_by_jev = true"));
    let template = include_str!("../config/theseus.example.toml");
    assert!(
        template.contains("# draft_by_jev = true"),
        "documented, commented"
    );
}
