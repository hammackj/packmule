import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import type { Api, AppState, LogEntry } from '../shared/types'

function subscribe<T>(channel: string, cb: (value: T) => void): () => void {
  const listener = (_e: IpcRendererEvent, value: T): void => cb(value)
  ipcRenderer.on(channel, listener)
  return () => ipcRenderer.removeListener(channel, listener)
}

const api: Api = {
  getState: () => ipcRenderer.invoke('state:get'),
  updateSettings: (update) => ipcRenderer.invoke('settings:update', update),
  pickFolder: () => ipcRenderer.invoke('dialog:pickFolder'),
  addAddon: (url) => ipcRenderer.invoke('addon:add', url),
  removeAddon: (repo) => ipcRenderer.invoke('addon:remove', repo),
  installAddon: (repo) => ipcRenderer.invoke('addon:install', repo),
  updateAll: () => ipcRenderer.invoke('addons:updateAll'),
  checkUpdates: () => ipcRenderer.invoke('addons:check'),
  openExternal: (url) => ipcRenderer.invoke('shell:openExternal', url),
  openAddonsFolder: () => ipcRenderer.invoke('shell:openAddons'),
  onState: (cb) => subscribe<AppState>('state', cb),
  onLog: (cb) => subscribe<LogEntry>('log', cb)
}

contextBridge.exposeInMainWorld('api', api)
