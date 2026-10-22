import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import DndControl from './DndControl'

function setup() {
  let status = { active: false, until: null as string | null }
  const fetchMock = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    if (init?.method === 'PUT') {
      const request = JSON.parse(String(init.body))
      status = { active: request.enabled, until: request.enabled && request.minutes ? new Date(Date.now() + request.minutes * 60000).toISOString() : null }
    }
    return Promise.resolve(new Response(JSON.stringify(status), { status: 200 }))
  })
  vi.stubGlobal('fetch', fetchMock)
  render(<DndControl onError={vi.fn()}/>)
  return fetchMock
}

describe('do not disturb', () => {
  it('pauses indefinitely, resumes, and supports a custom timer', async () => {
    const fetchMock = setup()
    await screen.findByText('Notifications are on.')
    fireEvent.click(screen.getByText('DND'))
    await userEvent.selectOptions(screen.getByLabelText('Pause for'), 'indefinite')
    await userEvent.click(screen.getByRole('button', { name: 'Pause notifications' }))
    expect(await screen.findByText('Notifications paused until you turn DND off.')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith('/api/dnd', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ enabled: true, minutes: null }) }))
    await userEvent.click(screen.getByRole('button', { name: 'Resume notifications' }))
    await screen.findByText('Notifications are on.')
    await userEvent.selectOptions(screen.getByLabelText('Pause for'), 'custom')
    fireEvent.change(screen.getByLabelText('Minutes'), { target: { value: '90' } })
    await userEvent.click(screen.getByRole('button', { name: 'Pause notifications' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/dnd', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ enabled: true, minutes: 90 }) })))
    expect(await screen.findByText(/Notifications paused until/)).toBeInTheDocument()
  })
  it('shows notifications resumed when a timer expires without a page reload', async () => {
    vi.useFakeTimers()
    try {
      const deadline = new Date(Date.now() + 1000).toISOString()
      vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(JSON.stringify({ active: true, until: deadline }), { status: 200 }))))
      render(<DndControl onError={vi.fn()}/>)
      await act(async () => { await Promise.resolve() })
      fireEvent.click(screen.getByLabelText('Do not disturb'))
      expect(screen.getByText(/Notifications paused until/)).toBeInTheDocument()
      await act(async () => { vi.advanceTimersByTime(2000) })
      expect(screen.getByText('Notifications are on.')).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Resume notifications' })).not.toBeInTheDocument()
    } finally { vi.useRealTimers() }
  })
})
