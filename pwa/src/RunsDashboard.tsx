import { useCallback, useEffect, useMemo, useState } from 'react'
import { api } from './api'
import type { RunBeacon, RunSnapshot, TrackedRun } from './types'
import './dashboard.css'

type Scope = 'all' | 'agents' | 'slurm'
const waiting = new Set(['waiting_input', 'waiting_approval'])
const active = new Set(['started', 'running'])
const terminal = new Set(['finished', 'ended', 'cancelled'])
function isCollector(run: TrackedRun) { return run.beacon.collector.endsWith('-collector') && run.beacon.run_id === 'connection' }

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const paths: Record<string, React.ReactNode> = {
    grid: <><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></>,
    refresh: <><path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1"/></>,
    search: <><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/></>,
    server: <><rect x="3" y="3" width="18" height="7" rx="2"/><rect x="3" y="14" width="18" height="7" rx="2"/><path d="M7 6.5h.01M7 17.5h.01M11 6.5h6M11 17.5h6"/></>,
    arrow: <path d="M5 12h14m-5-5 5 5-5 5"/>,
    clock: <><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></>,
    check: <path d="m5 12 4 4L19 6"/>,
    copy: <><rect x="8" y="8" width="12" height="13" rx="2"/><path d="M15 8V3H3v12h5"/></>,
    bell: <><path d="M5 16h14l-2-3V9a5 5 0 0 0-10 0v4l-2 3ZM10 20h4"/></>,
    pulse: <path d="M2 12h5l3-8 4 16 3-8h5"/>,
    close: <path d="m6 6 12 12M6 18 18 6"/>,
  }
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name] || paths.grid}</svg>
}
function relative(value: string) {
  const ms = Date.now() - new Date(value).getTime()
  if (!Number.isFinite(ms)) return 'Unknown'
  if (ms < 60000) return 'just now'
  if (ms < 3600000) return `${Math.floor(ms / 60000)}m ago`
  if (ms < 86400000) return `${Math.floor(ms / 3600000)}h ago`
  return `${Math.floor(ms / 86400000)}d ago`
}
function stateLabel(beacon: RunBeacon) {
  const labels: Record<string, string> = { started: 'Starting', running: 'In progress', queued: 'Queued', waiting_input: 'Needs your answer', waiting_approval: 'Needs approval', finished: beacon.collector === 'slurm' ? 'Completed' : 'Turn finished', failed: 'Failed', ended: 'Session ended', cancelled: 'Cancelled', unknown: 'Status unknown', needs_reauth: 'Reconnect needed' }
  return labels[beacon.state] || beacon.state.replaceAll('_', ' ')
}
function tone(state: string, stale = false) { return stale ? 'muted' : waiting.has(state) || state === 'needs_reauth' ? 'amber' : state === 'failed' ? 'red' : active.has(state) ? 'teal' : state === 'queued' ? 'blue' : 'muted' }
function durationSeconds(value: string) {
  const match = /^(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+)$/.exec(value)
  return match ? Number(match[1] || 0) * 86400 + Number(match[2] || 0) * 3600 + Number(match[3]) * 60 + Number(match[4]) : null
}
function memoryLabel(value: string) {
  const match = /^(\d+(?:\.\d+)?)([MGTP])?([cn])?$/i.exec(value)
  if (!match) return value || '—'
  let size = Number(match[1]); let unit = (match[2] || 'M').toUpperCase()
  if (unit === 'M' && size >= 1024) { size /= 1024; unit = 'G' }
  return `${Number(size.toFixed(1))} ${unit}iB${match[3] === 'c' ? '/CPU' : match[3] === 'n' ? '/node' : ''}`
}
function gpuLabel(value: string) {
  const model = /^gres\/gpu:([^:,]+):(\d+)$/.exec(value)
  if (model) return `${model[1].toUpperCase()} × ${model[2]}`
  const count = /^gres\/gpu:(\d+)$/.exec(value)
  return count ? `${count[1]} GPU` : value === 'N/A' || !value ? 'No GPU requested' : value
}
function JobDetails({ beacon }: { beacon: RunBeacon }) {
  const job = beacon.job
  if (!job) return null
  const elapsed = durationSeconds(job.elapsed); const limit = durationSeconds(job.time_limit)
  const percent = elapsed !== null && limit ? Math.min(100, Math.max(0, elapsed / limit * 100)) : null
  const reason = job.node_reason.replace(/^\((.*)\)$/, '$1')
  return <div className="job-context"><div className="job-resources"><span title="Requested or allocated CPUs">{job.cpus ?? '—'} <small>CPUs</small></span><span title="Minimum requested memory">{memoryLabel(job.min_memory)}</span><span title={job.tres_per_node}>{gpuLabel(job.tres_per_node)}</span></div><div className="job-time"><span>Elapsed / limit</span><strong>{job.elapsed} <span>/ {job.time_limit}</span></strong></div>{percent !== null && <div className="job-time-track" title={`${Math.round(percent)}% of the scheduled time limit used`}><span style={{ width: `${percent}%` }}/></div>}<div className="job-location"><Icon name={beacon.state === 'queued' ? 'clock' : 'server'} size={12}/><span>{beacon.state === 'queued' ? reason === 'Dependency' ? 'Waiting on dependency' : `Waiting · ${reason}` : reason || 'Node not reported'}</span></div><details className="job-expanded"><summary>Scheduling details</summary><dl><dt>Partition</dt><dd>{job.partition || '—'}</dd><dt>Reported start</dt><dd>{job.start_time === 'N/A' ? 'Not scheduled yet' : job.start_time.replace('T', ' · ')}</dd><dt>GPU / TRES per node</dt><dd>{job.tres_per_node || '—'}</dd><dt>Node / reason</dt><dd>{job.node_reason || '—'}</dd><dt>Job ID</dt><dd>{beacon.run_id}</dd></dl></details></div>
}
function Provider({ collector }: { collector: string }) {
  return <span className={`provider-mark provider-${collector}`} aria-hidden="true">{collector === 'claude' ? '✳' : collector === 'codex' ? '›_' : collector === 'slurm' ? <Icon name="server" size={20}/> : collector.slice(0, 1).toUpperCase()}</span>
}
function RunCard({ run, onError }: { run: TrackedRun; onError: (error: unknown) => void }) {
  const b = run.beacon
  const [copied, setCopied] = useState(false)
  return <article className={`run-card tone-${tone(b.state, run.stale)}`}>
    <div className="run-card-top"><Provider collector={b.collector}/><span className={`run-state tone-${tone(b.state, run.stale)}`}><i/>{run.stale ? 'Stale' : stateLabel(b)}</span></div>
    <div className="run-name"><h3 title={b.job?.name}>{b.collector === 'slurm' ? b.job?.name || `Job #${b.run_id}` : b.collector === 'claude' ? 'Claude Code' : b.collector === 'codex' ? 'Codex' : b.collector}</h3><span className="run-kind">{b.collector === 'slurm' ? b.job?.partition || 'Cluster job' : 'Agent session'}</span></div>
    <div className="run-machine"><Icon name="server" size={14}/>{b.machine}</div>
    <JobDetails beacon={b}/>
    <div className="run-card-bottom"><span title={new Date(b.observed_at).toLocaleString()}><Icon name="clock" size={13}/>{relative(b.observed_at)}</span><button className="id-copy" title={`Copy ${b.run_id}`} onClick={() => navigator.clipboard.writeText(b.run_id).then(() => setCopied(true)).catch(onError)} aria-label={`Copy ID ${b.run_id}`}><code>{copied ? 'Copied' : b.run_id.length > 16 ? b.run_id.slice(0, 8) + '…' + b.run_id.slice(-4) : b.run_id}</code><Icon name={copied ? 'check' : 'copy'} size={12}/></button></div>
    {run.stale && <p className="stale-note">No recent signal · last {stateLabel(b).toLowerCase()}</p>}
    {waiting.has(b.state) && <p className="question-note">Reply in your terminal on {b.machine}.</p>}
  </article>
}

export default function RunsDashboard({ onError, onAddSource }: { onError: (error: unknown) => void; onAddSource: () => void }) {
  const [data, setData] = useState<RunSnapshot | null>(null)
  const [refreshing, setRefreshing] = useState(false)
  const [lastUpdated, setLastUpdated] = useState<string | null>(null)
  const [unreachable, setUnreachable] = useState(false)
  const [scope, setScope] = useState<Scope>('all')
  const [query, setQuery] = useState('')
  const [machine, setMachine] = useState('all')
  const refresh = useCallback(async () => {
    setRefreshing(true)
    try { const next = await api.runs(); setData(next); setLastUpdated(new Date().toISOString()); setUnreachable(false) }
    catch (error) { setUnreachable(true); onError(error) }
    finally { setRefreshing(false) }
  }, [onError])
  useEffect(() => { void refresh(); const timer = window.setInterval(() => void refresh(), 10000); return () => clearInterval(timer) }, [refresh])
  const runs = useMemo(() => data?.runs.filter(r => !isCollector(r)) ?? [], [data])
  const collectors = data?.runs.filter(isCollector) ?? []
  const inbox = data?.inbox.filter(event => !(event.beacon.collector.endsWith('-collector') && event.beacon.run_id === 'connection')) ?? []
  const needsYou = runs.filter(r => waiting.has(r.beacon.state))
  const machines = [...new Set(runs.map(r => r.beacon.machine))].sort()
  const filtered = runs.filter(r => (scope === 'all' || (scope === 'slurm' ? r.beacon.collector === 'slurm' : r.beacon.collector !== 'slurm')) && (machine === 'all' || r.beacon.machine === machine) && [r.beacon.machine, r.beacon.collector, r.beacon.run_id, r.beacon.job?.name || '', r.beacon.job?.partition || ''].some(v => v.toLowerCase().includes(query.toLowerCase())))
  const ordered = [...filtered].sort((a, b) => Number(waiting.has(b.beacon.state)) - Number(waiting.has(a.beacon.state)) || Number(terminal.has(a.beacon.state)) - Number(terminal.has(b.beacon.state)))
  const stats = [
    { label: 'In progress', value: runs.filter(r => active.has(r.beacon.state) && !r.stale).length, icon: 'pulse', color: 'teal', detail: 'Actively working' },
    { label: 'Queued', value: runs.filter(r => r.beacon.state === 'queued' && !r.stale).length, icon: 'clock', color: 'blue', detail: 'Waiting for resources' },
    { label: 'Needs you', value: needsYou.length, icon: 'bell', color: 'amber', detail: 'Questions & approvals' },
    { label: 'Finished', value: runs.filter(r => r.beacon.state === 'finished').length, icon: 'check', color: 'muted', detail: 'Within the last 48 hours' },
  ]
  const dismiss = (source: string, event: string) => api.dismissRun(source, event).then(refresh).catch(onError)
  return <div className="runs-dashboard">
    <div className="dashboard-heading"><div><div className="dashboard-kicker"><Icon name="grid" size={13}/> WORKSPACE OVERVIEW</div><h1>Your work, in view<span>.</span></h1><p>Agents, cluster jobs, and the moments that need you.</p></div><div className="dashboard-heading-actions"><span className={`live-chip ${unreachable ? 'disconnected' : ''}`}><i/>{unreachable ? 'Disconnected' : data ? 'Live updates' : 'Connecting'}</span><button className={`icon-button refresh-button ${refreshing ? 'is-refreshing' : ''}`} aria-label="Refresh dashboard" disabled={refreshing} onClick={() => void refresh()}><Icon name="refresh"/></button></div></div>
    <div className="dashboard-stats" aria-label="Workload summary">{stats.map(stat => <article className="dashboard-stat" key={stat.label}><div><span className="stat-label">{stat.label}</span><span className={`stat-icon tone-${stat.color}`}><Icon name={stat.icon}/></span></div><strong>{data ? stat.value.toString().padStart(2, '0') : '—'}</strong><span className="stat-detail">{stat.detail}</span></article>)}</div>
    {needsYou.length > 0 && <section className="attention-strip" aria-label="Needs your attention"><span className="attention-symbol"><Icon name="bell" size={20}/></span><div><strong>{needsYou.length === 1 ? 'An agent is waiting for you' : `${needsYou.length} agents are waiting for you`}</strong><p>{needsYou.map(r => `${r.beacon.collector} on ${r.beacon.machine}`).slice(0, 3).join(' · ')}{needsYou.length > 3 ? ` + ${needsYou.length - 3} more` : ''}</p></div><button onClick={() => { setScope('agents'); setMachine('all'); setQuery(''); document.getElementById('workload-heading')?.scrollIntoView({ behavior: 'smooth', block: 'start' }) }}>View sessions<Icon name="arrow" size={16}/></button></section>}
    <div className="dashboard-columns"><section className="workload-panel" aria-label="Workload"><div className="panel-heading"><div><h2 id="workload-heading">Workload <span>{runs.length}</span></h2><p>A live pulse across your machines.</p></div><button className="text-button" onClick={onAddSource}>Add source <span>+</span></button></div>
      <div className="workload-toolbar"><div className="scope-tabs" role="group" aria-label="Workload type">{(['all', 'agents', 'slurm'] as Scope[]).map(tab => <button key={tab} aria-pressed={scope === tab} onClick={() => setScope(tab)}>{tab === 'all' ? 'All runs' : tab === 'agents' ? 'Agents' : 'Slurm'}</button>)}</div><label className="machine-filter"><span className="sr-only">Filter by machine</span><select value={machine} onChange={event => setMachine(event.target.value)}><option value="all">All machines</option>{machines.map(m => <option key={m}>{m}</option>)}</select></label></div>
      <label className="run-search"><Icon name="search" size={17}/><span className="sr-only">Search runs</span><input placeholder="Search by agent, machine, or job ID…" value={query} onChange={event => setQuery(event.target.value)}/>{query && <button className="icon-button" aria-label="Clear search" onClick={() => setQuery('')}><Icon name="close" size={14}/></button>}</label>
      {!data ? <div className="run-grid" aria-label="Loading runs">{[0, 1, 2, 3].map(i => <div className="run-card run-skeleton" key={i}><span/><span/><span/></div>)}</div> : ordered.length ? <div className="run-grid">{ordered.map(run => <RunCard key={[run.source_id, run.beacon.collector, run.beacon.machine, run.beacon.run_id].join(':')} run={run} onError={onError}/>)}</div> : <div className="dashboard-empty"><span><Icon name="pulse" size={28}/></span><h3>{runs.length ? 'No matching runs' : 'A little quiet here'}</h3><p>{runs.length ? 'Try another machine, collector, or search.' : 'New agent sessions and cluster jobs will appear here as their collectors report in.'}</p>{!runs.length && <button className="dashboard-primary" onClick={onAddSource}>Connect a source <Icon name="arrow" size={15}/></button>}</div>}
      <div className="workload-footnote"><Icon name="check" size={13}/> Metadata only. Your prompts and terminal stay private.<span>{lastUpdated ? `Updated ${relative(lastUpdated)}` : ''}</span></div>
    </section><aside className="dashboard-aside"><section className="collector-panel" aria-label="Collector health"><div className="panel-heading"><div><h2>Collectors</h2><p>The connections behind your jobs.</p></div><Icon name="server" size={17}/></div>{collectors.length ? <div className="collector-list">{collectors.map(run => <div className="collector-row" key={run.source_id + run.beacon.machine}><div className={`collector-indicator tone-${run.stale || run.beacon.state !== 'running' ? 'amber' : 'teal'}`}><Icon name="server" size={17}/></div><div><strong>{run.beacon.machine}</strong><span>Slurm · checked {relative(run.beacon.observed_at)}</span></div><span className={`connection-label ${run.stale || run.beacon.state !== 'running' ? 'connection-warning' : ''}`}><i/>{run.stale ? 'Stale' : run.beacon.state === 'running' ? 'Connected' : 'Reconnect'}</span></div>)}</div> : <div className="collector-empty"><Icon name="server" size={20}/><p>No cluster collectors yet.</p><span>Agent hooks work independently.</span></div>}<div className="collector-caption">Connection checks are shown here and excluded from workload counts.</div></section>
      <section className="activity-panel" aria-label="Recent activity"><div className="panel-heading"><div><h2>Recent activity</h2><p>Your last 48 hours.</p></div><button className="text-button" disabled={!inbox.length} onClick={() => api.clearRunInbox().then(refresh).catch(onError)}>Clear</button></div>{inbox.length ? <ol className="activity-list">{inbox.slice(0, 30).map(event => <li key={event.source_id + event.event_id}><span className={`activity-dot tone-${tone(event.beacon.state)}`}/><div><strong>{event.beacon.collector === 'slurm' ? event.beacon.job?.name || `Job #${event.beacon.run_id}` : event.beacon.collector}</strong><p>{stateLabel(event.beacon)}</p><span>{event.beacon.machine} · {relative(event.beacon.observed_at)}</span></div><button className="activity-dismiss" aria-label={`Dismiss activity ${event.event_id}`} onClick={() => void dismiss(event.source_id, event.event_id)}><Icon name="close" size={13}/></button></li>)}</ol> : <div className="activity-empty"><Icon name="check" size={20}/><strong>All caught up</strong><p>New activity will land here.</p></div>}<div className="activity-caption"><Icon name="clock" size={12}/> Automatically clears after 48 hours</div></section>
    </aside></div>
  </div>
}
