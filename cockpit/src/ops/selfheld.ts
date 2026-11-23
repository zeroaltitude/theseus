// The self loop held on its slots for hours (issue 41): `health.self_held`, said on the Ops page with the trees that
// hold the slots and the one act that clears each. None while the page shows the past, or with nothing held.

/** The line the page shows, or none. */
export function selfHeldLine(health: { self_held?: string } | undefined, past: boolean): string | null {
  const said = health?.self_held?.trim()
  if (past || !said) return null
  return said.charAt(0).toUpperCase() + said.slice(1)
}
