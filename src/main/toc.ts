import { promises as fs } from 'node:fs'
import path from 'node:path'

export interface TocInfo {
  title: string | null
  version: string | null
  author: string | null
  /** "## Category" (11.1+ TOCs): the group the in-game AddOns list shows it under */
  category: string | null
}

/** Per-client .toc suffixes, most specific first (e.g. Questie_Forever.toc). */
const TOC_SUFFIXES: Record<string, string[]> = {
  wowf: ['Forever'],
  wow: ['Mainline'],
  wow_classic_era: ['Vanilla', 'Classic'],
  wow_classic: ['Mists', 'Classic']
}

/** Removes WoW UI escape codes: |cAARRGGBB colors, |r resets, |n newlines. */
function stripEscapes(s: string): string {
  return s.replace(/\|c[0-9a-f]{8}/gi, '').replace(/\|[rn]/g, '').trim()
}

/**
 * Reads Title/Version/Author from an addon folder's .toc, preferring the file
 * for this client's flavor, then the plain <Folder>.toc, then any .toc.
 */
export async function readTocInfo(addonDir: string, product: string): Promise<TocInfo> {
  const empty: TocInfo = { title: null, version: null, author: null, category: null }
  const files = (await fs.readdir(addonDir).catch(() => [] as string[])).filter((f) =>
    f.toLowerCase().endsWith('.toc')
  )
  const folder = path.basename(addonDir).toLowerCase()
  const byName = (name: string): string | undefined => files.find((f) => f.toLowerCase() === name.toLowerCase())
  const suffixed = (TOC_SUFFIXES[product] ?? []).flatMap((s) => [`${folder}_${s}.toc`, `${folder}-${s}.toc`])
  const file = [...suffixed, `${folder}.toc`].map(byName).find(Boolean) ?? files[0]
  if (!file) return empty

  const text = await fs.readFile(path.join(addonDir, file), 'utf8').catch(() => '')
  const field = (name: string): string | null => {
    const m = text.match(new RegExp(`^##\\s*${name}\\s*:\\s*(.+)$`, 'mi'))
    const value = m ? stripEscapes(m[1]) : ''
    // Unreplaced packager tokens like @project-version@ aren't useful to show.
    return value && !/^@.+@$/.test(value) ? value : null
  }
  return { title: field('Title'), version: field('Version'), author: field('Author'), category: field('Category') }
}
