import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { app, BrowserWindow, dialog, ipcMain, shell } from 'electron'
import type { SettingsUpdate } from '../shared/types'
import { Manager } from './manager'

// Pin the data dir so it doesn't depend on how Electron was launched
// (running out/main/index.js directly would otherwise use ~/.config/Electron).
app.setName('packmule')
const userData = path.join(app.getPath('appData'), 'packmule')
app.setPath('userData', userData)
// Must match packaging/packmule.desktop so the taskbar pairs the window with its icon.
if (process.platform === 'linux') app.setDesktopName('packmule.desktop')

// The app was called wow-addon-mgr before; carry its settings and addon list
// over once. The old folder is left in place.
const legacyState = path.join(app.getPath('appData'), 'wow-addon-mgr', 'state.json')
const state = path.join(userData, 'state.json')
if (!existsSync(state) && existsSync(legacyState)) {
  mkdirSync(userData, { recursive: true })
  copyFileSync(legacyState, state)
}

const manager = new Manager(app.getVersion())
let win: BrowserWindow | null = null

function createWindow(): void {
  win = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 760,
    minHeight: 480,
    title: 'Packmule',
    backgroundColor: '#15171c',
    autoHideMenuBar: true,
    icon: path.join(__dirname, '../../packaging/packmule-256.png'),
    webPreferences: {
      preload: path.join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false
    }
  })

  // Links open in the system browser, never inside the app.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('https://')) void shell.openExternal(url)
    return { action: 'deny' }
  })
  win.webContents.on('will-navigate', (e) => e.preventDefault())

  if (process.env.ELECTRON_RENDERER_URL) void win.loadURL(process.env.ELECTRON_RENDERER_URL)
  else void win.loadFile(path.join(__dirname, '../renderer/index.html'))
  win.on('closed', () => (win = null))
}

function registerIpc(): void {
  ipcMain.handle('state:get', () => manager.state())
  ipcMain.handle('settings:update', (_e, update: SettingsUpdate) => manager.updateSettings(update))
  ipcMain.handle('addon:add', (_e, url: string) => manager.addAddon(url))
  ipcMain.handle('catalog:install', (_e, id: string) => manager.installCatalog(id))
  ipcMain.handle('addon:remove', (_e, key: string) => manager.removeAddon(key))
  ipcMain.handle('addon:install', (_e, key: string) => manager.installAddon(key))
  ipcMain.handle('addons:updateAll', () => manager.updateAll())
  ipcMain.handle('addons:check', () => manager.checkUpdates())
  ipcMain.handle('dialog:pickFolder', async () => {
    const result = await dialog.showOpenDialog(win!, {
      title: 'Select your World of Warcraft folder',
      properties: ['openDirectory']
    })
    return result.canceled ? null : result.filePaths[0]
  })
  ipcMain.handle('shell:openExternal', (_e, url: string) => {
    if (/^https:\/\/(github\.com|www\.curseforge\.com)\//.test(url)) return shell.openExternal(url)
  })
  ipcMain.handle('shell:openAddons', async () => {
    const p = manager.currentAddonsPath
    if (p) await shell.openPath(p)
  })

  manager.on('state', (state) => win?.webContents.send('state', state))
  manager.on('log', (entry) => win?.webContents.send('log', entry))
}

// Two instances would each hold their own copy of state.json in memory and
// overwrite each other's changes, so a second launch just focuses the first.
// The lock is scoped to userData, which is pinned above.
if (!app.requestSingleInstanceLock()) app.exit(0)
app.on('second-instance', () => {
  if (!win) return
  if (win.isMinimized()) win.restore()
  win.focus()
})

app.whenReady().then(async () => {
  registerIpc()
  await manager.init()
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
