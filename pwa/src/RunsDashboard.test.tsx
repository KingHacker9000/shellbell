import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import RunsDashboard from './RunsDashboard'
import type { RunBeacon, RunSnapshot, TrackedRun } from './types'

function run(collector: string, machine: string, run_id: string, state: string): TrackedRun {
  const beacon: RunBeacon = { collector, machine, run_id, state, heartbeat: false, observed_at: new Date().toISOString(), stale_after_seconds: 600 }
  return { source_id: 'source', source_name: machine, beacon, stale: false, received_at: beacon.observed_at }
}
const work = [run('claude', 'desktop', 'question-session', 'waiting_input'), run('codex', 'pi', 'active-session', 'running'), run('slurm', 'cluster', '123', 'queued'), run('slurm', 'cluster', '124', 'finished')]
const collectors = [run('slurm-collector', 'cluster', 'connection', 'running'), run('slurm-collector', 'dgp', 'connection', 'running')]
function setup(snapshot: RunSnapshot) {
  const fetchMock = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => Promise.resolve(new Response(init?.method === 'DELETE' ? null : JSON.stringify(snapshot), { status: init?.method === 'DELETE' ? 204 : 200 })))
  vi.stubGlobal('fetch', fetchMock)
  render(<RunsDashboard onError={vi.fn()} onAddSource={vi.fn()}/>)
  return fetchMock
}
describe('run dashboard', () => {
  it('separates poller health from real workload and summary counts', async () => {
    setup({ runs: [...work, ...collectors], inbox: [] })
    await screen.findByText('Job #123')
    const workload = screen.getByRole('region', { name: 'Workload' })
    expect(within(workload).getAllByRole('article')).toHaveLength(4)
    expect(within(workload).queryByText('connection')).not.toBeInTheDocument()
    const health = screen.getByRole('region', { name: 'Collector health' })
    expect(within(health).getAllByText('Connected')).toHaveLength(2)
    for (const label of ['In progress', 'Queued', 'Needs you', 'Finished']) {
      const summary = screen.getByLabelText('Workload summary')
      expect(within(summary).getByText(label).closest('article')).toHaveTextContent('01')
    }
    expect(screen.getByText('An agent is waiting for you')).toBeInTheDocument()
  })
  it('filters workload by type, machine and query', async () => {
    setup({ runs: [...work, ...collectors], inbox: [] })
    await screen.findByText('Job #123')
    await userEvent.click(screen.getByRole('button', { name: 'Slurm' }))
    const workload = screen.getByRole('region', { name: 'Workload' })
    expect(within(workload).getAllByRole('article')).toHaveLength(2)
    await userEvent.type(screen.getByRole('textbox', { name: 'Search runs' }), '124')
    expect(within(workload).getAllByRole('article')).toHaveLength(1)
    expect(within(workload).getByText('Job #124')).toBeInTheDocument()
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Filter by machine' }), 'desktop')
    expect(screen.getByText('No matching runs')).toBeInTheDocument()
  })
  it('dismisses and clears activity without exposing connection events as jobs', async () => {
    const fetchMock = setup({ runs: work, inbox: [{ source_id: 'source', event_id: 'event', source_name: 'desktop', beacon: work[0].beacon, created_at: new Date().toISOString() }, { source_id: 'source', event_id: 'health', source_name: 'cluster', beacon: collectors[0].beacon, created_at: new Date().toISOString() }] })
    await screen.findByRole('button', { name: 'Dismiss activity event' })
    expect(within(screen.getByRole('region', { name: 'Recent activity' })).getAllByRole('listitem')).toHaveLength(1)
    await userEvent.click(screen.getByRole('button', { name: 'Dismiss activity event' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/run-inbox/source/event', expect.objectContaining({ method: 'DELETE' })))
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/run-inbox', expect.objectContaining({ method: 'DELETE' })))
  })
  it('uses the job name and surfaces scheduling decisions without command text', async () => {
    const job = run('slurm', 'cluster', '123', 'queued')
    job.beacon.job = { name: 'training-baseline', partition: 'gpu', elapsed: '0:00', time_limit: '2:00:00', cpus: 4, min_memory: '48G', tres_per_node: 'gres/gpu:example:1', start_time: 'N/A', node_reason: '(Dependency)' }
    setup({ runs: [job, ...collectors], inbox: [] })
    expect(await screen.findByText('training-baseline')).toBeInTheDocument()
    expect(screen.getByText('Waiting on dependency')).toBeInTheDocument()
    expect(screen.getByText('48 GiB')).toBeInTheDocument()
    expect(screen.getByText('EXAMPLE × 1')).toBeInTheDocument()
    await userEvent.click(screen.getByText('Scheduling details'))
    expect(screen.getByText('Not scheduled yet')).toBeInTheDocument()
    await userEvent.type(screen.getByRole('textbox', { name: 'Search runs' }), 'baseline')
    expect(screen.getByText('training-baseline')).toBeInTheDocument()
  })
  it('shows full completion with actual runtime and distinguishes cancellations', async () => {
    const complete = run('slurm', 'cluster', 'done', 'finished')
    complete.beacon.job = { name: 'completed-job', partition: 'gpu', elapsed: '00:35:23', time_limit: '02:30:00', cpus: 4, min_memory: '48G', tres_per_node: '', start_time: 'N/A', node_reason: 'node1' }
    const cancelled = run('slurm', 'cluster', 'cancelled', 'cancelled')
    cancelled.beacon.job = { ...complete.beacon.job, name: 'cancelled-job', elapsed: '00:01:18' }
    setup({ runs: [complete, cancelled], inbox: [] })
    const card = (await screen.findByText('completed-job')).closest('article')!
    expect(within(card).getByText('Completed')).toBeInTheDocument()
    expect(within(card).getByText('Completed in')).toBeInTheDocument()
    expect(within(card).getByText('35m 23s')).toBeInTheDocument()
    expect(within(card).getByRole('progressbar', { name: 'Job completed' })).toHaveAttribute('aria-valuenow', '100')
    const cancelledCard = screen.getByText('cancelled-job').closest('article')!
    expect(within(cancelledCard).getByText('Cancelled')).toBeInTheDocument()
    expect(within(cancelledCard).getByText('Ran before cancellation')).toBeInTheDocument()
    expect(within(cancelledCard).getByText('1m 18s')).toBeInTheDocument()
    expect(within(cancelledCard).queryByRole('progressbar', { name: 'Job completed' })).not.toBeInTheDocument()
  })
  it('limits active-only results to fresh working or queued runs and restores other states', async () => {
    const stale = { ...run('slurm', 'cluster', 'stale-job', 'running'), stale: true }
    setup({ runs: [work[2], work[3], work[0], work[1], stale, run('slurm', 'cluster', 'cancelled', 'cancelled'), run('slurm', 'cluster', 'failed', 'failed')], inbox: [] })
    await screen.findByText('Job #123')
    const workload = screen.getByRole('region', { name: 'Workload' })
    await userEvent.click(screen.getByRole('switch', { name: 'Active only' }))
    expect(within(workload).getAllByRole('article')).toHaveLength(2)
    expect(within(workload).getAllByRole('heading', { level: 3 }).map(heading => heading.textContent)).toEqual(['Codex', 'Job #123'])
    expect(within(workload).getByText('Codex')).toBeInTheDocument()
    expect(within(workload).getByText('Job #123')).toBeInTheDocument()
    expect(within(workload).queryByText('Job #124')).not.toBeInTheDocument()
    await userEvent.click(screen.getByRole('switch', { name: 'Active only' }))
    expect(within(workload).getAllByRole('article')).toHaveLength(7)
  })

})
