//! Each background site keeps the one rule (route.v10 row 4, theseus-ej6bj;
//! design §2 rows 6 to 9 and 13): a configured profile key is a pin, an unset
//! key asks Jev, and a job never waits on Jev past `max_wait_ms`. The rule's
//! own cases are `tests_job_ask`'s; these prove each site is wired to it.
//!
//! For every site that takes a profile key, five behaviours:
//!
//! - **pinned**: the key set, Jev is asked nothing, the job runs on the pin;
//! - **asks**: the key unset, Jev is asked once (through the job builder) and
//!   the job runs on the model it names;
//! - **slow**: Jev holds its answer, and the job runs on `live` at about
//!   `max_wait_ms`, not when Jev answers;
//! - **legacy**: while the acting pack asks no `model`, an unset key runs on
//!   the profile it always did, and Jev is asked nothing;
//! - **the data rule**: the owner's material is told `not_allowed`, and a pick
//!   of Flash runs on `live` all the same.

use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use theseus_judge::fake::{FakeJev, FakeMode};

use crate::tests_job_ask::{pick, rig_job};
use crate::tests_route::{rig_parts, Rig};
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
    /// A key for the pin, and a profile for it that is not the legacy one.
    pin: (&'static str, &'static str),
    /// The option Jev names, and the model it is.
    pick: (&'static str, &'static str),
    /// The model an unset key ran on before Jev chose.
    legacy: &'static str,
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
    assert!(
        state.contains("not_allowed"),
        "the owner's material: {state}"
    );
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

async fn legacy(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, s.pick.0, 0.95);
    let r = rig_parts(
        Some(&jev),
        1,
        |c| {
            (s.unset)(c);
            c.routing.max_wait_ms = 300;
        },
        |_| {},
    );
    assert_eq!((s.call)(&r).await, s.legacy);
    assert_eq!(
        jev.connections(),
        0,
        "no pack asks `model`: nothing is asked"
    );
}

async fn china(s: &Site) {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, s.unset);
    assert_eq!((s.call)(&r).await, SONNET, "refused Flash, so live");
    assert!(jev.seen()[0].body["state"]
        .to_string()
        .contains("not_allowed"));
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
            async fn runs_its_old_profile_while_the_pack_asks_no_model() {
                legacy(&$site).await;
            }
            #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
            async fn never_sends_the_owners_material_to_a_china_origin_model() {
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
        pick: ("fable", "claude-fable-5-1"),
        legacy: "claude-haiku-5-5",
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
        pick: ("fable", "claude-fable-5-1"),
        legacy: "claude-haiku-5-5",
        says: "12 lines",
    }
);

site!(
    threads,
    Site {
        set: |c, p| c.threads.import_profile = Some(p.into()),
        unset: |c| c.threads.import_profile = None,
        call: |r| async { r.core.import_model(3).await.unwrap().model }.boxed(),
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-1"),
        legacy: "claude-haiku-5-5",
        says: "3 episodes",
    }
);

site!(
    ontology,
    Site {
        set: |c, p| c.ontology.name_profile = Some(p.into()),
        unset: |c| c.ontology.name_profile = None,
        call: |r| async { r.core.onto_model("project").await.unwrap().0.model }.boxed(),
        pin: ("opus", "claude-opus-5-5"),
        pick: ("fable", "claude-fable-5-1"),
        legacy: "claude-haiku-5-5",
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
                tokio::task::spawn_blocking(move || core.writer_target(&rt, "classify.v4"))
                    .await
                    .unwrap()
                    .unwrap()
                    .model
            }
            .boxed()
        },
        pin: ("fable", "claude-fable-5-1"),
        pick: ("haiku", "claude-haiku-5-5"),
        legacy: "claude-opus-5-5",
        says: "classify.v4",
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
        pick: ("fable", "claude-fable-5-1"),
        // Unset was the live profile.
        legacy: SONNET,
        says: "the morning note",
    }
);

// Row 7: consolidation's syntheses. The plan decides at its own point (a
// named profile, or none when Jev will be asked), and the cluster's call
// settles it.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consolidation_is_pinned_when_synth_profile_is_set() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    let r = rig_job(&jev, |c| c.memory.synth_profile = Some("opus".into()));
    assert!(!r.core.synth_asks_jev(), "a set key never asks");
    let t = r.core.synth_target(&[], "opus").await.unwrap();
    assert_eq!(t.model, "claude-opus-5-5");
    assert_eq!(jev.connections(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consolidation_asks_jev_when_synth_profile_is_unset() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    let r = rig_job(&jev, |_| {});
    assert!(r.core.synth_asks_jev());
    let t = r.core.synth_target(&[], "").await.unwrap();
    assert_eq!(t.model, "claude-fable-5-1");
    assert_eq!(jev.connections(), 1);
    let state = jev.seen()[0].body["state"].to_string();
    assert!(state.contains("synthesis"), "{state}");
    assert!(
        state.contains("not_allowed"),
        "a synthesis reads the owner's material: {state}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consolidation_never_waits_on_a_slow_jev_past_the_bound() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    jev.set_mode(FakeMode::Held);
    let r = rig_job(&jev, |c| c.judge.total_secs = 30);
    let t0 = Instant::now();
    let t = r.core.synth_target(&[], "").await.unwrap();
    assert_eq!(t.model, SONNET, "live");
    assert_eq!(jev.seen().len(), 1, "Jev was asked, and held its answer");
    assert!(
        t0.elapsed() < Duration::from_millis(2_500),
        "{:?}",
        t0.elapsed()
    );
    jev.release();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consolidation_lists_only_us_options_and_refuses_flash() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let t = r.core.synth_target(&[], "").await.unwrap();
    assert_eq!(t.model, SONNET, "refused Flash, so live");
    assert!(jev.seen()[0].body["state"]
        .to_string()
        .contains("not_allowed"));
}

/// Before the pack that asks `model`, an unset key plans as it always did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consolidation_plans_as_it_did_while_the_pack_asks_no_model() {
    let jev = FakeJev::start().unwrap();
    let r = rig_parts(Some(&jev), 1, |c| c.routing.max_wait_ms = 300, |_| {});
    assert!(!r.core.synth_asks_jev());
    assert_eq!(jev.connections(), 0);
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
    assert_eq!(t.model, "claude-fable-5-1");
    assert_eq!(jev.connections(), 1);
    let state = jev.seen()[0].body["state"].to_string();
    assert!(
        state.contains("job") && state.contains("not_allowed"),
        "{state}"
    );
}

/// A motif drawn from private sessions lists US options only, and a pick of
/// Flash runs the writer on the session's own profile; a bench-born motif,
/// which is nobody's material, may be written on Flash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_builders_options_follow_the_motifs_origin() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "deepseek_flash", 0.97);
    let r = rig_job(&jev, |_| {});
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(true))
        .await
        .unwrap();
    assert_eq!(t.model, SONNET, "private: refused Flash, so live");
    assert!(jev.seen()[0].body["state"]
        .to_string()
        .contains("not_allowed"));
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(false))
        .await
        .unwrap();
    assert_eq!(t.model, "deepseek-flash", "bench-born: nobody's material");
    assert!(jev.seen()[1].body["state"]
        .to_string()
        .contains("\"allowed\""));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_self_builder_runs_on_the_live_profile_while_the_pack_asks_no_model() {
    let jev = FakeJev::start().unwrap();
    pick(&jev, "fable", 0.95);
    let r = rig_parts(Some(&jev), 1, |c| c.routing.max_wait_ms = 300, |_| {});
    let t = r
        .core
        .mint_writer_target(&writer_session(&r), &pending(true))
        .await
        .unwrap();
    assert_eq!(t.model, SONNET);
    assert_eq!(jev.connections(), 0);
}
