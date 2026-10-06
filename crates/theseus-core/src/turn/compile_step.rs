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
        let nodes = t.tc.store.transcript(sid)?;
        let current = match session.compilation_id.as_deref() {
            Some(id) => self.store.get_compilation(id)?,
            None => None,
        };
        let (nodes, sources) = self.recall_view(t, nodes);
        let read_before = crate::stub::read(&nodes);
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
            window_override: None,
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
        };
        // Where the ring cut, a summary in its place (30c); past the window
        // with nothing left to drop, the turn fails before any call.
        let mut compiled = Box::pin(self.compact(t, compile(input), input, i)).await?;
        Self::recall_compiled(t, &mut compiled);
        // A check sees the task it checks by title and state (theseus-w8ys).
        let tasks =
            crate::task_graph::view::attach(&self.store, &self.kernel, session, &mut compiled);
        if let Some(f) = Self::overage(t, &compiled, i) {
            return Ok(Err(f));
        }
        if let Some(f) = self.admitted(t, &compiled, &nodes, tasks.is_some(), i)? {
            return Ok(Err(f));
        }
        // Routing may move the turn (25e): only the compilation the call
        // uses is persisted (`route_step`).
        if compiled.new_compilation && !t.route.defer_persist {
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
        let c1 = t.trace.now_us();
        let summary = Self::compiled_summary(t, &compiled, spec, (&nodes, read_before, i), tasks);
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
            },
            // The class of the place it speaks in, and the context files
            // that class left out (the place rule).
            class: Some(t.tc.class),
            withheld: compiled.withheld,
            signals: compiled.signals.fired.clone(),
            tasks,
            situation: Some(compiled.situation.clone()),
        }
    }
}

/// A first compile's rows, held while routing decides (theseus-d13v).
pub(super) struct Deferred {
    pub(super) summary: ContextCompiled,
    pub(super) c0: u64,
    pub(super) c1: u64,
}
