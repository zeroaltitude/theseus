// The published runs as this build embeds them (theseus-raf4): `docs/benchmarks/`, each report's markdown, its data
// file and its per-trial CSV, read by Vite at build time into chunks of their own, so the Benchmarks view loads only the
// files it shows; and every figure's SVG, light and dark, as an asset beside the page. The daemon serves them as it
// serves the rest of the cockpit (its build is embedded in `theseusd`): no protocol method, no read of the daemon's,
// nothing written. A new report shows after the next install.
import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { reportName, setsFrom, TASK_SETS } from '@/lib/bench'
import { datasetsUi, readManifest } from '@/lib/benchindex'
import { frontierSets } from '@/lib/benchslices'
import { readCatalogue } from '@/lib/benchtasks'

type Load = () => Promise<string>

const MD = import.meta.glob<string>('../../../docs/benchmarks/*.md', { query: '?raw', import: 'default' })
const JSONS = import.meta.glob<string>('../../../docs/benchmarks/*.json', { query: '?raw', import: 'default' })
const CSVS = import.meta.glob<string>('../../../docs/benchmarks/*.csv', { query: '?raw', import: 'default' })
const SVGS = import.meta.glob<string>('../../../docs/benchmarks/img/*/*.svg', { query: '?url', import: 'default', eager: true })

const byName = (g: Record<string, Load>) => new Map(Object.entries(g).map(([p, load]) => [reportName(p), load]))
const FILES = { md: byName(MD), json: byName(JSONS), csv: byName(CSVS) }
export type Ext = keyof typeof FILES

/** Every report's name, the README aside. */
export const REPORTS: readonly string[] = [...FILES.md.keys()].filter((n) => n !== 'README').sort()

export const has = (name: string, ext: Ext): boolean => FILES[ext].has(name)

/** A report's file, or undefined when the build has none (a report with no CSV). */
export function loadFile(name: string, ext: Ext): Promise<string | undefined> {
  const load = FILES[ext].get(name)
  return load ? load() : Promise.resolve(undefined)
}

/** A file, read once: the build's files never change under a page. */
export function useBenchFile(name: string | undefined, ext: Ext) {
  return useQuery({
    queryKey: ['bench', name, ext], enabled: !!name, staleTime: Infinity, gcTime: Infinity,
    queryFn: async () => (name ? (await loadFile(name, ext)) ?? null : null),
  })
}

/** Several reports' files at once (the frontier's task sets, the runs' titles). */
export function useBenchFiles(names: readonly string[], ext: Ext) {
  return useQuery({
    queryKey: ['bench', 'many', ext, names.join(',')], staleTime: Infinity, gcTime: Infinity,
    queryFn: async () => new Map(await Promise.all(names.map(async (n) => [n, (await loadFile(n, ext)) ?? ''] as const))),
  })
}

/** A figure's URL in the build, from its path in a report (`img/<report>/<figure>.svg`). */
export function figureUrl(path: string): string | undefined {
  return SVGS[`../../../docs/benchmarks/${path.replace(/^\.\//, '')}`]
}

/** The benchmark manifest, `docs/benchmarks/index.json` (theseus-jda0z): the JSON glob above embeds it as the report named `index`. */
export function useBenchManifest() {
  const file = useBenchFile('index', 'json')
  return { loading: file.isPending, manifest: useMemo(() => readManifest(file.data), [file.data]) }
}

/** The task sets the page offers: the manifest's slices (theseus-s5qwv), each absorbing the hand-written `ui` block of
 *  a report it spans, then any `ui` block no slice holds (theseus-tyv41), then the built-ins. The blocks come from the
 *  manifest's `dataset_ui` (theseus-2w02d), so the page no longer fetches every dataset to read two keys; a dataset the
 *  manifest does not list is still read in full (lazily, once) and its block taken. Until they are read the built-ins
 *  stand alone. */
export function useBenchSets() {
  // Newest first, because a dataset's name opens with its date and `setsFrom` promises that order.
  const names = useMemo(() => [...FILES.json.keys()].sort().reverse(), [])
  const { manifest } = useBenchManifest()
  const q = useQuery({
    queryKey: ['bench', 'sets'], staleTime: Infinity, gcTime: Infinity,
    queryFn: async () => setsFrom(await datasetsUi(names, await loadFile('index', 'json').catch(() => undefined), (n) => loadFile(n, 'json'))),
  })
  const sets = useMemo(() => (q.data ? frontierSets(manifest.rows, q.data, TASK_SETS) : TASK_SETS), [q.data, manifest.rows])
  return { sets, loading: q.isPending }
}

/** The task catalogue, `docs/benchmarks/tasks.json` (theseus-hbh4m): written locally by `bench/report/tasks.py`, so a build may not hold it; then there are no descriptions and the page is whole without them. */
export function useTaskCatalogue() {
  const file = useBenchFile('tasks', 'json')
  return useMemo(() => readCatalogue(file.data), [file.data])
}
