//! `WakeInfo` on the wire (theseus-lherl): it moved out of lib.rs into
//! wakes.rs and gained `days`, `until_ms` and `title`. The bytes of a wake
//! without them are what they always were, and the new fields ride after the
//! old ones when they are set.

use theseus_protocol::WakeInfo;

/// A one-shot wake as a daemon from before the move wrote it.
const ONE_SHOT: &str = r#"{"wake_id":"wak_0199aaaa3f9a1c","short":"3f9a1c","session_id":"ses_1","execution_id":"exe_1","due_at_ms":1540000,"due_local":"2026-09-30 13:15:00 -07:00","note":"check the build\nand the tests","set_at_ms":1000000,"target":"discord:dm:42","state":"waiting"}"#;

/// A series as the same daemon wrote it: its span, occurrence and next.
const SERIES: &str = r#"{"wake_id":"wak_0199aaaa3f9a1c","short":"3f9a1c","session_id":"ses_1","execution_id":"exe_1","task":"a1b2c3","due_at_ms":1540000,"due_local":"2026-09-30 13:15:00 -07:00","note":"evening haiku","set_at_ms":1000000,"state":"waiting","every":"1d","occurrence":4,"next":"21:00 Thu"}"#;

/// And one with the new fields.
const WITH_NEW: &str = r#"{"wake_id":"wak_0199aaaa3f9a1c","short":"3f9a1c","session_id":"ses_1","execution_id":"exe_1","due_at_ms":1540000,"due_local":"2026-09-30 13:15:00 -07:00","note":"evening haiku\nand post it","set_at_ms":1000000,"state":"waiting","every":"1d","occurrence":4,"next":"21:00 Thu","days":["mon","fri"],"until_ms":1793408400000,"title":"evening haiku"}"#;

#[test]
fn a_wake_without_the_new_fields_keeps_its_bytes() {
    for bytes in [ONE_SHOT, SERIES] {
        let w: WakeInfo = serde_json::from_str(bytes).unwrap();
        assert!(w.days.is_empty() && w.until_ms.is_none() && w.title.is_none());
        assert_eq!(serde_json::to_string(&w).unwrap(), bytes);
    }
}

#[test]
fn the_new_fields_ride_after_the_old_and_read_back() {
    let w: WakeInfo = serde_json::from_str(WITH_NEW).unwrap();
    assert_eq!(w.days, ["mon", "fri"]);
    assert_eq!(w.until_ms, Some(1_793_408_400_000));
    assert_eq!(w.title.as_deref(), Some("evening haiku"));
    assert_eq!(serde_json::to_string(&w).unwrap(), WITH_NEW);
}
