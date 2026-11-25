
impl Ontology {

    /// The records that merge `absorbed` into `survivor`, in the
    /// order they apply: every session's person list that holds `absorbed`
    /// with it replaced by `survivor` (once), the survivor with the other's
    /// handles and its guidance when it has none, and `absorbed` written as
    /// merged. Refused when either is not a held person, when they are one,
    /// when the transport made `absorbed` (its DM reads its id: keep it, and
    /// merge the other into it), or when both have guidance.
    pub fn merge(&self, absorbed: &CategoryId, survivor: &CategoryId) -> Result<Merge, Refusal> {
        let held = |id: &CategoryId| {
            self.category(id)
                .filter(|c| c.kind() == KIND)
                .ok_or_else(|| Refusal::Missing {
                    what: "person",
                    id: id.to_string(),
                })
        };
        let (gone, kept) = (held(absorbed)?, held(survivor)?);
        if gone.id == kept.id {
            return Err(Refusal::Duplicate {
                why: format!("`{}` is one person: a merge takes two", gone.id),
            });
        }
        if gone.added_by == Origin::Transport.name() {
            return Err(Refusal::Given {
                kind: KIND.into(),
                why: format!(
                    "`{}` is a DM's person, from the transport, which reads it by its id: keep it, \
                     and merge `{}` into it instead",
                    gone.id, kept.id
                ),
            });
        }
        let guide = |id: &CategoryId| self.guidance(id).filter(|g| !g.is_empty());
        let moved_guidance = match (guide(&gone.id), guide(&kept.id)) {
            (Some(_), Some(_)) => {
                return Err(Refusal::InUse {
                    id: gone.id.to_string(),
                    by: format!(
                        "its guidance, and `{}` has its own: clear one, then merge",
                        kept.id
                    ),
                })
            }
            (Some(g), None) => Some(g.text.clone()),
            _ => None,
        };
        let mut records = Vec::new();
        let mut sessions = Vec::new();
        let mut lists_before = Vec::new();
        for l in self.lists().filter(|l| l.kind == KIND) {
            if !l.members.iter().any(|m| m.category == gone.id) {
                continue;
            }
            let mut members: Vec<Membership> = Vec::with_capacity(l.members.len());
            for m in &l.members {
                let mut m = m.clone();
                if m.category == gone.id {
                    m.category = kept.id.clone();
                }
                if !members.iter().any(|o| o.category == m.category) {
                    members.push(m);
                }
            }
            sessions.push(l.session.clone());
            lists_before.push(MemberList::clone(&l));
            records.push(Record::Members(MemberList {
                session: l.session.clone(),
                kind: KIND.into(),
                members,
            }));
        }
        let before = kept.handles.clone();
        let mut next = kept.clone();
        for h in handles_of(gone) {
            if !handles_of(&next).contains(&h) {
                next.handles.push(h);
            }
        }
        if !next.handles.contains(&format!("name:{}", gone.name)) && gone.name != kept.name {
            if let Ok(h) = handle(&format!("name:{}", gone.name)) {
                next.handles.push(h);
            }
        }
        check_handles(&next)?;
        // Its own handles leave it before the survivor takes them, so the
        // two never hold one at once.
        let mut taken = gone.clone();
        taken.handles.clear();
        taken.merged_into = Some(kept.id.clone());
        if let Some(text) = &moved_guidance {
            let g = Guidance::new(
                kept.id.clone(),
                text,
                self.guidance(&kept.id).map_or(1, |g| g.version + 1),
                "merge",
            );
            records.push(Record::Guidance(g));
        }
        records.push(Record::Category(taken));
        records.push(Record::Category(next));
        Ok(Merge {
            absorbed: gone.clone(),
            survivor_handles_before: before,
            sessions,
            lists_before,
            guidance: moved_guidance,
            records,
        })
    }
}
