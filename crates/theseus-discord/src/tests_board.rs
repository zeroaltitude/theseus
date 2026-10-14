//! The live board, the needs-you card and the status line, end to end
//! (theseus-2j2ka): a real core with the work engine, the binding on the
//! stand-in's REST and gateway, and a stand-in model that starts tasks.
//!
//! The model is one provider that answers by the first message of the session
//! it is asked for: the owner's ask opens three tasks, and each task has its
//! own script (one asks for a write outside the roots, so it waits on a card;
//! two run a program that sleeps).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::approval::{Client, Surface};
use theseus_core::policy::Posture;
use theseus_core::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use theseus_protocol::answer::WorkAnswerParams;
use theseus_sim::fake_discord::{Guild, Msg, IS_COMPONENTS_V2};

use crate::render::board_v2;
use crate::rpc_client::RpcClient;
use crate::tests_gateway::{Rig, ANA, ANA_DM, BEN, CY, LAB};

use theseus_sim::fake_discord::DEFAULT_GUILD;

const ASK: &str = "START: run this as a background task, please";
const QUOTE: &str = "run this as a background task, please";
const REEF: &str = "Reef-chart: chart the reef";
const BUOY: &str = "Buoy-paint: paint the buoy";
const TIDE: &str = "Tide-count: count the tides";

/// One provider for the parent and its tasks: the session it is asked for is
/// found by what its first message holds.
struct Switch {
    parent: FakeProvider,
    kids: Vec<(&'static str, FakeProvider)>,
}

impl Provider for Switch {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let first = req
            .messages
            .first()
            .map(Value::to_string)
            .unwrap_or_default();
        let p = self
            .kids
            .iter()
            .find(|(m, _)| first.contains(m))
            .map_or(&self.parent, |(_, p)| p);
        p.stream_message(req, on_delta)
    }
}

fn scripted(s: Vec<Scripted>) -> FakeProvider {
    FakeProvider::scripted(s)
}

fn start_all(briefs: &[&str]) -> Scripted {
    start_quoting(briefs, QUOTE)
}

/// `task_create` for each brief, each quoting `quote` from the owner's ask.
fn start_quoting(briefs: &[&str], quote: &str) -> Scripted {
    let calls: Vec<(String, Value)> = briefs
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                format!("t_task{i}"),
                json!({"brief": b, "arrangement": {"pieces": [{"quote": quote, "role": "objective"}]}}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, v)| (id.as_str(), "task_create", v.clone()))
        .collect();
    Scripted::tools("Starting.", &calls)
}

/// A task that writes outside the roots, so it waits on a card.
fn asks(dir: &Path, name: &str) -> Vec<Scripted> {
    let path = dir.join("outside").join(name);
    vec![
        Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": path.to_string_lossy(), "content": name}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

/// A task that runs a program for a few seconds.
fn sleeps(secs: u32) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "",
            &[(
                "p1",
                "proc_run",
                json!({"argv": ["sleep", secs.to_string()]}),
            )],
        ),
        Scripted::text("Done."),
    ]
}

fn model(parent: Vec<Scripted>, kids: Vec<(&'static str, Vec<Scripted>)>) -> Arc<dyn Provider> {
    Arc::new(Switch {
        parent: scripted(parent),
        kids: kids.into_iter().map(|(m, s)| (m, scripted(s))).collect(),
    })
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA, BEN, CY])
}

/// ana's DM, and `#lab` bound private with cy allowed to press, who is not
/// an owner.
fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\", \"{CY}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn tweak(c: &mut theseus_core::Config) {
    c.policy.tools.insert("proc.run".into(), Posture::Open);
    c.discord.edit_interval_ms = 250;
}

/// The bot's V2 messages in `channel` that carry the board's Clear button.
fn boards(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| {
            m.flags & IS_COMPONENTS_V2 != 0
                && m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR)
        })
        .collect()
}

/// The bot's cards in `channel`: V2 messages with a `wa:` answer button.
fn cards(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| {
            m.flags & IS_COMPONENTS_V2 != 0
                && m.buttons.iter().any(|b| b.custom_id.starts_with("wa:"))
        })
        .collect()
}

#[tokio::test]
async fn three_tasks_one_asking_make_one_silent_pinned_board_and_one_pinging_card() {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![
                    start_all(&[REEF, BUOY, TIDE]),
                    Scripted::text("Started all three."),
                ],
                vec![
                    (REEF, asks(dir, "reef.txt")),
                    (BUOY, sleeps(8)),
                    (TIDE, sleeps(8)),
                ],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the board follows three tasks, one asking", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("🔴 1 needs you · ⚙️ 2 running"))
            && cards(&r, ANA_DM).len() == 1
    })
    .await;
    let b = boards(&r, ANA_DM);
    assert_eq!(b.len(), 1, "one board a place: {b:#?}");
    assert!(b[0].silent(), "the board never pings");
    r.until("the board pinned", || boards(&r, ANA_DM)[0].pinned)
        .await;
    assert!(
        b[0].content.contains("**🔴 Needs you**"),
        "{}",
        b[0].content
    );
    assert!(
        b[0].content.contains("Reef-chart: chart the reef"),
        "{}",
        b[0].content
    );
    assert!(
        b[0].content.contains("fs.write"),
        "the question is on the board: {}",
        b[0].content
    );
    let ids: Vec<&str> = b[0].buttons.iter().map(|x| x.custom_id.as_str()).collect();
    assert!(
        ids.contains(&"tb:clear") && ids.contains(&"tb:quiet") && ids.contains(&"tb:pick"),
        "{ids:?}"
    );
    // The card: the one ping.
    let c = &cards(&r, ANA_DM)[0];
    assert!(!c.silent(), "the needs-you card pings: {:?}", c.flags);
    let wa: Vec<&str> = c.buttons.iter().map(|x| x.custom_id.as_str()).collect();
    assert_eq!(wa.len(), 2, "{wa:?}");
    assert!(
        wa[0].starts_with("wa:") && wa[0].ends_with(":approve"),
        "{wa:?}"
    );
    assert!(wa[1].ends_with(":decline"), "{wa:?}");
    assert!(
        c.content.contains("**Approve?**"),
        "the card keeps its words: {}",
        c.content
    );
    // Nothing else the binding posted for the tasks pinged but chat messages
    // and that one card: the board edits are not creates.
    assert_eq!(boards(&r, ANA_DM).len(), 1);
}

#[tokio::test]
async fn an_answer_pressed_on_the_card_goes_through_work_answer_and_settles_card_and_board() {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start_all(&[REEF, BUOY]), Scripted::text("Started both.")],
                vec![(REEF, asks(dir, "reef.txt")), (BUOY, sleeps(6))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("a card and a board that says so", || {
        cards(&r, ANA_DM).len() == 1
            && boards(&r, ANA_DM)
                .first()
                .is_some_and(|b| b.content.contains("🔴 1 needs you"))
    })
    .await;
    let card = cards(&r, ANA_DM).remove(0);
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until(
        "the card settled and the board without the question",
        || {
            let c = r
                .fake
                .messages(ANA_DM)
                .into_iter()
                .find(|m| m.id == card.id)
                .unwrap();
            c.buttons.is_empty()
                && c.content.contains("Approved")
                && boards(&r, ANA_DM)
                    .first()
                    .is_some_and(|b| !b.content.contains("🔴"))
        },
    )
    .await;
    // The answer was work.answer's, as the presser.
    let rows = r.ledger("discord.confirm");
    assert!(
        rows.iter()
            .any(|d| d["option"] == "approve" && d["ok"] == true && d["by"] == "discord:ana"),
        "{rows:?}"
    );
    let written = r.dir.path().join("outside").join("reef.txt");
    r.until("the approved write ran", || written.exists()).await;
}

#[tokio::test]
async fn an_answer_from_the_cli_settles_the_discord_card() {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start_all(&[REEF]), Scripted::text("Started it.")],
                vec![(REEF, asks(dir, "reef.txt"))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the card", || cards(&r, ANA_DM).len() == 1).await;
    let card = cards(&r, ANA_DM).remove(0);
    let q6 = card.buttons[0]
        .custom_id
        .split(':')
        .nth(1)
        .unwrap()
        .to_string();
    let (cli, _notes) = RpcClient::connect(r.core.clone(), Client::new("cli", Surface::Cli));
    let done: theseus_protocol::answer::WorkAnswerResult = cli
        .call(
            theseus_protocol::method::WORK_ANSWER,
            WorkAnswerParams {
                question_id: q6,
                option: "approve".into(),
                author: Some("the owner".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(done.option, "approve");
    r.until("the Discord card settled by the CLI's answer", || {
        let c = r
            .fake
            .messages(ANA_DM)
            .into_iter()
            .find(|m| m.id == card.id)
            .unwrap();
        c.buttons.is_empty() && c.content.contains("Approved")
    })
    .await;
}

#[tokio::test]
async fn a_refused_press_is_told_to_the_presser_alone_and_the_card_keeps_waiting() {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start_all(&[REEF]), Scripted::text("Started it.")],
                vec![(REEF, asks(dir, "reef.txt"))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    r.until("the card in #lab", || cards(&r, LAB).len() == 1)
        .await;
    let card = cards(&r, LAB).remove(0);
    // cy may press in #lab, and is no owner: the core refuses the answer.
    r.press(&card.id, "Approve", (CY, "cy"));
    r.until("cy told, alone", || {
        r.fake.replies().iter().any(|x| {
            x.kind == "followup"
                && x.ephemeral
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.contains("did not count"))
        })
    })
    .await;
    let c = r
        .fake
        .messages(LAB)
        .into_iter()
        .find(|m| m.id == card.id)
        .unwrap();
    assert_eq!(c.buttons.len(), 2, "the card keeps its buttons: {c:?}");
    assert!(!c.content.contains("Approved"));
    // The owner's answer still counts.
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the owner's answer settles it", || {
        r.fake
            .messages(LAB)
            .into_iter()
            .find(|m| m.id == card.id)
            .unwrap()
            .buttons
            .is_empty()
    })
    .await;
}

#[tokio::test]
async fn a_burst_of_questions_pings_once_and_then_says_how_many_need_you() {
    crate::courier::v2::card_window_for_tests(Duration::from_secs(3));
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![
                    start_all(&[REEF, BUOY, TIDE]),
                    Scripted::text("Started all three."),
                ],
                vec![
                    (REEF, asks(dir, "a.txt")),
                    (BUOY, asks(dir, "b.txt")),
                    (TIDE, asks(dir, "c.txt")),
                ],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("three cards", || cards(&r, ANA_DM).len() == 3)
        .await;
    let pings: Vec<bool> = cards(&r, ANA_DM).iter().map(|c| !c.silent()).collect();
    assert_eq!(
        pings.iter().filter(|p| **p).count(),
        1,
        "one ping for the burst: {pings:?}"
    );
    r.until("the summary", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("3 questions need you"))
    })
    .await;
    let summary = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("3 questions need you"))
        .unwrap();
    assert!(!summary.silent(), "the burst's one buzz");
    // Answering them all closes the summary.
    for c in cards(&r, ANA_DM) {
        r.press(&c.id, "Approve", (ANA, "ana"));
    }
    r.until("the summary says they are answered", || {
        r.fake
            .messages(ANA_DM)
            .into_iter()
            .find(|m| m.id == summary.id)
            .is_some_and(|m| m.content.contains("answered"))
    })
    .await;
}

#[tokio::test]
async fn the_status_line_reads_the_counts_and_clears_when_nothing_works() {
    crate::runtime::presence::gap_for_tests(Duration::from_millis(600));
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start_all(&[REEF, BUOY]), Scripted::text("Started both.")],
                vec![(REEF, asks(dir, "reef.txt")), (BUOY, sleeps(3))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    let gw = r.fake.gateway().unwrap().clone();
    let states = move || -> Vec<(u64, Value)> { gw.state().presences };
    r.say((ANA, "ana"), None, ASK);
    r.until("a status with a count that needs you", || {
        states().iter().any(|(_, d)| {
            d["activities"][0]["state"]
                .as_str()
                .is_some_and(|s| s.starts_with("🔴 1 needs you"))
        })
    })
    .await;
    r.until("the card", || cards(&r, ANA_DM).len() == 1).await;
    let card = cards(&r, ANA_DM).remove(0);
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the status cleared when nothing works or waits", || {
        states()
            .last()
            .is_some_and(|(_, d)| d["activities"].as_array().is_some_and(Vec::is_empty))
    })
    .await;
    // Counts only, and never two inside the gap.
    let all = states();
    for (_, d) in &all {
        if let Some(s) = d["activities"][0]["state"].as_str() {
            assert!(
                s.chars()
                    .all(|c| !c.is_alphabetic() || "needsyourunning".contains(c)),
                "counts only: {s}"
            );
        }
    }
    // Never two inside the gap: the stand-in hears them in the order and at the
    // times the sender made them, give or take the load; an unpaced sender
    // would send within a few milliseconds.
    for w in all.windows(2) {
        assert!(
            w[1].0 - w[0].0 >= 300,
            "two updates {} ms apart",
            w[1].0 - w[0].0
        );
    }
}

/// The ids of the options in the board's select.
fn picks(board: &Msg) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![board.components_json.clone()];
    while let Some(v) = stack.pop() {
        match v {
            Value::Array(a) => stack.extend(a),
            Value::Object(o) => {
                if o.get("custom_id").and_then(Value::as_str) == Some("tb:pick") {
                    for opt in o["options"].as_array().into_iter().flatten() {
                        out.push(opt["value"].as_str().unwrap_or("").to_string());
                    }
                }
                stack.extend(o.into_iter().map(|(_, v)| v));
            }
            _ => {}
        }
    }
    out
}

/// A task that asks and one that runs for a while, on a board that says so.
async fn one_asking_one_running() -> Rig {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start_all(&[REEF, BUOY]), Scripted::text("Started both.")],
                vec![(REEF, asks(dir, "reef.txt")), (BUOY, sleeps(30))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the board with a question and a running task", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("🔴 1 needs you · ⚙️ 1 running"))
    })
    .await;
    r
}

/// The asking task's id (the Answer button's) and the running one's (the other
/// option of the select).
fn ids_of(board: &Msg) -> (String, String) {
    let asking = board
        .buttons
        .iter()
        .find_map(|b| b.custom_id.strip_prefix(board_v2::OPEN).map(str::to_string))
        .unwrap();
    let running = picks(board).into_iter().find(|v| *v != asking).unwrap();
    (asking, running)
}

#[tokio::test]
async fn the_answer_button_and_the_select_open_a_task_for_the_presser_alone() {
    let r = one_asking_one_running().await;
    let board = boards(&r, ANA_DM).remove(0);
    // Answer ↗ opens the asking task: ephemeral, with the question and its answers.
    let iid = r.press(&board.id, "Answer ↗", (ANA, "ana"));
    r.until("the task's view", || r.fake.reply_message(&iid).is_some())
        .await;
    let view = r.fake.reply_message(&iid).unwrap();
    assert!(
        view.flags & 64 != 0 && view.flags & IS_COMPONENTS_V2 != 0,
        "ephemeral V2: {}",
        view.flags
    );
    assert!(
        view.content.contains("Reef-chart: chart the reef"),
        "{}",
        view.content
    );
    assert!(view.content.contains("**Asks**"), "{}", view.content);
    let ids: Vec<&str> = view.buttons.iter().map(|b| b.custom_id.as_str()).collect();
    assert!(
        ids.iter().any(|i| i.ends_with(":approve"))
            && ids.iter().any(|i| i.starts_with("tb:stop:")),
        "{ids:?}"
    );
    // The view is the presser's alone: nothing was posted in the channel for it.
    assert!(r
        .posted(ANA_DM)
        .iter()
        .all(|m| !m.content.contains("**Asks**")));
    // The select opens the running one, which has Stop and no question.
    let (_, running) = ids_of(&board);
    let pick = r
        .fake
        .press_values(
            &theseus_sim::fake_discord::Pressed {
                message: &board.id,
                button: "tb:pick",
                user: ANA,
                name: "ana",
            },
            &[&running],
        )
        .unwrap();
    r.until("the running task's view", || {
        r.fake.reply_message(&pick).is_some()
    })
    .await;
    let view2 = r.fake.reply_message(&pick).unwrap();
    assert!(
        view2.content.contains("Buoy-paint: paint the buoy"),
        "{}",
        view2.content
    );
    assert!(!view2.content.contains("**Asks**"));
    // The question is still waiting: answer it from the view.
    let card_id = cards(&r, ANA_DM)[0].id.clone();
    r.fake.press_reply(&iid, "Approve", ANA, "ana").unwrap();
    r.until("the card settled by an answer pressed in the view", || {
        r.fake
            .messages(ANA_DM)
            .into_iter()
            .find(|m| m.id == card_id)
            .is_some_and(|c| c.buttons.is_empty() && c.content.contains("Approved"))
    })
    .await;
}

#[tokio::test]
async fn stop_asks_twice_and_the_second_press_stops_the_task() {
    let r = one_asking_one_running().await;
    let board = boards(&r, ANA_DM).remove(0);
    let (_, running) = ids_of(&board);
    let pick = r
        .fake
        .press_values(
            &theseus_sim::fake_discord::Pressed {
                message: &board.id,
                button: "tb:pick",
                user: ANA,
                name: "ana",
            },
            &[&running],
        )
        .unwrap();
    r.until("the running task's view", || {
        r.fake.reply_message(&pick).is_some()
    })
    .await;
    // Stop asks once more...
    let stop = r.fake.press_reply(&pick, "Stop", ANA, "ana").unwrap();
    r.until("the second question", || {
        r.fake
            .reply_message(&stop)
            .is_some_and(|m| m.content.contains("Stop **Buoy-paint"))
    })
    .await;
    let state = |exe: &str| r.core.kernel.execution(exe).unwrap().unwrap().state;
    assert_ne!(
        state(&running).as_str(),
        "cancelled",
        "one press stops nothing"
    );
    // ...and the second press stops it.
    let yes = r
        .fake
        .press_reply(&stop, "Yes, stop it", ANA, "ana")
        .unwrap();
    r.until("the task cancelled", || {
        state(&running).as_str() == "cancelled"
    })
    .await;
    r.until("the answer to the second press", || {
        r.fake
            .reply_message(&yes)
            .is_some_and(|m| m.content.contains("stopped"))
    })
    .await;
}

#[tokio::test]
async fn clear_done_drops_the_done_tasks_from_the_board() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start_all(&[BUOY]), Scripted::text("Started it.")],
                vec![(BUOY, sleeps(1))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the task done and not yet seen", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("✅ 1 done since you looked"))
    })
    .await;
    let board = boards(&r, ANA_DM).remove(0);
    r.press(&board.id, "✓ Clear done", (ANA, "ana"));
    r.until("the board says all clear", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("all clear") && !b.content.contains("Buoy-paint"))
    })
    .await;
}

#[tokio::test]
async fn quiet_one_hour_makes_the_places_cards_silent_until_it_is_ended() {
    const ASK2: &str = "START2: now run this other thing";
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![
                    start_all(&[BUOY]),
                    Scripted::text("Started it."),
                    start_quoting(&[REEF], "run this other thing"),
                    Scripted::text("Started the other."),
                ],
                vec![(BUOY, sleeps(30)), (REEF, asks(dir, "reef.txt"))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("a running task on the board", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("⚙️ 1 running"))
    })
    .await;
    let board = boards(&r, ANA_DM).remove(0);
    r.press(&board.id, "🔕 Quiet 1 h", (ANA, "ana"));
    r.until("the board says it is quiet", || {
        boards(&r, ANA_DM).first().is_some_and(|b| {
            b.content.contains("🔕 quiet until")
                && b.buttons.iter().any(|x| x.label == "🔔 Unquiet")
        })
    })
    .await;
    // A question that comes now is a card, silent.
    r.say((ANA, "ana"), None, ASK2);
    r.until("the card", || cards(&r, ANA_DM).len() == 1).await;
    assert!(
        cards(&r, ANA_DM)[0].silent(),
        "quiet: the card does not buzz"
    );
    // Unquiet: the board says so.
    r.press(&board.id, "🔔 Unquiet", (ANA, "ana"));
    r.until("the board is loud again", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| !b.content.contains("🔕 quiet until"))
    })
    .await;
}
