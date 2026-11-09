import type { ReleaseAsset } from './github'

const API = 'https://api.curseforge.com/v1'
const USER_AGENT = 'packmule'
const WOW_GAME_ID = 1
const RELEASE = 1 // releaseType: 1 = release, 2 = beta, 3 = alpha

export interface CurseForgeMod {
  id: number
  name: string
  slug: string
  links: { websiteUrl: string }
  /** false when the author has disabled downloads through third-party apps */
  allowModDistribution: boolean | null
}

interface CurseForgeFile {
  id: number
  displayName: string
  fileName: string
  releaseType: number
  fileDate: string
  fileLength: number
  isAvailable: boolean
  downloadUrl: string | null
  gameVersions: string[]
}

export interface FileChoice {
  tag: string
  asset: ReleaseAsset
  compatWarning: string | null
}

/** Accepts "https://www.curseforge.com/wow/addons/<slug>" (with or without trailing path). */
export function parseCurseForgeUrl(input: string): string | null {
  const m = input.trim().match(/^(?:https?:\/\/)?(?:www\.)?curseforge\.com\/wow\/addons\/([a-z0-9-]+)/i)
  return m ? m[1].toLowerCase() : null
}

export function curseForgePage(slug: string): string {
  return `https://www.curseforge.com/wow/addons/${slug}`
}

/** "1.60.1.70790" -> "1.60.1"; CurseForge tags files with three-part game versions. */
function gameVersion(clientVersion: string): string {
  return clientVersion.split('.').slice(0, 3).join('.')
}

export class CurseForgeClient {
  constructor(private getKey: () => string) {}

  private async api<T>(apiPath: string): Promise<T> {
    const key = this.getKey()
    if (!key) throw new Error('CurseForge addons need an API key; add one in Settings')
    const res = await fetch(API + apiPath, {
      headers: { Accept: 'application/json', 'User-Agent': USER_AGENT, 'x-api-key': key }
    })
    if (res.status === 401 || res.status === 403) {
      throw new Error('CurseForge rejected the API key; check it in Settings')
    }
    if (res.status === 404) throw new Error('CurseForge project not found')
    if (!res.ok) throw new Error(`CurseForge API ${res.status} for ${apiPath}`)
    return ((await res.json()) as { data: T }).data
  }

  async modBySlug(slug: string): Promise<CurseForgeMod> {
    const mods = await this.api<CurseForgeMod[]>(
      `/mods/search?gameId=${WOW_GAME_ID}&slug=${encodeURIComponent(slug)}`
    )
    const mod = mods.find((m) => m.slug === slug)
    if (!mod) throw new Error(`No WoW addon on CurseForge with slug "${slug}"`)
    return mod
  }

  mod(modId: number): Promise<CurseForgeMod> {
    return this.api<CurseForgeMod>(`/mods/${modId}`)
  }

  /**
   * Picks the newest release file tagged for the client's game version.
   * Falls back to the same major.minor (e.g. 1.60.x) with a warning, but never
   * to a different flavor: installing a Classic build into Forever is worse
   * than failing.
   */
  async chooseFile(mod: CurseForgeMod, clientVersion: string): Promise<FileChoice> {
    const page = mod.links?.websiteUrl || curseForgePage(mod.slug)
    if (mod.allowModDistribution === false) {
      throw new Error(`The author has disabled third-party downloads; get it from ${page}`)
    }
    const files = (await this.api<CurseForgeFile[]>(`/mods/${mod.id}/files?pageSize=50`))
      .filter((f) => f.isAvailable && f.releaseType === RELEASE)
      .sort((a, b) => b.fileDate.localeCompare(a.fileDate))

    const want = gameVersion(clientVersion)
    const family = want.split('.').slice(0, 2).join('.') + '.'
    let file = files.find((f) => f.gameVersions.includes(want))
    let compatWarning: string | null = null
    if (!file && want) {
      file = files.find((f) => f.gameVersions.some((v) => v.startsWith(family)))
      if (file) compatWarning = `No file lists ${want}; using one for ${file.gameVersions.join(', ')}`
    }
    if (!file) {
      const listed = [...new Set(files.flatMap((f) => f.gameVersions))].join(', ') || 'none'
      throw new Error(`No release file for game version ${want || '?'} (has: ${listed})`)
    }
    if (!file.downloadUrl) {
      throw new Error(`CurseForge gives no download link for ${file.fileName}; get it from ${page}`)
    }
    return {
      tag: file.displayName,
      asset: {
        id: file.id,
        name: file.fileName,
        size: file.fileLength,
        browser_download_url: file.downloadUrl
      },
      compatWarning
    }
  }
}
