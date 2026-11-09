import { useMemo, useState } from 'react'
import type { AppState, CatalogView } from '../../shared/types'

const FLAVOR_LABEL: Record<string, string> = {
  forever: 'WoW Forever',
  mainline: 'Retail',
  classic: 'Classic Era',
  mists: 'Mists Classic',
  bcc: 'Burning Crusade Classic',
  wrath: 'Wrath Classic',
  cata: 'Cataclysm Classic',
  titan: 'Titan Reforged'
}

export function flavorLabel(flavor: string | null): string {
  return flavor ? (FLAVOR_LABEL[flavor] ?? flavor) : 'this game client'
}

/**
 * The Browse tab: catalog addons grouped by category, with a category list
 * on the left like the in-game AddOns list.
 */
export default function Browse(props: {
  state: AppState
  disabled: boolean
  run: (fn: () => Promise<AppState>) => Promise<void>
}): React.JSX.Element {
  const { state, disabled, run } = props
  const [category, setCategory] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [showAll, setShowAll] = useState(false)

  const pool = useMemo(
    () => state.catalog.filter((e) => showAll || e.available),
    [state.catalog, showAll]
  )
  const hiddenCount = state.catalog.filter((e) => !e.available).length

  const categories = useMemo(() => {
    const counts = new Map<string, number>()
    for (const e of pool) counts.set(e.category, (counts.get(e.category) ?? 0) + 1)
    return [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [pool])

  const q = query.trim().toLowerCase()
  const shown = pool
    .filter((e) => category === null || e.category === category)
    .filter((e) => !q || `${e.name} ${e.description} ${e.repo}`.toLowerCase().includes(q))
    .sort((a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name))

  const names = new Map(state.catalog.map((e) => [e.id, e.name]))

  return (
    <div className="browse">
      <nav className="categories">
        <button className={category === null ? 'active' : ''} onClick={() => setCategory(null)}>
          All addons <span className="count">{pool.length}</span>
        </button>
        {categories.map(([name, count]) => (
          <button key={name} className={category === name ? 'active' : ''} onClick={() => setCategory(name)}>
            {name} <span className="count">{count}</span>
          </button>
        ))}
      </nav>

      <section className="catalog">
        <div className="catalog-bar">
          <input placeholder="Search addons" value={query} onChange={(e) => setQuery(e.target.value)} />
          {hiddenCount > 0 && (
            <label className="checkbox small muted">
              <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
              Show {hiddenCount} without a {flavorLabel(state.flavor)} build
            </label>
          )}
        </div>

        {shown.length === 0 ? (
          <div className="empty">No addons match.</div>
        ) : (
          <div className="cards">
            {shown.map((e) => (
              <Card key={e.id} entry={e} names={names} disabled={disabled || !state.activeInstall} run={run} />
            ))}
          </div>
        )}

        <p className="muted small">
          Only addons that publish packaged releases on GitHub are listed. Anything else can still be added by
          pasting its GitHub or CurseForge link on the Installed tab.
        </p>
      </section>
    </div>
  )
}

function Card(props: {
  entry: CatalogView
  names: Map<string, string>
  disabled: boolean
  run: (fn: () => Promise<AppState>) => Promise<void>
}): React.JSX.Element {
  const { entry: e, names, disabled, run } = props
  const [working, setWorking] = useState(false)
  const busy = working || e.busy

  async function install(): Promise<void> {
    setWorking(true)
    await run(() => window.api.installCatalog(e.id))
    setWorking(false)
  }

  return (
    <article className={`card${e.available ? '' : ' unavailable'}`}>
      <header>
        <button
          className="link repo"
          onClick={() => void window.api.openExternal(`https://github.com/${e.repo}`)}
          title={`github.com/${e.repo}`}
        >
          {e.name}
        </button>
        <span className="source">{e.category}</span>
      </header>
      <p>{e.description}</p>
      {e.requires && e.requires.length > 0 && (
        <p className="muted small">Needs {e.requires.map((id) => names.get(id) ?? id).join(', ')}</p>
      )}
      <footer>
        <span className="muted small">{e.repo}</span>
        <div className="spacer" />
        {e.installed ? (
          <span className="badge up-to-date">Installed</span>
        ) : e.available ? (
          <button className="primary" disabled={disabled || busy} onClick={() => void install()}>
            {busy ? 'Installing…' : 'Install'}
          </button>
        ) : (
          <span className="muted small" title={`Builds for: ${e.flavors.map(flavorLabel).join(', ')}`}>
            Not available for this client
          </span>
        )}
      </footer>
    </article>
  )
}
