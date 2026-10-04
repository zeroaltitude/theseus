//! A proposed extension's trial on the board (M7 43a, design §2.7 steps 2
//! and 3): its frozen copy started through the board's own `Connect` (the
//! `mcp-sandbox` role, in L1), as `ext-<name>`, in state `proposed`; its
//! handshake and list; then the caller's tests; then its stop. A server on
//! trial is never in the board's `servers`, so `rebuild` never offers its
//! tools: no turn sees them, before the ack or after it in this step.
//! Health's `mcp[]` shows it while it runs.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use theseus_mcp::types::Tool as Listed;
use theseus_mcp::Client;

use super::{Connected, McpBoard, Server, State};
use crate::config::McpServerConfig;

/// A server on trial. Dropped, it is stopped: a trial a cancel aborted
/// leaves nothing running.
pub struct Trial {
    board: Arc<McpBoard>,
    name: String,
    pub client: Client,
    pub tools: Vec<Listed>,
    /// Kept so the connection's events have somewhere to go.
    _events: theseus_mcp::Events,
}

impl Drop for Trial {
    fn drop(&mut self) {
        self.client.terminate();
        self.board
            .trials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.name);
    }
}

/// A server on the board's trial list, taken off it when dropped armed.
struct OnTrial<'a> {
    board: &'a McpBoard,
    name: &'a str,
    armed: bool,
}

impl Drop for OnTrial<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.board
                .trials
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(self.name);
        }
    }
}

impl McpBoard {
    /// Start `cfg`'s server on trial as `name`, in state `proposed`, and
    /// read its tools, within its `start_timeout_secs`. Its environment is
    /// the job's alone: an extension is granted no secret. The trial ends
    /// when the returned `Trial` drops.
    pub async fn trial(
        self: &Arc<Self>,
        name: &str,
        cfg: &McpServerConfig,
    ) -> Result<Trial, String> {
        let s = Arc::new(Server::new(name, cfg, None));
        s.set(|l| l.state = Some(State::Proposed));
        self.trials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.to_string(), s.clone());
        // Until it is a `Trial`, a failed start, or a cancel's abort of this
        // future, takes it off the board.
        let mut listed = OnTrial {
            board: self,
            name,
            armed: true,
        };
        let connect = self
            .connect
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let wait = Duration::from_secs(cfg.start_timeout_secs);
        let opened = tokio::time::timeout(wait, async {
            let c = connect.connect(name, cfg, Vec::new(), None).await?;
            match c.client.list_tools().await {
                Ok(tools) => Ok((c, tools)),
                Err(e) => {
                    c.client.terminate();
                    Err(e.to_string())
                }
            }
        })
        .await;
        let (c, tools) = match opened {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(format!("no handshake and list within {} s", wait.as_secs())),
        };
        listed.armed = false;
        let Connected { client, events } = c;
        s.set(|l| {
            l.client = Some(client.clone());
            l.pid = client.pid();
            l.started_at_ms = Some(theseus_protocol::now_unix_ms());
            l.protocol = Some(client.server_info().protocol_version.clone());
            l.tools = tools.clone();
        });
        Ok(Trial {
            board: self.clone(),
            name: name.to_string(),
            client,
            tools,
            _events: events,
        })
    }
}
