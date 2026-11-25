//! The injection flag's notice under `notify` (issue 54), through the
//! stand-in's gateway: it refuses nothing, so it is an FYI of its own kind
//! (`content_flag`) that goes to the owner's notices place when one is bound,
//! with its Remove button, whose press there is judged as any press is (the
//! owner, from a private place). Under `refuse` the notice needs the owner and
//! stays in the DM, as before.

use std::sync::Arc;

use theseus_core::content::{Flag, InjectionMode, ItemKind, Label};
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::session::SessionRecord;
use theseus_judge::band::Band;
use theseus_sim::fake_discord::{FakeDiscord, Guild, Msg, DEFAULT_GUILD};

use super::{Rig, ANA, ANA_DM, BEN, LAB};

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_060;

fn model(_: &std::path::Path, _: Arc<FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")]))
}

/// `#lab`, the notices channel (private, bound `notify = true`, ana and ben
/// admitted) and ana's DM.
fn file() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\", \"{BEN}\"]\nmention_only = false\nprivate = true\nnotify = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// Ana is the owner; the flag freezes at `mode`.
async fn rig(mode: InjectionMode) -> Rig {
    let guild = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .private_channel(LAB, "lab", &[ANA])
        .private_channel(NOTICES, "notices", &[ANA, BEN]);
    Rig::start_tweaked(model, guild, &file(), &[], move |c| {
        c.places.owner = Some(vec![format!("discord:{ANA}")]);
        c.content_based_security.injection = mode;
    })
    .await
}

/// The DM's session, once the owner has spoken in it.
async fn dm_session(r: &Rig) -> String {
    r.say((ANA, "ana"), None, "hello");
    r.until_on(&[ANA_DM], "the reply", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Hello."))
    })
    .await;
    r.core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .expect("the DM has a session")
}

fn label() -> Label {
    Label {
        flag: Flag::Injection,
        node: Some("nod_e1".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://example.com/x".into(),
        confidence: 0.93,
        band: Band::Act,
        at_ms: 1,
        judgment: String::new(),
    }
}

/// The label on the session's record, then the core's one post about it.
fn raise(r: &Rig, sid: &str) -> bool {
    let mut rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    let mut flags = rec.data.take().map(|b| *b).unwrap_or_default();
    let raised = flags.raise(label());
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(sid, &rec).unwrap();
    raised && r.core.tell_label(sid, &label()).unwrap()
}

fn carrying(r: &Rig, sid: &str) -> bool {
    let rec: SessionRecord = r.core.store.get_session(sid).unwrap().unwrap();
    rec.data.is_some_and(|d| d.has(Flag::Injection))
}

/// The messages in `channel` with a Remove button.
fn notices(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| {
            m.buttons
                .iter()
                .any(|b| b.custom_id.starts_with("content:remove:"))
        })
        .collect()
}

fn told(r: &Rig, pressed: &str, start: &str) -> impl Fn() -> bool {
    let fake = r.fake.clone();
    let (pressed, start) = (pressed.to_string(), start.to_string());
    move || {
        fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content.as_deref().is_some_and(|c| c.starts_with(&start))
        })
    }
}

/// Under `notify` the notice is an FYI in the notices place and not in the
/// DM; a guest's Remove there is refused and the flag stays; the owner's
/// there removes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn under_notify_the_flag_is_an_fyi_in_the_notices_place_and_its_remove_counts_there() {
    let r = rig(InjectionMode::Notify).await;
    let sid = dm_session(&r).await;
    assert!(raise(&r, &sid));
    r.until("the notice in the notices place", || {
        !notices(&r, NOTICES).is_empty()
    })
    .await;
    let m = notices(&r, NOTICES).remove(0);
    let words = m.versions.last().unwrap();
    assert!(
        words.starts_with("ℹ️ FYI · Theseus flagged what it read as trying to steer it\n"),
        "{words}"
    );
    assert!(words.contains("Nothing is refused here"), "{words}");
    assert_eq!(m.buttons.len(), 1, "one button");
    assert_eq!(
        m.buttons[0].custom_id,
        format!("content:remove:{sid}:injection")
    );
    assert!(notices(&r, ANA_DM).is_empty(), "not also in the DM");

    let p = r.press(&m.id, "Remove", (BEN, "ben"));
    r.until(
        "ben is refused",
        told(&r, &p, "🔐 Your removal did not count"),
    )
    .await;
    assert!(carrying(&r, &sid));
    r.press(&m.id, "Remove", (ANA, "ana"));
    r.until("the label is removed", || !carrying(&r, &sid))
        .await;
    r.stop();
}

/// Under `refuse` the notice needs the owner: it stays in the DM, in the
/// freeze's words, and nothing goes to the notices place.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn under_refuse_the_flag_stays_in_the_dm() {
    let r = rig(InjectionMode::Refuse).await;
    let sid = dm_session(&r).await;
    assert!(raise(&r, &sid));
    r.until("the notice in the DM", || !notices(&r, ANA_DM).is_empty())
        .await;
    let m = notices(&r, ANA_DM).remove(0);
    assert!(
        m.versions
            .last()
            .unwrap()
            .starts_with("⚑ **injection** — this session is holding"),
        "{:?}",
        m.versions
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        notices(&r, NOTICES).is_empty(),
        "nothing in the notices place"
    );
    r.stop();
}
