import { useCallback, useEffect, useState } from 'react'
import { api } from './api'

export type DndStatus = { active: boolean; until: string | null }
export default function DndControl({ onError }: { onError: (error: unknown) => void }) {
  const [status, setStatus] = useState<DndStatus | null>(null)
  const [duration, setDuration] = useState('60')
  const [minutes, setMinutes] = useState(60)
  const [busy, setBusy] = useState(false)
  const [now, setNow] = useState(Date.now)
  const refresh = useCallback(() => api.dnd().then(setStatus).catch(onError), [onError])
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 30000); return () => clearInterval(timer) }, [refresh])
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 1000); return () => clearInterval(timer) }, [])
  const active = status?.active && (!status.until || Date.parse(status.until) > now)
  const remaining = status?.until ? Math.max(0, Math.ceil((Date.parse(status.until) - now) / 60000)) : null
  const save = async (enabled: boolean) => {
    setBusy(true)
    try { setStatus(await api.setDnd(enabled, !enabled || duration === 'indefinite' ? null : duration === 'custom' ? minutes : Number(duration))) }
    catch (error) { onError(error) }
    finally { setBusy(false) }
  }
  return <details className={`dnd-control ${active ? 'dnd-active' : ''}`}>
    <summary aria-label="Do not disturb"><span aria-hidden="true">☾</span> {active ? `DND · ${remaining === null ? 'On' : remaining + 'm'}` : 'DND'}</summary>
    <div className="dnd-popover"><h2>Do not disturb</h2><p role="status">{active ? status?.until ? `Notifications paused until ${new Date(status.until).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}.` : 'Notifications paused until you turn DND off.' : status ? 'Notifications are on.' : 'Loading notification settings…'}</p>
      <p className="dnd-detail">Pauses push and Totem alerts on all devices. Runs and activity keep updating. Muted alerts will not replay.</p>
      <form onSubmit={event => { event.preventDefault(); void save(true) }}><label>Pause for<select value={duration} onChange={event => setDuration(event.target.value)}><option value="15">15 minutes</option><option value="30">30 minutes</option><option value="60">1 hour</option><option value="120">2 hours</option><option value="240">4 hours</option><option value="480">8 hours</option><option value="indefinite">Until I turn it off</option><option value="custom">Custom duration</option></select></label>
      {duration === 'custom' && <label>Minutes<input type="number" min="1" max="10080" required value={minutes} onChange={event => setMinutes(Number(event.target.value))}/></label>}
      <button type="submit" disabled={!status || busy}>{busy ? 'Saving…' : active ? 'Update DND' : 'Pause notifications'}</button>
      {active && <button type="button" className="secondary" disabled={busy} onClick={() => void save(false)}>Resume notifications</button>}</form>
    </div>
  </details>
}
