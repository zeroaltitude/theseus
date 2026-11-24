//! Each background site keeps the one rule (route.v10 row 4, theseus-ej6bj;
//! design §2 rows 6 to 9 and 13): a configured profile key is a pin, an unset
//! key asks Jev, and a job never waits on Jev past `max_wait_ms`. The rule's
//! own cases are `tests_job_ask`'s; these prove each site is wired to it.
//!
//! For every site that takes a profile key, four behaviours:
//!
//! - **pinned**: the key set, Jev is asked nothing, the job runs on the pin;
//! - **asks**: the key unset, Jev is asked once (through the job builder) and
//!   the job runs on the model it names;
//! - **slow**: Jev holds its answer, and the job runs on `live` at about
//!   `max_wait_ms`, not when Jev answers;
//! - **no origin rule**: nothing in the state the judge is shown restricts a
//!   model's origin (no `restricted_models` since issue 46), and a pick of Flash
//!   runs on Flash (the restricted-origin rule went on 2026-11-21).

use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use theseus_judge::fake::{FakeJev, FakeMode};

use crate::tests_job_ask::{pick, rig_job};
use crate::tests_route::Rig;
use crate::Config;

const SONNET: &str = "claude-sonnet-5-5";

/// One site's parameters.
struct Site {
    /// Sets the site's key to a profile.
    set: fn(&mut Config, &str),
    /// Leaves the site's key unset (the template may set one).
    unset: fn(&mut Config),
    /// Runs the site's selection, and says the model it settled on.
    call: for<'a> fn(&'a Rig) -> BoxFuture<'a, String>,
    /// A key for the pin, and a profile for it.
    pin: (&'static str, &'static str),
    /// The option Jev names, and the model it is.
    pick: (&'static str, &'static str),
    /// What the job's paragraph says, so the ask is the site's own.
    says: &'static str,
}

async fn pinned(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, s.pick.0, 0.95);
    let r = rig_job(&jev, |c| (s.set)(c, s.pin.0));
    assert_eq!((s.call)(&r).await, s.pin.1);
    assert_eq!(jev.connections(), 0, "a pin asks nothing");
}

/// Nothing in the state restricts a model's origin: no `restricted_models` and no
/// "not allowed" (issue 46 stopped writing the fact, which always said `allowed`).
fn no_floor(state: &str) {
    assert!(
        !state.contains("restricted_models") && !state.contains("not_allowed"),
        "no floor for the owner's material: {state}"
    );
}

async fn asks(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, s.pick.0, 0.95);
    let r = rig_job(&jev, s.unset);
    assert_eq!((s.call)(&r).await, s.pick.1);
    assert_eq!(jev.connections(), 1, "one ask");
    let state = jev.seen()[0].body["state"].to_string();
    assert!(state.contains("job"), "the place: {state}");
    assert!(
        state.contains(s.says),
        "the paragraph says `{}`: {state}",
        s.says
    );
    no_floor(&state);
}

async fn slow(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, s.pick.0, 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig_job(&jev, |c| {
        (s.unset)(c);
        c.judge.total_secs = 30;
    });
    let t0 = Instant::now();
    assert_eq!((s.call)(&r).await, SONNET, "live");
    let waited = t0.elapsed();
    assert_eq!(jev.seen().len(), 1, "Jev was asked, and held its answer");
    assert!(waited < Duration::from_millis(2_500), "{waited:?}");
    jev.release();
}

async fn china(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, s.unset);
    assert_eq!(
        (s.call)(&r).await,
        "deepseek-flash",
        "Flash is picked and runs"
    );
    no_floor(&jev.seen()[0].body["state"].to_string());
}

macro_rules! site {
    ($name:ident, $site:expr) => {
        mod $name {
            use super::*;
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn is_pinned_when_the_key_is_set() {
                pinned(&$site).await;
            }
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn asks_jev_when_the_key_is_unset() {
                asks(&$site).await;
            }
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn never_waits_on_a_slow_jev_past_the_bound() {
                slow(&$site).await;
            }
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn may_send_the_owners_material_to_a_china_origin_model() {
                china(&$site).await;
            }
        }
    };
}

// Row 13: the owner-material sites.

site!(
    card,
    Site {
        set: |c, p| c.card.profile = Some(p.into()),
        unset: |c| c.card.profile = None,
        call: |r| async { r.core.card_model(5).await.unwrap().0.model }.boxed(),
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-2"),
        says: "5 clusters",
    }
);

site!(
    people,
    Site {
        set: |c, p| c.people.extract_profile = Some(p.into()),
        unset: |c| c.people.extract_profile = None,
        call: |r| async { r.core.people_extractor(12).await.unwrap().0.model }.boxed(),
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-2"),
        says: "12 lines",
    }
);

site!(
    ontology,
    Site {
        set: |c, p| c.ontology.name_profile = Some(p.into()),
        unset: |c| c.ontology.name_profile = None,
        call: |r| async { r.core.onto_model("project").await.unwrap().0.model }.boxed(),
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-2"),
        says: "project",
    }
);

site!(
    learn_writer,
    Site {
        set: |c, p| c.judge.learn.writer_profile = Some(p.into()),
        unset: |c| c.judge.learn.writer_profile = None,
        call: |r| {
            async {
                let core = r.core.clone();
                let rt = tokio::runtime::Handle::current();
                tokio::task::spawn_blocking(move || core.writer_target(&rt, "classify.v5"))
                    .await
                    .unwrap()
                    .unwrap()
                    .model
            }
            .boxed()
        },
        pin: ("fable", "claude-fable-5-2"),
        pick: ("haiku", "claude-haiku-5-5"),
        says: "classify.v5",
    }
);

site!(
    initiative,
    Site {
        set: |c, p| c.initiative.profile = Some(p.into()),
        unset: |c| c.initiative.profile = None,
        call: |r| {
            async {
                r.core
                    .initiative_target("the morning note")
                    .await
                    .unwrap()
                    .model
            }
            .boxed()
        },
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-2"),
        // Unset was the live profile.
        says: "the morning note",
    }
);

// The card's observation call is its own job (lines the owner wrote), under
// the same key as the proposals' call.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_card_observation_asks_about_lines_not_clusters() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    let r = rig_job(&jev, |c| c.card.profile = None);
    let (t, _, _) = r.core.observe_model(11).await.unwrap();
    assert_eq!(t.model, "claude-fable-5-2");
    let state = jev.seen()[0].body["state"].to_string();
    assert!(state.contains("11 lines"), "{state}");
    assert!(!state.contains("clusters"), "{state}");
}

// Row 8: the self-improvement builder. No key pins it: Jev chooses from the
// motif's brief, and the motif's origin says which models are options.

fn pending(private: bool) -> crate::rsi::mint::run::Pending {
    let mut motif = crate::rsi::mint::tests::land();
    motif.bench_born = !private;
    crate::rsi::mint::run::Pending {
        placeholder: "mint-test".into(),
        kind: crate::rsi::mint::kind::Kind::Flow,
        item: "item".into(),
        motif,
    }
}

fn writer_session(r: &Rig) -> crate::session::SessionRecord {
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    rec
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_builder_asks_jev_and_runs_on_its_pick() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    let r = rig_job(&jev, |_| {});
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(true))
        .await
        .unwrap();
    assert_eq!(t.model, "claude-fable-5-2");
    assert_eq!(jev.connections(), 1);
    let state = jev.seen()[0].body["state"].to_string();
    assert!(state.contains("job"), "{state}");
    no_floor(&state);
}

/// A motif drawn from private sessions and a bench-born one may both be
/// written on Flash: no option is kept off by origin (2026-11-21).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_builder_may_pick_flash_for_any_motif() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(true))
        .await
        .unwrap();
    assert_eq!(t.model, "deepseek-flash", "private: Flash runs");
    no_floor(&jev.seen()[0].body["state"].to_string());
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(false))
        .await
        .unwrap();
    assert_eq!(t.model, "deepseek-flash", "bench-born: nobody's material");
    no_floor(&jev.seen()[1].body["state"].to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_builder_never_waits_on_a_slow_jev_past_the_bound() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig_job(&jev, |c| c.judge.total_secs = 30);
    let t0 = Instant::now();
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(true))
        .await
        .unwrap();
    assert_eq!(t.model, SONNET, "the session's own profile, which is live");
    assert_eq!(jev.seen().len(), 1, "Jev was asked, and held its answer");
    assert!(
        t0.elapsed() < Duration::from_millis(2_500),
        "{:?}",
        t0.elapsed()
    );
    jev.release();
}
