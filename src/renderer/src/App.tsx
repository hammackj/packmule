import { useEffect, useState } from 'react'
import type { AddonStatus, AddonView, AppState, LogEntry } from '../../shared/types'
import Settings from './Settings'

const api = window.api

const STATUS_LABEL: Record<AddonStatus, string> = {
  'not-installed': 'Not installed',
  'up-to-date': 'Up to date',
  'update-available': 'Update available',
  'missing-files': 'Missing files',
  unknown: 'Not checked'
}

/** Strips Electron's "Error invoking remote method 'x': Error: " prefix. */
function errorText(e: unknown): string {
  return String((e as Error)?.message ?? e).replace(/^Error invoking remote method '[^']+': (Error: )?/, '')
}

function timeAgo(iso: string | null): string {
  if (!iso) return 'never'
  const mins = Math.round((Date.now() - new Date(iso).getTime()) / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.round(mins / 60)
  return hours < 48 ? `${hours}h ago` : `${Math.round(hours / 24)}d ago`
}

export default function App(): React.JSX.Element {
  const [state, setState] = useState<AppState | null>(null)
  const [log, setLog] = useState<LogEntry[]>([])
  const [url, setUrl] = useState('')
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showSettings, setShowSettings] = useState(false)
  const [showLog, setShowLog] = useState(false)

  useEffect(() => {
    void api.getState().then((s) => {
      setState(s)
      if (!s.settings.wowPath) setShowSettings(true)
    })
    const offState = api.onState(setState)
    const offLog = api.onLog((entry) => setLog((l) => [...l.slice(-199), entry]))
    return () => {
      offState()
      offLog()
    }
  }, [])

  async function run(fn: () => Promise<AppState>): Promise<void> {
    setError(null)
    try {
      setState(await fn())
    } catch (e) {
      setError(errorText(e))
    }
  }

  async function add(e: React.FormEvent): Promise<void> {
    e.preventDefault()
    if (!url.trim()) return
    setAdding(true)
    await run(() => api.addAddon(url))
    setAdding(false)
    setUrl('')
  }

  if (!state) return <div className="loading">Loading…</div>

  const needsWork = state.addons.filter((a) => a.status !== 'up-to-date').length
  const anyBusy = state.checking || state.addons.some((a) => a.busy)

  return (
    <div className="app">
      <header className="topbar">
        <h1>Addon Manager</h1>
        <select
          value={state.settings.activeInstall}
          onChange={(e) => void run(() => api.updateSettings({ activeInstall: e.target.value }))}
          disabled={state.installs.length === 0}
        >
          {state.installs.map((i) => (
            <option key={i.dir} value={i.dir}>
              {i.dir} · {i.version || 'unknown version'} · interface {i.interface || '?'}
            </option>
          ))}
        </select>
        <div className="spacer" />
        <button onClick={() => void run(api.checkUpdates)} disabled={anyBusy || !state.activeInstall}>
          {state.checking ? 'Checking…' : 'Check for updates'}
        </button>
        <button
          className="primary"
          onClick={() => void run(api.updateAll)}
          disabled={anyBusy || needsWork === 0}
        >
          Update all{needsWork > 0 ? ` (${needsWork})` : ''}
        </button>
        <button onClick={() => setShowSettings(true)}>Settings</button>
      </header>

      {state.pathError && (
        <div className="banner warn">
          {state.pathError} <button onClick={() => setShowSettings(true)}>Open settings</button>
        </div>
      )}
      {error && (
        <div className="banner error">
          {error} <button onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}

      <form className="addbar" onSubmit={(e) => void add(e)}>
        <input
          placeholder="https://github.com/owner/repo"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          disabled={adding || !state.activeInstall}
        />
        <button className="primary" type="submit" disabled={adding || !url.trim() || !state.activeInstall}>
          {adding ? 'Adding…' : 'Add addon'}
        </button>
      </form>

      <main>
        {state.addons.length === 0 ? (
          <div className="empty">
            No addons yet. Paste a GitHub repository URL above — it must publish packaged
            releases (a .zip asset).
          </div>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Addon</th>
                <th>Installed</th>
                <th>Latest</th>
                <th>Status</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {state.addons.map((a) => (
                <AddonRow key={a.repo} addon={a} disabled={anyBusy} run={run} />
              ))}
            </tbody>
          </table>
        )}

        {state.unmanaged.length > 0 && (
          <p className="muted small">
            Not managed here (left untouched): {state.unmanaged.join(', ')}
          </p>
        )}
      </main>

      <footer>
        <span className="muted small">
          Last check {timeAgo(state.lastCheck)}
          {state.settings.autoUpdate ? ' · auto-update on' : ''}
          {state.settings.checkIntervalHours > 0
            ? ` · every ${state.settings.checkIntervalHours}h`
            : ' · automatic checks off'}
        </span>
        <div className="spacer" />
        <button className="link" onClick={() => void api.openAddonsFolder()} disabled={!state.activeInstall}>
          Open AddOns folder
        </button>
        <button className="link" onClick={() => setShowLog((v) => !v)}>
          {showLog ? 'Hide log' : `Log (${log.length})`}
        </button>
      </footer>

      {showLog && (
        <pre className="log">
          {log.length === 0
            ? 'Nothing yet.'
            : log.map((l) => `${new Date(l.time).toLocaleTimeString()}  ${l.level === 'error' ? '✖ ' : ''}${l.message}`).join('\n')}
        </pre>
      )}

      {showSettings && (
        <Settings
          state={state}
          onClose={() => setShowSettings(false)}
          onSave={async (update) => {
            await run(() => api.updateSettings(update))
            setShowSettings(false)
          }}
        />
      )}
    </div>
  )
}

function AddonRow(props: {
  addon: AddonView
  disabled: boolean
  run: (fn: () => Promise<AppState>) => Promise<void>
}): React.JSX.Element {
  const { addon: a, disabled, run } = props
  const actionLabel =
    a.status === 'update-available' ? 'Update' : a.status === 'up-to-date' ? 'Reinstall' : a.status === 'missing-files' ? 'Repair' : 'Install'

  return (
    <tr>
      <td>
        <button className="link repo" onClick={() => void window.api.openExternal(`https://github.com/${a.repo}`)}>
          {a.repo}
        </button>
        {a.folders.length > 0 && <div className="muted small">{a.folders.join(', ')}</div>}
      </td>
      <td>{a.installedTag ?? '—'}</td>
      <td>
        {a.latestTag ?? '—'}
        {a.latestAssetName && <div className="muted small">{a.latestAssetName}</div>}
      </td>
      <td>
        {a.busy ? (
          <span className="badge busy">Working…</span>
        ) : (
          <span className={`badge ${a.status}`}>{STATUS_LABEL[a.status]}</span>
        )}
        {a.compatWarning && <div className="warn-text small">⚠ {a.compatWarning}</div>}
        {a.error && <div className="error-text small">{a.error}</div>}
      </td>
      <td className="actions">
        <button
          className={a.status === 'up-to-date' ? '' : 'primary'}
          disabled={disabled}
          onClick={() => void run(() => window.api.installAddon(a.repo))}
        >
          {actionLabel}
        </button>
        <button
          className="danger"
          disabled={disabled}
          onClick={() => {
            if (confirm(`Remove ${a.repo} and delete its folders (${a.folders.join(', ') || 'none'})?`)) {
              void run(() => window.api.removeAddon(a.repo))
            }
          }}
        >
          Remove
        </button>
      </td>
    </tr>
  )
}
