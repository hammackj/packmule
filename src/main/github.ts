const API = 'https://api.github.com'
const USER_AGENT = 'packmule'

export interface ReleaseAsset {
  id: number
  name: string
  size: number
  browser_download_url: string
}

export interface Release {
  tag_name: string
  html_url: string
  draft?: boolean
  prerelease?: boolean
  assets: ReleaseAsset[]
}

/** "spoken/v3.8.4" -> "spoken/", "v1.2.3" -> "" */
export function tagStream(tag: string): string {
  const i = tag.lastIndexOf('/')
  return i >= 0 ? tag.slice(0, i + 1) : ''
}

/** Shape of the release.json written by the BigWigsMods packager */
interface PackagerManifest {
  releases: {
    filename: string
    nolib: boolean
    metadata: { flavor: string; interface: number }[]
  }[]
}

export interface AssetChoice {
  asset: ReleaseAsset
  compatWarning: string | null
  /** true/false when the release's release.json lists (or doesn't list) the interface; null without one */
  interfaceMatch: boolean | null
}

/**
 * Accepts "owner/repo" or any github.com URL pointing into the repo. A link to
 * a release (".../releases/tag/spoken/v3.8.4") also selects that release's
 * stream: tagPrefix is then set, and is "" for tags without a prefix.
 */
export function parseRepo(input: string): { repo: string; tagPrefix?: string } {
  const trimmed = input.trim().replace(/\.git$/, '').replace(/\/+$/, '')
  const match =
    trimmed.match(/^(?:https?:\/\/)?(?:www\.)?github\.com\/([\w.-]+)\/([\w.-]+)(?:\/releases\/tag\/(.+))?/i) ??
    trimmed.match(/^([\w.-]+)\/([\w.-]+)$/)
  if (!match) throw new Error(`Not a GitHub repository URL: ${input}`)
  const repo = `${match[1]}/${match[2]}`
  if (!match[3]) return { repo }
  return { repo, tagPrefix: tagStream(decodeURIComponent(match[3].split(/[?#]/)[0])) }
}

export class GitHubClient {
  /** ETag cache so unchanged releases cost no rate limit (304s are free). */
  private etags = new Map<string, { etag: string; body: unknown }>()
  private manifests = new Map<string, PackagerManifest>()

  constructor(private getToken: () => string) {}

  private async api<T>(apiPath: string): Promise<T> {
    const url = API + apiPath
    const headers: Record<string, string> = {
      Accept: 'application/vnd.github+json',
      'User-Agent': USER_AGENT,
      'X-GitHub-Api-Version': '2022-11-28'
    }
    const token = this.getToken()
    if (token) headers.Authorization = `Bearer ${token}`
    const cached = this.etags.get(url)
    if (cached) headers['If-None-Match'] = cached.etag

    const res = await fetch(url, { headers })
    if (res.status === 304 && cached) return cached.body as T
    if (res.status === 404) throw new Error('Repository not found or has no releases')
    if (res.status === 403 || res.status === 429) {
      const reset = res.headers.get('x-ratelimit-reset')
      const when = reset ? new Date(Number(reset) * 1000).toLocaleTimeString() : 'later'
      throw new Error(`GitHub rate limit hit; try again after ${when} or add a token in Settings`)
    }
    if (!res.ok) throw new Error(`GitHub API ${res.status} for ${apiPath}`)

    const body = (await res.json()) as T
    const etag = res.headers.get('etag')
    if (etag) this.etags.set(url, { etag, body })
    return body
  }

  /**
   * Newest published (non-draft, non-prerelease) release. With a tag prefix
   * (including "" for unprefixed tags), only that stream counts: GitHub's
   * /releases/latest is shared by every product a repo publishes.
   */
  async latestRelease(repo: string, tagPrefix?: string): Promise<Release> {
    if (tagPrefix === undefined) return this.api<Release>(`/repos/${repo}/releases/latest`)
    for (let page = 1; page <= 5; page++) {
      const list = await this.api<Release[]>(`/repos/${repo}/releases?per_page=100&page=${page}`)
      const hit = list.find((r) => !r.draft && !r.prerelease && tagStream(r.tag_name) === tagPrefix)
      if (hit) return hit
      if (list.length < 100) break
    }
    throw new Error(
      tagPrefix ? `No published release with a tag starting "${tagPrefix}"` : 'No published release with an unprefixed tag'
    )
  }

  /** Distinct tag streams among recent releases ("" = tags without a prefix). */
  async releaseStreams(repo: string): Promise<string[]> {
    const list = await this.api<Release[]>(`/repos/${repo}/releases?per_page=100`)
    return [...new Set(list.filter((r) => !r.draft).map((r) => tagStream(r.tag_name)))]
  }

  private async manifest(asset: ReleaseAsset): Promise<PackagerManifest> {
    // Release asset URLs are immutable per tag, so cache by URL for the session.
    const hit = this.manifests.get(asset.browser_download_url)
    if (hit) return hit
    const res = await fetch(asset.browser_download_url, { headers: { 'User-Agent': USER_AGENT } })
    if (!res.ok) throw new Error(`Failed to fetch release.json (${res.status})`)
    const manifest = (await res.json()) as PackagerManifest
    this.manifests.set(asset.browser_download_url, manifest)
    return manifest
  }

  /**
   * Picks the zip for a game client. Prefers the packager's release.json,
   * matching on interface number; otherwise falls back to the zip assets.
   */
  async chooseAsset(release: Release, gameInterface: number): Promise<AssetChoice> {
    const zips = release.assets.filter((a) => a.name.toLowerCase().endsWith('.zip'))
    if (zips.length === 0) {
      throw new Error(`Release ${release.tag_name} has no packaged .zip (source-only releases are not supported)`)
    }

    const manifestAsset = release.assets.find((a) => a.name === 'release.json')
    if (manifestAsset) {
      const entries = (await this.manifest(manifestAsset)).releases
        .slice()
        .sort((a, b) => Number(a.nolib) - Number(b.nolib))
      const match = entries.find((e) => e.metadata.some((m) => m.interface === gameInterface))
      const chosen = match ?? entries[0]
      const asset = chosen && zips.find((z) => z.name === chosen.filename)
      if (asset) {
        const listed = [...new Set(chosen.metadata.map((m) => `${m.flavor} ${m.interface}`))].join(', ')
        return {
          asset,
          compatWarning: match ? null : `No build lists interface ${gameInterface} (has: ${listed})`,
          interfaceMatch: match !== undefined
        }
      }
    }

    const full = zips.filter((z) => !/-nolib/i.test(z.name))
    const pool = full.length > 0 ? full : zips
    return {
      asset: pool[0],
      compatWarning:
        pool.length > 1 ? `No release.json; picked ${pool[0].name} of ${pool.length} zips` : null,
      interfaceMatch: null
    }
  }
}
