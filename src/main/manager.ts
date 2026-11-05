import { EventEmitter } from 'node:events'
import path from 'node:path'
import type { AddonRecord, AddonView, AppState, GameInstall, LogEntry, Settings } from '../shared/types'
import { CurseForgeClient, parseCurseForgeUrl } from './curseforge'
import { addonsDir, detectInstalls } from './game'
import { GitHubClient, parseRepo, type ReleaseAsset } from './github'
import { installAsset, listAddonFolders, removeFolders, verifyFolders } from './installer'
import { Store } from './store'
import { readTocInfo } from './toc'

export class Manager extends EventEmitter {
  private store = new Store()
  private github = new GitHubClient(() => this.store.token)
  private curseforge = new CurseForgeClient(() => this.store.curseforgeKey)
  private installs: GameInstall[] = []
  private pathError: string | null = null
  private busy = new Set<string>()
  private checking = false
  private lastCheck: string | null = null
  private timer: NodeJS.Timeout | null = null
  /** Serializes filesystem-changing work so installs never interleave */
  private queue: Promise<unknown> = Promise.resolve()

  async init(): Promise<void> {
    await this.store.load()
    await this.refreshInstalls()
    this.scheduleChecks()
    if (this.activeInstall) void this.checkUpdates().catch(() => {})
  }

  private log(level: LogEntry['level'], message: string): void {
    this.emit('log', { time: new Date().toISOString(), level, message } satisfies LogEntry)
  }

  private async changed(): Promise<AppState> {
    const state = await this.state()
    this.emit('state', state)
    return state
  }

  private serial<T>(fn: () => Promise<T>): Promise<T> {
    const run = this.queue.then(fn, fn)
    this.queue = run.catch(() => {})
    return run
  }

  private get activeInstall(): GameInstall | null {
    return this.installs.find((i) => i.dir === this.store.settings.activeInstall) ?? null
  }

  private get addonsPath(): string {
    const { wowPath, activeInstall } = this.store.settings
    return addonsDir(wowPath, activeInstall)
  }

  private get records(): AddonRecord[] {
    return this.store.addons(this.store.settings.activeInstall)
  }

  private persist(): Promise<void> {
    return this.store.setAddons(this.store.settings.activeInstall, this.records)
  }

  private async refreshInstalls(): Promise<void> {
    const { wowPath, activeInstall } = this.store.settings
    this.installs = []
    this.pathError = null
    if (!wowPath) {
      this.pathError = 'Set your World of Warcraft folder in Settings'
      return
    }
    try {
      this.installs = await detectInstalls(wowPath)
    } catch (e) {
      this.pathError = `Cannot read ${wowPath}: ${(e as Error).message}`
      return
    }
    if (this.installs.length === 0) {
      this.pathError = 'No game clients (_retail_, _classic_, ...) found in that folder'
    } else if (!this.installs.some((i) => i.dir === activeInstall)) {
      await this.store.setSettings({ activeInstall: this.installs[0].dir })
    }
  }

  private scheduleChecks(): void {
    if (this.timer) clearInterval(this.timer)
    this.timer = null
    const hours = this.store.settings.checkIntervalHours
    if (hours > 0) {
      this.timer = setInterval(() => void this.checkUpdates().catch(() => {}), hours * 3600_000)
    }
  }

  async state(): Promise<AppState> {
    const install = this.activeInstall
    const addons: AddonView[] = []
    const owned = new Set<string>()
    for (const r of install ? this.records : []) {
      r.folders.forEach((f) => owned.add(f))
      let status: AddonView['status'] = 'unknown'
      if (!r.installedTag) status = 'not-installed'
      else if (!(await verifyFolders(this.addonsPath, r.folders))) status = 'missing-files'
      else if (r.source === 'curseforge') {
        // Display names can repeat across uploads; file ids can't.
        if (r.latestAssetId != null) {
          status = r.latestAssetId !== r.installedAssetId ? 'update-available' : 'up-to-date'
        }
      } else if (r.latestTag && r.latestTag !== r.installedTag) status = 'update-available'
      else if (r.latestTag) status = 'up-to-date'
      addons.push({ ...r, status, busy: this.busy.has(r.repo) })
    }
    const unmanaged: AppState['unmanaged'] = []
    if (install) {
      const folders = (await listAddonFolders(this.addonsPath)).filter((f) => !owned.has(f) && !f.startsWith('.'))
      for (const folder of folders.sort((a, b) => a.localeCompare(b))) {
        const toc = await readTocInfo(path.join(this.addonsPath, folder), install.product)
        unmanaged.push({ folder, ...toc })
      }
    }

    const { settings } = this.store
    return {
      settings: { ...settings, githubToken: '', curseforgeApiKey: '' } satisfies Settings,
      hasToken: this.store.token !== '',
      hasCurseForgeKey: this.store.curseforgeKey !== '',
      installs: this.installs,
      activeInstall: install,
      addons,
      unmanaged,
      checking: this.checking,
      lastCheck: this.lastCheck,
      pathError: this.pathError
    }
  }

  async updateSettings(update: Partial<Settings>): Promise<AppState> {
    await this.store.setSettings(update)
    if (update.wowPath !== undefined || update.activeInstall !== undefined) {
      await this.refreshInstalls()
    }
    if (update.checkIntervalHours !== undefined) this.scheduleChecks()
    return this.changed()
  }

  private requireInstall(): GameInstall {
    const install = this.activeInstall
    if (!install) throw new Error(this.pathError ?? 'No game client selected')
    return install
  }

  /** Finds the newest build of a record for this client and records it as latest. */
  private async resolveLatest(r: AddonRecord, install: GameInstall): Promise<{ asset: ReleaseAsset }> {
    let tag: string
    let choice: { asset: ReleaseAsset; compatWarning: string | null }
    if (r.source === 'curseforge') {
      const mod = r.modId ? await this.curseforge.mod(r.modId) : await this.curseforge.modBySlug(r.repo)
      r.modId = mod.id
      r.name = mod.name
      const file = await this.curseforge.chooseFile(mod, install.version)
      tag = file.tag
      choice = file
    } else {
      const release = await this.github.latestRelease(r.repo)
      choice = await this.github.chooseAsset(release, install.interface)
      tag = release.tag_name
    }
    r.latestTag = tag
    r.latestAssetId = choice.asset.id
    r.latestAssetName = choice.asset.name
    r.compatWarning = choice.compatWarning
    r.lastChecked = new Date().toISOString()
    return choice
  }

  private async install(r: AddonRecord): Promise<void> {
    const install = this.requireInstall()
    this.busy.add(r.repo)
    await this.changed()
    try {
      const { asset } = await this.resolveLatest(r, install)
      this.log('info', `Installing ${r.repo} ${r.latestTag} (${asset.name})`)
      const foreign = new Set(this.records.filter((o) => o !== r).flatMap((o) => o.folders))
      r.folders = await installAsset({
        addonsPath: this.addonsPath,
        asset,
        previousFolders: r.folders,
        foreignFolders: foreign
      })
      r.installedTag = r.latestTag
      r.installedAssetId = asset.id
      r.installedAt = new Date().toISOString()
      r.error = null
      this.log('info', `Installed ${r.repo} ${r.installedTag}: ${r.folders.join(', ')}`)
    } catch (e) {
      r.error = (e as Error).message
      this.log('error', `${r.repo}: ${r.error}`)
      throw e
    } finally {
      this.busy.delete(r.repo)
      await this.persist()
      await this.changed()
    }
  }

  addAddon(url: string): Promise<AppState> {
    return this.serial(async () => {
      const install = this.requireInstall()
      const slug = parseCurseForgeUrl(url)
      const repo = slug ?? parseRepo(url)
      if (this.records.some((r) => r.repo.toLowerCase() === repo.toLowerCase())) {
        throw new Error(`${repo} is already managed`)
      }
      const record: AddonRecord = {
        repo,
        source: slug ? 'curseforge' : 'github',
        installedTag: null,
        installedAssetId: null,
        folders: [],
        installedAt: null,
        latestTag: null,
        latestAssetName: null,
        lastChecked: null,
        compatWarning: null,
        error: null
      }
      // Validate before saving: must have a packaged release.
      await this.resolveLatest(record, install)
      this.records.push(record)
      await this.persist()
      this.log('info', `Added ${repo}`)
      await this.install(record)
      return this.state()
    })
  }

  removeAddon(repo: string): Promise<AppState> {
    return this.serial(async () => {
      const r = this.records.find((x) => x.repo === repo)
      if (!r) return this.state()
      await removeFolders(this.addonsPath, r.folders)
      await this.store.setAddons(
        this.store.settings.activeInstall,
        this.records.filter((x) => x !== r)
      )
      this.log('info', `Removed ${repo} (${r.folders.join(', ') || 'no folders'})`)
      return this.changed()
    })
  }

  installAddon(repo: string): Promise<AppState> {
    return this.serial(async () => {
      const r = this.records.find((x) => x.repo === repo)
      if (!r) throw new Error(`${repo} is not managed`)
      await this.install(r)
      return this.state()
    })
  }

  /** Installs everything that is outdated, missing files, or not installed. */
  updateAll(): Promise<AppState> {
    return this.serial(async () => {
      const state = await this.state()
      const todo = state.addons.filter((a) => a.status !== 'up-to-date')
      for (const view of todo) {
        const r = this.records.find((x) => x.repo === view.repo)
        if (r) await this.install(r).catch(() => {})
      }
      return this.changed()
    })
  }

  async checkUpdates(): Promise<AppState> {
    if (this.checking) return this.state()
    const install = this.requireInstall()
    this.checking = true
    await this.changed()
    await this.serial(async () => {
      try {
        for (const r of this.records) {
          try {
            await this.resolveLatest(r, install)
            r.error = null
          } catch (e) {
            r.error = (e as Error).message
            this.log('error', `${r.repo}: ${r.error}`)
          }
        }
        await this.persist()
        this.lastCheck = new Date().toISOString()
        const outdated = (await this.state()).addons.filter((a) => a.status !== 'up-to-date')
        this.log('info', `Update check done: ${outdated.length} addon(s) need attention`)
      } finally {
        this.checking = false
      }
    })
    if (this.store.settings.autoUpdate) return this.updateAll()
    return this.changed()
  }

  get currentAddonsPath(): string | null {
    return this.activeInstall ? this.addonsPath : null
  }
}
