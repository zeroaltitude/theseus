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
        let assembled = t.recall.assembled_id().map(str::to_string);
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
        };
        // Where the ring cut, a summary in its place (30c); past the window
        // with nothing left to drop, the turn fails before any call.
        let mut compiled = self.compact(t, compile(input), input, i).await?;
        Self::recall_compiled(t, &mut compiled);
        let tasks = crate::task_graph::view::attach(&self.store, &self.kernel, sid, &mut compiled);
        if let Some(f) = Self::overage(t, &compiled, i) {
            return Ok(Err(f));
        }
        if compiled.new_compilation {
            Self::persist_compilation(t.tc.store, &compiled, session, t.tc.turn_id)?;
        }
        let c1 = t.trace.now_us();
        let summary = ContextCompiled {
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
        };
        // Its span and its row carry the notification's params.
        t.record(&fact::turn::ContextCompiled {
            summary: &summary,
            compiled: &compiled,
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
            kernel: &self.kernel,
        };
        if let Some(mark) = self.judge.at_compile(&compiled, at) {
            t.trace.mark("judge", "mark", mark);
        }
        t.record(&fact::turn::LoopStarted {
            turn_id: t.tc.turn_id,
            index: i,
            model: &t.target.model,
            tools_offered: spec.tools.len() as u32,
        });
        Ok(Ok(compiled))
    }
}
