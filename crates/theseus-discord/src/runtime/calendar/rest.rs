//! Discord's scheduled-events routes, as the bot (theseus-4zc4d). Plain
//! requests: the calendar needs four routes and no gateway state, and the same
//! writer answers the stand-in (`[discord] rest_proxy`) in tests.

use std::time::Duration;

use serde_json::Value;

/// A refused or failed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fail {
    /// The HTTP status; 0 when no answer came.
    pub status: u16,
    /// Discord's error code, 0 when it gave none.
    pub code: i64,
    pub why: String,
}

impl Fail {
    /// The bot may not manage events here.
    pub(super) fn forbidden(&self) -> bool {
        self.status == 403
    }

    /// The guild holds as many events as it may (30038).
    pub(super) fn full(&self) -> bool {
        self.code == 30038
    }

    /// The event is not there (any more).
    pub(super) fn gone(&self) -> bool {
        self.status == 404
    }

    /// Worth trying again at the next pass, and nothing to tell: Discord is
    /// down, slow, or rate limiting.
    pub(super) fn transient(&self) -> bool {
        self.status == 0 || self.status == 429 || self.status >= 500
    }
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status == 0 {
            write!(f, "{}", self.why)
        } else {
            write!(
                f,
                "Discord answered {} ({}): {}",
                self.status, self.code, self.why
            )
        }
    }
}

pub(super) struct Rest {
    http: reqwest::Client,
    base: String,
    token: String,
}

impl Rest {
    /// None until the bot's token resolves (it has, by Ready).
    pub(super) fn of(core: &theseus_core::Core) -> Option<Self> {
        use theseus_core::secrets::SecretState;
        let cfg = &core.cfg.discord;
        let token = match core.secrets.subscribe().borrow().get(&cfg.token_secret) {
            Some(SecretState::Ready(t)) => t.expose().to_string(),
            _ => return None,
        };
        let base = match cfg.rest_proxy.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => format!("http://{p}/api/v10"),
            None => "https://discord.com/api/v10".to_string(),
        };
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok()?,
            base,
            token,
        })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        guild: u64,
        id: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Value, Fail> {
        let tail = id.map(|i| format!("/{i}")).unwrap_or_default();
        let url = format!("{}/guilds/{guild}/scheduled-events{tail}", self.base);
        let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bot {}", self.token))
            .map_err(|_| Fail {
                status: 0,
                code: 0,
                why: "the token is not a header".into(),
            })?;
        auth.set_sensitive(true);
        let mut req = self
            .http
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, auth);
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = req.send().await.map_err(|e| Fail {
            status: 0,
            code: 0,
            why: e.without_url().to_string(),
        })?;
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(json);
        }
        Err(Fail {
            status,
            code: json["code"].as_i64().unwrap_or(0),
            why: json["message"]
                .as_str()
                .unwrap_or("no reason given")
                .to_string(),
        })
    }

    /// The guild's scheduled events.
    pub(super) async fn list(&self, guild: u64) -> Result<Vec<Value>, Fail> {
        let v = self.call(reqwest::Method::GET, guild, None, None).await?;
        Ok(v.as_array().cloned().unwrap_or_default())
    }

    pub(super) async fn create(&self, guild: u64, body: &Value) -> Result<Value, Fail> {
        self.call(reqwest::Method::POST, guild, None, Some(body))
            .await
    }

    pub(super) async fn edit(&self, guild: u64, id: &str, body: &Value) -> Result<Value, Fail> {
        self.call(reqwest::Method::PATCH, guild, Some(id), Some(body))
            .await
    }

    pub(super) async fn delete(&self, guild: u64, id: &str) -> Result<(), Fail> {
        self.call(reqwest::Method::DELETE, guild, Some(id), None)
            .await
            .map(|_| ())
    }
}
