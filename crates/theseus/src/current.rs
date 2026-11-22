//! The thread a shell names (`THESEUS_THREAD`): read only so `theseus ask` can say it is
//! ignored. Threads were retired on 2026-11-22 (issue 44): a turn runs where it was asked, and
//! no terminal keeps a current thread any more (the per-terminal file and the picker went).

pub const ENV: &str = "THESEUS_THREAD";
/// What a shell named for no thread: the place's home conversation.
pub const HOME: &str = "home";

/// The environment's thread: set, not empty, and not `home`.
pub fn from_env() -> Option<String> {
    named(std::env::var(ENV).ok().as_deref())
}

fn named(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    (!s.is_empty() && s != HOME).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_names_the_thread_and_home_or_nothing_names_none() {
        assert_eq!(named(Some("pr")), Some("pr".into()));
        assert_eq!(named(Some(" pr ")), Some("pr".into()));
        assert_eq!(named(Some("home")), None);
        assert_eq!(named(Some("")), None);
        assert_eq!(named(None), None);
    }
}
