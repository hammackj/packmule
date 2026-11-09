#!/usr/bin/env node
// Refreshes each catalog entry from its latest release:
//  - "flavors" from release.json (BigWigs packager). Entries whose releases
//    have no release.json keep the flavors written by hand.
//  - "folders": the addon folders the release installs, primary first, read
//    from the zip's index without downloading it. Packmule uses these to
//    recognise addons that were installed by hand.
//
//   GITHUB_TOKEN=... npm run catalog
//
// A token is optional but avoids the 60 requests/hour limit.

import { readFile, writeFile } from 'node:fs/promises'
import { addonFoldersIn, remoteZipEntries } from '../src/main/zipindex.ts'

const FILE = new URL('../catalog.json', import.meta.url)
const headers: Record<string, string> = { 'User-Agent': 'packmule-catalog', Accept: 'application/vnd.github+json' }
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`

const stream = (tag: string): string => (tag.includes('/') ? tag.slice(0, tag.lastIndexOf('/') + 1) : '')

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function json(url: string, api = true): Promise<any> {
  const res = await fetch(url, { headers: api ? headers : { 'User-Agent': headers['User-Agent'] } })
  if (!res.ok) throw new Error(`${res.status} ${url}`)
  return res.json()
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function latest(entry: any): Promise<any> {
  if (entry.tagPrefix === undefined) return json(`https://api.github.com/repos/${entry.repo}/releases/latest`)
  const list = await json(`https://api.github.com/repos/${entry.repo}/releases?per_page=100`)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const hit = list.find((r: any) => !r.draft && !r.prerelease && stream(r.tag_name) === entry.tagPrefix)
  if (!hit) throw new Error(`no release in stream "${entry.tagPrefix}"`)
  return hit
}

const catalog = JSON.parse(await readFile(FILE, 'utf8'))
for (const entry of catalog.addons) {
  try {
    const release = await latest(entry)
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const assets: any[] = release.assets
    const zips = assets.filter((a) => a.name.toLowerCase().endsWith('.zip'))
    const zip = zips.find((a) => !/-nolib/i.test(a.name)) ?? zips[0]
    if (zip) {
      const folders = addonFoldersIn(await remoteZipEntries(zip.browser_download_url, zip.size))
      // Primary folder first: the one most others are named after (BigWigs for
      // BigWigs_Core), else the shortest.
      const prefixes = (f: string): number => folders.filter((o) => o !== f && o.startsWith(f)).length
      entry.folders = folders.sort((a, b) => prefixes(b) - prefixes(a) || a.length - b.length || a.localeCompare(b))
    }
    const manifest = assets.find((a) => a.name === 'release.json')
    if (manifest) {
      const data = await json(manifest.browser_download_url, false)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      entry.flavors = [...new Set(data.releases.flatMap((r: any) => r.metadata.map((m: any) => m.flavor)))].sort()
    }
    const note = manifest ? '' : ' (no release.json; flavors kept)'
    console.log(`${entry.id}: ${release.tag_name} [${entry.flavors.join(', ')}]${note} folders: ${(entry.folders ?? []).join(', ')}`)
  } catch (e) {
    console.log(`${entry.id}: ERROR ${(e as Error).message}`)
    process.exitCode = 1
  }
}
await writeFile(FILE, JSON.stringify(catalog, null, 2) + '\n')
