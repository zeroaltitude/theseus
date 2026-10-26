//! The data floor's facts (theseus-a1wvq.3): a request the provider chokepoint
//! refused to a model of restricted origin, for want of a pass; and a
//! session's data state's rows (theseus-a1wvq.5).

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// `data_gate.blocked`: a request to a model of restricted origin carried no
/// pass for its data, and nothing was sent.
pub struct DataGateBlocked<'a> {
    /// The provider the request was for.
    pub provider: &'a str,
    pub model: &'a str,
    /// `cn` or `unknown`.
    pub origin: &'a str,
}

impl Fact for DataGateBlocked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::DataGateBlocked);

    fn row(&self) -> Value {
        json!({"provider": self.provider, "model": self.model, "origin": self.origin})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Data floor: a request to {} (origin {}) had no pass for its data; nothing was sent.",
                self.model, self.origin
            ),
        );
    }
}

/// `session.data`, `opened`: the session was opened (an open task, a
/// China-origin coding draft).
pub struct SessionDataOpened {
    pub how: crate::data::OpenHow,
}

impl Fact for SessionDataOpened {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionData);

    fn row(&self) -> Value {
        json!({"row": "opened", "how": self.how})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!("Data: the session was opened ({}).", self.how.as_str()),
        );
    }
}

/// `session.data`, `tainted`: an open session's first node that was not
/// open. Bookkeeping: it gates nothing.
pub struct SessionDataTainted<'a> {
    pub why: crate::data::DataWhy,
    pub node: Option<&'a str>,
}

impl Fact for SessionDataTainted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionData);

    fn row(&self) -> Value {
        json!({"row": "tainted", "why": self.why, "node": self.node})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Data: the open session met a node that was not open ({}).",
                self.why.as_str()
            ),
        );
    }
}

/// `session.data`, `screened`: a tool result failed the tool screen, with the
/// screen's word (`Unscreened::as_str`). The run ends there; the row is the
/// record of why (theseus-lm38z).
pub struct SessionDataScreened<'a> {
    pub node: &'a str,
    pub word: &'a str,
}

impl Fact for SessionDataScreened<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionData);

    fn row(&self) -> Value {
        json!({"row": "screened", "node": self.node, "word": self.word})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!("Data: a tool result failed the screen ({}).", self.word),
        );
    }
}

/// `session.data`, `program`: a program's result passed the tool screen on its
/// provenance (theseus-ouqru), and how: `l1` for an enforced job whose recorded
/// view held nothing private, `seal` for a sealed benchmark container, with the
/// seal's verdict and reason so a reader can see what the daemon decided.
pub struct SessionDataProgram<'a> {
    pub node: &'a str,
    pub via: &'a str,
    pub seal: &'a str,
}

impl Fact for SessionDataProgram<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SessionData);

    fn row(&self) -> Value {
        json!({"row": "program", "node": self.node, "via": self.via, "seal": self.seal})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!("Data: a program's result passed the screen ({}).", self.via),
        );
    }
}
