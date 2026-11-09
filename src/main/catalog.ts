import bundled from '../../catalog.json'
import type { AddonRecord, CatalogEntry } from '../shared/types'
import { tagStream } from './github'

/**
 * The catalog shipped with the app is a fallback: the copy in the GitHub repo
 * is fetched at startup so new addons appear without an app release.
 */
const REMOTE_URL = 'https://raw.githubusercontent.com/hammackj/packmule/master/catalog.json'

function valid(e: unknown): e is CatalogEntry {
  const x = e as CatalogEntry
  return (
    typeof x?.id === 'string' &&
    typeof x.name === 'string' &&
    typeof x.repo === 'string' &&
    /^[\w.-]+\/[\w.-]+$/.test(x.repo) &&
    (x.tagPrefix === undefined || typeof x.tagPrefix === 'string') &&
    typeof x.category === 'string' &&
    typeof x.description === 'string' &&
    Array.isArray(x.flavors)
  )
}

function parse(data: unknown): CatalogEntry[] {
  const list = (data as { addons?: unknown })?.addons
  return Array.isArray(list) ? list.filter(valid) : []
}

let entries: CatalogEntry[] = parse(bundled)

export function catalogEntries(): CatalogEntry[] {
  return entries
}

export function catalogEntry(id: string): CatalogEntry | undefined {
  return entries.find((e) => e.id === id)
}

/** Replaces the bundled catalog with the published one; keeps the old list on any failure. */
export async function refreshCatalog(): Promise<void> {
  try {
    const res = await fetch(REMOTE_URL, { headers: { 'User-Agent': 'packmule' } })
    if (!res.ok) return
    const remote = parse(await res.json())
    if (remote.length > 0) entries = remote
  } catch {
    // Offline or not published yet: the bundled catalog stays.
  }
}

/**
 * The packager's flavor name for a game client's product code (from
 * .flavor.info): wowf = WoW Forever, wow = retail, and so on.
 */
export function productFlavor(product: string): string | null {
  if (product.startsWith('wowf')) return 'forever'
  if (product.startsWith('wow_classic_era')) return 'classic'
  if (product.startsWith('wow_anniversary')) return 'bcc'
  if (product.startsWith('wow_classic')) return 'mists'
  if (product === 'wow' || product === 'wowt' || product === 'wowxptr' || product === 'wow_beta') return 'mainline'
  return null
}

/** The release stream a record follows ("" for unprefixed tags). */
function recordStream(r: AddonRecord): string | undefined {
  if (r.tagPrefix !== undefined) return r.tagPrefix
  return r.installedTag ? tagStream(r.installedTag) : undefined
}

/** Whether a managed addon is this catalog entry (same repo and release stream). */
export function matchesEntry(r: AddonRecord, e: CatalogEntry): boolean {
  if (r.repo.toLowerCase() !== e.repo.toLowerCase()) return false
  const stream = recordStream(r)
  return stream === undefined || stream === (e.tagPrefix ?? '')
}
