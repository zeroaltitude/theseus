//! The given kinds at a bind (a guild channel's guild, the channel's parent),
//! through a whole core (onto-core, theseus-e3mjc).

use std::sync::Arc;

use theseus_ontology::{CategoryId, Origin};

use crate::places::BoundPlace;
use crate::provider::{FakeProvider, Scripted};
use crate::store::Store;
use crate::{Config, Core};

const GUILD: &str = "g-1";
const LAB: u64 = 314_159_265_358_979_323;

fn core() -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")]));
    (
        Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap(),
        dir,
    )
}

fn lab(guild: Option<&str>) -> BoundPlace {
    BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        guild: guild.map(str::to_string),
        ..Default::default()
    }
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn channel() -> CategoryId {
    id(&format!("channel:{LAB}"))
}

fn frames_in(f: impl FnOnce()) -> u64 {
    let before = theseus_store::frames_written_here();
    f();
    theseus_store::frames_written_here() - before
}

#[test]
fn a_guild_channels_bind_makes_its_guild_and_gives_the_channel_its_parent() {
    let (core, _dir) = core();
    let frames = frames_in(|| core.bind_places(vec![lab(Some(GUILD))]));
    assert_eq!(frames, 1, "the guild and the channel are one frame");
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let guild = o.category(&id("guild:g-1")).unwrap();
    assert_eq!((guild.name.as_str(), guild.parent.as_ref()), ("g-1", None));
    assert_eq!(
        o.category(&channel()).unwrap().parent,
        Some(id("guild:g-1"))
    );
    // The session's given memberships read the guild through the parent.
    let place = format!("discord:channel:{LAB}");
    let given = crate::ontology::given(Some(&place), &o, 7);
    let cats: Vec<&str> = given.iter().map(|m| m.category.as_str()).collect();
    assert_eq!(cats, ["guild:g-1".to_string(), channel().to_string()]);
    assert!(given.iter().all(|m| m.origin == Origin::Transport));
    // Nothing of it is stored per session.
    assert!(o.memberships("any").is_empty());
    // A second identical bind writes no frame.
    assert_eq!(frames_in(|| core.bind_places(vec![lab(Some(GUILD))])), 0);
}

#[test]
fn a_dm_bind_is_as_it_was() {
    let (core, _dir) = core();
    core.bind_places(vec![BoundPlace {
        target: "discord:dm:271828".into(),
        name: "DM @wren".into(),
        ..Default::default()
    }]);
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let wren = o.category(&id("person:271828")).unwrap();
    assert_eq!((wren.name.as_str(), wren.parent.as_ref()), ("@wren", None));
    assert_eq!(o.categories().count(), 1);
}

#[test]
fn a_store_from_before_gets_the_channels_parent_at_its_next_bind() {
    let (core, _dir) = core();
    // As the build before wrote it: the channel, with no parent and no guild.
    core.bind_places(vec![lab(None)]);
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    assert_eq!(o.category(&channel()).unwrap().parent, None);
    assert!(o.category(&id("guild:g-1")).is_none());
    let frames = frames_in(|| core.bind_places(vec![lab(Some(GUILD))]));
    assert_eq!(frames, 1);
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    assert_eq!(
        o.category(&channel()).unwrap().parent,
        Some(id("guild:g-1"))
    );
    assert_eq!(frames_in(|| core.bind_places(vec![lab(Some(GUILD))])), 0);
}
