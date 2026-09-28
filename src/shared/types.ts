// Types shared between the main process and the renderer.

export interface Settings {
  /** Root of the WoW install, e.g. ".../World of Warcraft" */
  wowPath: string
  /** Game client folder inside wowPath, e.g. "_classic_beta_" */
  activeInstall: string
  /** Optional GitHub token to raise the API rate limit (60/hr unauthenticated) */
  githubToken: string
  /** Hours between automatic update checks; 0 disables the timer */
  checkIntervalHours: number
  /** Install updates automatically after a check finds them */
  autoUpdate: boolean
}

/** A game client found inside the WoW folder (_retail_, _classic_beta_, ...) */
export interface GameInstall {
  dir: string
  product: string
  version: string
  /** Interface number derived from the version, e.g. 1.60.1 -> 16001 */
  interface: number
}

export interface AddonRecord {
  /** "owner/repo" */
  repo: string
  installedTag: string | null
  installedAssetId: number | null
  /** Top-level AddOns folders this addon owns */
  folders: string[]
  installedAt: string | null
  latestTag: string | null
  latestAssetName: string | null
  lastChecked: string | null
  /** Set when the latest release has no build listed for this client's interface */
  compatWarning: string | null
  error: string | null
}

export type AddonStatus =
  | 'not-installed'
  | 'up-to-date'
  | 'update-available'
  | 'missing-files'
  | 'unknown'

export interface AddonView extends AddonRecord {
  status: AddonStatus
  busy: boolean
}

export interface AppState {
  settings: Settings
  hasToken: boolean
  installs: GameInstall[]
  activeInstall: GameInstall | null
  addons: AddonView[]
  /** Folders in AddOns not owned by any managed addon */
  unmanaged: string[]
  checking: boolean
  lastCheck: string | null
  pathError: string | null
}

export interface LogEntry {
  time: string
  level: 'info' | 'error'
  message: string
}

/** Settings as sent to the renderer: the token is never sent back */
export type SettingsUpdate = Partial<Settings>

export interface Api {
  getState(): Promise<AppState>
  updateSettings(update: SettingsUpdate): Promise<AppState>
  pickFolder(): Promise<string | null>
  addAddon(url: string): Promise<AppState>
  removeAddon(repo: string): Promise<AppState>
  installAddon(repo: string): Promise<AppState>
  updateAll(): Promise<AppState>
  checkUpdates(): Promise<AppState>
  openExternal(url: string): Promise<void>
  openAddonsFolder(): Promise<void>
  onState(cb: (state: AppState) => void): () => void
  onLog(cb: (entry: LogEntry) => void): () => void
}
