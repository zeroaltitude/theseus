//! The probe behind `jev-probe` (design §3, the L1 and L2 live checks): real
//! calls on synthetic states, each printed with its answers and bands, the
//! usage, the cost at the catalog price, and the latency, then a summary
//! (p50 and max latency, total usage and cost).
//!
//! The key comes from `THESEUS_JEV_KEY`, is taken out of the environment at
//! once, and is never printed: no error here carries a header, and the
//! client redacts the key from any body that echoes it. `--fake` runs the
//! same calls against the in-process fake Jev with a dummy key, for a dry run
//! that spends nothing.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde_json::json;

use crate::batch::namespaced;
use crate::breaker::BreakerConfig;
use crate::builders::{prepare, Input, ProbeInput};
use crate::client::{
    Called, ChoiceOption, ClientConfig, JevClient, KeySource, Question, Request, StaticKey,
    Urgency, Usage,
};
use crate::decision::{decide, Verdict};
use crate::eval;
use crate::fake::{FakeJev, Scripted};
use crate::judge::{Ask, DecisionPoint, JevJudge, Judge, Judgment, Mode, Outcome};
use crate::pack::{by_name, Pack, Point};
use crate::price;
use crate::state::NoScrub;

/// The variable the probe's key arrives in.
pub const KEY_VAR: &str = "THESEUS_JEV_KEY";

#[derive(Parser, Debug)]
#[command(
    name = "jev-probe",
    about = "Real Jev calls on synthetic states: answers, bands, usage, cost, latency"
)]
struct Args {
    /// One call holding a Choice, a Score, and a Noul, to see the wire shape.
    #[arg(long)]
    discover: bool,
    /// The L1 live check: three calls on the test pack's state, one Choice,
    /// one Score, one Noul.
    #[arg(long)]
    l1: bool,
    /// Packs to judge (`loop.v1`; several, comma-separated, share one call
    /// when their states match), on the input in `--input`.
    #[arg(long, value_delimiter = ',')]
    pack: Vec<String>,
    /// A JSON file holding the packs' builder input.
    #[arg(long)]
    input: Option<std::path::PathBuf>,
    /// Print each response body as received.
    #[arg(long)]
    raw: bool,
    /// Run against an in-process fake Jev, with no key and no spend.
    #[arg(long)]
    fake: bool,
    /// Run a pack's planted-injection eval set (`security.v3`): one call per
    /// case, each expectation printed as met or missed, then a tally. With
    /// `--fake` the fake is scripted to agree with every case (a dry run of
    /// the plumbing, not of Jev). `--only` runs the named cases.
    #[arg(long, value_name = "PACK")]
    eval: Option<String>,
    /// With `--eval`: run only these cases (comma-separated names).
    #[arg(long, value_delimiter = ',', requires = "eval")]
    only: Vec<String>,
    /// Print every embedded pack's questions as Markdown, for review, and
    /// make no call.
    #[arg(long)]
    questions: bool,
}

pub fn main() -> Result<()> {
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(args))
}

fn key_from_env() -> Result<StaticKey> {
    let key = std::env::var(KEY_VAR).with_context(|| format!("{KEY_VAR} is not set"))?;
    // Out of the environment at once, so nothing started later inherits it.
    std::env::remove_var(KEY_VAR);
    if key.trim().is_empty() {
        bail!("{KEY_VAR} is empty");
    }
    Ok(StaticKey::new(key))
}

/// Latencies and spend across the run.
#[derive(Default)]
struct Tally {
    http_ms: Vec<u64>,
    usage: Usage,
    cost_micros: u64,
    calls: usize,
}

impl Tally {
    fn add(&mut self, http_ms: u64, usage: Option<Usage>) {
        self.calls += 1;
        self.http_ms.push(http_ms);
        if let Some(u) = usage {
            self.usage.input_tokens += u.input_tokens;
            self.usage.output_tokens += u.output_tokens;
            self.cost_micros += price::JevPrice::jev_1_13_0().cost_micros(&u);
        }
    }

    fn print(&self) {
        let mut ms = self.http_ms.clone();
        ms.sort_unstable();
        let p50 = ms.get(ms.len().saturating_sub(1) / 2).copied().unwrap_or(0);
        let max = ms.last().copied().unwrap_or(0);
        println!("== summary");
        println!(
            "calls: {}; http latency p50 {p50} ms, max {max} ms; all: {ms:?}",
            self.calls
        );
        println!(
            "usage: {} in, {} out; cost {} micro-dollars (${:.6}) at the catalog price",
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.cost_micros,
            price::micros_to_usd(self.cost_micros)
        );
    }
}

/// The fake for `--fake`: the test pack's three questions scripted.
fn scripted_fake() -> Result<FakeJev> {
    let fake = FakeJev::start()?;
    fake.script(
        "cause",
        Scripted::Choice {
            option: "dependency_change".into(),
            confidence: 0.93,
        },
    );
    fake.script(
        "severity",
        Scripted::Score {
            level: 2,
            confidence: 0.81,
        },
    );
    fake.script("needs_human", Scripted::Noul(0.89));
    Ok(fake)
}

async fn run(args: Args) -> Result<()> {
    if args.questions {
        print!("{}", questions_markdown()?);
        return Ok(());
    }
    let mut config = ClientConfig::default();
    let mut fake_for_eval: Option<FakeJev> = None;
    let key: Arc<dyn KeySource> = if args.fake {
        // The fake's thread answers until the process ends.
        let fake = scripted_fake()?;
        config.api_base = fake.base();
        fake_for_eval = Some(fake);
        Arc::new(StaticKey::new("fake-key-for-the-dry-run".into()))
    } else {
        Arc::new(key_from_env()?)
    };
    let client = JevClient::new(config, key)?;
    let mut tally = Tally::default();
    if let Some(name) = &args.eval {
        let fake = fake_for_eval.take();
        let r = run_eval(name, &args.only, client, fake.as_ref(), &mut tally).await;
        tally.print();
        return r;
    }
    if args.discover {
        let req = discovery_request();
        let called = client.call(&req, Urgency::Shadow).await;
        report_call("discover", &req, &called, args.raw, &mut tally);
    }
    if args.l1 {
        for req in l1_requests()? {
            let label = format!("l1 {}", req.questions[0].0);
            let called = client.call(&req, Urgency::Shadow).await;
            report_call(&label, &req, &called, args.raw, &mut tally);
        }
    }
    if !args.pack.is_empty() {
        let path = args.input.as_ref().context("--pack needs --input")?;
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let packs: Vec<Arc<Pack>> = args
            .pack
            .iter()
            .map(|n| by_name(n).with_context(|| format!("no embedded pack {n}")))
            .collect::<Result<_>>()?;
        let mut asks = Vec::new();
        for p in &packs {
            let input = Input::parse(p.builder, &text)
                .with_context(|| format!("{} as {:?} input", path.display(), p.builder))?;
            let prepared = prepare(p, &input, &NoScrub)?;
            asks.push(Ask::new(
                p.clone(),
                &prepared,
                Mode::Shadow,
                json!({"probe": true}),
            ));
        }
        let judge = JevJudge::new(client, price::builtin(), BreakerConfig::default());
        let judgments = judge
            .judge(DecisionPoint {
                asks: asks.clone(),
                urgency: Urgency::Shadow,
            })
            .await;
        let mut counted = std::collections::BTreeSet::new();
        for (a, j) in asks.iter().zip(&judgments) {
            report_judgment(a, j);
            // One call per batch, whatever it carried.
            let call = j.call.as_ref().map(|c| c.id.clone()).unwrap_or_default();
            if counted.insert(call) {
                let usage = judgments
                    .iter()
                    .filter(|o| o.call == j.call)
                    .filter_map(|o| o.usage)
                    .fold(None, |acc: Option<Usage>, u| {
                        let a = acc.unwrap_or_default();
                        Some(Usage {
                            input_tokens: a.input_tokens + u.input_tokens,
                            output_tokens: a.output_tokens + u.output_tokens,
                        })
                    });
                tally.add(j.timing.http_ms, usage);
            }
        }
    } else if !args.discover && !args.l1 {
        bail!("nothing to do: pass --discover, --l1, or --pack with --input");
    }
    tally.print();
    Ok(())
}

/// The eval set of `name`, one call per case.
async fn run_eval(
    name: &str,
    only: &[String],
    client: JevClient,
    fake: Option<&FakeJev>,
    tally: &mut Tally,
) -> Result<()> {
    let pack = eval::pack_of(name)?;
    let mut cases = eval::set(name)?;
    if !only.is_empty() {
        for o in only {
            if !cases.iter().any(|c| &c.name == o) {
                bail!("no case {o} in the {name} set");
            }
        }
        cases.retain(|c| only.contains(&c.name));
    }
    let judge = JevJudge::new(client, price::builtin(), BreakerConfig::default());
    let (mut met, mut missed, mut unanswered) = (0usize, 0usize, 0usize);
    let mut table: Vec<String> = Vec::new();
    for case in &cases {
        println!(
            "==== case {} [{:?}]\n{}",
            case.name, case.category, case.why
        );
        if let Some(f) = fake {
            for (q, want) in &case.expect {
                let answer = match want.as_str() {
                    "high" => Scripted::Noul(0.93),
                    "low" => Scripted::Noul(0.05),
                    option => Scripted::Choice {
                        option: option.into(),
                        confidence: 0.9,
                    },
                };
                f.script(&format!("{name}/{q}"), answer);
            }
        }
        let input = Input::Security2(case.input.clone());
        let prepared = prepare(&pack, &input, &NoScrub)?;
        let ask = Ask::new(
            pack.clone(),
            &prepared,
            Mode::Shadow,
            json!({"eval": case.name}),
        );
        let judgments = judge
            .judge(DecisionPoint {
                asks: vec![ask.clone()],
                urgency: Urgency::Shadow,
            })
            .await;
        let j = &judgments[0];
        report_judgment(&ask, j);
        tally.add(j.timing.http_ms, j.usage);
        if j.outcome != Outcome::Answered {
            unanswered += 1;
            println!("EVAL {}: not answered", case.name);
            table.push(format!("{:<48} (not answered)", case.name));
            continue;
        }
        let decision = eval::check_decision(case, &pack, &j.answers);
        table.push(decision_row(case, &pack, &j.answers));
        for c in eval::check(case, &pack, &j.answers)
            .into_iter()
            .chain(decision)
        {
            if c.met {
                met += 1;
            } else {
                missed += 1;
            }
            println!(
                "EVAL {} {}: wanted {}, got {} ... {}",
                case.name,
                c.question,
                c.wanted,
                c.got,
                if c.met { "met" } else { "MISSED" }
            );
        }
    }
    println!("== decisions under {name}");
    for line in &table {
        println!("{line}");
    }
    println!(
        "== eval {name}: {} case(s); {met} expectation(s) met, {missed} missed, {unanswered} case(s) unanswered",
        cases.len()
    );
    Ok(())
}

/// One line of the decisions table: the case, what it should decide, what
/// the pack's deciding questions decided, and which question made it.
fn decision_row(case: &eval::Case, pack: &Pack, answers: &[crate::judge::AnswerRecord]) -> String {
    let d = decide(pack, answers);
    format!(
        "{:<48} {:<8} wanted {:<6} decided {:<5} {}",
        case.name,
        format!("{:?}", case.category).to_lowercase(),
        case.decision.as_deref().unwrap_or("-"),
        if d.verdict == Verdict::Quiet {
            "quiet"
        } else {
            "ask"
        },
        match (&d.by, d.value) {
            (Some(by), Some(p)) => format!("by {by} {p:.2}"),
            _ => String::new(),
        },
    )
}

/// Every embedded pack but the test pack, its questions in full, as the
/// lane's report lists them for review.
pub fn questions_markdown() -> Result<String> {
    use std::fmt::Write as _;
    let packs = crate::pack::embedded()
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut out = String::new();
    for p in packs.iter().filter(|p| p.point != Point::Probe) {
        writeln!(out, "#### `{}`\n", p.name())?;
        writeln!(
            out,
            "{} At `{:?}`; state `{:?}`, capped at {} tokens; baseline `{:?}`; live action `{:?}`.\n",
            p.description, p.point, p.builder, p.state_cap_tokens, p.baseline, p.action
        )?;
        for q in &p.questions {
            writeln!(
                out,
                "- **`{}`** ({:?}{}; act {:.2}, confirm {:.2}): {}",
                q.id,
                q.kind,
                match (q.decides, q.decide_above) {
                    (_, Some(bar)) => format!(", decides above {bar:.2}"),
                    (true, None) => ", decides".to_string(),
                    (false, None) => String::new(),
                },
                q.thresholds.act,
                q.thresholds.confirm,
                q.instructions
            )?;
            if let Some(s) = q.options_from {
                writeln!(out, "  - options: one per item of `{s:?}`, then these:")?;
            }
            if let Some(s) = q.only_when {
                writeln!(out, "  - asked only when `{s:?}` has items")?;
            }
            for o in &q.options {
                let nm = if q.no_match.as_deref() == Some(o.id.as_str()) {
                    " (no match)"
                } else {
                    ""
                };
                writeln!(
                    out,
                    "  - `{}`{nm}: {}",
                    o.id,
                    o.means.as_deref().unwrap_or("")
                )?;
            }
            for (i, l) in q.levels.iter().enumerate() {
                writeln!(out, "  - level {i}: {l}")?;
            }
            if let Some(a) = &q.applies {
                writeln!(out, "  - applies when `{a}` is true")?;
            }
            if let Some(s) = q.per {
                writeln!(
                    out,
                    "  - asked once per item of `{s:?}`, at most {}; `{{item}}` names it",
                    q.max
                )?;
            }
            if let (Some(t), Some(f)) = (&q.when_true, &q.when_false) {
                writeln!(out, "  - true: {t}\n  - false: {f}")?;
            }
        }
        if !p.rollback.is_empty() {
            let rules: Vec<String> = p
                .rollback
                .iter()
                .map(|r| serde_json::to_string(r).unwrap_or_default())
                .collect();
            writeln!(out, "\nRolls back on: {}.", rules.join("; "))?;
        }
        writeln!(out)?;
    }
    Ok(out)
}

/// The test pack's state.
fn probe_state(pack: &Pack) -> Result<Arc<crate::state::BuiltState>> {
    let input: ProbeInput = serde_json::from_str(include_str!("../fixtures/inputs/probe.json"))?;
    Ok(prepare(pack, &Input::Probe(input), &NoScrub)?.state)
}

/// Three requests on the test pack's state: its Choice, its Score, and its
/// Noul `needs_human`, one question each.
fn l1_requests() -> Result<Vec<Request>> {
    let pack = by_name("probe.v1").context("the test pack")?;
    let state = probe_state(&pack)?;
    let asked = pack.ask(&Default::default());
    ["cause", "severity", "needs_human"]
        .iter()
        .map(|id| {
            let a = asked
                .iter()
                .find(|a| a.id == *id)
                .context("a test pack question")?;
            Ok(Request {
                state: state.json.clone(),
                model: pack.jev_model.clone(),
                questions: vec![(namespaced(&pack.name(), id), a.question.clone())],
            })
        })
        .collect()
}

/// The discovery call of 2026-09-30: a synthetic state and one question of
/// each type, ids namespaced as a batch's are, one option sent as `null`.
fn discovery_request() -> Request {
    let state = json!({
        "service": "build-runner",
        "event": "The nightly build failed at the link step: undefined reference to zstd_compress in libarchive.",
        "recent_events": [
            "The zstd dependency was bumped from 1.5 to 1.6 an hour before the build.",
            "The previous nightly build passed."
        ],
        "operator_on_call": true
    });
    let opt = |id: &str, means: Option<&str>| ChoiceOption {
        id: id.into(),
        means: means.map(str::to_string),
    };
    let q = vec![
        (
            "probe.v1/cause".to_string(),
            Question::Choice {
                instructions: "What most likely caused this build failure?".into(),
                options: vec![
                    opt("dependency_change", Some("A dependency changed version shortly before the failure, and the error points at it.")),
                    opt("flaky_infrastructure", Some("The build machine, network, or cache failed, not the code.")),
                    opt("code_bug", Some("A change to the project's own code broke the build.")),
                    opt("other", None),
                ],
            },
        ),
        (
            "probe.v1/severity".to_string(),
            Question::Score {
                instructions: "How badly does this failure affect the team's release work?".into(),
                levels: vec![
                    "Cosmetic: nothing is blocked.".into(),
                    "Minor: a workaround exists.".into(),
                    "Major: a release is blocked until it is fixed.".into(),
                    "Critical: software already shipped is affected.".into(),
                ],
            },
        ),
        (
            "probe.v1/needs_human".to_string(),
            Question::Noul {
                instructions: "Should a person look at this failure today?".into(),
                when_true: Some("A person should look at it today.".into()),
                when_false: Some("It can wait for the next scheduled review.".into()),
            },
        ),
    ];
    Request::new(&state, price::JEV_MODEL, q)
}

/// One raw call, printed.
fn report_call(label: &str, req: &Request, c: &Called, raw: bool, tally: &mut Tally) {
    println!("== {label}");
    println!(
        "latency: queued {} ms, http {} ms, total {} ms",
        c.timing.queued_ms, c.timing.http_ms, c.timing.total_ms
    );
    println!(
        "state: {} bytes, about {} tokens by estimate; {} question(s); request {} bytes, reserved {} micro-dollars",
        req.state.len(),
        req.state_tokens(),
        req.questions.len(),
        req.body().len(),
        price::JevPrice::jev_1_13_0().reserve_request(req)
    );
    if !c.rate_limit.is_empty() {
        println!("rate headers: {:?}", c.rate_limit);
    }
    if raw {
        println!("raw: {}", String::from_utf8_lossy(&c.raw));
    }
    match &c.result {
        Ok(r) => {
            tally.add(c.timing.http_ms, Some(r.usage));
            let p = price::JevPrice::jev_1_13_0();
            println!(
                "model: asked {}, answered {}{}",
                req.model,
                r.model,
                if r.model == req.model { "" } else { " (drift)" }
            );
            println!(
                "usage: {} in, {} out; cost {} micro-dollars (${:.8}) at the catalog price",
                r.usage.input_tokens,
                r.usage.output_tokens,
                p.cost_micros(&r.usage),
                p.cost_usd(&r.usage)
            );
            for (id, a) in &r.answers {
                let t = crate::band::Thresholds::CONSERVATIVE;
                let b = crate::band::band(a, t);
                println!(
                    "answer {id}: {} | band {:?} on {:?} at {:.2}",
                    serde_json::to_string(a).unwrap_or_default(),
                    b.band,
                    b.top,
                    b.value
                );
            }
        }
        Err(e) => {
            tally.add(c.timing.http_ms, None);
            println!("error: {e}");
        }
    }
}

/// One pack's judgment, printed.
fn report_judgment(a: &Ask, j: &Judgment) {
    println!("== {}", j.pack);
    println!(
        "state: {} bytes, about {} tokens by estimate (cap {}); truncated {:?}",
        j.state.bytes, j.state.tokens, j.state.cap_tokens, j.state.truncated
    );
    if let Some(c) = &j.call {
        println!(
            "call: {} ({} pack(s), {} question(s))",
            c.id, c.packs, c.questions
        );
    }
    println!(
        "latency: queued {} ms, http {} ms, total {} ms",
        j.timing.queued_ms, j.timing.http_ms, j.timing.total_ms
    );
    match &j.outcome {
        Outcome::Answered => {
            println!(
                "model: asked {}, answered {}{}",
                j.model,
                j.answered_by.as_deref().unwrap_or("?"),
                if j.model_drift {
                    " (drift: never acted on)"
                } else {
                    ""
                }
            );
        }
        other => println!(
            "outcome: {}",
            serde_json::to_string(other).unwrap_or_default()
        ),
    }
    if let (Some(u), Some(c)) = (j.usage, j.cost_micros) {
        println!(
            "usage (this pack's share): {} in, {} out; cost {c} micro-dollars; reserved {}",
            u.input_tokens,
            u.output_tokens,
            j.reserve_micros.unwrap_or(0)
        );
    }
    println!(
        "asked {} question(s); answered {}",
        a.asked.len(),
        j.answers.len()
    );
    for r in &j.answers {
        println!(
            "answer {}{}: {} | band {:?} on {:?} at {:.2}",
            r.question,
            r.about
                .as_ref()
                .map(|x| format!(" (about {x})"))
                .unwrap_or_default(),
            serde_json::to_string(&r.answer).unwrap_or_default(),
            r.band.band,
            r.band.top,
            r.band.value
        );
    }
}
