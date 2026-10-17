//! The bot's own animated emojis for task states (theseus-ss36w): a spinning
//! "working", a pulsing "needs you", a check, a cross, a moon and a gear,
//! made by `assets/emoji/make.py` and uploaded to the application once.
//! A child of `runtime`.
//!
//! - **One function a site.** Every place the renderers draw a state asks
//!   `state` (or `job`) for its glyph: the application's emoji when it is
//!   known, else the Unicode one it always drew (`⚙️ 🔴 ✅ ❌ 💤 🔧`; a
//!   cancelled task's `⛔` and an expired one's `⌛` have no art and stay
//!   Unicode). The table is the process's, so a render stays pure and a test
//!   that never syncs renders what it always did.
//! - **Uploaded after READY, off every path** (`on_ready`): the application's
//!   emojis are listed, the ones this build wants that are missing are added
//!   (so a deleted one is added again), and the table is what the list says.
//!   The ids are kept in META `discord.app_emojis` and read back at the
//!   next READY, so the glyphs are the animated ones from the first board
//!   while the list is on its way; a list that fails clears the table, and
//!   the Unicode ones draw until a READY that works. `[discord] app_emojis =
//!   false` never starts it.
//! - **Labels stay Unicode.** A select option's label and a button's label are
//!   plain text: Discord draws no custom emoji in them, so those sites keep
//!   the Unicode glyphs.

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use theseus_protocol::work::WorkState;
use twilight_model::id::marker::ApplicationMarker;
use twilight_model::id::Id;

use super::Shared;

/// A thing the emojis draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Working,
    NeedsYou,
    Done,
    Failed,
    Sleeping,
    Job,
}

/// One emoji: its name on Discord, its image, and what draws without it.
struct Art {
    glyph: Glyph,
    name: &'static str,
    mime: &'static str,
    image: &'static [u8],
    animated: bool,
    fallback: &'static str,
}

const ART: [Art; 6] = [
    Art {
        glyph: Glyph::Working,
        name: "theseus_working",
        mime: "image/gif",
        image: include_bytes!("../../assets/emoji/theseus_working.gif"),
        animated: true,
        fallback: "⚙️",
    },
    Art {
        glyph: Glyph::NeedsYou,
        name: "theseus_needs_you",
        mime: "image/gif",
        image: include_bytes!("../../assets/emoji/theseus_needs_you.gif"),
        animated: true,
        fallback: "🔴",
    },
    Art {
        glyph: Glyph::Done,
        name: "theseus_done",
        mime: "image/png",
        image: include_bytes!("../../assets/emoji/theseus_done.png"),
        animated: false,
        fallback: "✅",
    },
    Art {
        glyph: Glyph::Failed,
        name: "theseus_failed",
        mime: "image/png",
        image: include_bytes!("../../assets/emoji/theseus_failed.png"),
        animated: false,
        fallback: "❌",
    },
    Art {
        glyph: Glyph::Sleeping,
        name: "theseus_sleeping",
        mime: "image/png",
        image: include_bytes!("../../assets/emoji/theseus_sleeping.png"),
        animated: false,
        fallback: "💤",
    },
    Art {
        glyph: Glyph::Job,
        name: "theseus_job",
        mime: "image/png",
        image: include_bytes!("../../assets/emoji/theseus_job.png"),
        animated: false,
        fallback: "🔧",
    },
];

/// Discord's limit on an emoji's image.
pub(crate) const MAX_BYTES: usize = 256 * 1024;

/// The META key of the ids this application's emojis have.
pub(crate) const META: &str = "discord.app_emojis";

/// An emoji as the application has it, as kept in META.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Known {
    name: String,
    id: u64,
    animated: bool,
}

impl Known {
    /// How a message writes it.
    fn markup(&self) -> String {
        format!(
            "<{}:{}:{}>",
            if self.animated { "a" } else { "" },
            self.name,
            self.id
        )
    }
}

static TABLE: RwLock<Vec<Known>> = RwLock::new(Vec::new());

fn glyph(g: Glyph) -> String {
    let art = ART
        .iter()
        .find(|a| a.glyph == g)
        .expect("every glyph has art");
    TABLE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|k| k.name == art.name)
        .map_or_else(|| art.fallback.to_string(), Known::markup)
}

/// The glyph a work state draws: the application's emoji, else the Unicode one.
pub(crate) fn state(s: WorkState) -> String {
    match s {
        WorkState::NeedsYou => glyph(Glyph::NeedsYou),
        WorkState::Done => glyph(Glyph::Done),
        WorkState::Failed => glyph(Glyph::Failed),
        WorkState::Ready => glyph(Glyph::Sleeping),
        WorkState::Cancelled => "⛔".into(),
        WorkState::Expired => "⌛".into(),
        _ => glyph(Glyph::Working),
    }
}

/// The glyph of a job.
pub(crate) fn job() -> String {
    glyph(Glyph::Job)
}

/// The table as the application's list and this build's art make it: each
/// emoji of ours the list has, by name.
fn table_of(list: &[Known]) -> Vec<Known> {
    ART.iter()
        .filter_map(|a| list.iter().find(|k| k.name == a.name).cloned())
        .collect()
}

fn set(table: Vec<Known>) {
    *TABLE.write().unwrap_or_else(|e| e.into_inner()) = table;
}

#[cfg(test)]
pub(crate) fn reset() {
    set(Vec::new());
}

/// Standard base64, for the image's data URI.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The gateway is ready: sync the application's emojis, off this path.
pub(super) fn on_ready(shared: &Arc<Shared>, app: Id<ApplicationMarker>) {
    if !shared.core.cfg.discord.app_emojis {
        return;
    }
    // The ids kept last time draw until the list answers.
    if let Ok(Some(kept)) = shared.core.store.get_meta::<Vec<Known>>(META) {
        set(table_of(&kept));
    }
    let shared = shared.clone();
    tokio::spawn(async move { sync(&shared, app).await });
}

/// List the application's emojis, add the ones of ours it lacks, and keep
/// the table and META as the application has them.
async fn sync(shared: &Shared, app: Id<ApplicationMarker>) {
    let listed = match shared.http.get_application_emojis(app).await {
        Ok(r) => r.model().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let mut have: Vec<Known> = match listed {
        Ok(l) => l
            .items
            .into_iter()
            .map(|e| Known {
                name: e.name,
                id: e.id.get(),
                animated: e.animated,
            })
            .collect(),
        Err(e) => {
            // The route failed: the Unicode glyphs draw.
            set(Vec::new());
            shared.board.error("app emojis", None, e);
            return;
        }
    };
    let missing: Vec<&Art> = ART
        .iter()
        .filter(|a| !have.iter().any(|k| k.name == a.name))
        .collect();
    for art in missing {
        if art.image.len() > MAX_BYTES {
            continue;
        }
        let data = format!("data:{};base64,{}", art.mime, base64(art.image));
        let added = match shared
            .http
            .add_application_emoji(app, art.name, &data)
            .await
        {
            Ok(r) => r.model().await.map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        match added {
            Ok(e) => have.push(Known {
                name: e.name,
                id: e.id.get(),
                animated: art.animated || e.animated,
            }),
            Err(e) => shared.board.error("app emoji upload", None, e),
        }
    }
    let table = table_of(&have);
    if let Err(e) = shared.core.store.put_meta(META, &table) {
        tracing::warn!(error = %format!("{e:#}"), "the app emojis' ids were not kept");
    }
    set(table);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// Discord's size limit holds for every image this build uploads, and each
    /// has a name Discord accepts (2 to 32 letters, digits or underscores).
    #[test]
    fn every_image_fits_discords_limit_and_name() {
        for a in &ART {
            assert!(a.image.len() <= MAX_BYTES, "{}", a.name);
            assert!((2..=32).contains(&a.name.len()), "{}", a.name);
            assert!(a
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'));
            let gif = a.image.starts_with(b"GIF89a");
            assert_eq!(gif, a.animated, "{}: animated art is a GIF", a.name);
        }
    }

    /// With no table every state draws its Unicode glyph, and a state with
    /// no art draws its own.
    #[test]
    fn with_no_emojis_the_unicode_glyphs_draw() {
        reset();
        assert_eq!(state(WorkState::Running), "⚙️");
        assert_eq!(state(WorkState::Waiting), "⚙️");
        assert_eq!(state(WorkState::NeedsYou), "🔴");
        assert_eq!(state(WorkState::Done), "✅");
        assert_eq!(state(WorkState::Failed), "❌");
        assert_eq!(state(WorkState::Cancelled), "⛔");
        assert_eq!(state(WorkState::Expired), "⌛");
        assert_eq!(state(WorkState::Ready), "💤");
        assert_eq!(job(), "🔧");
    }

    use std::time::Duration;

    use theseus_core::secrets::SecretBoard;
    use theseus_sim::fake_discord::{FakeDiscord, APP_ID};

    const APP: Id<ApplicationMarker> = Id::new(APP_ID);

    fn shared_on(dir: &std::path::Path, fake: &FakeDiscord, on: bool) -> Arc<Shared> {
        let secrets = SecretBoard::new([], std::time::Instant::now());
        let core = crate::runtime::tests::core_with(dir, secrets, |c| {
            c.discord.rest_proxy = Some(fake.addr.clone());
            c.discord.app_emojis = on;
        });
        crate::runtime::shared_for_tests(&core)
    }

    fn calls(fake: &FakeDiscord, method: &str) -> usize {
        fake.seen()
            .iter()
            .filter(|s| s.method == method && s.path.ends_with("/emojis"))
            .count()
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        let t0 = std::time::Instant::now();
        while !f() {
            assert!(t0.elapsed() < Duration::from_secs(10), "no {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// The emojis are uploaded once, reused (never uploaded again) after a
    /// restart, which draws the kept ids at once, and uploaded again when
    /// one was deleted.
    #[tokio::test]
    async fn the_emojis_are_uploaded_once_reused_after_a_restart_and_uploaded_again_when_deleted() {
        reset();
        let (dir, fake) = (tempfile::tempdir().unwrap(), FakeDiscord::start());
        let shared = shared_on(dir.path(), &fake, true);
        on_ready(&shared, APP);
        until("six emojis", || fake.app_emojis().len() == 6).await;
        until("the table", || {
            state(WorkState::Done).starts_with("<:theseus_done:")
        })
        .await;
        assert_eq!(calls(&fake, "POST"), 6);
        let working = state(WorkState::Running);
        assert!(
            working.starts_with("<a:theseus_working:"),
            "animated: {working}"
        );
        assert!(fake.app_emojis().iter().all(|e| e.bytes <= MAX_BYTES));
        assert!(fake.app_emojis().iter().filter(|e| e.animated).count() == 2);
        let kept: Vec<Known> = shared.core.store.get_meta(META).unwrap().unwrap();
        assert_eq!(kept.len(), 6, "the ids are kept");

        // A restart: the table is empty, the ids kept draw before the list answers.
        reset();
        let lists = calls(&fake, "GET");
        on_ready(&shared, APP);
        assert_eq!(state(WorkState::Running), working, "the kept ids, at once");
        until("the list read again", || calls(&fake, "GET") == lists + 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(calls(&fake, "POST"), 6, "nothing uploaded again");

        // Someone deletes one: the next READY uploads it again, with a new id.
        let old = state(WorkState::Done);
        fake.drop_app_emoji("theseus_done");
        on_ready(&shared, APP);
        until("it uploaded again", || calls(&fake, "POST") == 7).await;
        until("the new id drawn", || {
            let now = state(WorkState::Done);
            now != old && now.starts_with("<:theseus_done:")
        })
        .await;
        reset();
    }

    /// When the route fails, or `app_emojis` is off, the Unicode glyphs draw.
    #[tokio::test]
    async fn when_the_route_fails_or_it_is_off_the_unicode_glyphs_draw() {
        reset();
        let (dir, fake) = (tempfile::tempdir().unwrap(), FakeDiscord::start());
        let shared = shared_on(dir.path(), &fake, true);
        on_ready(&shared, APP);
        until("six emojis", || fake.app_emojis().len() == 6).await;
        until("the table", || state(WorkState::Done).starts_with('<')).await;
        // The kept ids draw until the list fails, which clears them.
        fake.refuse_app_emojis(true);
        on_ready(&shared, APP);
        until("the Unicode glyphs", || state(WorkState::Done) == "✅").await;
        assert_eq!(state(WorkState::Running), "⚙️");
        assert_eq!(job(), "🔧");
        // Off: not so much as a list.
        reset();
        let (dir, fake) = (tempfile::tempdir().unwrap(), FakeDiscord::start());
        on_ready(&shared_on(dir.path(), &fake, false), APP);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(fake.seen().is_empty(), "{:?}", fake.seen());
        assert_eq!(state(WorkState::NeedsYou), "🔴");
    }

    /// An image over Discord's limit is refused by the stand-in as by Discord,
    /// and the sites that draw a state ask the table: with the emojis known
    /// the board's header and lines carry them.
    #[test]
    fn the_board_draws_the_emojis_when_it_has_them() {
        reset();
        let known = |name: &str, id, animated| Known {
            name: name.into(),
            id,
            animated,
        };
        set(vec![
            known("theseus_working", 11, true),
            known("theseus_needs_you", 12, true),
            known("theseus_done", 13, false),
        ]);
        let h = crate::render::board_v2::headline(1, 2, 3);
        assert_eq!(
            h,
            "<a:theseus_needs_you:12> 1 needs you · <a:theseus_working:11> 2 running · <:theseus_done:13> 3 done since you looked"
        );
        reset();
        assert_eq!(
            crate::render::board_v2::headline(1, 2, 3),
            "🔴 1 needs you · ⚙️ 2 running · ✅ 3 done since you looked"
        );
    }
}
