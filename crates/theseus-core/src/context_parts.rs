//! A request's parts, as `context.explain` shows them (theseus-7n3e): the
//! header's, each context file's, and each category's guidance, built from
//! the compiler's own inputs (`request_spec` and the compile walk) without
//! compiling, writing, or warning of anything.
//!
//! A compilation records its system blocks' digest, never their text, so the
//! parts are always today's: built as the session's next turn would build
//! them, with the memberships the asked turn's compilation recorded. Their
//! digest against that compilation's says whether they are still the bytes
//! that turn saw.

use theseus_ontology::{GuidanceUsed, MembershipUsed};
use theseus_protocol::SessionKind;

use crate::catalog::TokenRates;
use crate::ceiling::PlaceView;
use crate::compiler::situation;
use crate::compiler::RequestSpec;
use crate::context_files::ContextFile;
use crate::places::PlaceClass;
use crate::turn::{Target, TurnRunner, ASSEMBLY, PERSONA};

/// A request's system parts as the next turn would build them.
pub struct Built {
    /// The header's parts, named: what `system_blocks` joins.
    pub header: Vec<(&'static str, String)>,
    /// The context files, each its section (a shared place's withheld ones
    /// their header and why).
    pub files: Vec<ContextFile>,
    /// The guidance's sections, in order: the preamble (no guidance of its
    /// own), then each category's, with what the manifest records of it.
    pub guidance: Vec<(String, Option<GuidanceUsed>, String)>,
    /// The spec they make, the walk's guidance in it: its system blocks are
    /// the request's, byte for byte.
    pub spec: RequestSpec,
}

impl TurnRunner {
    /// The header's parts, each named, in the order `system_blocks` joins
    /// them.
    pub fn header_parts(&self, target: &Target, place: PlaceView) -> Vec<(&'static str, String)> {
        let mut parts = vec![
            ("persona", PERSONA.to_string()),
            ("assembly", ASSEMBLY.to_string()),
            ("precedence", situation::PRECEDENCE.to_string()),
        ];
        let note = self.tools.system_note_for(place);
        if !note.is_empty() {
            parts.push(("tools note", note));
        }
        if let Some(s) = target.system.as_ref().filter(|s| !s.trim().is_empty()) {
            parts.push(("profile", s.clone()));
        }
        parts
    }

    /// The parts of `session`'s request on `target`, with `recorded`'s
    /// memberships (a turn's compilation's) or, without, the walk's current
    /// ones (a recompile's). The files are read without a warning
    /// (`ContextFiles::peek`).
    pub fn built_parts(
        &self,
        session: &str,
        target: &Target,
        kind: SessionKind,
        recorded: Option<&[MembershipUsed]>,
    ) -> Built {
        let place = self.view_of(session);
        let paths = self.cfg.context_paths(target.persona.as_deref());
        let mut files = self.context_files.peek(&paths);
        if place.class == PlaceClass::Shared {
            crate::context_files::withhold_shared(&mut files);
        }
        let spec = self.spec_of(target, kind, place, files.clone());
        let header = self.header_parts(target, place);
        let Some(walk) = self.walk(session, place.class) else {
            return Built {
                header,
                files,
                guidance: Vec::new(),
                spec,
            };
        };
        let composition = match recorded {
            Some(r) => walk.compose(&crate::ontology::recorded(r)),
            None => walk.compose(&walk.current),
        };
        let guidance = named_sections(&composition.sections, &composition.guidance);
        let spec = crate::ontology::guided(&spec, composition).into_owned();
        Built {
            header,
            files,
            guidance,
            spec,
        }
    }
}

/// The walk's sections by what each is: a preamble leads them when any
/// guidance is admitted (`theseus_ontology::compose::PREAMBLE`), and each
/// category's section follows in the order its `GuidanceUsed` does.
pub fn named_sections(
    sections: &[String],
    used: &[GuidanceUsed],
) -> Vec<(String, Option<GuidanceUsed>, String)> {
    let lead = sections.len().saturating_sub(used.len());
    sections
        .iter()
        .enumerate()
        .map(
            |(i, text)| match i.checked_sub(lead).and_then(|j| used.get(j)) {
                Some(g) => (g.category.to_string(), Some(g.clone()), text.clone()),
                None => ("preamble".to_string(), None, text.clone()),
            },
        )
        .collect()
}

/// A text's tokens at the model's figures, as the compiler's census counts
/// a system block's.
pub fn text_tokens(bytes: usize, rates: TokenRates) -> u64 {
    let census = crate::provider::Census {
        text: bytes as u64,
        ..crate::provider::Census::default()
    };
    census.tokens(rates)
}

/// JSON's tokens at the model's figures (the tools' definitions).
pub fn json_tokens(bytes: usize, rates: TokenRates) -> u64 {
    let census = crate::provider::Census {
        json: bytes as u64,
        ..crate::provider::Census::default()
    };
    census.tokens(rates)
}
