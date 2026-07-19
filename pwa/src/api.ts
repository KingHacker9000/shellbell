import type { Pairing, Receiver, Ring, Settings, Source } from './types'

export class ApiClientError extends Error {
  constructor(public status: number, public code: string, message: string) { super(message) }
}

function csrfToken() {
  return document.cookie.split(';').map((value) => value.trim()).find((value) => value.startsWith('sb_csrf='))?.slice('sb_csrf='.length)
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const method = init.method ?? 'GET'
  const headers = new Headers(init.headers)
  if (init.body) headers.set('content-type', 'application/json')
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method)) {
    const csrf = csrfToken()
    if (csrf) headers.set('x-csrf-token', csrf)
  }
  let response: Response
  try { response = await fetch(path, { ...init, method, headers, credentials: 'include' }) }
  catch { throw new ApiClientError(0, 'offline', 'The relay is unreachable. Check your connection.') }
  if (!response.ok) {
    const data = await response.json().catch(() => null) as { error?: { code?: string; message?: string } } | null
    throw new ApiClientError(response.status, data?.error?.code ?? 'http_error', data?.error?.message ?? `Request failed (${response.status})`)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export const api = {
  bootstrapStatus: () => request<{ bootstrap_required: boolean }>('/api/owner/bootstrap/status'),
  bootstrap: (bootstrap_token: string) => request('/api/owner/bootstrap', { method: 'POST', body: JSON.stringify({ bootstrap_token }) }),
  login: (bootstrap_token: string) => request('/api/owner/session', { method: 'POST', body: JSON.stringify({ bootstrap_token }) }),
  session: () => request('/api/owner/session'),
  logout: () => request('/api/owner/logout', { method: 'POST' }),
  pairings: () => request<{ pairings: Pairing[] }>('/api/pairings'),
  approvePairing: (id: string) => request(`/api/pairings/${id}/approve`, { method: 'POST', body: '{}' }),
  rejectPairing: (id: string) => request(`/api/pairings/${id}/reject`, { method: 'POST', body: '{}' }),
  rings: () => request<{ rings: Ring[] }>('/api/rings?limit=100'),
  sources: () => request<{ sources: Source[] }>('/api/sources'),
  renameSource: (id: string, name: string) => request(`/api/sources/${id}`, { method: 'PATCH', body: JSON.stringify({ name }) }),
  revokeSource: (id: string) => request(`/api/sources/${id}`, { method: 'DELETE' }),
  receivers: () => request<{ receivers: Receiver[] }>('/api/receivers'),
  registerReceiver: (body: object) => request<Receiver>('/api/receivers', { method: 'POST', body: JSON.stringify(body) }),
  updateReceiver: (id: string, body: object) => request<Receiver>(`/api/receivers/${id}`, { method: 'PATCH', body: JSON.stringify(body) }),
  revokeReceiver: (id: string) => request(`/api/receivers/${id}`, { method: 'DELETE' }),
  settings: () => request<Settings>('/api/settings'),
  updateSettings: (history_retention_days: number) => request<Settings>('/api/settings', { method: 'PUT', body: JSON.stringify({ history_retention_days }) }),
}
