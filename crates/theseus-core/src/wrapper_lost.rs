//! A lost job wrapper's reason, kept in the unknown completion's producer
//! (`reconciler:wrapper_lost:SIGTERM`), so a result can say what ended it
//! (theseus-53ujg.2). No stored field: the producer was always free text.

const REASON: &str = "wrapper_lost";

/// The signal's name, as a shell says it.
fn name(signal: i32) -> String {
    match signal {
        libc::SIGHUP => "SIGHUP".into(),
        libc::SIGINT => "SIGINT".into(),
        libc::SIGQUIT => "SIGQUIT".into(),
        libc::SIGABRT => "SIGABRT".into(),
        libc::SIGKILL => "SIGKILL".into(),
        libc::SIGSEGV => "SIGSEGV".into(),
        libc::SIGPIPE => "SIGPIPE".into(),
        libc::SIGALRM => "SIGALRM".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGUSR1 => "SIGUSR1".into(),
        libc::SIGUSR2 => "SIGUSR2".into(),
        n => format!("signal {n}"),
    }
}

/// The reason `mark_unknown` is given for a wrapper that `signal` ended.
pub(crate) fn reason(signal: i32) -> String {
    format!("{REASON}:{}", name(signal))
}

/// The signal a completion's producer names, when a wrapper's loss wrote it.
pub(crate) fn signal_of(producer: &str) -> Option<&str> {
    producer
        .strip_prefix("reconciler:")?
        .strip_prefix(REASON)?
        .strip_prefix(':')
}

/// The header's words for an unknown outcome.
pub(crate) fn words(producer: Option<&str>) -> String {
    match producer.and_then(signal_of) {
        Some(sig) => {
            format!("[outcome unknown: the job's wrapper was ended by {sig} before it reported]\n")
        }
        None => {
            "[outcome unknown: the harness could not establish whether this finished]\n".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_wrappers_signal_rides_the_producer_and_reads_back() {
        let producer = format!("reconciler:{}", reason(libc::SIGTERM));
        assert_eq!(signal_of(&producer), Some("SIGTERM"));
        assert!(words(Some(&producer)).contains("ended by SIGTERM before it reported"));
        // Every other unknown keeps today's sentence.
        for p in [
            None,
            Some("reconciler:wrapper_gone_at_start"),
            Some("reconciler:wrapper_lost"),
        ] {
            assert_eq!(
                words(p),
                "[outcome unknown: the harness could not establish whether this finished]\n"
            );
        }
    }
}
