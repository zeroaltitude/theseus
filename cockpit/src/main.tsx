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
import { Shell } from './components/Shell'
import { Crash, NotFound } from './components/Crash'

// Every view loads on first visit. The Ship (three.js) is the landing view; the others never pay for its bundle.
const Ship = lazy(() => import('./views/Ship'))
const Bridge = lazy(() => import('./views/Bridge').then((m) => ({ default: m.Bridge })))
const Fleet = lazy(() => import('./views/Fleet'))
const SessionDeck = lazy(() => import('./views/SessionDeck'))
const Actions = lazy(() => import('./views/Actions'))
const Ledger = lazy(() => import('./views/Ledger'))
const Economics = lazy(() => import('./views/Economics'))
const Systems = lazy(() => import('./views/Systems'))

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
            { path: '*', element: <NotFound /> },
          ],
        },
      ],
    },
  ],
  { basename: '/cockpit' },
)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queries}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
)
