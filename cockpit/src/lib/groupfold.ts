// The Ontology view's kind groups, open and folded (theseus-5kd37). The folds are the reader's: a filter shows every group
// that holds a match whole while it is on, and leaves the reader's own set as it was, so a fold button does nothing then
// (the view disables it) and clearing the filter brings the groups back as the reader left them. Pure, with no import, so
// `node --test` runs its test (`cockpit/test/groupfold.test.ts`) as it is.

/** The groups shown open: the reader's own (`groupsOpen`), the selected category's group, and, while filtering, every
 *  group (`allKeys`). */
export function openGroups(groupsOpen: ReadonlySet<string>, selectedKey: string | null, filtering: boolean, allKeys: readonly string[]): Set<string> {
  const s = new Set(groupsOpen)
  if (selectedKey) s.add(selectedKey)
  if (filtering) for (const k of allKeys) s.add(k)
  return s
}

/** The reader's own open set after a fold button on `key`: the same set while filtering, else `key` toggled in it (never in
 *  what the selection or the filter forces open). */
export function toggled(groupsOpen: Set<string>, key: string, filtering: boolean): Set<string> {
  if (filtering) return groupsOpen
  const s = new Set(groupsOpen)
  if (s.has(key)) s.delete(key)
  else s.add(key)
  return s
}
