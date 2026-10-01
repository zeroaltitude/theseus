import { StrictMode, Suspense, lazy } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { createBrowserRouter, RouterProvider } from 'react-router'
import '@fontsource-variable/inter'
import '@fontsource-variable/jetbrains-mono'
import './index.css'
import './lib/rpc'
import { Shell } from './components/Shell'
import { Bridge } from './views/Bridge'
import { Crash, NotFound } from './components/Crash'

// Heavier views load on first visit, so the bridge paints fast.
const Fleet = lazy(() => import('./views/Fleet'))
const SessionDeck = lazy(() => import('./views/SessionDeck'))
const Actions = lazy(() => import('./views/Actions'))
const Ledger = lazy(() => import('./views/Ledger'))
const Economics = lazy(() => import('./views/Economics'))
const Systems = lazy(() => import('./views/Systems'))

const queries = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false, placeholderData: (prev: unknown) => prev } },
})

const wrap = (el: React.ReactNode) => <Suspense fallback={<div className="p-6 text-ink-faint">Loading…</div>}>{el}</Suspense>

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
            { index: true, element: <Bridge /> },
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
