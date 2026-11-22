//! A thread's cold resume (theseus-h4a7, thread-context): its bounds, from `[threads]`. The rewrite that
//! made a cold thread's request small rode the transcript's ring, which is gone; what is left of it here
//! is the `del-cost` lane's to remove with the cold half of `thread_context`.

/// A cold resume's bounds: from `[threads]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cold {
    /// The newest turns a resume keeps whole.
    pub keep_turns: u32,
    /// The tokens a resume stays within.
    pub working_tokens: u64,
}
