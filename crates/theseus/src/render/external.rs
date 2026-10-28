//! Health's line about waiting executions, said for what they do
//! (theseus-g00vv). Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::KernelStatus;

/// The kernel's executions by state, `4 cancelled, 189 waiting: 180 at rest on
/// input, 9 retired, 0 on work, 0 on a time`: the waiting count says what each
/// waits on when the daemon sends the split.
pub fn executions(k: &KernelStatus) -> String {
    if k.executions_by_state.is_empty() {
        return "none".to_string();
    }
    k.executions_by_state
        .iter()
        .map(|(state, n)| match state.as_str() {
            "waiting" if k.waiting_split != Default::default() => {
                format!("{n} {state}: {}", k.waiting_split.words())
            }
            _ => format!("{n} {state}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
