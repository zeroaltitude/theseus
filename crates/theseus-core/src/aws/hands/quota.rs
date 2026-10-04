//! The room AWS's quotas leave a group (AWS design §3.3, "Budget per
//! hand"; step 40 part 2, theseus-mgw.11): before a wave launches, the
//! Fargate vCPU quota (Service Quotas' `L-3032A538`, On-Demand vCPUs) or
//! Lambda's unreserved concurrency (its account settings), read once an
//! hour per account and region and kept. A group bigger than the room
//! launches in waves, as `concurrency` makes it; it never fails for a
//! quota: a quota that cannot be read leaves the group as it was, and one
//! smaller than a hand still runs one at a time.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use super::launch::{Backend, HandsRequest};
use crate::aws::session::Kind;
use crate::aws::{Account, Request, Signer};

/// Fargate's On-Demand vCPU quota.
pub const FARGATE_VCPU: &str = "L-3032A538";

/// How long a quota read is kept.
const KEEP: Duration = Duration::from_secs(3600);

/// A quota read: by account, region, and backend, its value and when.
type Read = BTreeMap<(String, String, &'static str), (f64, Instant)>;

/// Each account's quotas, as they were last read.
#[derive(Default)]
pub struct Quotas {
    read: Mutex<Read>,
}

impl Quotas {
    /// The quota for `backend` in `account`'s `region`: Fargate's vCPUs, or
    /// Lambda's unreserved concurrency; read at most once an hour.
    pub async fn get(
        &self,
        account: &Arc<Account>,
        region: &str,
        backend: Backend,
    ) -> Result<f64, String> {
        let k = (account.id.clone(), region.to_string(), backend.as_str());
        if let Some((v, at)) = self.read.lock().unwrap().get(&k) {
            if at.elapsed() < KEEP {
                return Ok(*v);
            }
        }
        let v = read(account, region, backend).await?;
        self.read.lock().unwrap().insert(k, (v, Instant::now()));
        Ok(v)
    }
}

/// One read of the quota.
async fn read(account: &Arc<Account>, region: &str, backend: Backend) -> Result<f64, String> {
    let (service, operation, input) = match backend {
        Backend::Fargate => (
            "service-quotas",
            "GetServiceQuota",
            json!({"ServiceCode": "fargate", "QuotaCode": FARGATE_VCPU}),
        ),
        Backend::Lambda => ("lambda", "GetAccountSettings", json!({})),
    };
    let out = account
        .request(
            None,
            &Request {
                service,
                operation,
                input: &input,
                region,
                pages: 1,
                class: "read",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    let v = match backend {
        Backend::Fargate => out.body["Quota"]["Value"].as_f64(),
        Backend::Lambda => out.body["AccountLimit"]["UnreservedConcurrentExecutions"]
            .as_f64()
            .or_else(|| out.body["AccountLimit"]["ConcurrentExecutions"].as_f64()),
    };
    v.ok_or_else(|| format!("{operation} named no quota"))
}

/// How many of a group's hands its quota lets run at once: at least one.
pub fn hands_that_fit(quota: f64, backend: Backend, r: &HandsRequest) -> u32 {
    let per = match backend {
        Backend::Fargate => r.vcpu.max(0.25),
        Backend::Lambda => 1.0,
    };
    ((quota / per).floor() as u32).max(1)
}

/// The cap the quota puts on a group's running hands, or none when it
/// cannot be read (said in the log; the group goes on as it was).
pub async fn cap(
    quotas: &Quotas,
    account: &Arc<Account>,
    region: &str,
    backend: Backend,
    r: &HandsRequest,
) -> Option<u32> {
    match quotas.get(account, region, backend).await {
        Ok(q) => Some(hands_that_fit(q, backend, r)),
        Err(e) => {
            tracing::warn!(account = %account.id, backend = backend.as_str(), error = %e, "hands: the quota was not read; the group launches as concurrency says");
            None
        }
    }
}
