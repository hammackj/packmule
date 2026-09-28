import { promises as fs } from 'node:fs'
import path from 'node:path'
import type { GameInstall } from '../shared/types'

/**
 * Reads a Blizzard "table" file (.build.info / .flavor.info): a header row of
 * `Name!TYPE:size` columns separated by `|`, followed by data rows.
 */
function parseBlizzardTable(text: string): Record<string, string>[] {
  const lines = text.split(/\r?\n/).filter((l) => l.trim() !== '')
  if (lines.length === 0) return []
  const headers = lines[0].split('|').map((h) => h.split('!')[0])
  return lines.slice(1).map((line) => {
    const cells = line.split('|')
    return Object.fromEntries(headers.map((h, i) => [h, cells[i] ?? '']))
  })
}

/** "1.60.1.70009" -> 16001, "12.1.5.12345" -> 120105 */
export function versionToInterface(version: string): number {
  const [major, minor, patch] = version.split('.').map((n) => parseInt(n, 10))
  if ([major, minor, patch].some((n) => Number.isNaN(n))) return 0
  return major * 10000 + minor * 100 + patch
}

async function readIfExists(file: string): Promise<string | null> {
  try {
    return await fs.readFile(file, 'utf8')
  } catch {
    return null
  }
}

/** Finds game clients (_retail_, _classic_beta_, ...) inside a WoW folder. */
export async function detectInstalls(wowPath: string): Promise<GameInstall[]> {
  const entries = await fs.readdir(wowPath, { withFileTypes: true })
  const buildInfo = await readIfExists(path.join(wowPath, '.build.info'))
  const builds = buildInfo ? parseBlizzardTable(buildInfo) : []

  const installs: GameInstall[] = []
  for (const entry of entries) {
    if (!entry.isDirectory() || !/^_.+_$/.test(entry.name)) continue
    const flavorInfo = await readIfExists(path.join(wowPath, entry.name, '.flavor.info'))
    const product = flavorInfo ? (parseBlizzardTable(flavorInfo)[0]?.['Product Flavor'] ?? '') : ''
    const version = builds.find((b) => b.Product === product)?.Version ?? ''
    installs.push({
      dir: entry.name,
      product,
      version,
      interface: versionToInterface(version)
    })
  }
  return installs.sort((a, b) => a.dir.localeCompare(b.dir))
}

export function addonsDir(wowPath: string, installDir: string): string {
  return path.join(wowPath, installDir, 'Interface', 'AddOns')
}
