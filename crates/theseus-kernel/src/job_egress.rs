//! An L1 job's egress (M4 18c; design §2.4). For a job whose list is not
//! empty, the init opens a listener inside the job's network namespace and
//! hands it over; the wrapper serves it with `theseus_sandbox`'s proxy from
//! the host's namespace, and stops the proxy once the job has ended, a stop
//! or a deadline included. The proxy's records go into the completion's
//! `detail.egress` (`theseus_sandbox::egress::Summary`), which the core reads
//! for the `sandbox.egress` rows and for whether the result is outside text.
//! A job with no list gets no listener and no proxy, and pays nothing.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_sandbox::egress::{proxy_env, Allow, Proxy, Running, Summary, PORT};
use theseus_sandbox::{SandboxChild, Spec};

use crate::job::L1;

/// How long a stop waits for the job's tunnels to close by themselves
/// before it ends them: the job has gone, so nothing more reaches it.
const DRAIN: Duration = Duration::from_millis(250);

/// The job's list, and its spec readied for it: a job with a list gets the
/// listener inside its namespace and the proxy's variables, in place of any
/// the call set; one without gets neither. An entry that does not parse is
/// dropped, so the list never widens: the core checked each one before the
/// job was planned.
pub(crate) fn prepare(spec: &mut Spec, l1: &L1) -> Vec<Allow> {
    let allow: Vec<Allow> = l1.egress.iter().filter_map(|s| s.parse().ok()).collect();
    if !allow.is_empty() {
        spec.egress_port = Some(PORT);
        let vars = proxy_env(PORT);
        spec.env.retain(|(k, _)| !vars.iter().any(|(v, _)| v == k));
        spec.env.extend(vars);
    }
    allow
}

/// The proxy, on the listener the init handed over, for a job with a list;
/// None for one without. A proxy that cannot start closes the listener, so
/// the job has no route out at all, and `detail.egress` says why.
pub(crate) fn start(
    child: &mut SandboxChild,
    l1: &L1,
    allow: &[Allow],
    detail: &mut Value,
) -> Option<Running> {
    let fd = child.take_egress_listener()?;
    let resolver = l1.egress_dns.clone().unwrap_or_default();
    match Proxy::new(fd, allow.to_vec(), resolver).start() {
        Ok(running) => Some(running),
        Err(e) => {
            let mut s = serde_json::to_value(Summary::of(allow, &[], 0)).unwrap_or_default();
            s["error"] = json!(format!("the egress proxy did not start: {e}"));
            detail["egress"] = s;
            None
        }
    }
}

/// Stops the job's proxy, once the job has ended, and puts what it recorded
/// in `detail.egress`. None: the job had no proxy.
pub(crate) fn stop(running: Option<Running>, allow: &[Allow], detail: &mut Value) {
    let Some(running) = running else {
        return;
    };
    let (log, dropped) = running.finish(DRAIN);
    detail["egress"] = serde_json::to_value(Summary::of(allow, &log, dropped)).unwrap_or_default();
}
