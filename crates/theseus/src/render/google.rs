//! Google Workspace's lines (the Google design §2.2): health's one line for the
//! bound account, and `theseus google status`.

use theseus_protocol::google::GoogleStatus;

use super::fmt_date;

/// `google: owner@example.test · ok · 15 tools on`, and, when it is not ok, why
/// and what to run. Nothing when the config binds no account.
pub fn google_lines(s: Option<&GoogleStatus>) -> Vec<String> {
    let Some(s) = s else {
        return Vec::new();
    };
    let mut v = vec![format!(
        "google: {} · {} · {} tools on",
        s.account,
        s.state,
        s.tools_on.len()
    )];
    if !s.error.is_empty() {
        v.push(format!("  {}", s.error));
    }
    v
}

/// `theseus google status`: the account, the state, the services, the scopes
/// Google granted, the tools they turn on, where the token is kept, and the day's sends.
pub fn status_lines(s: &GoogleStatus) -> Vec<String> {
    let none = |v: &[String], sep: &str| {
        if v.is_empty() {
            "(none)".to_string()
        } else {
            v.join(sep)
        }
    };
    let mut v = vec![format!("google: {} · {}", s.account, s.state)];
    if !s.error.is_empty() {
        v.push(format!("  {}", s.error));
    }
    v.push(format!("  services: {}", s.services.join(", ")));
    v.push(format!("  granted: {}", none(&s.granted, " ")));
    v.push(format!("  tools on: {}", none(&s.tools_on, ", ")));
    v.push(format!("  refresh token kept in: {}", s.store));
    v.push(format!(
        "  sends today: {} of {}",
        s.sends_today, s.max_sends_per_day
    ));
    if let Some(ms) = s.last_refresh_ms {
        v.push(format!("  last refresh: {}", fmt_date(ms)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_says_the_account_and_why_it_is_not_ok() {
        assert!(google_lines(None).is_empty(), "no account bound");
        let ok = GoogleStatus {
            account: "owner@example.test".into(),
            state: "ok".into(),
            tools_on: vec!["google.call".into(), "gmail.read".into()],
            ..Default::default()
        };
        assert_eq!(
            google_lines(Some(&ok)),
            ["google: owner@example.test · ok · 2 tools on"]
        );
        let lapsed = GoogleStatus {
            state: "needs_auth".into(),
            error: "Google access lapsed: run `theseus google auth`".into(),
            ..ok
        };
        let lines = google_lines(Some(&lapsed));
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("theseus google auth"));
        assert!(status_lines(&lapsed)[1].contains("lapsed"));
    }
}
