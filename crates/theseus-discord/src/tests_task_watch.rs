//! A place's new session is on the binding's work watch before its first turn runs (theseus-hzdlf). The binding
//! learns a task only from the watch: its snapshot carries no ended task, so one that was born, ran and ended
//! before the watch began is never heard of, and its place gets no thread or post. The watch used to begin a
//! debounce (100 ms) after the session was opened, which a task answered at once beat.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use theseus_core::provider::{DeltaSink, Provider, ProviderFuture, ProviderRequest, Scripted};
use theseus_core::Core;

use crate::tests_gateway::{Rig, ANA, LAB};
use crate::tests_task_threads::{bindings, guild, model, replied, start, tweak, until, ASK, REEF};

/// A model that notes, at each call, whether the work board is watched for the lab place's session.
struct Probe {
    inner: Arc<dyn Provider>,
    // Weak: the core holds this model, and a strong cycle would keep it (and the rig's drop) alive.
    core: Arc<OnceLock<Weak<Core>>>,
    watching: Arc<Mutex<Vec<bool>>>,
}

impl Provider for Probe {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        // The place's own session, not any watcher: another connection's watch would pass for it.
        let watched = self.core.get().and_then(Weak::upgrade).is_some_and(|c| {
            c.outbox
                .place_session(&format!("channel:{LAB}"))
                .ok()
                .flatten()
                .is_some_and(|s| c.bus.work_watched(&s))
        });
        self.watching.lock().unwrap().push(watched);
        self.inner.stream_message(req, on_delta)
    }
}

/// The first turn of a place that has no session yet: by the time its model is asked, the binding
/// already watches the work board, so every state the turn's task goes through reaches it.
#[tokio::test]
async fn a_places_new_session_is_watched_before_its_first_turn_runs() {
    let core = Arc::new(OnceLock::new());
    let watching = Arc::new(Mutex::new(Vec::new()));
    let r = Rig::start_tweaked(
        |_, _| {
            Arc::new(Probe {
                inner: model(
                    vec![start(&[REEF]), Scripted::text("Started.")],
                    vec![(REEF, vec![Scripted::text("Reef charted.")])],
                ),
                core: core.clone(),
                watching: watching.clone(),
            })
        },
        guild(),
        &bindings(true, ""),
        &[],
        tweak,
    )
    .await;
    assert!(core.set(Arc::downgrade(&r.core)).is_ok());
    r.say((ANA, "ana"), Some(LAB), ASK);
    // The turn runs to its end, so the rig is not dropped with a task in flight.
    replied(&r, LAB, "Started.").await;
    until(&r, "the first turn's model call", || {
        !watching.lock().unwrap().is_empty()
    })
    .await;
    let first = watching.lock().unwrap()[0];
    assert!(
        first,
        "the first model call found its session unwatched: {:?}",
        watching.lock().unwrap()
    );
}
