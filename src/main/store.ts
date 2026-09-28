import { promises as fs } from 'node:fs'
import path from 'node:path'
import { app, safeStorage } from 'electron'
import type { AddonRecord, Settings } from '../shared/types'

interface StoredData {
  settings: Omit<Settings, 'githubToken'>
  /** Encrypted with safeStorage when available, else prefixed "plain:" */
  token: string
  /** Managed addons per game client folder ("_retail_", "_classic_beta_", ...) */
  addons: Record<string, AddonRecord[]>
}

const DEFAULTS: StoredData = {
  settings: {
    wowPath: '',
    activeInstall: '',
    checkIntervalHours: 6,
    autoUpdate: false
  },
  token: '',
  addons: {}
}

export class Store {
  private data: StoredData = structuredClone(DEFAULTS)
  private file = path.join(app.getPath('userData'), 'state.json')

  async load(): Promise<void> {
    try {
      const raw = JSON.parse(await fs.readFile(this.file, 'utf8')) as Partial<StoredData>
      this.data = {
        settings: { ...DEFAULTS.settings, ...raw.settings },
        token: raw.token ?? '',
        addons: raw.addons ?? {}
      }
    } catch {
      this.data = structuredClone(DEFAULTS)
    }
  }

  private async save(): Promise<void> {
    await fs.mkdir(path.dirname(this.file), { recursive: true })
    const tmp = this.file + '.tmp'
    await fs.writeFile(tmp, JSON.stringify(this.data, null, 2))
    await fs.rename(tmp, this.file)
  }

  get settings(): Omit<Settings, 'githubToken'> {
    return this.data.settings
  }

  async setSettings(update: Partial<Settings>): Promise<void> {
    const { githubToken, ...rest } = update
    this.data.settings = { ...this.data.settings, ...rest }
    if (githubToken !== undefined) this.data.token = this.encrypt(githubToken.trim())
    await this.save()
  }

  get token(): string {
    const t = this.data.token
    if (!t) return ''
    if (t.startsWith('plain:')) return t.slice(6)
    try {
      return safeStorage.decryptString(Buffer.from(t, 'base64'))
    } catch {
      return ''
    }
  }

  private encrypt(token: string): string {
    if (!token) return ''
    if (safeStorage.isEncryptionAvailable()) {
      return safeStorage.encryptString(token).toString('base64')
    }
    return 'plain:' + token
  }

  addons(install: string): AddonRecord[] {
    return (this.data.addons[install] ??= [])
  }

  async setAddons(install: string, list: AddonRecord[]): Promise<void> {
    this.data.addons[install] = list
    await this.save()
  }
}
