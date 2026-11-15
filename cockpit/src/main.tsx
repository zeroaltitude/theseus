import { StrictMode, Suspense, lazy } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createBrowserRouter, Navigate, RouterProvider, useLocation } from 'react-router'
import '@fontsource-variable/inter'
import '@fontsource-variable/jetbrains-mono'
import '@fontsource-variable/cinzel'
import './index.css'
import { bindPush } from './lib/rpc'
import './lib/calm'
import { useMode } from './lib/mode'
import { Shell } from './components/Shell'
import { SimpleShell } from './simple/Shell'
import { Crash, NotFound } from './components/Crash'
import { redirectOf } from './ops/address'

// Every view loads on first visit. The simple pages (`src/simple/`) are the front door: `/` is Ops. The Ship
// (three.js) and its instrument pages are one link away, in their own frame; no other view pays for its bundle.
const loadShip = () => import('./views/Ship')
const Ship = lazy(loadShip)
// Arriving at the Ship, its chunk starts loading at once, beside the app's own start.
if (/^\/ship\/?$/.test(window.location.pathname)) void loadShip()
const SimpleOps = lazy(() => import('./simple/Ops'))
const SimpleBench = lazy(() => import('./simple/Bench'))
const SimpleTelemetry = lazy(() => import('./simple/Telemetry'))
const SimpleConfig = lazy(() => import('./simple/Config'))
const SimpleMemories = lazy(() => import('./simple/Memories'))
// A session's own pages sit under the simple bar too: the whole session, one turn of it, a label taken off it.
const SimpleSession = lazy(() => import('./simple/memories/Session'))
const Bridge = lazy(() => import('./views/Bridge').then((m) => ({ default: m.Bridge })))
const Fleet = lazy(() => import('./views/Fleet'))
const Actions = lazy(() => import('./views/Actions'))
const Ledger = lazy(() => import('./views/Ledger'))
const Economics = lazy(() => import('./views/Economics'))
const Systems = lazy(() => import('./views/Systems'))
// Round two: the money river, the boundaries board, and the speed wall, each in its own chunk.
const Money = lazy(() => import('./views/Money'))
const Boundaries = lazy(() => import('./views/Boundaries'))
const Policy = lazy(() => import('./views/Policy'))
const Speed = lazy(() => import('./views/Speed'))
const Ontology = lazy(() => import('./views/Ontology'))
// Jev's judgments (M5 23b).
const Judgment = lazy(() => import('./views/Judgment'))
// The published benchmark runs and the frontier of measured harnesses (theseus-raf4), and each run's report.
const Benchmarks = lazy(() => import('./views/Benchmarks'))
const BenchRun = lazy(() => import('./views/BenchRun'))

const queries = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false, placeholderData: (prev: unknown) => prev } },
})
bindPush(queries)

/** The Agenda's, Tasks' and Threads' old addresses: their Operations lens, the query kept (the owner's decision 1). */
function ToOps() {
  const l = useLocation()
  return <Navigate to={redirectOf(l.pathname, l.search) ?? '/ops'} replace />
}

const wrap = (el: React.ReactNode) => <Suspense fallback={<div className="p-6 text-ink-faint">Loading…</div>}>{el}</Suspense>
const wrapShip = (el: React.ReactNode) => <Suspense fallback={<div className="ship-root h-full w-full" />}>{el}</Suspense>

const router = createBrowserRouter(
  [
    {
      // The simple pages: the five sections under one bar. Each is its own chunk (the frame's Suspense waits for it).
      path: '/',
      element: <SimpleShell />,
      children: [
        {
          errorElement: <Crash />,
          children: [
            { index: true, element: <SimpleOps /> },
            { path: 'bench', element: <SimpleBench /> },
            { path: 'telemetry', element: <SimpleTelemetry /> },
            { path: 'config', element: <SimpleConfig /> },
            { path: 'memories', element: <SimpleMemories /> },
            { path: 'session/:id', element: <SimpleSession view="deck" /> },
            // One turn, whole (theseus-x9swu): the flame graph, each loop's request and answer, each tool call.
            { path: 'session/:id/turn/:turn', element: <SimpleSession view="turn" /> },
            // Taking one label off one session (theseus-emqbm): the address every reply's footer links, one per label.
            { path: 'session/:id/unlabel/:flag', element: <SimpleSession view="unlabel" /> },
          ],
        },
      ],
    },
    {
      // The Ship and its instrument pages, in the brass frame ("Creative takes" on the simple pages' bar).
      element: <Shell />,
      children: [
        {
          // One boundary for every view: a view that throws shows what broke, and the shell keeps running.
          errorElement: <Crash />,
          children: [
            { path: 'ship', element: wrapShip(<Ship />) },
            { path: 'bridge', element: wrap(<Bridge />) },
            { path: 'fleet', element: wrap(<Fleet />) },
            // Operations (theseus-jid7t.3): the needs-you queue, the lenses and the inspector. The route loads its chunk.
            { path: 'ops', lazy: async () => ({ Component: (await import('./views/Ops')).default }) },
            // The threads on the work board (theseus-5i8i) are the Threads lens now; one thread's view stays a page.
            { path: 'threads', element: <ToOps /> },
            { path: 'threads/:slug', lazy: async () => ({ Component: (await import('./views/Threads')).ThreadView }) },
            // The Tasks and Agenda pages are the Board and Timeline lenses now (theseus-jid7t.3).
            { path: 'tasks', element: <ToOps /> },
            { path: 'agenda', element: <ToOps /> },
            { path: 'actions', element: wrap(<Actions />) },
            { path: 'ledger', element: wrap(<Ledger />) },
            { path: 'economics', element: wrap(<Economics />) },
            { path: 'systems', element: wrap(<Systems />) },
            { path: 'money', element: wrap(<Money />) },
            { path: 'boundaries', element: wrap(<Boundaries />) },
            { path: 'policy', element: wrap(<Policy />) },
            { path: 'speed', element: wrap(<Speed />) },
            { path: 'ontology', element: wrap(<Ontology />) },
            // The context explorer (theseus-7n3e): what Theseus knows and what a turn sees. The route loads its chunk.
            { path: 'context', lazy: async () => ({ Component: (await import('./views/Context')).default }) },
            // The books, first cut (theseus-civ0): the imported episodes by their book. The route loads its chunk.
            { path: 'books', lazy: async () => ({ Component: (await import('./views/Books')).default }) },
            { path: 'judgment', element: wrap(<Judgment />) },
            { path: 'benchmarks', element: wrap(<Benchmarks />) },
            { path: 'benchmarks/:run', element: wrap(<BenchRun />) },
            { path: '*', element: <NotFound /> },
          ],
        },
      ],
    },
  ],
)

/** Night or daylight (lib/mode.ts): a change of mode draws every view again from the start, so each inline tone and
 *  chart takes the mode's colours (the reads stay in the query cache, the place in the address). */
function Cockpit() {
  const mode = useMode((s) => s.mode)
  return <RouterProvider key={mode} router={router} />
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queries}>
      <Cockpit />
    </QueryClientProvider>
  </StrictMode>,
)
