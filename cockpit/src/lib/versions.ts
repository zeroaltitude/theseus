// A pack's versions as the cockpit shows them (M5 25f; design §2.17): each lineage (its compiled-in root and the
// versions the learning loop wrote from it), and the line diff between any two. Pure, so `npm test` runs it.

type Version = { pack: string; source: string; root?: string | null; parent?: string | null; text: string }

/** A version's number: `classify.v101` is 101. */
export function versionOf(pack: string): number {
  const m = /\.v(\d+)$/.exec(pack)
  return m ? Number(m[1]) : 0
}

/** Each lineage with a learned version: its root first, then its learned versions, oldest first. */
export function lineages<V extends Version>(packs: V[]): { root: string; versions: V[] }[] {
  const learned = packs.filter((p) => p.source === 'learned' && p.root)
  const roots = [...new Set(learned.map((p) => p.root as string))].sort()
  return roots.map((root) => {
    const own = learned.filter((p) => p.root === root).sort((a, b) => versionOf(a.pack) - versionOf(b.pack))
    const head = packs.find((p) => p.pack === root)
    return { root, versions: head ? [head, ...own] : own }
  })
}

export type DiffLine = { op: ' ' | '-' | '+'; line: string }

/** The line diff of `a` to `b` (a longest common subsequence of their lines), comments and blank lines aside. */
export function lineDiff(a: string, b: string): DiffLine[] {
  const lines = (t: string) => t.split('\n').map((l) => l.trimEnd()).filter((l) => l.trim() !== '' && !l.trim().startsWith('#'))
  const x = lines(a)
  const y = lines(b)
  const n = x.length
  const m = y.length
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = x[i] === y[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1])
    }
  }
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      out.push({ op: ' ', line: x[i] })
      i++
      j++
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ op: '-', line: x[i++] })
    } else {
      out.push({ op: '+', line: y[j++] })
    }
  }
  while (i < n) out.push({ op: '-', line: x[i++] })
  while (j < m) out.push({ op: '+', line: y[j++] })
  return out
}

/** Only the changed lines, with `context` unchanged lines around each change. */
export function changed(d: DiffLine[], context = 1): DiffLine[] {
  const keep = d.map((l, k) => l.op !== ' ' || d.slice(Math.max(0, k - context), k + context + 1).some((x) => x.op !== ' '))
  return d.filter((_, k) => keep[k])
}
