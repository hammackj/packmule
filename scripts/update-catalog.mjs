#!/usr/bin/env node
// Refreshes each catalog entry's "flavors" from its latest release's
// release.json (BigWigs packager). Entries whose releases have no release.json
// keep the flavors written by hand.
//
//   GITHUB_TOKEN=... node scripts/update-catalog.mjs
//
// A token is optional but avoids the 60 requests/hour limit.

import { readFile, writeFile } from 'node:fs/promises'

const FILE = new URL('../catalog.json', import.meta.url)
const headers = { 'User-Agent': 'packmule-catalog', Accept: 'application/vnd.github+json' }
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`

const stream = (tag) => (tag.includes('/') ? tag.slice(0, tag.lastIndexOf('/') + 1) : '')

async function json(url, api = true) {
  const res = await fetch(url, { headers: api ? headers : { 'User-Agent': headers['User-Agent'] } })
  if (!res.ok) throw new Error(`${res.status} ${url}`)
  return res.json()
}

async function latest(entry) {
  if (entry.tagPrefix === undefined) return json(`https://api.github.com/repos/${entry.repo}/releases/latest`)
  const list = await json(`https://api.github.com/repos/${entry.repo}/releases?per_page=100`)
  const hit = list.find((r) => !r.draft && !r.prerelease && stream(r.tag_name) === entry.tagPrefix)
  if (!hit) throw new Error(`no release in stream "${entry.tagPrefix}"`)
  return hit
}

const catalog = JSON.parse(await readFile(FILE, 'utf8'))
for (const entry of catalog.addons) {
  try {
    const release = await latest(entry)
    const manifest = release.assets.find((a) => a.name === 'release.json')
    if (!manifest) {
      console.log(`${entry.id}: ${release.tag_name} has no release.json, kept [${entry.flavors.join(', ')}]`)
      continue
    }
    const data = await json(manifest.browser_download_url, false)
    const flavors = [...new Set(data.releases.flatMap((r) => r.metadata.map((m) => m.flavor)))].sort()
    entry.flavors = flavors
    console.log(`${entry.id}: ${release.tag_name} [${flavors.join(', ')}]`)
  } catch (e) {
    console.log(`${entry.id}: ERROR ${e.message}`)
    process.exitCode = 1
  }
}
await writeFile(FILE, JSON.stringify(catalog, null, 2) + '\n')
