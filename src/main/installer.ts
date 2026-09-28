import { createWriteStream, promises as fs } from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'
import { Readable } from 'node:stream'
import type { ReadableStream as NodeReadableStream } from 'node:stream/web'
import { pipeline } from 'node:stream/promises'
import yauzl from 'yauzl'
import type { ReleaseAsset } from './github'

const MAX_UNPACKED_BYTES = 2 * 1024 ** 3

async function exists(p: string): Promise<boolean> {
  return fs.access(p).then(
    () => true,
    () => false
  )
}

async function download(url: string, dest: string): Promise<void> {
  const res = await fetch(url, { headers: { 'User-Agent': 'wow-addon-mgr' } })
  if (!res.ok || !res.body) throw new Error(`Download failed (${res.status}): ${url}`)
  await pipeline(Readable.fromWeb(res.body as NodeReadableStream), createWriteStream(dest))
}

/**
 * Extracts a zip, refusing symlinks and any entry that would land outside
 * `dest`. Zips come from arbitrary repos, so this is not optional.
 */
function extractZip(zipPath: string, dest: string): Promise<void> {
  return new Promise((resolve, reject) => {
    yauzl.open(zipPath, { lazyEntries: true, strictFileNames: true }, (err, zip) => {
      if (err || !zip) return reject(err)
      let total = 0
      const fail = (e: Error): void => {
        zip.close()
        reject(e)
      }

      zip.on('entry', async (entry: yauzl.Entry) => {
        try {
          const target = path.resolve(dest, entry.fileName)
          if (!target.startsWith(path.resolve(dest) + path.sep)) {
            throw new Error(`Unsafe path in zip: ${entry.fileName}`)
          }
          const mode = (entry.externalFileAttributes >>> 16) & 0o170000
          if (mode === 0o120000) throw new Error(`Symlink in zip: ${entry.fileName}`)

          if (entry.fileName.endsWith('/')) {
            await fs.mkdir(target, { recursive: true })
            return zip.readEntry()
          }
          total += entry.uncompressedSize
          if (total > MAX_UNPACKED_BYTES) throw new Error('Zip unpacks to more than 2 GB')

          await fs.mkdir(path.dirname(target), { recursive: true })
          zip.openReadStream(entry, async (streamErr, stream) => {
            if (streamErr || !stream) return fail(streamErr ?? new Error('Unreadable zip entry'))
            try {
              await pipeline(stream, createWriteStream(target))
              zip.readEntry()
            } catch (e) {
              fail(e as Error)
            }
          })
        } catch (e) {
          fail(e as Error)
        }
      })
      zip.on('end', () => resolve())
      zip.on('error', reject)
      zip.readEntry()
    })
  })
}

async function hasToc(dir: string): Promise<boolean> {
  const files = await fs.readdir(dir).catch(() => [] as string[])
  return files.some((f) => f.toLowerCase().endsWith('.toc'))
}

/** Addon folders are the top-level dirs with a .toc; unwrap one extra level if needed. */
async function findAddonFolders(root: string): Promise<{ base: string; folders: string[] }> {
  const dirs = async (p: string): Promise<string[]> =>
    (await fs.readdir(p, { withFileTypes: true })).filter((e) => e.isDirectory()).map((e) => e.name)

  const top = await dirs(root)
  const withToc = []
  for (const d of top) if (await hasToc(path.join(root, d))) withToc.push(d)
  if (withToc.length > 0) return { base: root, folders: withToc }

  if (top.length === 1) return findAddonFolders(path.join(root, top[0]))
  throw new Error('No addon folders (with a .toc file) found in the zip')
}

/** True if every folder exists and still has a .toc file. */
export async function verifyFolders(addonsPath: string, folders: string[]): Promise<boolean> {
  if (folders.length === 0) return false
  for (const f of folders) if (!(await hasToc(path.join(addonsPath, f)))) return false
  return true
}

/**
 * Downloads and installs an addon, replacing `previousFolders`.
 * Work happens in a staging dir on the same filesystem as AddOns so the
 * final swap is a set of renames, and old folders are restored on failure.
 */
export async function installAsset(opts: {
  addonsPath: string
  asset: ReleaseAsset
  previousFolders: string[]
  /** Folders owned by other managed addons; overwriting them is an error */
  foreignFolders: Set<string>
}): Promise<string[]> {
  const { addonsPath, asset, previousFolders, foreignFolders } = opts
  const staging = path.join(path.dirname(path.dirname(addonsPath)), '.wow-addon-mgr-tmp', randomUUID())
  const extracted = path.join(staging, 'extracted')
  const backup = path.join(staging, 'backup')
  await fs.mkdir(extracted, { recursive: true })
  await fs.mkdir(backup, { recursive: true })
  await fs.mkdir(addonsPath, { recursive: true })

  try {
    const zipPath = path.join(staging, 'addon.zip')
    await download(asset.browser_download_url, zipPath)
    await extractZip(zipPath, extracted)
    const { base, folders } = await findAddonFolders(extracted)

    const clash = folders.filter((f) => foreignFolders.has(f))
    if (clash.length > 0) {
      throw new Error(`Folder(s) already owned by another managed addon: ${clash.join(', ')}`)
    }

    const moved: string[] = []
    const backedUp: string[] = []
    try {
      for (const f of new Set([...previousFolders, ...folders])) {
        if (await exists(path.join(addonsPath, f))) {
          await fs.rename(path.join(addonsPath, f), path.join(backup, f))
          backedUp.push(f)
        }
      }
      for (const f of folders) {
        await fs.rename(path.join(base, f), path.join(addonsPath, f))
        moved.push(f)
      }
    } catch (e) {
      for (const f of moved) await fs.rm(path.join(addonsPath, f), { recursive: true, force: true })
      for (const f of backedUp) await fs.rename(path.join(backup, f), path.join(addonsPath, f))
      throw e
    }
    return folders
  } finally {
    await fs.rm(staging, { recursive: true, force: true })
    await fs.rmdir(path.dirname(staging)).catch(() => {}) // only succeeds once empty
  }
}

export async function removeFolders(addonsPath: string, folders: string[]): Promise<void> {
  for (const f of folders) await fs.rm(path.join(addonsPath, f), { recursive: true, force: true })
}

export async function listAddonFolders(addonsPath: string): Promise<string[]> {
  const entries = await fs.readdir(addonsPath, { withFileTypes: true }).catch(() => [])
  return entries.filter((e) => e.isDirectory()).map((e) => e.name)
}
