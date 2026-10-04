//! `aws.hands.run`, the fan-out tool (AWS design §3.3): its definition and
//! plan. The runtime runs it through `ToolRuntime::run_hands`
//! (`toolrun/hands.rs`), since its call outlives its turn: it answers
//! `background`, and the group's aggregate is its late result.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_tools::{
    AsyncRun, AwsPlan, Backend as ToolBackend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
};

use super::launch::{self, Backend, RUN};
use crate::aws::Aws;

pub struct HandsRun(pub Arc<Aws>);

impl Tool for HandsRun {
    fn name(&self) -> &'static str {
        RUN
    }

    fn description(&self) -> &'static str {
        "Run a command as one or more hands in AWS: each hand runs argv in the Theseus hand image \
         (or `image`, on Fargate) under its own deadline, with its index and input in \
         THESEUS_HAND_INDEX and THESEUS_HAND_INPUT ({index} and {input} in argv are replaced), and \
         uploads its exit code, output, and the files it writes under out/ to S3. Give count, or \
         inputs (one hand each). until: \"all\" (default), \"first_success\", or {\"quorum\": n}. \
         Short work (up to 10 minutes) runs on Lambda; longer, larger, another image, or another \
         profile runs on Fargate. The call answers at once, as a background group; its result, \
         each hand's outcome, exit code, duration, cost, and S3 prefix, arrives in a later message."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {"type": "array", "items": {"type": "string"}, "minItems": 1,
                    "description": "The program and its arguments, run directly (no shell unless you run one)."},
                "count": {"type": "integer", "minimum": 1, "maximum": launch::MAX_HANDS,
                    "description": "How many hands, each with a null input."},
                "inputs": {"type": "array", "maxItems": launch::MAX_HANDS,
                    "description": "One hand per item; a string is given as itself, anything else as JSON."},
                "image": {"type": "string", "description": "Another container image to run in (Fargate)."},
                "vcpu": {"type": "number", "description": "Fargate's vCPU (default 1)."},
                "memory_mb": {"type": "integer", "description": "Memory in MB (default 2048)."},
                "ttl_secs": {"type": "integer", "minimum": 60, "maximum": 43200,
                    "description": "Each hand's time to live (default 600)."},
                "max_usd": {"type": "number", "description": "The group's cap: no hand launches past it."},
                "until": {"description": "\"all\", \"first_success\", or {\"quorum\": n}."},
                "concurrency": {"type": "integer", "minimum": 1,
                    "description": "The most hands running at once (default: all)."},
                "profile": {"type": "string", "enum": ["basic", "read", "owner"],
                    "description": "The hands' role: basic (default: its own S3 prefix, logs, the queue), read (adds reading the account), owner (allow-all under the guards)."},
                "backend": {"type": "string", "enum": ["lambda", "fargate"]},
                "account": {"type": "string"},
                "region": {"type": "string"}
            },
            "required": ["argv"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> ToolBackend {
        ToolBackend::Async
    }

    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }

    fn family(&self) -> &'static str {
        "aws"
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let r = launch::parse(input)?;
        let backend = launch::choose(&r)?;
        let account = self.0.account(r.account.as_deref())?;
        let region = account.region(r.region.as_deref())?;
        let (service, operation) = match backend {
            Backend::Lambda => ("lambda", "Invoke"),
            Backend::Fargate => ("ecs", "RunTask"),
        };
        Ok(Plan {
            summary: format!(
                "run {} hand{} on {} in {region}: {}",
                r.inputs.len(),
                if r.inputs.len() == 1 { "" } else { "s" },
                backend.as_str(),
                r.argv.join(" ")
            ),
            class: Some(ToolClass::Run),
            aws: Some(AwsPlan {
                account: account.id.clone(),
                region,
                service: service.into(),
                operation: operation.into(),
                cost_bearing: true,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn run_async(&self, _input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        Box::pin(async {
            Err(ToolFailure::new(
                "aws.hands.run runs through the runtime's hands path, never alone",
            ))
        })
    }
}
