import { useState } from 'react'
import type { AppState, SettingsUpdate } from '../../shared/types'

export default function Settings(props: {
  state: AppState
  onClose: () => void
  onSave: (update: SettingsUpdate) => Promise<void>
}): React.JSX.Element {
  const { state, onClose, onSave } = props
  const [wowPath, setWowPath] = useState(state.settings.wowPath)
  const [interval, setInterval] = useState(state.settings.checkIntervalHours)
  const [autoUpdate, setAutoUpdate] = useState(state.settings.autoUpdate)
  const [token, setToken] = useState('')
  const [clearToken, setClearToken] = useState(false)

  async function browse(): Promise<void> {
    const picked = await window.api.pickFolder()
    if (picked) setWowPath(picked)
  }

  function save(e: React.FormEvent): void {
    e.preventDefault()
    const update: SettingsUpdate = {
      wowPath: wowPath.trim(),
      checkIntervalHours: Math.max(0, interval),
      autoUpdate
    }
    if (clearToken) update.githubToken = ''
    else if (token.trim()) update.githubToken = token.trim()
    void onSave(update)
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={save}>
        <h2>Settings</h2>

        <label>
          World of Warcraft folder
          <div className="row">
            <input
              value={wowPath}
              onChange={(e) => setWowPath(e.target.value)}
              placeholder=".../drive_c/Program Files (x86)/World of Warcraft"
            />
            <button type="button" onClick={() => void browse()}>
              Browse…
            </button>
          </div>
          <span className="muted small">
            The folder that contains _retail_, _classic_, etc. Pick the game client from the
            dropdown in the main window.
          </span>
        </label>

        <label>
          Check for updates every (hours, 0 = off)
          <input
            type="number"
            min={0}
            step={1}
            value={interval}
            onChange={(e) => setInterval(Number(e.target.value))}
          />
        </label>

        <label className="checkbox">
          <input type="checkbox" checked={autoUpdate} onChange={(e) => setAutoUpdate(e.target.checked)} />
          Install updates automatically after a check
        </label>

        <label>
          GitHub token (optional)
          <input
            type="password"
            value={token}
            disabled={clearToken}
            onChange={(e) => setToken(e.target.value)}
            placeholder={state.hasToken ? '•••••••• saved — type to replace' : 'Raises the API limit from 60 to 5000 requests/hour'}
          />
          {state.hasToken && (
            <span className="checkbox small">
              <input type="checkbox" checked={clearToken} onChange={(e) => setClearToken(e.target.checked)} />
              Remove saved token
            </span>
          )}
        </label>

        <div className="row end">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Save
          </button>
        </div>
      </form>
    </div>
  )
}
