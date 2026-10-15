//! A reminder that fired, on the wire (theseus-8ux6x): the `wake.noticed`
//! notification, and the notice a surface makes of it, with the reminder's
//! four answers in order. The bytes are the goldens.

use theseus_protocol::notices::{reminder_notice, Urgency};
use theseus_protocol::{reminder_answers, WakeNoticed};

const NOTICED: &str = r#"{"wake_id":"wak_0199aaaa3f9a1c","short":"3f9a1c","session_id":"ses_1","execution_id":"exe_1","purpose":"remind","note":"stretch\nand drink water","due_at_ms":1790000000000,"late_ms":0,"posted_to":"discord:dm:42","home":true,"answers":[{"id":"done","label":"Done","method":"wake.done"},{"id":"snooze_10m","label":"Snooze 10 min","method":"wake.snooze","when":"10m"},{"id":"snooze_1h","label":"Snooze 1 h","method":"wake.snooze","when":"1h"},{"id":"tomorrow_9","label":"Tomorrow 9:00","method":"wake.snooze","when":"tomorrow 9:00"}]}"#;

const NOTICE: &str = r#"{"urgency":"interrupt","why":"woke","work":{"id":"wak_0199aaaa3f9a1c","kind":"wake","session_id":"ses_1","title":"stretch"},"root":{"id":"exe_1","kind":"conversation","session_id":"ses_1","title":""},"line":"⏰ stretch","answers":[{"id":"done","label":"Done","method":"wake.done"},{"id":"snooze_10m","label":"Snooze 10 min","method":"wake.snooze","when":"10m"},{"id":"snooze_1h","label":"Snooze 1 h","method":"wake.snooze","when":"1h"},{"id":"tomorrow_9","label":"Tomorrow 9:00","method":"wake.snooze","when":"tomorrow 9:00"}],"position":1790000000000,"at_ms":1790000000000}"#;

fn hm(ms: u64) -> String {
    format!("t{ms}")
}

#[test]
fn the_notification_keeps_its_bytes_and_carries_the_four_answers_in_order() {
    let n: WakeNoticed = serde_json::from_str(NOTICED).unwrap();
    assert_eq!(serde_json::to_string(&n).unwrap(), NOTICED);
    let labels: Vec<_> = n.answers.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Done", "Snooze 10 min", "Snooze 1 h", "Tomorrow 9:00"]
    );
    assert_eq!(n.answers, reminder_answers());
}

#[test]
fn a_reminders_notice_interrupts_with_its_four_answers() {
    let n: WakeNoticed = serde_json::from_str(NOTICED).unwrap();
    let notice = reminder_notice(&n, &hm);
    assert_eq!(notice.urgency, Urgency::Interrupt);
    assert_eq!(serde_json::to_string(&notice).unwrap(), NOTICE);
}

#[test]
fn a_late_reminder_says_so_and_a_check_in_only_informs_with_no_answers() {
    let mut n: WakeNoticed = serde_json::from_str(NOTICED).unwrap();
    n.late_ms = 125_000;
    n.while_down = true;
    let notice = reminder_notice(&n, &hm);
    assert_eq!(
        notice.line,
        "⏰ stretch · due t1790000000000, 2 min late, the daemon was down"
    );
    n.purpose = "check".into();
    let notice = reminder_notice(&n, &hm);
    assert_eq!(notice.urgency, Urgency::Inform);
    assert!(notice.answers.is_empty());
}

/// A project's milestone reminder reads as a milestone's (theseus-n5ui2): no
/// clock icon, an Interrupt with its four answers; the overdue one is an
/// Interrupt once with no sound.
#[test]
fn a_milestones_reminder_is_its_own_line_and_the_overdue_one_has_no_sound() {
    use theseus_protocol::notices::{deliver, Delivery, Viewer, Why};
    let mut n: WakeNoticed = serde_json::from_str(NOTICED).unwrap();
    n.purpose = "remind".into();
    n.note = "⚑ due today: Harbour survey, milestone 2: Chart the channel".into();
    let due = reminder_notice(&n, &hm);
    assert_eq!(
        due.line,
        "⚑ due today: Harbour survey, milestone 2: Chart the channel"
    );
    assert_eq!((due.urgency, due.why), (Urgency::Interrupt, Why::Woke));
    assert_eq!(due.answers.len(), 4);
    let sounds = Viewer {
        sound: true,
        ..Default::default()
    };
    assert_eq!(deliver(&due, &sounds), Delivery::Ping { sound: true });
    n.note = "⚑ overdue: Harbour survey, milestone 2: Chart the channel".into();
    let late = reminder_notice(&n, &hm);
    assert_eq!((late.urgency, late.why), (Urgency::Interrupt, Why::Overdue));
    assert_eq!(deliver(&late, &sounds), Delivery::Ping { sound: false });
    // A plain reminder keeps its clock.
    n.note = "stretch".into();
    assert_eq!(reminder_notice(&n, &hm).line, "⏰ stretch");
}
