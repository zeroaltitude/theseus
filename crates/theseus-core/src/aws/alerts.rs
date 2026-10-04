//! The alerts topic's subscription, confirmed by the operator (theseus-9p40;
//! AWS design §3.7): `theseus aws confirm-alerts <token>`.
//!
//! The foundation stack subscribes the operator's address to
//! `theseus-alerts`, and SNS mails it a confirmation link. Opening the link
//! confirms the subscription unauthenticated, so every alert then carries a
//! link that unsubscribes with no credentials, and a mail scanner that
//! follows links removes the subscription minutes after each confirmation.
//! So the operator pastes the link's token here instead: the core calls
//! `sns:ConfirmSubscription` with `AuthenticateOnUnsubscribe=true`, signed by
//! the account (so only the account can unsubscribe), reads the
//! subscription back, and reports SNS's `ConfirmationWasAuthenticated`.
//!
//! The token is a secret: it is never printed, logged, ledgered, or kept;
//! any text of AWS's that carried it back has it masked.

use std::sync::Arc;

use serde_json::json;
use theseus_aws::CallError;
use theseus_protocol::AwsConfirmAlertsResult;

use super::session::Kind;
use super::{Account, Failure, Request, Signer};

/// The alerts topic the foundation stack makes (`infra/aws/theseus-foundation.yaml`).
pub const TOPIC: &str = "theseus-alerts";

/// The token from what the operator pasted: the token itself, or the whole
/// confirmation link (copied, never opened), whose `Token` it reads.
pub fn token_of(pasted: &str) -> Result<String, String> {
    let p = pasted.trim();
    let token = if p.contains("Token=") {
        p.split(['?', '&'])
            .find_map(|kv| kv.strip_prefix("Token="))
            .unwrap_or_default()
            .to_string()
    } else {
        p.to_string()
    };
    // SNS's tokens are long runs of hex; anything else was pasted wrong.
    if token.len() < 32 || token.len() > 2048 || !token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(
            "that is not a confirmation token: paste the long Token=… value from the link in \
             SNS's email (copy the link's address; do not open it), or the whole address"
                .into(),
        );
    }
    Ok(token)
}

/// An address with its local part masked: `e…@example.com`.
fn masked(endpoint: &str) -> String {
    match endpoint.split_once('@') {
        Some((local, domain)) => {
            format!("{}…@{domain}", local.chars().next().unwrap_or('?'))
        }
        None => "(not an address)".into(),
    }
}

/// AWS's error, in words, with the token masked wherever it appears.
fn said(f: Failure, token: &str) -> String {
    let words = match &f {
        Failure::Call(CallError::Aws(e))
            if e.code == "InvalidParameter" || e.code == "InvalidParameterValue" =>
        {
            "SNS did not accept the token: it is mistyped, or older than three days (a \
             confirmation token expires after three days). Nothing was confirmed. A new \
             confirmation email comes when the address is subscribed again (the foundation's \
             AlertEmail, through theseus aws bootstrap)."
                .to_string()
        }
        Failure::Call(CallError::Aws(e)) if e.code == "NotFound" => format!(
            "SNS has no {TOPIC} topic for this token in this region: the token is for another \
             topic or region, or the foundation stack is not made yet ({})",
            e.code
        ),
        Failure::Call(CallError::Aws(e)) if e.code == "AuthorizationError" => format!(
            "AWS refused the confirmation to this account's credentials: {}",
            e.message
        ),
        Failure::Unbound(why) => format!("nothing was sent: {why}"),
        other => format!("the confirmation failed: {other}"),
    };
    words.replace(token, "<token>")
}

/// Confirm the account's alerts subscription with `token`, authenticated
/// on unsubscribe, and read the subscription back.
pub async fn confirm(
    account: &Arc<Account>,
    token: &str,
) -> Result<AwsConfirmAlertsResult, String> {
    let region = account.cfg.region.clone();
    let topic = format!("arn:aws:sns:{region}:{}:{TOPIC}", account.id);
    let input = json!({
        "TopicArn": topic,
        "Token": token,
        "AuthenticateOnUnsubscribe": "true",
    });
    // The owner role's work session once the config names it (the key
    // before then): either way the account's own signature, which is what
    // makes an unsubscribe need the account.
    let req = Request {
        service: "sns",
        operation: "ConfirmSubscription",
        input: &input,
        region: &region,
        pages: 1,
        class: "write",
        signer: Signer::As(Kind::Work),
    };
    let out = account
        .request(None, &req)
        .await
        .map_err(|f| said(f, token))?;
    let subscription = out.body["SubscriptionArn"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if subscription.is_empty() {
        return Err("SNS confirmed nothing: its answer named no subscription".into());
    }
    let input = json!({"SubscriptionArn": subscription});
    let req = Request {
        service: "sns",
        operation: "GetSubscriptionAttributes",
        input: &input,
        region: &region,
        pages: 1,
        class: "read",
        signer: Signer::As(Kind::Work),
    };
    let attrs = account.request(None, &req).await.map_err(|f| {
        format!(
            "the subscription {subscription} was confirmed, but reading it back failed: {}",
            said(f, token)
        )
    })?;
    let a = &attrs.body["Attributes"];
    let s = |k: &str| a[k].as_str().map(String::from);
    let authenticated = s("ConfirmationWasAuthenticated").as_deref() == Some("true");
    tracing::info!(account = %account.id, subscription = %subscription, authenticated, "aws: the alerts subscription was confirmed");
    Ok(AwsConfirmAlertsResult {
        account: account.id.clone(),
        topic,
        subscription,
        authenticated,
        pending: s("PendingConfirmation").as_deref() == Some("true"),
        endpoint: s("Endpoint").map(|e| masked(&e)),
        request_id: out.request_id,
    })
}

/// What the result says to the operator, line by line.
pub fn lines(r: &AwsConfirmAlertsResult) -> Vec<String> {
    let mut out = vec![format!(
        "Confirmed {}'s subscription to {} ({}).",
        r.endpoint.as_deref().unwrap_or("the alert address"),
        r.topic,
        r.subscription
    )];
    out.push(if r.authenticated {
        "SNS says ConfirmationWasAuthenticated = true: only the account can unsubscribe it, so \
         a link in an alert, or a scanner that follows it, cannot."
            .into()
    } else {
        "SNS says ConfirmationWasAuthenticated = false: the subscription was confirmed before \
         without authentication (its link was opened), so an alert's unsubscribe link still \
         works. Subscribe the address again and confirm the new token here."
            .into()
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws::tests::{board, layer, Fake, Reply, Seen, ACCOUNT};

    const TOKEN: &str = "2336412f37fb687f5d51e6e2425c464de12884d6f1a2b3c4d5e6f7a8b9c0d1e2";

    fn param<'a>(s: &'a Seen, k: &str) -> Option<&'a str> {
        s.body
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")[..]))
    }

    fn sns(s: &Seen, n: usize) -> Reply {
        let xml = |body: String| {
            (
                200,
                vec![
                    ("x-amzn-requestid", format!("req-{n}")),
                    ("content-type", "text/xml".into()),
                ],
                body,
            )
        };
        match s.action() {
            Some("GetCallerIdentity") => crate::aws::tests::sts(ACCOUNT, n),
            Some("ConfirmSubscription") if param(s, "Token") == Some(TOKEN) => xml(format!(
                "<ConfirmSubscriptionResponse><ConfirmSubscriptionResult><SubscriptionArn>\
                 arn:aws:sns:us-west-2:{ACCOUNT}:theseus-alerts:sub-1</SubscriptionArn>\
                 </ConfirmSubscriptionResult><ResponseMetadata><RequestId>req-{n}</RequestId>\
                 </ResponseMetadata></ConfirmSubscriptionResponse>"
            )),
            Some("ConfirmSubscription") => (
                400,
                vec![("x-amzn-requestid", format!("req-{n}"))],
                "<ErrorResponse><Error><Type>Sender</Type><Code>InvalidParameter</Code>\
                 <Message>Invalid parameter: Token</Message></Error></ErrorResponse>"
                    .into(),
            ),
            Some("GetSubscriptionAttributes") => xml(format!(
                "<GetSubscriptionAttributesResponse><GetSubscriptionAttributesResult><Attributes>\
                 <entry><key>ConfirmationWasAuthenticated</key><value>true</value></entry>\
                 <entry><key>PendingConfirmation</key><value>false</value></entry>\
                 <entry><key>Endpoint</key><value>operator@example.com</value></entry>\
                 </Attributes></GetSubscriptionAttributesResult><ResponseMetadata>\
                 <RequestId>req-{n}</RequestId></ResponseMetadata></GetSubscriptionAttributesResponse>"
            )),
            _ => (400, vec![], "<ErrorResponse><Error><Code>InvalidAction</Code></Error></ErrorResponse>".into()),
        }
    }

    /// The confirmation is signed, carries the token and
    /// `AuthenticateOnUnsubscribe=true`, and goes to the account's alerts
    /// topic; the subscription is read back, and the result says it was
    /// authenticated, with the address masked and no token anywhere.
    #[tokio::test]
    async fn the_confirmation_is_signed_and_authenticates_the_unsubscribe() {
        let fake = Fake::start(sns);
        let aws = layer(&fake, board());
        let account = aws.account(None).unwrap().clone();
        let r = confirm(&account, TOKEN).await.unwrap();
        assert!(r.authenticated && !r.pending, "{r:?}");
        assert_eq!(
            r.topic,
            format!("arn:aws:sns:us-west-2:{ACCOUNT}:theseus-alerts")
        );
        assert_eq!(r.endpoint.as_deref(), Some("o…@example.com"));
        let seen: Vec<Seen> = fake
            .seen()
            .into_iter()
            .filter(|s| s.action() != Some("GetCallerIdentity"))
            .collect();
        assert_eq!(seen.len(), 2);
        let c = &seen[0];
        assert_eq!(c.action(), Some("ConfirmSubscription"));
        assert_eq!(param(c, "AuthenticateOnUnsubscribe"), Some("true"));
        assert_eq!(param(c, "Token"), Some(TOKEN));
        assert!(param(c, "TopicArn").unwrap().contains("theseus-alerts"));
        let auth = c.header("authorization").expect("signed");
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential="), "{auth}");
        assert!(auth.contains("/us-west-2/sns/aws4_request"), "{auth}");
        assert_eq!(seen[1].action(), Some("GetSubscriptionAttributes"));
        let said = format!("{r:?} {}", lines(&r).join(" "));
        assert!(!said.contains(TOKEN), "{said}");
        assert!(said.contains("only the account can unsubscribe"), "{said}");
    }

    /// A bad or expired token says so in plain words, and never echoes it.
    #[tokio::test]
    async fn a_bad_or_expired_token_says_so() {
        let fake = Fake::start(sns);
        let aws = layer(&fake, board());
        let account = aws.account(None).unwrap().clone();
        let wrong = "ffffffffffffffffffffffffffffffffffffffffffffffffffff0000";
        let e = confirm(&account, wrong).await.unwrap_err();
        assert!(e.contains("SNS did not accept the token"), "{e}");
        assert!(e.contains("older than three days"), "{e}");
        assert!(!e.contains(wrong), "{e}");
    }

    /// The params' Debug, which a log of a request would print, withholds it.
    #[test]
    fn the_params_never_print_the_token() {
        let p = theseus_protocol::AwsConfirmAlertsParams {
            account: None,
            token: TOKEN.into(),
        };
        let printed = format!("{p:?}");
        assert!(
            !printed.contains(TOKEN) && printed.contains("<withheld>"),
            "{printed}"
        );
    }

    #[test]
    fn a_pasted_link_or_token_gives_the_token() {
        let link = format!(
            "https://sns.us-west-2.amazonaws.com/confirmation.html?TopicArn=arn:aws:sns:us-west-2:{ACCOUNT}:theseus-alerts&Token={TOKEN}&Endpoint=o@example.com"
        );
        assert_eq!(token_of(&link).unwrap(), TOKEN);
        assert_eq!(token_of(&format!("  {TOKEN}\n")).unwrap(), TOKEN);
        for bad in [
            "",
            "abc",
            "https://example.com/?x=1",
            "not a token at all, but long enough to pass",
        ] {
            assert!(token_of(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_unauthenticated_confirmation_says_what_to_do() {
        let r = AwsConfirmAlertsResult {
            authenticated: false,
            ..Default::default()
        };
        assert!(lines(&r)[1].contains("Subscribe the address again"));
    }
}
