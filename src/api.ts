import { invoke } from '@tauri-apps/api/core'

export const call = invoke

export type Device = {
  transport: number
  model: string
  state: string
  connection: 'USB' | 'Wireless' | 'Emulator'
}

export type DeviceList = {
  adbPath: string | null
  devices: Device[]
  error: string | null
  activeUser: { transport: number; user: number } | null
}
export type Selection = { transport: number; identity: string }
export type DeviceInfo = {
  model: string
  brand: string
  android: string
  build: string
  user: number
  xiaomi: boolean
  identity: string
}
export type Package = {
  id: string
  system: boolean
  installed: boolean
  disabled: boolean
  protection: string | null
}
export type Setting = {
  id: string
  label: string
  description: string
  namespace: 'global' | 'system'
  options: [string, string][]
}
export type Preset = { name: string; version: 1; packages: string[] }
export type CatalogEntry = { id: string; name: string; description: string }
export type Metadata = { settings: Setting[]; catalog: CatalogEntry[]; builtIn: Preset[] }
export type Snapshot = {
  info: DeviceInfo
  packages: Package[]
  protectionReady: boolean
  settings: Record<string, string | null>
  unavailableSettings: string[]
}
export type Verb = 'disable' | 'remove' | 'enable' | 'restore'
export type Action = { kind: 'package'; id: string; verb: Verb } | { kind: 'setting'; id: string; value: string }
export type Preview = {
  token: number
  rows: { action: Action; label: string; current: string | null; next: string | null; reason: string | null }[]
}
export type Entry = {
  id: number
  at: string
  device: string
  user: number
  label: string
  action: Action | null
  status: 'pending' | 'success' | 'skipped' | 'unknown' | 'failed'
  detail: string | null
  undoOf: number | null
  canUndo: boolean
}
export type ApkFile = { path: string; name: string }

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
