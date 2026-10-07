//! A failed turn's words for what follows it (theseus-ljr): apart from
//! `render.rs`, whose length the shape budget caps.

/// What follows a failed turn, by its run's `then`: the driver's backoff, its
/// one retry, or the next message. A call the daemon's stop kept from being
/// sent (class `stopping`, theseus-36re) is retried by its next start.
pub(super) fn then_words(class: Option<&str>, then: Option<&str>) -> &'static str {
    match then {
        Some("backoff") if class == Some("stopping") => {
            " [not sent as the daemon stopped: its next start retries it]"
        }
        Some("backoff") => " [retrying with backoff]",
        Some("retry") => " [retrying once]",
        Some("park") => " [not retried: the next message retries]",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::then_words;

    #[test]
    fn a_turn_the_stop_kept_back_says_the_next_start_retries_it() {
        assert_eq!(
            then_words(Some("stopping"), Some("backoff")),
            " [not sent as the daemon stopped: its next start retries it]"
        );
        assert_eq!(
            then_words(Some("network"), Some("backoff")),
            " [retrying with backoff]"
        );
        assert_eq!(
            then_words(Some("stopping"), Some("park")),
            " [not retried: the next message retries]"
        );
        assert_eq!(then_words(None, None), "");
    }
}
