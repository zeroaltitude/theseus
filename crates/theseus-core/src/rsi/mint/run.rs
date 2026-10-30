//! The mint's run, in the daemon: the writer's session and the open replays,
//! from a motif to a [`ValidateReport`]. Pure parts are in `spec`, `brief`,
//! `kind` and `validate`; this module is the daemon's side of them: the
//! split key in the store, the pending record, the writer's turn, the
//! instances read from sessions, the replays run in L1 over a copy, the money
//! reserved on `[self.budget]`, and the one row.
//!
//! The writer is a `"self"` session, behind `rsi::gate` and on a model the
//! data rule allows (`super::data_rule`, checked before the session opens and
//! again before its turn). It ends its reply with one line,
//! `MINT {"name": …, "description": …}`; nothing else of its reply is read.
//! Nothing is loaded: a pass leaves the frozen spec at
//! `<state>/self/mint/<name>/<digest>/` and the report for the deploy step.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::own_bench::TurnReplayParams;
use theseus_store::{kinds, NewRecord};

use super::brief::{self, brief, occurrence_id, split, write_check};
use super::kind::{choose, Choice, Kind, Needs};
use super::spec::{flow_spec, placeholder_name};
use super::validate::ab::{ab_replay, Arm, ArmFacts, Arms, Caps};
use super::validate::dry::dry_replay;
use super::validate::fixtures::{arms_of, run_fixtures, GitFixtures};
use super::validate::{isolated, ValidateReport};
use crate::fact::rsi_mint::{MintValidated, TierRow};
use crate::flow::spec::InputType;
use crate::own_bench::whole::hands::{Hands, Minted};
use crate::rpc::Core;
use crate::rsi::backlog::Settled;
use crate::rsi::exams::SplitKey;
use crate::rsi::mine::{Motif, Occurrence};

/// The META record of the split key.
pub const KEY_META: &str = "self.mint.key";
/// What a started item keeps beside its check half.
const PENDING: &str = "pending.json";
/// The brief the writer was given, as its bytes were.
const BRIEF: &str = "brief.md";

/// A started mint: everything its validation reads, written when the writer's
/// session starts so the split and the skeleton do not move under it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pending {
    pub placeholder: String,
    pub kind: Kind,
    pub item: String,
    pub motif: Motif,
}

/// What the writer settles: the tool's name and description, and the type of
/// any input it finds ambiguous.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub name: String,
    pub description: String,
    pub types: BTreeMap<String, InputType>,
}

/// The writer's answer: the one line `MINT {json}` at the end of its reply,
/// `{"name", "description", "types"?: {"<input>": "ref|path|num|str|argv"}}`.
pub fn writer_reply(text: &str) -> Option<Named> {
    let line = text
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("MINT "))?;
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let name = v.get("name")?.as_str()?.trim().to_string();
    let description = v.get("description")?.as_str()?.trim().to_string();
    let types = match v.get("types") {
        None => BTreeMap::new(),
        Some(t) => serde_json::from_value(t.clone()).ok()?,
    };
    (!name.is_empty() && !description.is_empty()).then_some(Named {
        name,
        description,
        types,
    })
}

impl Core {
    fn mint_root(&self) -> PathBuf {
        self.tools.flows.root().to_path_buf()
    }

    fn mint_key(&self) -> Result<SplitKey> {
        if let Some(k) = self
            .store
            .get_meta::<Value>(KEY_META)?
            .and_then(|v| v["key"].as_str().and_then(SplitKey::from_hex))
        {
            return Ok(k);
        }
        let k = SplitKey::fresh();
        self.store.append(&[NewRecord::json(
            kinds::META,
            Some(KEY_META),
            &serde_json::json!({"key": k.hex()}),
        )?])?;
        Ok(k)
    }

    /// The rule's answer for a motif, under `[self.mint] kinds`.
    pub(crate) fn mint_choice(&self, m: &Motif) -> Choice {
        choose(&Needs::of(m), &self.cfg.self_improve.mint.kinds)
    }

    /// Whether the live profile may write for `m`: the data rule.
    pub(crate) fn mint_profile_ok(&self, m: &Motif) -> Result<(), String> {
        let (live, _) = self.live_profile();
        super::data_rule(&self.cfg, &self.catalog, &live, super::private(m))
    }

    /// Split the occurrences, write the check half and the pending record, and
    /// return the writer's brief, made from the build half alone.
    pub(crate) fn mint_prepare(
        &self,
        item: &str,
        m: &Motif,
        kind: Kind,
    ) -> Result<(Pending, String)> {
        let key = self.mint_key()?;
        let halves = split(&key, &m.digest, &m.occurrences);
        let placeholder = placeholder_name(m);
        // What the writer is shown is what would be minted: the flow spec for a
        // flow (a skeleton that is no valid flow is no flow: the item says why),
        // the miner's skeleton for another kind.
        let skeleton = match kind {
            Kind::Flow => {
                let scrub = |s: &str| self.tools.scrubber.scrub(s).0;
                let sk = flow_spec(m, &placeholder, &scrub).map_err(|e| anyhow!(e))?;
                serde_json::to_value(&sk.spec)?
            }
            _ => m.spec(),
        };
        let mut text = brief(&skeleton, kind, &halves.build);
        if kind == Kind::Rust {
            text.push_str(&format!(
                "\nCall self.propose_change {{ item: \"{item}\", issue: <the issue>, why: <one \
                 sentence> }}; the item carries the tag mint:{placeholder}.\n"
            ));
        }
        if !isolated(&text, &halves.check) {
            bail!("the brief holds a check-half id: not started");
        }
        let root = self.mint_root();
        write_check(&root, &placeholder, &halves.check)?;
        let pending = Pending {
            placeholder: placeholder.clone(),
            kind,
            item: item.into(),
            motif: m.clone(),
        };
        let dir = root.join(&placeholder);
        std::fs::write(dir.join(BRIEF), &text)?;
        std::fs::write(dir.join(PENDING), serde_json::to_vec_pretty(&pending)?)?;
        Ok((pending, text))
    }

    pub(crate) fn mint_pending(&self, placeholder: &str) -> Result<Pending> {
        let bytes = std::fs::read(self.mint_root().join(placeholder).join(PENDING))
            .with_context(|| format!("no pending mint {placeholder}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// The writer's turn and then the validation: what a started mint item
    /// does. Stops short, quietly, if the gate no longer allows it.
    pub(crate) async fn mint_run(
        self: &Arc<Self>,
        s: crate::rsi::backlog::Started,
        placeholder: String,
    ) {
        use crate::rsi::{gate, Gate};
        if gate(&self.self_ctx()) != Gate::Allowed {
            return;
        }
        let run = async {
            let pending = self.mint_pending(&placeholder)?;
            self.mint_profile_ok(&pending.motif)
                .map_err(|e| anyhow!(e))?;
            let reply = self.mint_writer_turn(&s, &pending).await?;
            // A Rust mint is the writer's `self.propose_change` and the self-join's:
            // its item carries `mint:<id>` and closes at the join, not here.
            if pending.kind == Kind::Rust {
                return Ok::<_, anyhow::Error>(None);
            }
            let named = writer_reply(&reply)
                .ok_or_else(|| anyhow!("the writer's reply ended with no MINT line"))?;
            Ok(Some(match pending.kind {
                Kind::Mcp => self.mint_mcp_settle(&s, &placeholder, &named),
                _ => self.mint_flow_settle(&pending, &named).await?,
            }))
        };
        let settled = match run.await {
            Ok(Some(settled)) => settled,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(item = %s.item, error = %format!("{e:#}"), "self mint: the mint did not finish");
                Settled::Failed(format!("{e:#}"))
            }
        };
        if let Err(e) = self.backlog_mint_settle(&s.item, settled) {
            tracing::warn!(item = %s.item, error = %format!("{e:#}"), "self mint: the item was not closed");
        }
    }

    /// A flow's validation, and on a pass its deploy: a tool that finds no room
    /// waits, validated, for the tender (`mint_deploy_waiting`).
    async fn mint_flow_settle(
        self: &Arc<Self>,
        pending: &Pending,
        named: &Named,
    ) -> Result<Settled> {
        let r = self.mint_validate(pending, named).await?;
        Ok(match (&r.back_to_backlog, r.pass) {
            (Some(evidence), _) => Settled::Back(evidence.clone()),
            (None, true) => {
                if let Err(e) = self.mint_deploy_flow(&pending.placeholder) {
                    tracing::info!(tool = %r.name, why = %format!("{e:#}"), "self mint: validated, not deployed");
                }
                Settled::Passed(r.digest.clone())
            }
            (None, false) => Settled::Failed(
                r.tiers()
                    .iter()
                    .find(|t| !t.pass)
                    .map_or_else(String::new, |t| format!("{}: {}", t.name, t.why)),
            ),
        })
    }

    /// An MCP mint's end: the extension the writer proposed, self-loaded when
    /// its manifest allows it (no network, no secret, every test passed), else
    /// left to the owner's card with the reason.
    fn mint_mcp_settle(
        &self,
        s: &crate::rsi::backlog::Started,
        placeholder: &str,
        named: &Named,
    ) -> Settled {
        let found = crate::extend::manifests(&self.store).map(|ms| {
            ms.into_iter()
                .filter(|m| {
                    m.name == named.name
                        && m.state == "proposed"
                        && m.proposed_by.session_id == s.session_id
                })
                .max_by_key(|m| m.proposed_at_ms)
        });
        match found {
            Ok(Some(m)) => match self.mint_self_load(placeholder, &m.name, &m.digest) {
                Ok(d) => Settled::Passed(d.digest),
                Err(e) => Settled::Failed(format!("left for the owner's card: {e:#}")),
            },
            Ok(None) => Settled::Failed(format!(
                "the writer proposed no extension named {:?}",
                named.name
            )),
            Err(e) => Settled::Failed(format!("{e:#}")),
        }
    }

    /// Whether this motif was minted already: its pending record exists.
    pub(crate) fn mint_pending_exists(&self, m: &Motif) -> bool {
        self.mint_root()
            .join(placeholder_name(m))
            .join(PENDING)
            .is_file()
    }

    /// The writer's target: Jev's choice for the motif's brief (route.v10,
    /// theseus-ej6bj; there is no key to pin it), listing US options only when
    /// the motif is drawn from private sessions; the live profile (as the
    /// session last ran) when Jev is not asked or cannot be used.
    pub(crate) async fn mint_writer_target(
        &self,
        session: &crate::session::SessionRecord,
        pending: &Pending,
    ) -> Result<crate::turn::Target> {
        use crate::job_model::{How, Site};
        use theseus_judge::builders::{JobKind, RestrictedModels};
        let (live, _) = self.live_profile();
        let own = self.runner.target_for_session(session, &live)?;
        let brief =
            std::fs::read_to_string(self.mint_root().join(&pending.placeholder).join(BRIEF))
                .unwrap_or_default();
        let private = super::private(&pending.motif);
        let site = Site {
            name: "self builder",
            pin: None,
            legacy: None,
            job: JobKind::SelfBuilder { brief, private },
            restricted: RestrictedModels::allowed_if(!private),
        };
        let (choice, how) = self.runner.job_choice(&live, site).await;
        if !matches!(how, How::Jev { .. }) {
            return Ok(own);
        }
        let picked = self.runner.choice_target(&live, &choice)?;
        Ok(crate::turn::Target {
            persona: own.persona,
            engages: own.engages,
            ..picked
        })
    }

    /// The writer's one turn, in its own `"self"` session; its reply.
    async fn mint_writer_turn(
        self: &Arc<Self>,
        s: &crate::rsi::backlog::Started,
        pending: &Pending,
    ) -> Result<String> {
        let session = self
            .store
            .get_session::<crate::session::SessionRecord>(&s.session_id)?
            .ok_or_else(|| anyhow!("session {} is gone", s.session_id))?;
        let target = self.mint_writer_target(&session, pending).await?;
        let sink = crate::bus::EventSink::new(self.bus.clone(), &session.session_id, None);
        let done = self
            .runner
            .run(crate::turn::TurnRequest {
                session,
                input: Some(s.objective.clone()),
                target,
                sink,
                author: "self:mint".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                prompt: None,
                answer_at: None,
                surface: None,
            })
            .await?;
        Ok(done.output)
    }

    /// The flow spec the writer's answer makes of the pending skeleton: named,
    /// described, its ambiguous inputs typed, sealed, and scrubbed (a spec the
    /// scrubber would change, a secret in a description, say, is refused).
    pub(crate) fn mint_spec(
        &self,
        pending: &Pending,
        named: &Named,
    ) -> Result<super::spec::Skeleton> {
        let scrub = |s: &str| self.tools.scrubber.scrub(s).0;
        let mut sk = flow_spec(&pending.motif, &named.name, &scrub).map_err(|e| anyhow!(e))?;
        sk.spec.description.clone_from(&named.description);
        for (input, ty) in &named.types {
            let i = sk
                .spec
                .inputs
                .get_mut(input)
                .ok_or_else(|| anyhow!("{input:?} is not an input"))?;
            i.ty = *ty;
        }
        sk.spec = sk.spec.sealed();
        sk.spec.validate().map_err(|e| anyhow!(e))?;
        let canonical = crate::flow::spec::canonical(&serde_json::to_value(&sk.spec)?);
        if self.tools.scrubber.scrub(&canonical).1 > 0 {
            bail!("the spec holds a value the scrubber would hide: a minted tool's text is never a secret");
        }
        Ok(sk)
    }

    /// The replay's hands over `dir`: its programs run in L1 (a stand-in in a
    /// test), on `dir` alone.
    fn mint_hands(&self, dir: &std::path::Path) -> Hands<'_> {
        Hands::new(
            &self.runner.tools,
            (dir.to_path_buf(), dir.to_path_buf()),
            vec![],
            crate::own_bench::whole::runloop::programs(),
        )
    }

    /// Validate the flow the writer named: freeze its spec, run the fixtures,
    /// the dry replay on the check half, and, when both pass, the model A/B
    /// replay; write `self.mint.validated`. Loads nothing.
    pub(crate) async fn mint_validate(
        self: &Arc<Self>,
        pending: &Pending,
        named: &Named,
    ) -> Result<ValidateReport> {
        if pending.kind != Kind::Flow {
            bail!(
                "only a flow's tiers run here: a {} is checked by its own trial or gate",
                pending.kind.name()
            );
        }
        let m = &pending.motif;
        let sk = self.mint_spec(pending, named)?;
        let digest = sk.spec.digest.clone().unwrap_or_default();
        self.tools.flows.freeze(&sk.spec)?;

        let root = self.mint_root();
        let check = brief::read_check(&root, &pending.placeholder)?;
        // The writer's isolation, asserted on the brief's own bytes.
        let isolated = {
            let text = std::fs::read_to_string(root.join(&pending.placeholder).join(BRIEF))?;
            let refs: Vec<&Occurrence> = check.iter().collect();
            isolated(&text, &refs)
        };
        // Tier 1: the fixtures. Tier 2: the dry replay on the check half.
        let fx_dir = Tmp::new(&pending.placeholder)?;
        let hands = |dir: &std::path::Path| self.mint_hands(dir);
        let fixtures = run_fixtures(
            &arms_of(m),
            &sk.spec,
            &GitFixtures,
            fx_dir.0.as_path(),
            hands,
        )
        .await;
        let (instances, _scratch) = self.mint_instances((m, &sk), &check).await?;
        let dry = dry_replay(&sk.spec, &instances, |i| self.mint_hands(&i.dir)).await;
        // Tier 3: the model, only when the first two pass.
        let ab = if fixtures.pass && dry.pass && isolated {
            Some(self.mint_ab(pending, &sk.spec, &check).await?)
        } else {
            None
        };
        let report = ValidateReport::assemble(
            (named.name.as_str(), pending.kind, &digest),
            (Some(fixtures), Some(dry), ab),
            isolated,
        );
        self.mint_keep_report(&pending.placeholder, &report)?;
        self.mint_record(&report)?;
        Ok(report)
    }

    /// The one row a validation writes.
    fn mint_record(&self, report: &ValidateReport) -> Result<()> {
        let tiers = report.tiers();
        let fact = MintValidated {
            name: report.name.clone(),
            kind: report.kind.name().into(),
            digest: report.digest.clone(),
            tiers: tiers
                .iter()
                .map(|t| TierRow {
                    name: t.name.clone(),
                    ran: t.ran,
                    pass: t.pass,
                })
                .collect(),
            pass: report.pass,
            why: tiers
                .iter()
                .find(|t| !t.pass)
                .map(|t| format!("{}: {}", t.name, t.why))
                .unwrap_or_default(),
        };
        self.store.append(&[crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(())
    }

    /// The A/B replay over the check half's turns, each arm reserved on the
    /// self budget first.
    pub(crate) async fn mint_ab(
        self: &Arc<Self>,
        pending: &Pending,
        spec: &crate::flow::spec::Spec,
        check: &[Occurrence],
    ) -> Result<super::validate::ab::AbReport> {
        let cfg = &self.cfg.self_improve.mint;
        // The data rule holds for the replays' model as for the writer's.
        self.mint_profile_ok(&pending.motif)
            .map_err(|e| anyhow!(e))?;
        let (profile, _) = self.live_profile();
        let arms = CoreArms {
            core: self,
            occurrences: check
                .iter()
                .filter(|o| !o.turn.is_empty())
                .map(|o| (occurrence_id(o), o.clone()))
                .collect(),
            minted: Minted::new(spec.clone()),
            profile,
        };
        let ids: Vec<String> = arms.occurrences.keys().cloned().collect();
        let caps = Caps {
            mint_usd: cfg.usd,
            arm_usd: self.cfg.route_bench.whole_usd,
            turns: cfg.replay_turns as usize,
        };
        ab_replay(
            &arms,
            &self.runner.self_budget,
            &pending.placeholder,
            &ids,
            &caps,
        )
        .await
    }
}

/// A temp directory removed when it drops: the fixtures' ground.
struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Result<Self> {
        // A test's core is leaked with its tasks, so this may never drop:
        // the test helper removes it at exit.
        #[cfg(test)]
        let dir = crate::scratch::dir(tag);
        #[cfg(not(test))]
        let dir = std::env::temp_dir().join(format!("{tag}-{}", crate::new_id("fx")));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The arms over the daemon's whole-turn replay.
struct CoreArms<'a> {
    core: &'a Arc<Core>,
    occurrences: BTreeMap<String, Occurrence>,
    minted: Minted,
    profile: String,
}

impl Arms for CoreArms<'_> {
    async fn run(&self, turn: &str, arm: Arm, left_usd: f64) -> Result<ArmFacts> {
        let o = self.occurrences.get(turn).context("no such occurrence")?;
        let p = TurnReplayParams {
            session: o.session.clone(),
            turn: o.turn.clone(),
            profile: self.profile.clone(),
            effort: None,
            scope: Some(crate::own_bench::whole::WHOLE_TURN.into()),
            compare: Some(false),
        };
        let mut spent = 0;
        let left = theseus_kernel::usd_to_micros(left_usd);
        let minted = (arm == Arm::B).then(|| self.minted.clone());
        let r = self
            .core
            .replay_turn_with(&p, "mint", Some(left), &mut spent, minted)
            .await?;
        let whole = r
            .result
            .whole
            .context("a whole-turn replay carries its facts")?;
        Ok(ArmFacts {
            matched: whole.same.files && whole.same.exits && whole.same.tests != Some(false),
            tokens: r.result.replay.input_tokens + r.result.replay.output_tokens,
            wall_ms: r.result.elapsed_ms,
            decisions: whole.loops,
            called: whole.calls.iter().any(|c| c.tool == self.minted.wire),
            usd: r.result.cost_usd,
        })
    }
}
