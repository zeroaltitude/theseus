//! The loop's compile step (moved out of turn.rs at cloud compaction-roots' join, M6 30c, to keep the
//! turn loop under its line ceiling): this loop's request, an append or a recompile; where the ring
//! would cut, a compaction (`turn::compaction`); the task view (39a); the overage check; and the
//! `context.compiled` record.

use super::*;

/// A turn's new system blocks on their way to the blobs (theseus-nsp9l.4):
/// the texts no blob holds yet, kept back until the call's plan frame is
/// durable, then their writes, waited for once the turn's body is done.
#[derive(Default)]
pub(super) struct Keeping {
    texts: Vec<Vec<u8>>,
    writing: Vec<tokio::task::JoinHandle<()>>,
}

impl TurnRunner {
    /// Render the session into this loop's request: an append to the current
    /// compilation, or a recompile (persisted with the session's pointer).
    /// The images the provider refused in the session render as their line;
    /// `strip` recompiles without the prefix's thinking (theseus-0s4).
    /// The prefix library's pick for this loop's spec (theseus-rr15): at a
    /// compile point (`point`), else the current compilation's plan replayed.
    fn prefix_pick(
        &self,
        t: &Turn<'_>,
        spec: &RequestSpec,
        (current, nodes): (
            Option<&crate::compiler::Compilation>,
            &[(u64, crate::stub::Stub)],
        ),
        point: bool,
        i: u32,
    ) -> (
        Option<RequestSpec>,
        Option<std::sync::Arc<crate::prefix::Picked>>,
    ) {
        let at = crate::prefix::turn::At {
            current,
            nodes,
            point,
            first: i == 0,
            class: t.tc.place().content_class(),
        };
        self.prefix_spec(spec, at)
    }

    /// The request's last block, in its order: the task view, the wakes and
    /// routines, the surface's standing records, the persona line, then the
    /// project's slice.
    fn tail_blocks(
        &self,
        t: &mut Turn<'_>,
        session: &SessionRecord,
        compiled: &mut Compiled,
    ) -> (
        Option<theseus_protocol::tasks::TaskViewSummary>,
        Option<theseus_protocol::projects::ProjectSliceSummary>,
    ) {
        // A check sees the task it checks by title and state (theseus-w8ys).
        // The owner's graph and project are not a draft's (theseus-a1wvq.13).
        // Nor are they a restricted model's: their titles and objectives are
        // written by any session in scope, and a label belongs to the session
        // that raised it (theseus-9ymz0, the review's B1).
        let draft = t.tc.draft.is_some();
        let shared = !draft && !self.restricted_profile(&t.target.profile);
        let tasks = shared
            .then(|| crate::task_graph::view::attach(&self.store, &self.kernel, session, compiled))
            .flatten();
        // The wakes this conversation has pending, in the same last block
        // (theseus-lherl): from the turn's list, never a read of its own.
        // And the routines it made, from memory (theseus-pvm5c).
        let routines = crate::routine::lines(
            &self.kernel,
            &self.tools.routines.made_in(&session.session_id),
        );
        crate::commitments::attach(
            t.tc.commitments,
            &routines,
            self.restricted_profile(&t.target.profile),
            self.kernel.now_ms(),
            compiled,
            &|w| crate::watch::run::shown_value(&self.kernel, w),
        );
        // The records of this turn's surface (theseus-slvl.4), in the tail too.
        Self::standing_here(t, compiled);
        // Who answers this turn, one tail line (theseus-c1uz).
        Self::persona_line(t, compiled);
        // A deep_coding turn's reading hint (theseus-tdkcn).
        Self::coding_hint(t, compiled);
        // The static fan-out cue's line, when it fired (theseus-z9mze.4).
        Self::fan_out_line(t, compiled);
        // A working turn's deliverable, called as the task spells it (theseus-eb4o4).
        self.deliverable_hint(t, compiled);
        // The place's other threads, for a turn that asks across them (theseus-98bz.9);
        // their cards are the owner's material, so a draft has none (theseus-rcb2u).
        if shared {
            Self::digest_line(t, compiled);
        }
        // The project this conversation serves, last of all (theseus-n5ui2).
        let project = shared
            .then(|| crate::project::slice::attach(&self.store, &self.kernel, session, compiled))
            .flatten();
        (tasks, project)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn compile_step(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        spec: &RequestSpec,
        force: Option<Recompile>,
        strip: Option<&'static str>,
        overflowed: Option<&Overflowed>,
        i: u32,
    ) -> Result<Result<Compiled, Failure>> {
        let sid = t.tc.session_id;
        let c0 = t.trace.now_us();
        let mut ph = crate::compiler::phases::Phases::new("compile_step");
        let nodes = t.tc.store.transcript(sid)?;
        ph.mark("transcript");
        let current = match session.compilation_id.as_deref() {
            Some(id) => self.store.get_compilation(id)?,
            None => None,
        };
        let (nodes, sources) = self.recall_view(t, nodes, &spec.profile);
        // A big old attachment is a stub at a cold rewrite, and in the warm
        // window after it (theseus-ezeg): decided at the turn's first compile.
        let cold = (i == 0).then(|| self.cold_now(sid, session, spec));
        // A thread resumed cold is rewritten small (`thread_context`).
        let cold_spec = self.cold_spec(spec, cold);
        let spec = cold_spec.as_ref().unwrap_or(spec);
        let (nodes, stubbed) = self.stubbed_view(sid, &spec.model, &session.not_shown, nodes, cold);
        let read_before = crate::stub::read(&nodes);
        ph.mark("views");
        // The prefix library's third layer, picked only at a compile point.
        let point = current.is_none() || force.is_some() || cold_spec.is_some();
        let (prefix_spec, picked) = self.prefix_pick(t, spec, (current.as_ref(), &nodes), point, i);
        let spec = prefix_spec.as_ref().unwrap_or(spec);
        let assembled = t.recall.assembled_id().map(str::to_string);
        let given = self.situation_of(t, &nodes, current.is_none(), session);
        let input = CompileInput {
            session_id: sid,
            current: current.as_ref(),
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec,
            catalog: &self.catalog,
            force,
            // `[context] working_window_tokens` lowers the window (theseus-3ki0).
            window_override: self.working_window(spec),
            blobs: Some(self.store.blobs()),
            hidden: &session.not_shown,
            strip,
            overflowed,
            sources: &sources,
            signals: Some(crate::signals::SignalsAt {
                config: self.judge.config().signals,
                now_ms: theseus_protocol::now_unix_ms(),
            }),
            assembled: assembled.as_deref(),
            situation: &given,
            // The render kept across turns, under this view of the nodes:
            // the old attachments a cold rewrite stubbed (theseus-0l3g).
            kept: Some((&self.kept, {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{stubbed:?}").hash(&mut h);
                h.finish()
            })),
        };
        // Where the ring cut, a summary in its place (30c); past the window
        // with nothing left to drop, the turn fails before any call.
        let mut compiled = Box::pin(self.compact(t, compile(input), input, i)).await?;
        ph.mark("compile");
        compiled.prefix = picked.filter(|p| {
            compiled.new_compilation
                && compiled.compilation.manifest.prefix.as_ref() == Some(&p.plan.used)
        });
        self.prefix_touched(&compiled, spec);
        Self::recall_compiled(t, &mut compiled);
        t.fill = super::fill_step::Fill::of(&compiled, self.fill_window(spec));
        self.home_size_checked(t, compiled.est_tokens, i);
        // A clearing pass (theseus-3ki0): one row a pass.
        if let Some(pass) = compiled.cleared {
            t.record(&fact::compaction::Cleared {
                compilation_id: &compiled.compilation.id,
                pass,
                limit: compiled.budget.limit_tokens,
                loop_index: i,
            });
        }
        Self::standing_listed(t, &mut compiled);
        self.defer_noted(t, spec, &compiled);
        let (tasks, project) = self.tail_blocks(t, session, &mut compiled);
        if let Some(f) = Self::overage(t, &compiled, i) {
            return Ok(Err(f));
        }
        if let Some(f) = self.admitted(t, &compiled, &nodes, tasks.is_some(), i)? {
            return Ok(Err(f));
        }
        ph.mark("admitted");
        // Routing may move the turn (25e): only the compilation the call
        // uses is persisted (`route_step`).
        if compiled.new_compilation && !t.route.defer_persist {
            Self::keep_blocks(t, &mut compiled);
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
        ph.mark("persist");
        ph.log(&nodes, "");
        let c1 = t.trace.now_us();
        let mut summary = Self::compiled_summary(
            t,
            &compiled,
            spec,
            (&nodes, read_before, i),
            (tasks, project),
        );
        summary.cache.stubbed = stubbed;
        summary.walk_tokens = self.walk_tokens(spec, &compiled);
        // While routing decides, the rows wait for the compile the call uses
        // (`route_step`, theseus-d13v): one `context.compiled` and one
        // `loop.started` a loop, each naming a stored compilation.
        if t.route.defer_persist {
            t.route.deferred = Some(Box::new(Deferred { summary, c0, c1 }));
            return Ok(Ok(compiled));
        }
        self.compiled_rows(t, &summary, &compiled, spec, (c0, c1), i);
        Ok(Ok(compiled))
    }

    /// The new compilation's system blocks, each stored once under its digest
    /// (own-benchmark): stamped here, written once the call's plan frame is
    /// durable (`start_keeping`, `cache::keep`, theseus-nsp9l.4).
    pub(super) fn keep_blocks(t: &mut Turn<'_>, compiled: &mut Compiled) {
        let texts = crate::compiler::cache::stamp(&mut compiled.compilation, &compiled.request);
        let fresh = crate::compiler::cache::fresh(t.tc.store.blobs(), &texts);
        t.keeping.texts.extend(fresh);
    }

    /// Start writing the blocks `keep_blocks` kept back, on the blocking pool.
    pub(super) fn start_keeping(t: &mut Turn<'_>) {
        let fresh = std::mem::take(&mut t.keeping.texts);
        let blobs = t.tc.store.blobs_handle();
        t.keeping
            .writing
            .extend(crate::compiler::cache::keep(&blobs, fresh));
    }

    /// Wait for the blocks' writes, starting any no call reached: called once
    /// the turn's body is done, so a turn that has ended has its blocks kept.
    pub(super) async fn blocks_kept(t: &mut Turn<'_>) {
        Self::start_keeping(t);
        for writing in std::mem::take(&mut t.keeping.writing) {
            if let Err(e) = writing.await {
                tracing::warn!(error = %e, "a system blocks' write did not finish; its turns cannot be replayed");
            }
        }
    }

    /// The window a request on `spec`'s model compiles against, when
    /// `[context] working_window_tokens` lowers the catalog's.
    pub(crate) fn working_window(&self, spec: &RequestSpec) -> Option<u64> {
        let model = self.catalog.get(&spec.model).map(|e| e.context_window);
        self.cfg.context.working_window(model)
    }

    /// The loop's compile, recorded: its `context.compiled` row, the judge's
    /// mark at the compile, and `loop.started`.
    pub(super) fn compiled_rows(
        &self,
        t: &mut Turn<'_>,
        summary: &ContextCompiled,
        compiled: &Compiled,
        spec: &RequestSpec,
        (c0, c1): (u64, u64),
        i: u32,
    ) {
        let sid = t.tc.session_id;
        // Its span and its row carry the notification's params.
        t.record(&fact::turn::ContextCompiled {
            summary,
            compiled,
            spec,
            c0,
            c1,
        });
        // A signal and no trigger: `continue.v1` in shadow, spawned (M5 25b).
        let at = crate::judge::AtCompile {
            session_id: sid,
            execution_id: t.tc.execution_id,
            turn_id: t.tc.turn_id,
            loop_index: i,
            task: t.tc.task.is_some(),
            kernel: &self.kernel,
        };
        if let Some(mark) = self.judge.at_compile(compiled, at) {
            t.trace.mark("judge", "mark", mark);
        }
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: spec.tools.len() as u32,
        });
    }

    /// The ontology walk's guidance the request carried, in tokens, counted
    /// as the census and `context.explain` count it (`context_parts`): the
    /// sections of the memberships the compile used, at the model's figures.
    /// Measured only (theseus-d9je7): no budget holds it yet.
    fn walk_tokens(&self, spec: &RequestSpec, compiled: &Compiled) -> u64 {
        let Some(walk) = spec.walk.as_deref() else {
            return 0;
        };
        let used = &compiled.compilation.manifest.memberships;
        let composition = walk.compose(&crate::ontology::recorded(used));
        let rates = self.catalog.get(&spec.model).map_or_else(
            || crate::catalog::TokenRates::of(&spec.model),
            |e| e.bytes_per_token,
        );
        (composition.sections.iter())
            .map(|s| crate::context_parts::text_tokens(s.len(), rates))
            .sum()
    }

    /// The loop's `context.compiled`: its span, its row, and the
    /// notification carry it.
    fn compiled_summary(
        t: &Turn<'_>,
        compiled: &Compiled,
        spec: &RequestSpec,
        (nodes, read_before, i): (&[(u64, crate::stub::Stub)], u64, u32),
        (tasks, project): (
            Option<theseus_protocol::tasks::TaskViewSummary>,
            Option<theseus_protocol::projects::ProjectSliceSummary>,
        ),
    ) -> ContextCompiled {
        let sid = t.tc.session_id;
        ContextCompiled {
            session_id: sid.into(),
            turn_id: t.tc.turn_id.into(),
            loop_index: i,
            decision: compiled.decision().into(),
            trigger: compiled.trigger.clone(),
            compilation_id: compiled.compilation.id.clone(),
            strategy: compiled.compilation.strategy.clone(),
            prefix_nodes: compiled.prefix_nodes as u64,
            tail_nodes: compiled.tail_nodes as u64,
            messages: compiled.messages as u64,
            est_tokens: compiled.est_tokens,
            digest: compiled.digest.clone(),
            repairs: compiled.repairs.clone(),
            tools: spec.tools.len() as u64,
            nodes_scanned: nodes.len() as u64,
            decoded: crate::stub::read(nodes) - read_before,
            stubs: nodes.len() as u64 - crate::stub::read(nodes),
            context_files: spec.context_files.clone(),
            persona: spec.persona.clone(),
            // How the compiler sized the request (theseus-f5hf).
            estimate: Some(compiled.estimate.summary()),
            // The request's cache breakpoints and their TTLs (theseus-ev1).
            cache: CacheSummary {
                breakpoints: compiled
                    .cache
                    .breakpoints()
                    .into_iter()
                    .map(String::from)
                    .collect(),
                ttl: spec.cache_ttl.as_str().into(),
                conversation_ttl: spec.conversation_ttl.min(spec.cache_ttl).as_str().into(),
                stubbed: Vec::new(),
                layers: crate::compiler::layers::digests_of(
                    &compiled.cache,
                    &compiled.request.system,
                )
                .into_iter()
                .collect(),
            },
            // The class of the place it speaks in, and the context files
            // that class left out (the place rule).
            class: Some(t.tc.class),
            withheld: compiled.withheld,
            signals: compiled.signals.fired.clone(),
            tasks,
            project,
            situation: Some(compiled.situation.clone()),
            standing_here: (t.standing.here.iter())
                .flat_map(|b| b.used.iter().map(|u| u.id.clone()))
                .collect(),
            // Counted by the step, which reads the catalog.
            walk_tokens: 0,
        }
    }
}

/// A first compile's rows, held while routing decides (theseus-d13v).
pub(super) struct Deferred {
    pub(super) summary: ContextCompiled,
    pub(super) c0: u64,
    pub(super) c1: u64,
}
