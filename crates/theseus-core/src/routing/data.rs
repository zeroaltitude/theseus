//! route.v6's `data` question (theseus-a1wvq): may what the message carries go
//! to a model provider outside the owner's trust? Read here, acted on by
//! nothing yet.

use theseus_judge::band::Band;

/// route.v6's question, by its id in the pack.
pub const QUESTION: &str = "data";

/// Jev's answer to it: `open`, `private` or `unclear`, how sure, and the band
/// that sureness falls in. A restricted model may be sent a request only on
/// `open` in band `Act`; the band is kept so no reader recomputes it.
#[derive(Debug, Clone, PartialEq)]
pub struct DataAnswer {
    pub choice: String,
    pub confidence: f64,
    pub band: Band,
}
