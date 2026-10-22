//! `data.reply` (theseus-a1wvq.15): Jev's answer to the data question about
//! one reply Theseus wrote, and whether it kept the session's open run going.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::Fact;

/// One reply's judgment, as the run's chain read it. No sentence: the reply
/// already went out, and a run that ended says so where the next request
/// would have carried it.
pub struct DataReply<'a> {
    pub node: &'a str,
    /// The answer's choice and band, none when Jev gave none in time.
    pub choice: Option<&'a str>,
    pub band: Option<theseus_judge::band::Band>,
    pub cost_micros: Option<u64>,
    /// The run now ends at this reply.
    pub extended: bool,
}

impl Fact for DataReply<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DataReply);

    fn row(&self) -> Value {
        json!({"node": self.node, "choice": self.choice, "band": self.band,
               "cost_micros": self.cost_micros, "extended": self.extended})
    }
}
