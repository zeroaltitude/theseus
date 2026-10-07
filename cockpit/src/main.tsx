import { StrictMode, Suspense, lazy } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'
import '@fontsource-variable/inter'
import '@fontsource-variable/jetbrains-mono'
import '@fontsource-variable/cinzel'
import './index.css'
import { bindPush } from './lib/rpc'
import './lib/calm'
import { useMode } from './lib/mode'
import { Shell } from './components/Shell'
import { Crash, NotFound } from './components/Crash'

// Every view loads on first visit. The Ship (three.js) is the landing view; the others never pay for its bundle.
const loadShip = () => import('./views/Ship')
const Ship = lazy(loadShip)
// The landing view: its chunk starts loading at once, beside the app's own start, not after the router's redirect.
if (/^\/(ship\/?)?$/.test(window.location.pathname)) void loadShip()
const Bridge = lazy(() => import('./views/Bridge').then((m) => ({ default: m.Bridge })))
const Fleet = lazy(() => import('./views/Fleet'))
const SessionDeck = lazy(() => import('./views/SessionDeck'))
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

const wrap = (el: React.ReactNode) => <Suspense fallback={<div className="p-6 text-ink-faint">Loading…</div>}>{el}</Suspense>
const wrapShip = (el: React.ReactNode) => <Suspense fallback={<div className="ship-root h-full w-full" />}>{el}</Suspense>

const router = createBrowserRouter(
  [
    {
      path: '/',
      element: <Shell />,
      children: [
        {
          // One boundary for every view: a view that throws shows what broke, and the shell keeps running.
          errorElement: <Crash />,
          children: [
            { index: true, element: <Navigate to="/ship" replace /> },
            { path: 'ship', element: wrapShip(<Ship />) },
            { path: 'bridge', element: wrap(<Bridge />) },
            { path: 'fleet', element: wrap(<Fleet />) },
            { path: 'session/:id', element: wrap(<SessionDeck />) },
            { path: 'actions', element: wrap(<Actions />) },
            { path: 'ledger', element: wrap(<Ledger />) },
            { path: 'economics', element: wrap(<Economics />) },
            { path: 'systems', element: wrap(<Systems />) },
            { path: 'money', element: wrap(<Money />) },
            { path: 'boundaries', element: wrap(<Boundaries />) },
            { path: 'policy', element: wrap(<Policy />) },
            { path: 'speed', element: wrap(<Speed />) },
            { path: 'ontology', element: wrap(<Ontology />) },
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
