//! Initiative with a character (theseus-slvl.9), through the whole core: a
//! promise to follow up is kept by a wake in its turn or counted unkept; a
//! wake's turn and a task's speak as the session's persona and are said
//! first; a note the daemon writes itself carries the persona's file and the
//! standing block, stays inside the day's line, and falls to its template
//! (which names the persona) past it; the morning note obeys the level; and
//! a verdict is recorded and counted.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::initiative::{InitiativeRateParams, InitiativeWeekParams};
use theseus_protocol::memory::{MemoryConsolidateResult, SynthesisReport};
use theseus_protocol::standing::StandingAddParams;
use theseus_protocol::{Initiative, SessionKind, StandingKind, StandingScope};

use crate::bus::EventSink;
use crate::config::PersonaConfig;
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::test_support::rows_100k as rows;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const OWNER: u64 = 271_828_182_845_904_523;
const PLACE: &str = "dm:271828182845904523";

/// A stand-in model that keeps every request. `WAKE <after>: <note>` sets a
/// wake and then promises; `PROMISE` promises with nothing scheduled; a
/// wake's input is answered without a promise; the rest is "Noted." Its
/// first `cut` notes stop on `max_tokens` with half a sentence, as Sonnet 5.5
/// did when its thinking spent the cap (theseus-050g2).
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
    cut: Mutex<u32>,
}

/// What the 2026-10-23 morning note said before its cap ran out.
const CUT: &str = "Nine tasks finished in the last day and I've";

fn last_user(req: &ProviderRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .map(|m| match &m["content"] {
            Value::String(s) => s.clone(),
            Value::Array(b) => b
                .iter()
                .filter_map(|b| b["text"].as_str())
                .filter(|t| !crate::task_graph::view::is_head(t))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn script(req: &ProviderRequest) -> Scripted {
    let last = last_user(req);
    let answers_a_call = req
        .messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
    if answers_a_call {
        return Scripted::text("Pushed. I'll check back after CI.");
    }
    if last.contains("⏰ wake") {
        return Scripted::text("The build is green.");
    }
    if let Some(rest) = last.strip_prefix("WAKE ") {
        let (after, note) = rest.split_once(": ").unwrap();
        return Scripted::tools(
            "Setting it.",
            &[("t_wake", "wake_at", json!({"after": after, "note": note}))],
        );
    }
    if last.starts_with("PROMISE") {
        return Scripted::text("Pushed. I'll let you know when it lands.");
    }
    if last.starts_with("Facts:") || last.contains("Facts:\n") {
        return Scripted::text("Two builds finished overnight and nothing needs you.");
    }
    Scripted::text("Noted.")
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let cut = {
                let mut left = self.cut.lock().unwrap();
                let cut = *left > 0 && last_user(req).starts_with("Facts:");
                *left -= u32::from(cut);
                cut
            };
            let answer = match cut {
                true => Scripted::Blocks {
                    blocks: vec![json!({"type": "text", "text": CUT})],
                    stop_reason: "max_tokens".into(),
                },
                false => script(req),
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

/// A core with a persona `ondine` (name Ondine, emoji 🌊, its file's marker
/// words) in play, a DM bound, and `tweak` applied.
fn rig(level: Initiative, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let f: PathBuf = dir.path().join("ondine.md");
    std::fs::write(
        &f,
        "ONDINE-WORDS: you are Ondine, who keeps the tide tables.",
    )
    .unwrap();
    cfg.personas.insert(
        "ondine".into(),
        PersonaConfig {
            files: vec![f.to_string_lossy().to_string().into()],
            name: Some("Ondine".into()),
            emoji: Some("🌊".into()),
            initiative: level,
            ..Default::default()
        },
    );
    cfg.context.default_persona = Some("ondine".into());
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![BoundPlace {
        target: format!("discord:dm:{OWNER}"),
        name: "DM @wren".into(),
        ..Default::default()
    }]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

fn session(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox.bind_place(PLACE, &r.session_id).unwrap();
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn system(req: &ProviderRequest) -> String {
    req.system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn finding(text: &str) -> MemoryConsolidateResult {
    MemoryConsolidateResult {
        clusters: vec![SynthesisReport {
            cluster: "c1".into(),
            outcome: "supported".into(),
            text: Some(text.into()),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// A reply that promises a follow-up in a turn that set a wake keeps its word;
/// one that schedules nothing is `promise.unkept`; a reply that promises
/// nothing writes neither.
#[tokio::test]
async fn a_promise_with_a_wake_is_kept_and_one_without_is_counted_unkept() {
    let r = rig(Initiative::Normal, |_| {});
    let sid = session(&r.core);
    turn(&r.core, &sid, "WAKE 1h: check CI").await;
    turn(&r.core, &sid, "PROMISE").await;
    turn(&r.core, &sid, "hello").await;
    let kept = rows(&r.core, "promise.kept");
    let unkept = rows(&r.core, "promise.unkept");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0]["scheduled"], 1);
    assert_eq!(kept[0]["phrase"], "i'll check");
    assert_eq!(unkept.len(), 1, "{unkept:?}");
    assert_eq!(unkept[0]["phrase"], "i'll let you know");
    let week = r
        .core
        .initiative_week(&InitiativeWeekParams::default())
        .unwrap();
    assert_eq!((week.kept, week.unkept, week.kept_share), (1, 1, Some(0.5)));
}

/// A wake's turn is the session's persona speaking first: its request carries
/// the persona's file, and its `initiative.spoke` row names the persona.
#[tokio::test]
async fn a_wakes_turn_speaks_as_the_persona_and_is_said_first() {
    let r = rig(Initiative::Normal, |_| {});
    let sid = session(&r.core);
    let driver = tokio::spawn(crate::harness::drive(r.core.clone()));
    turn(&r.core, &sid, "WAKE 1s: check the build").await;
    until("a wake's row", || {
        !rows(&r.core, "initiative.spoke").is_empty()
    })
    .await;
    driver.abort();
    let spoke = rows(&r.core, "initiative.spoke");
    assert_eq!(spoke[0]["kind"], "wake");
    assert_eq!(spoke[0]["persona"], "Ondine");
    let wake_request = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|q| last_user(q).contains("⏰ wake"))
        .cloned()
        .expect("the wake's turn asked the model");
    assert!(
        system(&wake_request).contains("ONDINE-WORDS"),
        "{}",
        system(&wake_request)
    );
}

/// A task's turns speak as its parent's persona: its request spec carries the
/// persona's file, so its report to the owner is in that voice.
#[tokio::test]
async fn a_tasks_request_carries_its_parents_persona() {
    let r = rig(Initiative::Normal, |_| {});
    let parent = session(&r.core);
    let mut child = SessionRecord::new(SessionKind::Task, None);
    child.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_none".into(),
        by: "toolu_none".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&child.session_id, &child).unwrap();
    let (live, _) = r.core.live_profile();
    let mut target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    target.persona = r.core.runner.persona_of(&child);
    assert_eq!(target.persona.as_deref(), Some("ondine"));
    let (spec, _) = r.core.runner.request_spec(
        &target,
        SessionKind::Task,
        crate::ceiling::PlaceView {
            class: crate::places::PlaceClass::Private,
            ceiling: None,
            voice: false,
            draft: None,
        },
    );
    assert!(
        spec.persona_text.contains("ONDINE-WORDS"),
        "{}",
        spec.persona_text
    );
}

/// A consolidation finding is written by a short call whose system holds the
/// persona's file and the owner's standing orders, posted to the owner's DM
/// with its row, only when the level lets an observation through.
#[tokio::test]
async fn a_consolidation_finding_carries_the_persona_and_the_standing_block() {
    let chatty = rig(Initiative::Chatty, |_| {});
    chatty
        .core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: "Never use a table for a quick answer.".into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    chatty
        .core
        .say_consolidation(&finding("Tide tables are kept in the harbour file."));
    until("a consolidation row", || {
        !rows(&chatty.core, "initiative.spoke").is_empty()
    })
    .await;
    let req = chatty
        .model
        .requests
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    let sys = system(&req);
    assert!(sys.contains("ONDINE-WORDS"), "{sys}");
    assert!(sys.contains("Never use a table"), "{sys}");
    assert!(req.tools.is_empty(), "no tools, no history: one short call");
    let spoke = rows(&chatty.core, "initiative.spoke");
    assert_eq!(spoke[0]["kind"], "consolidation");
    assert_eq!(
        spoke[0]["text"],
        "Two builds finished overnight and nothing needs you."
    );
    let posts: Vec<Value> = chatty
        .core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "initiative")
        .map(|a| crate::outbox::body_of(&a).clone())
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["id"], spoke[0]["id"]);

    // At `normal` an observation is not said at all.
    let normal = rig(Initiative::Normal, |_| {});
    normal
        .core
        .say_consolidation(&finding("Tide tables are kept in the harbour file."));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(rows(&normal.core, "initiative.spoke").is_empty());
    assert!(normal.model.requests.lock().unwrap().is_empty());
}

/// Past the day's line, or with the model off, a note is its template, which
/// takes the persona's name and emoji, and no model is asked.
#[tokio::test]
async fn past_the_days_line_a_note_is_its_template_in_the_personas_name() {
    for day_usd in [0.0, 0.000_001] {
        let r = rig(Initiative::Chatty, |c| c.initiative.day_usd = day_usd);
        r.core
            .say_consolidation(&finding("Tide tables are kept in the harbour file."));
        until("a template row", || {
            !rows(&r.core, "initiative.spoke").is_empty()
        })
        .await;
        let spoke = rows(&r.core, "initiative.spoke");
        assert_eq!(
            spoke[0]["text"],
            "🌊 Ondine: tonight's memory pass wrote one new note from what we talked about."
        );
        assert!(
            r.model.requests.lock().unwrap().is_empty(),
            "day_usd {day_usd}"
        );
    }
}

/// The day's line is read back from the notes' rows: once today's voiced notes
/// have spent what the line leaves less than a call's worst case, the next
/// is its template, and no model is asked.
#[tokio::test]
async fn the_day_cap_counts_what_the_voiced_notes_spent() {
    let r = rig(Initiative::Chatty, |c| c.initiative.day_usd = 0.05);
    let facts = crate::initiative::speak::Facts {
        kind: "consolidation",
        brief: "Tonight's memory pass wrote notes.",
        line: crate::initiative::lines::Line::Consolidation,
        n: 2,
    };
    let first = r.core.initiative_speak(&facts).await;
    assert!(first.voiced, "{first:?}");
    assert_eq!(r.model.requests.lock().unwrap().len(), 1);
    // An earlier note today that spent all but a tenth of a cent.
    let earlier = crate::initiative::speak::Spoken {
        id: "ini_earlier".into(),
        text: "Earlier.".into(),
        voiced: true,
        cost_usd: 0.049,
        model: None,
        stop_reason: None,
        output_tokens: None,
    };
    r.core.initiative_post("consolidation", &earlier).unwrap();
    let spent = r.core.initiative_spent_today();
    assert!(
        spent >= theseus_judge::price::usd_to_micros(0.049),
        "{spent}"
    );
    let second = r.core.initiative_speak(&facts).await;
    assert!(!second.voiced, "{second:?}");
    assert!(second.text.starts_with("🌊 Ondine: "), "{}", second.text);
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        1,
        "the second note asked no model"
    );
}

/// The morning note: `quiet` says none and reads nothing; `normal` says one on
/// a day something happened, once, and not on a quiet one.
#[tokio::test]
async fn the_morning_note_obeys_the_level_and_goes_once_on_a_day_something_happened() {
    let hour = |c: &mut Config| c.initiative.morning_hour = 0;
    let quiet = rig(Initiative::Quiet, hour);
    let sid = session(&quiet.core);
    turn(&quiet.core, &sid, "PROMISE").await;
    assert!(
        !quiet.core.post_morning_if_due(),
        "quiet sends no morning note"
    );

    let calm = rig(Initiative::Normal, hour);
    assert!(calm.core.post_morning_if_due(), "the day is claimed");
    until("the day's mark", || {
        calm.core
            .store
            .get_meta::<String>(crate::initiative::morning::MARK)
            .ok()
            .flatten()
            .is_some()
    })
    .await;
    assert!(
        rows(&calm.core, "initiative.spoke").is_empty(),
        "nothing happened"
    );
    assert!(!calm.core.post_morning_if_due(), "once a day");

    let busy = rig(Initiative::Normal, hour);
    let sid = session(&busy.core);
    turn(&busy.core, &sid, "PROMISE").await;
    assert!(busy.core.post_morning_if_due());
    until("the morning note", || {
        !rows(&busy.core, "initiative.spoke").is_empty()
    })
    .await;
    let spoke = rows(&busy.core, "initiative.spoke");
    assert_eq!(spoke[0]["kind"], "morning");
    assert!(
        system(&busy.model.requests.lock().unwrap().last().cloned().unwrap())
            .contains("ONDINE-WORDS")
    );
    assert!(
        !busy.core.post_morning_if_due(),
        "a second ask the same day writes none"
    );
}

/// The morning note is unprompted: it waits for the pacer's window (another
/// unprompted post was just written) and claims the day only once it opens.
#[tokio::test]
async fn the_morning_note_waits_for_the_pacers_window() {
    let r = rig(Initiative::Normal, |c| {
        c.initiative.morning_hour = 0;
        c.notify.unprompted_pace_secs = 2;
    });
    let sid = session(&r.core);
    turn(&r.core, &sid, "PROMISE").await;
    r.core.outbox.pacer.took(theseus_protocol::now_unix_ms());
    assert!(!r.core.post_morning_if_due(), "the window is closed");
    assert!(
        r.core
            .store
            .get_meta::<String>(crate::initiative::morning::MARK)
            .ok()
            .flatten()
            .is_none(),
        "the day is not claimed while it waits"
    );
    tokio::time::sleep(std::time::Duration::from_millis(2_100)).await;
    assert!(r.core.post_morning_if_due(), "the window opened");
    until("the morning note", || {
        !rows(&r.core, "initiative.spoke").is_empty()
    })
    .await;
}

/// A verdict is recorded against the message it names, the last one stands,
/// and the week counts it; a message Theseus did not send first is refused.
#[tokio::test]
async fn a_verdict_is_recorded_and_counted_and_the_last_one_stands() {
    let r = rig(Initiative::Chatty, |_| {});
    r.core
        .say_consolidation(&finding("Tide tables are kept in the harbour file."));
    until("a note", || !rows(&r.core, "initiative.spoke").is_empty()).await;
    let id = rows(&r.core, "initiative.spoke")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rate = |rating: &str, turn: Option<&str>| InitiativeRateParams {
        session_id: None,
        turn_id: turn.map(str::to_string),
        rating: rating.into(),
        via: Some("cli".into()),
        discord: None,
    };
    let done = r.core.initiative_rate(&rate("up", None), "cli").unwrap();
    assert_eq!(
        (done.id.as_str(), done.rating.as_str()),
        (id.as_str(), "up")
    );
    let rated = rows(&r.core, "initiative.rated");
    assert_eq!(rated.len(), 1);
    assert_eq!(rated[0]["id"], json!(id));
    let week = r
        .core
        .initiative_week(&InitiativeWeekParams::default())
        .unwrap();
    assert_eq!((week.up, week.down, week.up_share), (1, 0, Some(1.0)));
    r.core
        .initiative_rate(&rate("down", Some(&id)), "cli")
        .unwrap();
    let week = r
        .core
        .initiative_week(&InitiativeWeekParams::default())
        .unwrap();
    assert_eq!((week.up, week.down, week.up_share), (0, 1, Some(0.0)));
    assert_eq!(week.spoke, 1);
    assert_eq!(week.level, "chatty");
    // Not a message sent first, and not a verdict.
    assert!(r
        .core
        .initiative_rate(&rate("up", Some("turn_nothing")), "cli")
        .unwrap_err()
        .to_string()
        .contains("not a message Theseus sent first"));
    assert!(r
        .core
        .initiative_rate(&rate("sideways", None), "cli")
        .unwrap_err()
        .to_string()
        .contains("no verdict"));
}

fn note_facts() -> crate::initiative::speak::Facts<'static> {
    crate::initiative::speak::Facts {
        kind: "morning",
        brief: "In the last day:\n- 9 tasks ended",
        line: crate::initiative::lines::Line::Morning,
        n: 9,
    }
}

/// An answer cut off at its cap is never posted (theseus-050g2): the note is
/// asked once more with more room, and when that is cut off too, the
/// template goes out. The row says how the last call stopped and what it
/// wrote, and books both calls.
#[tokio::test]
async fn a_cut_off_answer_is_never_posted_and_two_are_the_template() {
    use crate::initiative::speak::{MAX_TOKENS, RETRY_MAX_TOKENS};
    let r = rig(Initiative::Chatty, |_| {});
    *r.model.cut.lock().unwrap() = 2;
    let spoken = r.core.initiative_speak(&note_facts()).await;
    assert!(!spoken.voiced, "{spoken:?}");
    assert!(!spoken.text.contains(CUT), "{}", spoken.text);
    assert!(spoken.text.starts_with("🌊 Ondine: "), "{}", spoken.text);
    let caps: Vec<u32> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| q.max_tokens)
        .collect();
    assert_eq!(caps, [MAX_TOKENS, RETRY_MAX_TOKENS]);
    // Both calls are booked: the stand-in bills each the same (the
    // request's estimate in, CUT's nine words out).
    let one = {
        let reqs = r.model.requests.lock().unwrap();
        let usage = theseus_protocol::Usage {
            input_tokens: reqs[0].estimate_tokens(),
            output_tokens: 9,
            ..Default::default()
        };
        r.core
            .runner
            .catalog
            .get(&reqs[0].model)
            .unwrap()
            .cost_micros(&usage)
    };
    assert!(one > 0);
    assert_eq!(
        theseus_judge::price::usd_to_micros(spoken.cost_usd),
        2 * one,
        "both calls are booked: {spoken:?}"
    );
    r.core.initiative_post("morning", &spoken).unwrap();
    let spoke = rows(&r.core, "initiative.spoke");
    assert_eq!(spoke[0]["text"], spoken.text);
    assert_eq!(spoke[0]["stop_reason"], "max_tokens");
    assert_eq!(spoke[0]["output_tokens"], 9);
}

/// The retry is held to the day's line with what the first call spent
/// (theseus-kf4pf): a line that admits the 2,048 reservation alone (so the
/// 1,024 one first) but not the first call's cost plus it sends one request,
/// and the template stands in with the first call's stop.
#[tokio::test]
async fn a_cut_off_answer_whose_retry_the_days_line_refuses_is_the_template() {
    use crate::initiative::speak::{MAX_TOKENS, RETRY_MAX_TOKENS};
    // The reservations, on a rig like the capped one.
    let probe = rig(Initiative::Chatty, |_| {});
    let persona = probe.core.personas().persona_for(None, None);
    let reserve = |cap| {
        let q = probe
            .core
            .initiative_request(&persona, &note_facts(), "claude-sonnet-5-5", cap);
        probe.core.audit_reserve(&q).unwrap()
    };
    let (first, retry) = (reserve(MAX_TOKENS), reserve(RETRY_MAX_TOKENS));
    assert!(retry > first);
    // A line one micro past the retry's reservation: both fit alone, and the
    // cut call's cost plus the retry's does not.
    let line = theseus_judge::price::micros_to_usd(retry + 1);
    let r = rig(Initiative::Chatty, |c| c.initiative.day_usd = line);
    *r.model.cut.lock().unwrap() = 1;
    let spoken = r.core.initiative_speak(&note_facts()).await;
    assert!(!spoken.voiced, "{spoken:?}");
    assert!(spoken.text.starts_with("🌊 Ondine: "), "{}", spoken.text);
    assert_eq!(spoken.stop_reason.as_deref(), Some("max_tokens"));
    let caps: Vec<u32> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| q.max_tokens)
        .collect();
    assert_eq!(caps, [MAX_TOKENS], "the retry was refused by the line");
}

/// One cut-off answer is retried with more room, and the retry's whole
/// answer is the note; its row records `end_turn`.
#[tokio::test]
async fn a_cut_off_answer_is_retried_with_more_room() {
    let r = rig(Initiative::Chatty, |_| {});
    *r.model.cut.lock().unwrap() = 1;
    let spoken = r.core.initiative_speak(&note_facts()).await;
    assert!(spoken.voiced, "{spoken:?}");
    assert_eq!(
        spoken.text,
        "Two builds finished overnight and nothing needs you."
    );
    assert_eq!(r.model.requests.lock().unwrap().len(), 2);
    r.core.initiative_post("morning", &spoken).unwrap();
    let spoke = rows(&r.core, "initiative.spoke");
    assert_eq!(spoke[0]["stop_reason"], "end_turn");
    assert_eq!(spoke[0]["output_tokens"], 8);
}

/// The note's request turns thinking off (Sonnet 5.5's `between_tools`, its
/// `disabled` being a 400) and leaves room past two sentences.
#[tokio::test]
async fn the_notes_request_turns_thinking_off_at_a_roomy_cap() {
    let r = rig(Initiative::Chatty, |_| {});
    let spoken = r.core.initiative_speak(&note_facts()).await;
    assert!(spoken.voiced, "{spoken:?}");
    let reqs = r.model.requests.lock().unwrap();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].model, "claude-sonnet-5-5");
    assert_eq!(reqs[0].thinking, Some(json!({"type": "between_tools"})));
    assert_eq!(reqs[0].body()["thinking"], json!({"type": "between_tools"}));
    assert!(reqs[0].max_tokens >= 1024, "{}", reqs[0].max_tokens);
}
