//! The loop's compile step (moved out of turn.rs at cloud compaction-roots' join, M6 30c, to keep the
//! turn loop under its line ceiling): this loop's request, an append or a recompile; where the ring
//! would cut, a compaction (`turn::compaction`); the task view (39a); the overage check; and the
//! `context.compiled` record.

use super::*;

impl TurnRunner {
    /// Render the session into this loop's request: an append to the current
    /// compilation, or a recompile (persisted with the session's pointer).
    /// The images the provider refused in the session render as their line;
    /// `strip` recompiles without the prefix's thinking (theseus-0s4).
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
        let (nodes, sources) = self.recall_view(t, nodes);
        // A big old attachment is a stub at a cold rewrite, and in the warm
        // window after it (theseus-ezeg): decided at the turn's first compile.
        let cold = (i == 0).then(|| self.cold_now(sid, session, spec));
        // A thread resumed cold is rewritten small (`thread_context`).
        let cold_spec = self.cold_spec(spec, cold);
        let spec = cold_spec.as_ref().unwrap_or(spec);
        let (nodes, stubbed) = self.stubbed_view(sid, &spec.model, &session.not_shown, nodes, cold);
        let read_before = crate::stub::read(&nodes);
        ph.mark("views");
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
        Self::recall_compiled(t, &mut compiled);
        t.fill = super::fill_step::Fill::of(&compiled, self.fill_window(spec));
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
        // A check sees the task it checks by title and state (theseus-w8ys).
        let tasks =
            crate::task_graph::view::attach(&self.store, &self.kernel, session, &mut compiled);
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
            self.kernel.now_ms(),
            &mut compiled,
            &|w| crate::watch::run::shown_value(&self.kernel, w),
        );
        // The records of this turn's surface (theseus-slvl.4), in the tail too.
        Self::standing_here(t, &mut compiled);
        Self::persona_line(t, &mut compiled);
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
            Self::keep_blocks(t.tc.store, &mut compiled);
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
        ph.mark("persist");
        ph.log(&nodes, "");
        let c1 = t.trace.now_us();
        let mut summary =
            Self::compiled_summary(t, &compiled, spec, (&nodes, read_before, i), tasks);
        summary.cache.stubbed = stubbed;
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
    /// (own-benchmark), before the frame that names them.
    pub(super) fn keep_blocks(store: &Store, compiled: &mut Compiled) {
        let texts = crate::compiler::cache::stamp(&mut compiled.compilation, &compiled.request);
        crate::compiler::cache::keep(store.blobs(), &texts);
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

    /// The loop's `context.compiled`: its span, its row, and the
    /// notification carry it.
    fn compiled_summary(
        t: &Turn<'_>,
        compiled: &Compiled,
        spec: &RequestSpec,
        (nodes, read_before, i): (&[(u64, crate::stub::Stub)], u64, u32),
        tasks: Option<theseus_protocol::tasks::TaskViewSummary>,
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
            situation: Some(compiled.situation.clone()),
            standing_here: (t.standing.here.iter())
                .flat_map(|b| b.used.iter().map(|u| u.id.clone()))
                .collect(),
        }
    }
}

/// A first compile's rows, held while routing decides (theseus-d13v).
pub(super) struct Deferred {
    pub(super) summary: ContextCompiled,
    pub(super) c0: u64,
    pub(super) c1: u64,
}
