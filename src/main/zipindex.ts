/**
 * Lists the addon folders inside a remote zip without downloading it: the
 * zip's central directory (its file index) sits at the end of the file, so
 * two HTTP range requests are enough even for zips of hundreds of MB.
 */

const USER_AGENT = 'packmule'
const EOCD_SIG = 0x06054b50
const ZIP64_LOCATOR_SIG = 0x07064b50
const ZIP64_EOCD_SIG = 0x06064b50
const CD_ENTRY_SIG = 0x02014b50
/** End-of-central-directory record (22 bytes) plus the longest allowed comment */
const TAIL = 22 + 0xffff + 20

// GitHub's download servers reject suffix ranges ("bytes=-N"), so ranges are always explicit.
async function range(url: string, start: number, end: number): Promise<Buffer> {
  const res = await fetch(url, { headers: { Range: `bytes=${start}-${end}`, 'User-Agent': USER_AGENT } })
  if (res.status !== 206) throw new Error(`Range request not supported (${res.status})`)
  return Buffer.from(await res.arrayBuffer())
}

/** Names of every entry in the zip at `url`, which is `size` bytes long. */
export async function remoteZipEntries(url: string, size: number): Promise<string[]> {
  const tailStart = Math.max(0, size - TAIL)
  const tail = await range(url, tailStart, size - 1)

  let eocd = -1
  for (let i = tail.length - 22; i >= 0; i--) {
    if (tail.readUInt32LE(i) === EOCD_SIG) {
      eocd = i
      break
    }
  }
  if (eocd < 0) throw new Error('Not a zip file')

  let count = tail.readUInt16LE(eocd + 10)
  let cdSize = tail.readUInt32LE(eocd + 12)
  let offset = tail.readUInt32LE(eocd + 16)

  // Zip64: the real values live in a separate record pointed to by a locator.
  if ((offset === 0xffffffff || cdSize === 0xffffffff || count === 0xffff) && eocd >= 20) {
    const loc = eocd - 20
    if (tail.readUInt32LE(loc) === ZIP64_LOCATOR_SIG) {
      const recordAt = Number(tail.readBigUInt64LE(loc + 8))
      const rec = recordAt >= tailStart ? tail.subarray(recordAt - tailStart) : await range(url, recordAt, recordAt + 55)
      if (rec.readUInt32LE(0) === ZIP64_EOCD_SIG) {
        count = Number(rec.readBigUInt64LE(32))
        cdSize = Number(rec.readBigUInt64LE(40))
        offset = Number(rec.readBigUInt64LE(48))
      }
    }
  }

  const cd =
    offset >= tailStart
      ? tail.subarray(offset - tailStart, offset - tailStart + cdSize)
      : await range(url, offset, offset + cdSize - 1)

  const names: string[] = []
  let p = 0
  for (let i = 0; i < count && p + 46 <= cd.length; i++) {
    if (cd.readUInt32LE(p) !== CD_ENTRY_SIG) break
    const nameLen = cd.readUInt16LE(p + 28)
    const extraLen = cd.readUInt16LE(p + 30)
    const commentLen = cd.readUInt16LE(p + 32)
    names.push(cd.toString('utf8', p + 46, p + 46 + nameLen))
    p += 46 + nameLen + extraLen + commentLen
  }
  return names
}

/**
 * Addon folders in a list of zip entries: folders holding a .toc directly,
 * at the shallowest depth found (some zips wrap everything in one folder).
 */
export function addonFoldersIn(entries: string[]): string[] {
  let best: { depth: number; folders: Set<string> } | null = null
  for (const name of entries) {
    const parts = name.replace(/\\/g, '/').split('/').filter(Boolean)
    if (parts.length < 2 || !/\.toc$/i.test(parts[parts.length - 1])) continue
    const depth = parts.length - 2
    if (!best || depth < best.depth) best = { depth, folders: new Set() }
    if (depth === best.depth) best.folders.add(parts[parts.length - 2])
  }
  return best ? [...best.folders].sort() : []
}
