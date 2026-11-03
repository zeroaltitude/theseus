//! The seven readers outside routing that matched route.v8's class names (theseus-h66v4.6; the
//! omnibus review's structural finding). route.v10's verdict carries a model name in the old `mode`
//! word, so each reader either reads the new fact or is dead on purpose, and every test here is
//! green only because it says which:
//!
//! 1. the injection tier (`turn/tier.rs::tier_of`) reads `work.v1`: `tier::tests`;
//! 2. the coding flag, the narrow read and the coding hint read `work = coding`: here;
//! 3. the tier's confidence line reads the work answer's own bar, not a class's: `tier::tests`;
//! 4. the fan-out cue reads `split.v1`: `tests_fan_out_cue`;
//! 5. the learning report's acting list reads the pack's own options: here;
//! 6. the correction's `To::Mode` is gone with the class table (row 6): no class word resolves, here.
//! 7. the task offer's `trivial`/`quick` exemption (`turn/reply_step.rs::offer_for`, theseus-h66v4.9):
//!    retired on purpose under route.v10: `tests_offer`. A task's brief and checkpoint ask `work.v1`
//!    too, or reader 1 and 2 would have no answer on a task turn: `tests_task_checkpoint`.

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::ProviderRequest;
use crate::tests_route::turn;
use crate::tests_route_open::says;
use crate::toolrun::session_dir::read_window::{self, NARROW_LINES};
use crate::turn::tier::CODING_HINT;

fn work(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "work",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

fn carries(req: &ProviderRequest) -> bool {
    req.messages.iter().any(|m| {
        m["content"]
            .as_array()
            .is_some_and(|b| b.iter().any(|b| b["text"] == CODING_HINT))
    })
}

/// Reader 2: a turn Jev labelled `coding` at the bar gets the hint in its tail and the narrow
/// read, and no cached prefix byte moves; a `conversation` turn gets neither, whatever model Jev
/// chose (the class names were read off the model pick, which spells none of them).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_coding_is_what_puts_the_coding_hint_in_the_tail() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    work(&jev, "conversation", 0.95);
    // The tiered rig lands each verdict before the compile that reads it (the hint is a compile's
    // tail block).
    let r = crate::tests_injection_tiers::tiered(&jev, 3, true, |_| {});
    let r = &r;
    let one = turn(
        &r.core,
        None,
        "Which moorings on the outer wall were checked?",
        None,
    )
    .await;
    assert_eq!(read_window::get(&one.session_id), 0, "an ordinary turn");
    work(&jev, "coding", 0.95);
    let two = turn(
        &r.core,
        Some(&one.session_id),
        "Rename the helper in the mooring crate.",
        None,
    )
    .await;
    assert_eq!(
        read_window::get(&two.session_id),
        NARROW_LINES,
        "a coding turn"
    );
    // Under the confirm bar the label is not acted on: no hint, no window.
    work(&jev, "coding", 0.40);
    let three = turn(&r.core, Some(&one.session_id), "And the chains?", None).await;
    assert_eq!(read_window::get(&three.session_id), 0, "an unsure label");
    let q = r.claude.requests();
    let (a, b, c) = (&q[0], &q[1], &q[2]);
    assert_eq!(
        (carries(a), carries(b), carries(c)),
        (false, true, false),
        "the hint on the first, second and third turns"
    );
    assert_eq!(
        serde_json::to_string(&a.system).unwrap(),
        serde_json::to_string(&b.system).unwrap(),
        "the system"
    );
    assert_eq!(a.tools, b.tools, "the tools");
    let last = b.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .cloned();
    assert_eq!(
        last.unwrap()["text"],
        Value::from(CODING_HINT),
        "in the tail"
    );
}

/// Reader 5: the learning report's acting list names, for each pack of the family, classes that
/// are real options of the pack's own deciding question, so a promotion counts what the pack says.
#[test]
fn the_acting_list_names_each_packs_own_options() {
    use crate::learning::report::ACTING;
    for (pack, question) in [
        ("route.v10", "model"),
        ("effort.v1", "effort"),
        ("work.v1", "work"),
        ("split.v1", "split"),
    ] {
        let p = theseus_judge::pack::by_name(pack).unwrap();
        let row = ACTING
            .iter()
            .find(|(id, q, _)| *id == p.id && *q == question)
            .unwrap_or_else(|| panic!("{pack} has no acting row"));
        let options: Vec<&str> = p
            .question(question)
            .unwrap()
            .options
            .iter()
            .map(|o| o.id.as_str())
            .collect();
        for class in row.2 {
            assert!(options.contains(class), "{pack}/{question}: {class}");
        }
        assert!(
            p.question(question).unwrap().decides,
            "{pack}: it is the deciding question"
        );
    }
    // route.v8's `mode` row is still there for route.v8, and route.v8 has no `model` question to
    // match the new row against: one pack id, one row per question the pack asks.
    let v8 = theseus_judge::pack::by_name("route.v8").unwrap();
    assert!(ACTING
        .iter()
        .any(|(id, q, _)| *id == "route" && *q == "mode"));
    assert!(v8.question("model").is_none());
}

/// Reader 6: the correction has no mode vocabulary: `To::Mode` and the names it was spelled with went
/// with the class table (row 6), so no class word resolves to anything. A profile, a persona and
/// `stronger` / `cheaper` are the corrections that remain.
#[test]
fn a_correction_names_no_mode() {
    let cfg = crate::Config::example();
    let book = crate::personas::Book::default();
    let names = crate::correction::names(&cfg, &book);
    for word in ["deep_coding", "quick", "sophisticated"] {
        assert!(crate::correction::to_of(word, &names).is_none(), "{word}");
    }
}
