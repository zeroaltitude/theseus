//! A person's typing, told to the core (theseus-tnky), through the stand-in's
//! gateway: the owner's typing in a place warms the provider's connection
//! once per idle spell, someone else's in a shared place warms nothing (the
//! core refuses what the binding tells it, and the binding does not even ask
//! for a person the place does not let drive it), and a channel that is no
//! place of ours is no one's business.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use theseus_core::provider::FakeProvider;
use theseus_sim::fake_discord::{Guild, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, BEN, CY, LAB};

/// `#lab` shared, where ana and cy may drive Theseus, and ana's DM. Ana and
/// ben are the rig's owners; cy is no owner.
fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\", \"{CY}\"]\nmention_only = false\nprivate = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

async fn rig() -> (Rig, Arc<FakeProvider>) {
    let model = Arc::new(FakeProvider::default());
    let kept = model.clone();
    let guild = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .channel(LAB, "lab");
    let r = Rig::start_on(move |_, _| model, guild, &bindings(), &[]).await;
    (r, kept)
}

async fn warmed(model: &FakeProvider, n: u64) -> bool {
    for _ in 0..200 {
        if model.warms.load(Ordering::SeqCst) >= n {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owners_typing_warms_once_and_anyone_elses_in_a_shared_place_warms_nothing() {
    let (r, model) = rig().await;
    // Ben may not drive #lab (not in its users): the binding drops his. A
    // channel of nobody's is no place of ours.
    r.fake.typing(BEN, Some(LAB)).unwrap();
    r.fake.typing(ANA, Some(LAB + 99)).unwrap();
    settle().await;
    assert_eq!(model.warms.load(Ordering::SeqCst), 0);
    assert_eq!(
        r.core.warmth.health().started + r.core.warmth.health().dropped,
        0
    );
    // Cy may drive #lab and is no owner: the binding tells the core, which
    // refuses, and nothing is warmed for them.
    r.fake.typing(CY, Some(LAB)).unwrap();
    settle().await;
    assert_eq!(
        model.warms.load(Ordering::SeqCst),
        0,
        "cy's typing warmed nothing"
    );
    assert_eq!(
        r.core.warmth.health().dropped,
        1,
        "the core was told, and refused"
    );
    // Ana, an owner, in the same shared channel: one warm-up, however often.
    for _ in 0..4 {
        r.fake.typing(ANA, Some(LAB)).unwrap();
    }
    assert!(
        warmed(&model, 1).await,
        "the owner's typing warmed the provider"
    );
    settle().await;
    assert_eq!(model.warms.load(Ordering::SeqCst), 1, "once per idle spell");
    // Her DM is its own place, told as well: both are fresh sessions with no
    // id yet, so the core holds them to one spell, and says it dropped it.
    r.fake.typing(ANA, None).unwrap();
    for _ in 0..200 {
        if r.core.warmth.health().dropped == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        r.core.warmth.health().dropped,
        2,
        "the DM's notice reached the core"
    );
    assert_eq!(
        model.warms.load(Ordering::SeqCst),
        1,
        "inside the spell: no second warm-up"
    );
    assert!(
        model.requests.lock().unwrap().is_empty(),
        "a warm-up is no model call"
    );
}
