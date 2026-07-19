import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'

const json = (value: unknown, status = 200) => Promise.resolve(new Response(status === 204 ? null : JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } }))

function routeFetch(routes: Record<string, unknown>) {
  return vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input); const key = `${init?.method ?? 'GET'} ${path}`
    const value = routes[key] ?? routes[path]
    if (value instanceof Error) return Promise.reject(value)
    if (value === undefined) return json({ error: { code: 'not_found', message: key } }, 404)
    return json(value)
  })
}

beforeEach(() => {
  Object.defineProperty(navigator, 'onLine', { configurable: true, value: true })
  history.replaceState(null, '', '/')
})

describe('owner states', () => {
  it('shows bootstrap required and submits the token', async () => {
    const fetchMock = routeFetch({ '/api/owner/bootstrap/status': { bootstrap_required: true }, 'POST /api/owner/bootstrap': { authenticated: true } }); vi.stubGlobal('fetch', fetchMock)
    render(<App />); expect(await screen.findByRole('heading', { name: 'Bootstrap owner' })).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('Bootstrap token'), 'x'.repeat(32)); await userEvent.click(screen.getByRole('button', { name: 'Create owner session' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/owner/bootstrap', expect.objectContaining({ method: 'POST' })))
  })

  it('shows an expired session state after an owner request returns 401', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => String(input).includes('bootstrap/status') ? json({ bootstrap_required: false }) : json({ error: { code: 'unauthorized', message: 'authentication required' } }, 401)); vi.stubGlobal('fetch', fetchMock)
    render(<App />); expect(await screen.findByRole('heading', { name: 'Owner sign in' })).toBeInTheDocument()
  })
})

describe('authenticated application', () => {
  function authenticated(extra: Record<string, unknown> = {}) {
    return routeFetch({ '/api/owner/bootstrap/status': { bootstrap_required: false }, '/api/owner/session': { authenticated: true }, '/api/rings?limit=100': { rings: [] }, '/api/pairings': { pairings: [] }, '/api/sources': { sources: [] }, '/api/receivers': { receivers: [] }, '/api/settings': { history_retention_days: 14, vapid_public_key: 'test' }, ...extra })
  }

  it('shows empty and populated ring history', async () => {
    vi.stubGlobal('fetch', authenticated()); const first = render(<App />); expect(await screen.findByText(/No rings yet/)).toBeInTheDocument(); first.unmount()
    vi.stubGlobal('fetch', authenticated({ '/api/rings?limit=100': { rings: [{ event_id: 'e', source_id: 's', source_name: 'laptop', message: 'Test message', created_at: '2026-01-01T00:00:00Z', target_tags: ['pc'] }] } })); render(<App />); expect(await screen.findByText('Test message')).toBeInTheDocument()
  })

  it('lists and approves a pending pairing', async () => {
    const fetchMock = authenticated({ '/api/pairings': { pairings: [{ id: 'p', code: 'ABCD-2345', display_name: 'laptop', created_at: '2026-01-01T00:00:00Z', expires_at: '2030-01-01T00:00:00Z' }] }, 'POST /api/pairings/p/approve': {} }); vi.stubGlobal('fetch', fetchMock)
    render(<App />); await userEvent.click(await screen.findByRole('button', { name: 'Sources' })); expect(await screen.findByText('ABCD-2345')).toBeInTheDocument(); await userEvent.click(screen.getByRole('button', { name: 'Approve' })); await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/pairings/p/approve', expect.objectContaining({ method: 'POST' })))
  })

  it('shows receiver registration and default notification permission without prompting', async () => {
    const requestPermission = vi.fn(); vi.stubGlobal('Notification', { permission: 'default', requestPermission }); vi.stubGlobal('fetch', authenticated())
    render(<App />); await userEvent.click(await screen.findByRole('button', { name: 'Receivers' })); expect(await screen.findByText('Notification permission has not been requested.')).toBeInTheDocument(); expect(requestPermission).not.toHaveBeenCalled()
  })

  it.each(['granted', 'denied'] as const)('renders the %s notification permission state', async (permission) => {
    vi.stubGlobal('Notification', { permission, requestPermission: vi.fn() }); vi.stubGlobal('fetch', authenticated()); render(<App />); await userEvent.click(await screen.findByRole('button', { name: 'Receivers' })); expect(await screen.findByText(new RegExp(`permission is ${permission}`))).toBeInTheDocument()
  })

  it('registers a Push receiver after the explicit button press', async () => {
    const subscribe = vi.fn().mockResolvedValue({ toJSON: () => ({ endpoint: 'https://push.example/browser', keys: { p256dh: 'key', auth: 'auth' } }) })
    const originalWorker = navigator.serviceWorker
    Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { ready: Promise.resolve({ pushManager: { subscribe } }) } })
    vi.stubGlobal('PushManager', class {})
    vi.stubGlobal('Notification', { permission: 'granted', requestPermission: vi.fn().mockResolvedValue('granted') })
    const fetchMock = authenticated({ '/api/settings': { history_retention_days: 14, vapid_public_key: 'AQID' }, 'POST /api/receivers': { id: 'r', name: 'This browser', tags: [], enabled: true, created_at: '2026-01-01T00:00:00Z' } }); vi.stubGlobal('fetch', fetchMock)
    render(<App />); await userEvent.click(await screen.findByRole('button', { name: 'Receivers' })); await userEvent.click(await screen.findByRole('button', { name: 'Register receiver' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/receivers', expect.objectContaining({ method: 'POST' })))
    expect(subscribe).toHaveBeenCalledWith(expect.objectContaining({ userVisibleOnly: true }))
    Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: originalWorker })
  })

  it('shows the offline state', async () => {
    Object.defineProperty(navigator, 'onLine', { configurable: true, value: false }); vi.stubGlobal('fetch', authenticated()); render(<App />); expect(await screen.findByText(/Offline/)).toBeInTheDocument()
  })

  it('shows an API error state', async () => {
    vi.stubGlobal('fetch', authenticated({ '/api/rings?limit=100': new Error('relay unavailable') })); render(<App />); expect(await screen.findByRole('alert')).toHaveTextContent('relay is unreachable')
  })
})
