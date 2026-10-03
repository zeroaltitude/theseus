//! `aws.cost` (AWS design §3.7, §4; C2 = 14b): the month's spend by
//! service, from Cost Explorer. A call costs $0.01, so an answer is kept for
//! a day: the same question within it is answered from what was kept, and
//! says so.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{
    parse, AsyncRun, AwsPlan, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
    ToolOutput,
};

use super::session::Kind;
use super::tools::{cents, failure, to_cents};
use super::{Aws, Request, Signer};

/// How long an answer is kept.
const KEPT: Duration = Duration::from_secs(24 * 3600);

type Kept = Arc<Mutex<HashMap<String, (Instant, String)>>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CostArgs {
    /// `month` (to date, the default) or `last_month`.
    #[serde(default)]
    period: Option<String>,
    #[serde(default)]
    account: Option<String>,
}

/// The first day of a period and the day after its end, `YYYY-MM-DD`, at
/// `now_ms`: this month to tomorrow, or last month whole.
pub fn period(name: &str, now_ms: u64) -> Result<(String, String), String> {
    let today = (now_ms / 86_400_000) as i64;
    let (y, m, _) = crate::wake::civil_from_days(today);
    let first = |y: i64, m: u32| format!("{y:04}-{m:02}-01");
    match name {
        "month" => {
            let (ty, tm, td) = crate::wake::civil_from_days(today + 1);
            Ok((first(y, m), format!("{ty:04}-{tm:02}-{td:02}")))
        }
        "last_month" => {
            let (ly, lm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            Ok((first(ly, lm), first(y, m)))
        }
        other => Err(format!(
            "period {other:?} is not month (to date) or last_month"
        )),
    }
}

/// The spend as the model reads it: the total, then each service, most first.
fn spend_text(account: &str, period: &str, start: &str, end: &str, body: &Value) -> String {
    let groups = body["ResultsByTime"][0]["Groups"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut rows: Vec<(u64, String)> = groups
        .iter()
        .filter_map(|g| {
            let amount = g["Metrics"]["UnblendedCost"]["Amount"].as_str()?;
            Some((to_cents(amount)?, g["Keys"][0].as_str()?.to_string()))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let total: u64 = rows.iter().map(|(c, _)| c).sum();
    let mut text = format!(
        "Account {account}'s spend, {period} ({start} to {end}, exclusive): ${} across {} \
         services\n",
        cents(total),
        rows.len()
    );
    for (c, service) in rows.iter().filter(|(c, _)| *c > 0) {
        text.push_str(&format!("  ${} · {service}\n", cents(*c)));
    }
    let free = rows.iter().filter(|(c, _)| *c == 0).count();
    if free > 0 {
        text.push_str(&format!("  ({free} more at $0.00)\n"));
    }
    text
}

pub struct Cost {
    aws: Arc<Aws>,
    kept: Kept,
}

impl Cost {
    pub fn new(aws: Arc<Aws>) -> Self {
        Self {
            aws,
            kept: Kept::default(),
        }
    }
}

impl Tool for Cost {
    fn name(&self) -> &'static str {
        "aws.cost"
    }
    fn description(&self) -> &'static str {
        "What Theseus's AWS account has spent, by service, from Cost Explorer: this month to \
         date (the default) or last month. Each call costs AWS $0.01, so an answer is kept for \
         a day. The budget's own line (health, aws_whoami) is free and needs no call."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period": {"type": "string", "enum": ["month", "last_month"], "description": "This month to date (default), or last month."},
                "account": {"type": "string", "description": "The account's id, when several are bound."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: CostArgs = parse(input)?;
        let account = self.aws.account(a.account.as_deref())?;
        let p = a.period.unwrap_or_else(|| "month".into());
        period(&p, theseus_protocol::now_unix_ms())?;
        Ok(Plan {
            summary: format!(
                "read account {}'s spend ({p}) from Cost Explorer",
                account.id
            ),
            class: Some(ToolClass::Read),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region: "us-east-1".into(),
                service: "ce".into(),
                operation: "GetCostAndUsage".into(),
                cost_bearing: true,
                ..Default::default()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let args = parse::<CostArgs>(input).and_then(|a| {
            let account = self.aws.account(a.account.as_deref())?.clone();
            let p = a.period.unwrap_or_else(|| "month".into());
            let (start, end) = period(&p, theseus_protocol::now_unix_ms())?;
            Ok((account, p, start, end))
        });
        let binding = ctx.aws.clone();
        let kept = self.kept.clone();
        Box::pin(async move {
            let (account, p, start, end) = args.map_err(ToolFailure::new)?;
            let key = format!("{}|{p}|{start}", account.id);
            let hit = kept
                .lock()
                .unwrap()
                .get(&key)
                .filter(|(at, _)| at.elapsed() < KEPT)
                .map(|(at, text)| (at.elapsed(), text.clone()));
            if let Some((age, text)) = hit {
                return Ok((
                    ToolOutput {
                        text: format!(
                            "{text}(from Cost Explorer {} minutes ago: kept a day, since a call \
                             costs $0.01)\n",
                            age.as_secs() / 60
                        ),
                        meta: json!({"account": account.id, "period": p, "kept": true}),
                    },
                    None,
                ));
            }
            let input = json!({
                "TimePeriod": {"Start": start, "End": end},
                "Granularity": "MONTHLY",
                "Metrics": ["UnblendedCost"],
                "GroupBy": [{"Type": "DIMENSION", "Key": "SERVICE"}],
            });
            let r = Request {
                service: "ce",
                operation: "GetCostAndUsage",
                input: &input,
                region: "us-east-1",
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Work),
            };
            let out = account
                .request(binding.as_deref(), &r)
                .await
                .map_err(|f| failure("ce:GetCostAndUsage", f))?;
            let text = spend_text(&account.id, &p, &start, &end, &out.body);
            kept.lock()
                .unwrap()
                .insert(key, (Instant::now(), text.clone()));
            Ok((
                ToolOutput {
                    text,
                    meta: json!({"account": account.id, "period": p, "request_id": out.request_id}),
                },
                None,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_is_this_month_to_tomorrow_or_last_month_whole() {
        // 2026-10-03T12:00:00Z.
        let now = 1_791_028_800_000;
        assert_eq!(
            period("month", now).unwrap(),
            ("2026-10-01".into(), "2026-10-04".into())
        );
        assert_eq!(
            period("last_month", now).unwrap(),
            ("2026-09-01".into(), "2026-10-01".into())
        );
        // January's last month is December of the year before.
        let jan = 1_798_804_800_000; // 2027-01-01T12:00:00Z
        assert_eq!(
            period("last_month", jan).unwrap(),
            ("2026-12-01".into(), "2027-01-01".into())
        );
        assert!(period("year", now).is_err());
    }
}
