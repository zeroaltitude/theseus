//! A gateway event that cannot be read is recorded by its kind alone (issue 29). The library's own text for
//! it is `event=<the whole raw JSON>`, and an interaction can carry a typed secret: the keep form's value.
//! A malformed `INTERACTION_CREATE` that carries a distinctive invented value in a modal submit's field is
//! sent through the fake gateway, and the value is searched for in the log at trace level, every
//! `discord.error` row and health's last error.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};

use super::{bindings, guild, Rig};

/// Invented, and never in any output.
const VALUE: &str = "INVENTED-GATEWAY-VALUE-3317";

#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

#[tokio::test]
async fn a_form_the_gateway_could_not_read_is_recorded_without_its_content() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    // A modal submit whose `type` is not a number: the event is known and cannot be read.
    let bad = json!({"id": "1000000000000000099", "application_id": "1000000000000000002",
        "type": "not-a-number", "token": "not-a-token", "version": 1,
        "data": {"custom_id": "kf:act_0000feed",
                 "components": [{"type": 18, "id": 1,
                                 "component": {"type": 4, "id": 2, "custom_id": "value", "value": VALUE}}]}});
    assert!(r
        .fake
        .gateway()
        .unwrap()
        .dispatch("INTERACTION_CREATE", bad));
    r.until("the binding to record the failed read", || {
        !r.ledger("discord.error").is_empty()
    })
    .await;
    let rows = r.ledger("discord.error");
    let kept = format!("{rows:?}");
    assert!(
        kept.contains("a gateway event could not be read (its content is not recorded)"),
        "{kept}"
    );
    assert!(!kept.contains(VALUE), "in a discord.error row: {kept}");
    let status = r.core.bindings.all();
    let health = format!("{:?}", status.first().map(|b| b.last_error.clone()));
    assert!(health.contains("could not be read"), "{health}");
    assert!(!health.contains(VALUE), "in health's last error: {health}");
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    assert!(log.contains("could not be read"), "the failure was logged");
    assert!(!log.contains(VALUE), "in the log at trace level");
    // Nothing else stored it either: no ledger row at all.
    r.core.store.flush_waiting();
    let all: Vec<(u64, theseus_core::ledger::LedgerRow)> =
        r.core.store.ledger_tail(100_000).unwrap();
    assert!(!format!("{all:?}").contains(VALUE), "in a ledger row");
    r.stop();
}
