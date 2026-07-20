import { FormEvent, useCallback, useEffect, useState } from 'react'
import { ApiClientError, api } from './api'
import type { NotificationState, Pairing, Receiver, Ring, Source } from './types'

type View = 'rings' | 'sources' | 'receivers' | 'settings'
type AuthState = 'loading' | 'bootstrap' | 'login' | 'authenticated' | 'expired'

export default function App() {
  const initialView = new URLSearchParams(location.search).get('view') as View | null
  const [auth, setAuth] = useState<AuthState>('loading')
  const [view, setView] = useState<View>(['rings', 'sources', 'receivers', 'settings'].includes(initialView ?? '') ? initialView! : 'rings')
  const [offline, setOffline] = useState(!navigator.onLine)
  const [error, setError] = useState('')

  const handleError = useCallback((value: unknown) => {
    if (value instanceof ApiClientError && value.status === 401) { setAuth('expired'); return }
    setError(value instanceof Error ? value.message : 'Something went wrong')
  }, [])

  useEffect(() => {
    const online = () => setOffline(false); const offlineEvent = () => setOffline(true)
    window.addEventListener('online', online); window.addEventListener('offline', offlineEvent)
    return () => { window.removeEventListener('online', online); window.removeEventListener('offline', offlineEvent) }
  }, [])
  useEffect(() => { api.bootstrapStatus().then(({ bootstrap_required }) => { if (bootstrap_required) setAuth('bootstrap'); else api.session().then(() => setAuth('authenticated')).catch(() => setAuth('login')) }).catch(handleError) }, [handleError])

  function navigate(next: View) { setView(next); history.replaceState(null, '', `/?view=${next}`) }

  if (auth === 'loading') return <Centered title="Loading Shellbell…" detail="Connecting to the private relay." />
  if (auth === 'bootstrap') return <TokenScreen title="Bootstrap owner" detail="Enter the one-time owner bootstrap token from the relay data directory." button="Create owner session" onSubmit={(token) => api.bootstrap(token).then(() => setAuth('authenticated')).catch(handleError)} error={error} />
  if (auth === 'login' || auth === 'expired') return <TokenScreen title={auth === 'expired' ? 'Owner session expired' : 'Owner sign in'} detail="Enter the owner bootstrap token to create a new revocable session." button="Sign in" onSubmit={(token) => api.login(token).then(() => setAuth('authenticated')).catch(handleError)} error={error} />

  return <div className="app">
    <header><div><span className="brand-mark">›_</span><strong>Shellbell</strong></div><span className="privacy">Private terminal notifications</span></header>
    {offline && <div className="banner" role="status">Offline — showing the current view; changes will not work.</div>}
    {error && <div className="banner error" role="alert">{error}<button onClick={() => setError('')}>Dismiss</button></div>}
    <main>{view === 'rings' ? <Rings onError={handleError} /> : view === 'sources' ? <Sources onError={handleError} /> : view === 'receivers' ? <Receivers onError={handleError} /> : <SettingsView onError={handleError} onLogout={() => api.logout().then(() => setAuth('login')).catch(handleError)} />}</main>
    <nav aria-label="Main navigation">{(['rings', 'sources', 'receivers', 'settings'] as View[]).map((item) => <button className={view === item ? 'active' : ''} key={item} onClick={() => navigate(item)}>{item[0].toUpperCase() + item.slice(1)}</button>)}</nav>
  </div>
}

function Centered({ title, detail }: { title: string; detail: string }) { return <div className="center"><div className="card"><h1>{title}</h1><p>{detail}</p></div></div> }

function TokenScreen({ title, detail, button, onSubmit, error }: { title: string; detail: string; button: string; onSubmit: (token: string) => void; error: string }) {
  const [token, setToken] = useState(''); const submit = (event: FormEvent) => { event.preventDefault(); onSubmit(token); setToken('') }
  return <div className="center"><form className="card auth" onSubmit={submit}><span className="eyebrow">Private owner access</span><h1>{title}</h1><p>{detail}</p>{error && <div className="inline-error" role="alert">{error}</div>}<label>Bootstrap token<input type="password" autoComplete="current-password" required minLength={32} value={token} onChange={(event) => setToken(event.target.value)} /></label><button type="submit">{button}</button><small>The token stays in this request and is never written to browser storage.</small></form></div>
}

function Section({ title, detail, children }: { title: string; detail: string; children: React.ReactNode }) { return <section><div className="section-title"><div><h1>{title}</h1><p>{detail}</p></div></div>{children}</section> }
function Empty({ children }: { children: React.ReactNode }) { return <div className="empty">{children}</div> }
function Loading() { return <div className="empty">Loading…</div> }

function useLoad<T>(loader: () => Promise<T>, onError: (error: unknown) => void, interval?: number) {
  const [data, setData] = useState<T | null>(null)
  const refresh = useCallback(() => loader().then(setData).catch(onError), [loader, onError])
  useEffect(() => { refresh(); if (!interval) return; const timer = window.setInterval(refresh, interval); return () => clearInterval(timer) }, [refresh, interval])
  return { data, refresh }
}

function Rings({ onError }: { onError: (error: unknown) => void }) {
  const loader = useCallback(() => api.rings(), []); const { data } = useLoad(loader, onError, 15000)
  return <Section title="Recent rings" detail="Only notification messages and privacy-safe routing metadata are retained.">{!data ? <Loading /> : data.rings.length === 0 ? <Empty>No rings yet. Pair a source, then run <code>shellbell ring</code>.</Empty> : <div className="list">{data.rings.map((ring: Ring) => <article className="list-item" key={`${ring.source_id}-${ring.event_id}`}><div><strong>{ring.message || 'Your terminal is ready'}</strong><p>{ring.source_name} · {new Date(ring.created_at).toLocaleString()}</p></div><div className="tags">{ring.target_tags.length ? ring.target_tags.map((tag) => <span key={tag}>{tag}</span>) : <span>all</span>}</div></article>)}</div>}</Section>
}

function Pairings({ onError }: { onError: (error: unknown) => void }) {
  const loader = useCallback(() => api.pairings(), []); const { data, refresh } = useLoad(loader, onError, 5000)
  const decide = (pairing: Pairing, approved: boolean) => (approved ? api.approvePairing(pairing.id) : api.rejectPairing(pairing.id)).then(refresh).catch(onError)
  if (!data) return <Loading />
  return <div><h2>Pending pairings</h2>{data.pairings.length === 0 ? <Empty>No pairing requests waiting for approval.</Empty> : <div className="list">{data.pairings.map((pairing) => <article className="list-item pairing" key={pairing.id}><div><strong>{pairing.display_name}</strong><p>Code <b>{pairing.code}</b> · expires {new Date(pairing.expires_at).toLocaleTimeString()}</p></div><div className="actions"><button onClick={() => decide(pairing, true)}>Approve</button><button className="secondary" onClick={() => decide(pairing, false)}>Reject</button></div></article>)}</div>}</div>
}

function Sources({ onError }: { onError: (error: unknown) => void }) {
  const loader = useCallback(() => api.sources(), []); const { data, refresh } = useLoad(loader, onError)
  const rename = (source: Source) => { const name = prompt('Source name', source.display_name); if (name) api.renameSource(source.id, name).then(refresh).catch(onError) }
  const revoke = (source: Source) => { if (confirm(`Revoke ${source.display_name}?`)) api.revokeSource(source.id).then(refresh).catch(onError) }
  return <Section title="Sources" detail="Approve pairing codes and manage send-only source identities."><Pairings onError={onError} /><h2>Source machines</h2>{!data ? <Loading /> : data.sources.length === 0 ? <Empty>No paired sources.</Empty> : <div className="list">{data.sources.map((source) => <article className="list-item" key={source.id}><div><strong>{source.display_name}</strong><p>{source.revoked_at ? 'Revoked' : source.last_seen_at ? `Last ring ${new Date(source.last_seen_at).toLocaleString()}` : 'Never rang'}</p></div>{!source.revoked_at && <div className="actions"><button className="secondary" onClick={() => rename(source)}>Rename</button><button className="danger" onClick={() => revoke(source)}>Revoke</button></div>}</article>)}</div>}</Section>
}

function permissionState(): NotificationState { return 'Notification' in window ? Notification.permission : 'unsupported' }
function decodeVapid(value: string) { const padding = '='.repeat((4 - value.length % 4) % 4); const raw = atob((value + padding).replace(/-/g, '+').replace(/_/g, '/')); return Uint8Array.from([...raw].map((char) => char.charCodeAt(0))) }

function ReceiverRegistration({ onRegistered, onError }: { onRegistered: () => void; onError: (error: unknown) => void }) {
  const [permission, setPermission] = useState<NotificationState>(permissionState()); const [name, setName] = useState('This browser'); const [tags, setTags] = useState<string[]>([]); const [busy, setBusy] = useState(false)
  const register = async () => {
    setBusy(true)
    try {
      if (!('Notification' in window) || !('serviceWorker' in navigator) || !('PushManager' in window)) throw new Error('Push notifications are not supported by this browser.')
      const result = await Notification.requestPermission(); setPermission(result); if (result !== 'granted') return
      const settings = await api.settings(); const registration = await navigator.serviceWorker.ready
      const subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: decodeVapid(settings.vapid_public_key) })
      const json = subscription.toJSON(); if (!json.endpoint || !json.keys?.p256dh || !json.keys.auth) throw new Error('Browser returned an incomplete Push subscription.')
      await api.registerReceiver({ name, tags, subscription: { endpoint: json.endpoint, p256dh: json.keys.p256dh, auth: json.keys.auth } }); onRegistered()
    } catch (error) { onError(error) } finally { setBusy(false) }
  }
  const toggle = (tag: string) => setTags((current) => current.includes(tag) ? current.filter((item) => item !== tag) : [...current, tag])
  return <div className="card registration"><h2>Register this browser</h2><p>{permission === 'default' ? 'Notification permission has not been requested.' : permission === 'granted' ? 'Notification permission is granted.' : permission === 'denied' ? 'Notification permission is denied. Change it in browser settings.' : 'Push notifications are unsupported.'}</p><label>Receiver name<input value={name} maxLength={64} onChange={(event) => setName(event.target.value)} /></label><fieldset><legend>Tags</legend>{['phone', 'pc', 'mobile', 'desktop'].map((tag) => <label className="check" key={tag}><input type="checkbox" checked={tags.includes(tag)} onChange={() => toggle(tag)} />{tag}</label>)}</fieldset><button onClick={register} disabled={busy || permission === 'denied' || permission === 'unsupported'}>{busy ? 'Registering…' : permission === 'granted' ? 'Register receiver' : 'Allow notifications and register'}</button></div>
}

function Receivers({ onError }: { onError: (error: unknown) => void }) {
  const loader = useCallback(() => api.receivers(), []); const { data, refresh } = useLoad(loader, onError)
  const rename = (receiver: Receiver) => { const name = prompt('Receiver name', receiver.name); if (name) api.updateReceiver(receiver.id, { name }).then(refresh).catch(onError) }
  const revoke = (receiver: Receiver) => { if (confirm(`Revoke ${receiver.name}?`)) api.revokeReceiver(receiver.id).then(refresh).catch(onError) }
  return <Section title="Receivers" detail="Register browsers and choose which target tags reach them."><ReceiverRegistration onRegistered={refresh} onError={onError} />{!data ? <Loading /> : data.receivers.length === 0 ? <Empty>No registered Push receivers.</Empty> : <div className="list">{data.receivers.map((receiver) => <article className="list-item" key={receiver.id}><div><strong>{receiver.name}</strong><p>{receiver.enabled ? 'Enabled' : 'Disabled'} · {receiver.tags.length ? receiver.tags.join(', ') : 'all rings'}</p></div><div className="actions"><button className="secondary" onClick={() => api.updateReceiver(receiver.id, { enabled: !receiver.enabled }).then(refresh).catch(onError)}>{receiver.enabled ? 'Disable' : 'Enable'}</button><button className="secondary" onClick={() => rename(receiver)}>Rename</button><button className="danger" onClick={() => revoke(receiver)}>Revoke</button></div></article>)}</div>}</Section>
}

function SettingsView({ onError, onLogout }: { onError: (error: unknown) => void; onLogout: () => void }) {
  const loader = useCallback(() => api.settings(), []); const { data, refresh } = useLoad(loader, onError)
  return <Section title="Settings" detail="Basic retention and owner-session controls.">{!data ? <Loading /> : <SettingsForm key={data.history_retention_days} initialDays={data.history_retention_days} onSave={(days) => api.updateSettings(days).then(refresh).catch(onError)} onLogout={onLogout} />}</Section>
}

function SettingsForm({ initialDays, onSave, onLogout }: { initialDays: number; onSave: (days: number) => void; onLogout: () => void }) {
  const [days, setDays] = useState(initialDays)
  return <div className="card settings"><label>Ring history retention (days)<input type="number" min="1" max="90" value={days} onChange={(event) => setDays(Number(event.target.value))} /></label><button onClick={() => onSave(days)}>Save settings</button><hr /><button className="secondary" onClick={onLogout}>Sign out and revoke this session</button></div>
}
