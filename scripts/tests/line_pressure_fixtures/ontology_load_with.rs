
impl Ontology {

    fn load_with(
        records: impl IntoIterator<Item = Record>,
        source: Option<Arc<dyn MemberSource>>,
    ) -> (Ontology, Vec<(String, Refusal)>) {
        let mut onto = Ontology::seeded();
        if let Some(source) = source {
            onto.members = Members::lean(source);
        }
        let mut dropped = Vec::new();
        let (mut kinds, mut cats, mut guides, mut lists) = (vec![], vec![], vec![], vec![]);
        for r in records {
            match r {
                Record::Kind(k) => kinds.push(k),
                Record::Category(c) => cats.push(c),
                Record::Guidance(g) => guides.push(g),
                Record::Members(l) => lists.push(l),
            }
        }

        kinds.sort_by(|a, b| a.name.cmp(&b.name));
        let mut pending: Vec<Kind> = Vec::new();
        for k in kinds {
            match k.check_row() {
                Ok(()) => pending.push(k),
                Err(e) => dropped.push((keys::kind(&k.name), e)),
            }
        }
        passes(&mut pending, |k| {
            let mut next = onto.kinds.clone();
            next.insert(k.name.clone(), k.clone());
            check_table(&next)?;
            onto.kinds = next;
            Ok(())
        });
        for k in pending {
            let mut next = onto.kinds.clone();
            next.insert(k.name.clone(), k.clone());
            if let Err(e) = check_table(&next) {
                dropped.push((keys::kind(&k.name), e));
            }
        }

        // A category taken away (retired or merged) is none: it is neither
        // held nor dropped, and a merged one's old guidance is left unread.
        let gone = take_gone(&mut cats);
        cats.sort_by(|a, b| a.id.cmp(&b.id));
        let mut pending = cats;
        passes(&mut pending, |c| {
            onto.check_category(c)?;
            onto.insert_category(c.clone());
            Ok(())
        });
        let stuck: BTreeMap<CategoryId, Category> =
            pending.iter().map(|c| (c.id.clone(), c.clone())).collect();
        for c in pending {
            if let Some(e) = stuck_loop(&stuck, &c.id)
                .map(|path| Refusal::Cycle { path })
                .or_else(|| onto.check_category(&c).err())
            {
                dropped.push((keys::category(&c.id), e));
            }
        }

        for g in guides {
            if gone.contains(&g.category) && !onto.categories.contains_key(&g.category) {
                continue;
            }
            match onto.check_guidance(&g) {
                Ok(()) => {
                    onto.guidance.insert(g.category.clone(), g);
                }
                Err(e) => dropped.push((keys::guidance(&g.category), e)),
            }
        }

        onto.load_lists(lists, &mut dropped);
        debug_assert_eq!(onto.check_all(), Ok(()));
        (onto, dropped)
    }
}
