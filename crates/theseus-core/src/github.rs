//! GitHub token hygiene: at startup, learn when the configured token expires
//! and say so, loudly when it is close. Never fatal; the network may be down.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::secrets::Secret;

#[derive(Debug, Clone, Serialize)]
pub struct TokenStatus {
    pub login: Option<String>,
    pub expires_at: Option<String>,
    pub days_left: Option<i64>,
    pub fine_grained: bool,
}

pub async fn token_status(token: &Secret) -> Result<TokenStatus> {
    let http = reqwest::Client::builder()
        .user_agent(format!("theseus/{}", crate::VERSION))
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    let resp = http
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token.expose()))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("GET /user")?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub token check returned {}", resp.status());
    }
    let expires_at = resp
        .headers()
        .get("github-authentication-token-expiration")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let days_left = expires_at.as_deref().and_then(parse_days_left);
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    Ok(TokenStatus {
        login: body
            .get("login")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        expires_at,
        days_left,
        fine_grained: token.expose().starts_with("github_pat_"),
    })
}

/// Header format: `2027-02-18 07:00:00 UTC`. Days from now, floor.
fn parse_days_left(s: &str) -> Option<i64> {
    let date = s.split_whitespace().next()?;
    let mut it = date.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    let exp = days_from_civil(y, m, d);
    let now_days = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs()
        / 86_400) as i64;
    Some(exp - now_days)
}

// Howard Hinnant's days-from-civil, days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn parses_github_header() {
        let d = parse_days_left("2999-01-01 07:00:00 UTC").unwrap();
        assert!(d > 300_000);
        assert!(parse_days_left("garbage").is_none());
    }
}
