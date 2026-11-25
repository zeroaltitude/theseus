
impl Ontology {
    /// Compose the guidance for a session's memberships: given ones from its
    /// place (origin `transport`), and interpreted ones (at a recompile, the
    /// snapshot's current ones; on an append, the ones its manifest
    /// recorded). Reads memory only, and never fails: a membership it cannot
    /// use is skipped, with the reason.
    pub fn compose(&self, memberships: &[Membership]) -> Composition {
        let mut out = Composition::default();

        // 1. The memberships it can use, in the composition's order, each
        //    category once, and no kind past its count.
        let mut usable: Vec<(Key<'_>, &Membership)> = Vec::new();
        for m in memberships {
            match self.usable(m) {
                Ok(kind) => usable.push((self.key(kind, &m.category), m)),
                Err(why) => out.skipped.push(Skipped {
                    membership: m.clone(),
                    why: why.to_string(),
                }),
            }
        }
        usable.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| tie(a, b)));
        let mut kept: Vec<&Membership> = Vec::new();
        for (_, m) in usable {
            let kind = &self.kinds[m.kind()];
            let why = if kept.iter().any(|k| k.category == m.category) {
                Some(format!("`{}` is listed more than once", m.category))
            } else if !kind
                .per_session
                .allows(kept.iter().filter(|k| k.kind() == m.kind()).count() + 1)
            {
                Some(format!(
                    "a session holds at most {} `{}` categories",
                    kind.per_session, kind.name
                ))
            } else {
                None
            };
            match why {
                Some(why) => out.skipped.push(Skipped {
                    membership: m.clone(),
                    why,
                }),
                None => kept.push(m),
            }
        }

        // 2. The categories in play: each membership's, and under `chain`
        //    every ancestor of it.
        let mut in_play: BTreeSet<&CategoryId> = BTreeSet::new();
        for m in &kept {
            match self.kinds[m.kind()].rule {
                Rule::Chain => in_play.extend(self.path(&m.category).into_iter().map(|c| &c.id)),
                Rule::IntentLine => {
                    in_play.insert(&m.category);
                }
                // Refused by the table, so never in a valid snapshot.
                Rule::Ranked | Rule::RecallOnly => {}
            }
        }
        let mut order: Vec<(Key<'_>, &CategoryId)> = in_play
            .into_iter()
            .filter_map(|id| Some((self.key(self.kinds.get(id.kind())?, id), id)))
            .collect();
        order.sort();

        // 3. The sections: a block per category under `chain`, and one per
        //    kind under `intent_line`, with a line per category.
        let mut lines: Vec<String> = Vec::new();
        for (i, (key, id)) in order.iter().enumerate() {
            let kind = &self.kinds[id.kind()];
            if let Some(g) = self.guidance.get(*id).filter(|g| !g.is_empty()) {
                let path = key.names.join(PATH_SEP);
                match kind.rule {
                    Rule::Chain => out
                        .sections
                        .push(format!("# Guidance ({} {path})\n\n{}", kind.name, g.text)),
                    Rule::IntentLine => lines.push(format!("- {path}: {}", g.text)),
                    Rule::Ranked | Rule::RecallOnly => continue,
                }
                out.guidance.push(GuidanceUsed {
                    category: g.category.clone(),
                    version: g.version,
                    digest: g.digest.clone(),
                });
            }
            let last_of_kind = order
                .get(i + 1)
                .is_none_or(|(_, next)| next.kind() != id.kind());
            if last_of_kind && !lines.is_empty() {
                out.sections.push(format!(
                    "# Guidance ({})\n\n{}",
                    kind.name,
                    lines.join("\n")
                ));
                lines.clear();
            }
        }
        if !out.sections.is_empty() {
            out.sections.insert(0, PREAMBLE.to_string());
        }

        out.memberships = kept
            .into_iter()
            .map(|m| MembershipUsed {
                kind: m.kind().to_string(),
                category: m.category.clone(),
                origin: m.origin,
                confidence: m.confidence,
                as_of_ms: m.as_of_ms,
            })
            .collect();
        out
    }
}
