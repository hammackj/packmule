import { EventEmitter } from 'node:events'
import path from 'node:path'
import {
  addonKey,
  type AddonRecord,
  type AddonView,
  type AppState,
  type AppUpdate,
  type CatalogView,
  type GameInstall,
  type LogEntry,
  type Settings
} from '../shared/types'
import { catalogEntries, catalogEntry, matchesEntry, productFlavor, refreshCatalog } from './catalog'
import { addonsDir, detectInstalls } from './game'
import { GitHubClient, parseRepo, tagStream, type ReleaseAsset } from './github'
import { installAsset, listAddonFolders, removeFolders, verifyFolders } from './installer'
import { Store } from './store'
import { readTocInfo } from './toc'
import { newerVersion } from './version'
import { addonFoldersIn, remoteZipEntries } from './zipindex'

const APP_REPO = 'hammackj/packmule'

export class Manager extends EventEmitter {
  private store = new Store()
  private github = new GitHubClient(() => this.store.token)
  private installs: GameInstall[] = []
  private pathError: string | null = null
  private busy = new Set<string>()
  private checking = false
  private lastCheck: string | null = null
  private timer: NodeJS.Timeout | null = null
  /** Serializes filesystem-changing work so installs never interleave */
  private queue: Promise<unknown> = Promise.resolve()
  private appUpdate: AppUpdate | null = null
  /** "folder>owner/repo" pairs from .toc links already checked this session */
  private linkChecked = new Set<string>()

  /** appVersion: this build's version, compared against Packmule's GitHub releases */
  constructor(private appVersion: string) {
    super()
  }

  async init(): Promise<void> {
    await this.store.load()
    await this.refreshInstalls()
    this.scheduleChecks()
    if (this.activeInstall) void this.checkUpdates().catch(() => {})
    void refreshCatalog().then(() => this.changed())
    void this.checkAppUpdate()
  }

  /** Looks for a newer Packmule release; shown as a banner, never installed automatically. */
  private async checkAppUpdate(): Promise<void> {
    try {
      const release = await this.github.latestRelease(APP_REPO)
      const latest = release.tag_name.replace(/^v/, '')
      if (newerVersion(latest, this.appVersion)) {
        this.appUpdate = { version: latest, url: release.html_url }
        this.log('info', `Packmule ${latest} is available (you have ${this.appVersion})`)
        await this.changed()
      }
    } catch {
      // No releases yet, offline or rate limited: try again next launch.
    }
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
      else if (r.latestTag && r.latestTag !== r.installedTag) status = 'update-available'
      else if (r.latestTag) status = 'up-to-date'
      const toc = r.folders[0] && install ? await readTocInfo(path.join(this.addonsPath, r.folders[0]), install.product) : null
      const fromCatalog = catalogEntries().find((e) => matchesEntry(r, e))
      addons.push({
        ...r,
        name: r.name ?? fromCatalog?.name,
        status,
        busy: this.busy.has(addonKey(r)),
        category: toc?.category ?? fromCatalog?.category ?? null
      })
    }
    const unmanaged: AppState['unmanaged'] = []
    if (install) {
      const folders = (await listAddonFolders(this.addonsPath)).filter((f) => !owned.has(f) && !f.startsWith('.'))
      for (const folder of folders.sort((a, b) => a.localeCompare(b))) {
        const toc = await readTocInfo(path.join(this.addonsPath, folder), install.product)
        unmanaged.push({ folder, ...toc })
      }
    }

    const flavor = install ? productFlavor(install.product) : null
    const catalog: CatalogView[] = catalogEntries().map((e) => {
      const record = this.records.find((r) => matchesEntry(r, e))
      return {
        ...e,
        installed: install !== null && record !== undefined && record.installedTag !== null,
        available: flavor !== null && e.flavors.includes(flavor),
        busy: record !== undefined && this.busy.has(addonKey(record))
      }
    })

    const { settings } = this.store
    return {
      settings: { ...settings, githubToken: '' } satisfies Settings,
      hasToken: this.store.token !== '',
      installs: this.installs,
      activeInstall: install,
      addons,
      unmanaged,
      catalog,
      flavor,
      appVersion: this.appVersion,
      appUpdate: this.appUpdate,
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
    const release = await this.github.latestRelease(r.repo, r.tagPrefix)
    const choice = await this.github.chooseAsset(release, install.interface)
    const tag = release.tag_name
    r.latestTag = tag
    r.latestAssetId = choice.asset.id
    r.latestAssetName = choice.asset.name
    r.compatWarning = choice.compatWarning
    r.lastChecked = new Date().toISOString()
    return choice
  }

  private async install(r: AddonRecord): Promise<void> {
    const install = this.requireInstall()
    this.busy.add(addonKey(r))
    await this.changed()
    try {
      const { asset } = await this.resolveLatest(r, install)
      this.log('info', `Installing ${addonKey(r)} ${r.latestTag} (${asset.name})`)
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
      this.log('info', `Installed ${addonKey(r)} ${r.installedTag}: ${r.folders.join(', ')}`)
    } catch (e) {
      r.error = (e as Error).message
      this.log('error', `${addonKey(r)}: ${r.error}`)
      throw e
    } finally {
      this.busy.delete(addonKey(r))
      await this.persist()
      await this.changed()
    }
  }

  addAddon(url: string): Promise<AppState> {
    return this.serial(async () => {
      const install = this.requireInstall()
      const gh = parseRepo(url)
      const repo = gh.repo
      let tagPrefix = gh.tagPrefix
      if (tagPrefix === undefined) {
        // Repos that publish several products (tags like "spoken/v1", "manager/v1")
        // share one /releases/latest, so make the user pick a stream.
        const streams = await this.github.releaseStreams(repo)
        if (streams.length > 1) {
          const names = streams.map((s) => s.replace(/\/$/, '') || '(no prefix)').join(', ')
          throw new Error(
            `${repo} publishes several release streams (${names}). Paste the link to a release ` +
              `from the one you want, e.g. https://github.com/${repo}/releases/tag/<stream>/<version>`
          )
        }
        if (streams.length === 1 && streams[0]) tagPrefix = streams[0]
      }
      await this.addRecord(install, repo, tagPrefix)
      return this.state()
    })
  }

  /** Installs an addon from the Browse tab's catalog. */
  installCatalog(id: string): Promise<AppState> {
    return this.serial(async () => {
      const install = this.requireInstall()
      const entry = catalogEntry(id)
      if (!entry) throw new Error(`Unknown catalog addon: ${id}`)
      const existing = this.records.find((r) => matchesEntry(r, entry))
      if (existing) await this.install(existing)
      else await this.addRecord(install, entry.repo, entry.tagPrefix, entry.name)
      return this.state()
    })
  }

  private async addRecord(
    install: GameInstall,
    repo: string,
    tagPrefix: string | undefined,
    name?: string
  ): Promise<void> {
    const key = addonKey({ repo, tagPrefix })
    if (this.records.some((r) => addonKey(r).toLowerCase() === key.toLowerCase())) {
      throw new Error(`${key} is already managed`)
    }
    const record: AddonRecord = {
      repo,
      ...(tagPrefix !== undefined ? { tagPrefix } : {}),
      ...(name ? { name } : {}),
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
    this.log('info', `Added ${key}`)
    await this.install(record)
  }

  removeAddon(key: string): Promise<AppState> {
    return this.serial(async () => {
      const r = this.records.find((x) => addonKey(x) === key)
      if (!r) return this.state()
      await removeFolders(this.addonsPath, r.folders)
      await this.store.setAddons(
        this.store.settings.activeInstall,
        this.records.filter((x) => x !== r)
      )
      this.log('info', `Removed ${key} (${r.folders.join(', ') || 'no folders'})`)
      return this.changed()
    })
  }

  installAddon(key: string): Promise<AppState> {
    return this.serial(async () => {
      const r = this.records.find((x) => addonKey(x) === key)
      if (!r) throw new Error(`${key} is not managed`)
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
        const r = this.records.find((x) => addonKey(x) === addonKey(view))
        if (r) await this.install(r).catch(() => {})
      }
      return this.changed()
    })
  }

  /**
   * Finds addons installed by hand that Packmule can manage and adds them:
   * first by the catalog's folder lists, then by a GitHub link in the .toc
   * (only when that repo's latest release really contains the folder).
   */
  private async adoptManual(install: GameInstall): Promise<void> {
    const owned = new Set(this.records.flatMap((r) => r.folders))
    const present = new Set(
      (await listAddonFolders(this.addonsPath)).filter((f) => !owned.has(f) && !f.startsWith('.'))
    )
    if (present.size === 0) return
    const flavor = productFlavor(install.product)

    for (const e of catalogEntries()) {
      if (!flavor || !e.flavors.includes(flavor) || !e.folders?.length) continue
      if (!present.has(e.folders[0]) || this.records.some((r) => matchesEntry(r, e))) continue
      const folders = e.folders.filter((f) => present.has(f))
      if (await this.adopt(install, e.repo, e.tagPrefix, folders, e.name)) folders.forEach((f) => present.delete(f))
    }

    for (const folder of [...present]) {
      if (!present.has(folder)) continue // taken by an earlier adoption
      const toc = await readTocInfo(path.join(this.addonsPath, folder), install.product)
      const repo = toc.githubRepo
      const key = `${folder}>${repo}`
      if (!repo || this.linkChecked.has(key)) continue
      this.linkChecked.add(key)
      if (this.records.some((r) => r.repo.toLowerCase() === repo.toLowerCase())) continue
      try {
        const release = await this.github.latestRelease(repo)
        const { asset, interfaceMatch } = await this.github.chooseAsset(release, install.interface)
        // A release that lists its builds but none for this client would "update" it to the wrong game version.
        if (interfaceMatch === false) continue
        const inZip = addonFoldersIn(await remoteZipEntries(asset.browser_download_url, asset.size))
        if (!inZip.includes(folder)) continue
        const folders = inZip.filter((f) => present.has(f))
        if (await this.adopt(install, repo, undefined, folders)) folders.forEach((f) => present.delete(f))
      } catch {
        // No usable release: it stays a manual addon.
      }
    }
  }

  /** Starts managing folders that are already installed; true on success. */
  private async adopt(
    install: GameInstall,
    repo: string,
    tagPrefix: string | undefined,
    folders: string[],
    name?: string
  ): Promise<boolean> {
    const record: AddonRecord = {
      repo,
      ...(tagPrefix !== undefined ? { tagPrefix } : {}),
      ...(name ? { name } : {}),
      installedTag: null,
      installedAssetId: null,
      folders,
      installedAt: null,
      latestTag: null,
      latestAssetName: null,
      lastChecked: null,
      compatWarning: null,
      error: null
    }
    try {
      await this.resolveLatest(record, install)
    } catch (e) {
      this.log('error', `Found ${name ?? repo} in AddOns but couldn't check its releases: ${(e as Error).message}`)
      return false
    }
    // Up to date if the installed .toc version is the latest release's version.
    const toc = await readTocInfo(path.join(this.addonsPath, folders[0]), install.product)
    const bare = (v: string): string => v.slice(tagStream(v).length).replace(/^v/i, '').split('+')[0].trim().toLowerCase()
    const current = toc.version !== null && record.latestTag !== null && bare(toc.version) === bare(record.latestTag)
    record.installedTag = current ? record.latestTag : toc.version ? `${toc.version} (by hand)` : 'installed by hand'
    record.installedAssetId = current ? (record.latestAssetId ?? null) : null
    this.records.push(record)
    await this.persist()
    this.log(
      'info',
      `Now managing ${name ?? repo} (${folders.join(', ')}), found installed by hand` +
        (current ? '; up to date' : `; ${record.latestTag} is available`)
    )
    return true
  }

  async checkUpdates(): Promise<AppState> {
    if (this.checking) return this.state()
    const install = this.requireInstall()
    this.checking = true
    await this.changed()
    await this.serial(async () => {
      try {
        await this.adoptManual(install)
        for (const r of this.records) {
          try {
            await this.resolveLatest(r, install)
            r.error = null
          } catch (e) {
            r.error = (e as Error).message
            this.log('error', `${addonKey(r)}: ${r.error}`)
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
