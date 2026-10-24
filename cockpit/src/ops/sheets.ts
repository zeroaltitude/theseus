// What the thread sheets check (theseus-jid7t.6), pure: a slug from a title, and whether a slug is one the daemon takes
// (letters, digits and dashes). They replace `window.prompt`.
export const slugOf = (title: string): string => title.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')
export const slugBad = (slug: string): boolean => !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(slug.trim())

/** An edit's params: only the fields that changed. Null when nothing did. */
export function editParams(slug: string, was: { title: string; goal?: string | null }, now: { title: string; goal: string }): Record<string, unknown> | null {
  const title = now.title.trim() !== was.title ? { title: now.title.trim() } : {}
  const goal = now.goal !== (was.goal ?? '') ? { goal: now.goal } : {}
  return Object.keys(title).length || Object.keys(goal).length ? { slug, ...title, ...goal } : null
}
