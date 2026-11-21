import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Channel } from '@tauri-apps/api/core'
import {
  Alert, Button, Chip, createTheme, CssBaseline, Dialog, DialogActions, DialogContent, DialogTitle, IconButton,
  LinearProgress, MenuItem, Select, Snackbar, Tab, Tabs, TextField, ThemeProvider, Tooltip,
} from '@mui/material'
import AppsIcon from '@mui/icons-material/AppsRounded'
import DarkIcon from '@mui/icons-material/DarkModeOutlined'
import OverviewIcon from '@mui/icons-material/DashboardOutlined'
import HistoryIcon from '@mui/icons-material/HistoryRounded'
import InstallIcon from '@mui/icons-material/InstallMobileRounded'
import LightIcon from '@mui/icons-material/LightModeOutlined'
import PhoneIcon from '@mui/icons-material/PhoneAndroidRounded'
import RefreshIcon from '@mui/icons-material/RefreshRounded'
import AdbIcon from '@mui/icons-material/UsbRounded'
import PresetIcon from '@mui/icons-material/ViewQuiltOutlined'
import TuneIcon from '@mui/icons-material/TuneRounded'
import WifiIcon from '@mui/icons-material/WifiRounded'
import { call, errorText, type Action, type ApkFile, type DeviceList, type Entry, type Metadata, type Preset, type Preview, type Selection, type Snapshot } from './api'
import { Activity, Apps, Install, Overview, Presets, Tweaks, type Page } from './views'

const nav = [
  { id: 'overview', label: 'Overview', icon: OverviewIcon },
  { id: 'apps', label: 'Apps', icon: AppsIcon },
  { id: 'presets', label: 'Presets', icon: PresetIcon },
  { id: 'tweaks', label: 'Tweaks', icon: TuneIcon },
  { id: 'install', label: 'Install', icon: InstallIcon },
  { id: 'activity', label: 'Activity', icon: HistoryIcon },
] as const

const emptyMetadata: Metadata = { settings: [], catalog: [], builtIn: [] }
type Notice = { severity: 'success' | 'error' | 'warning' | 'info'; text: string }
type Confirm = { title: string; description: string; label: string; danger?: boolean; onConfirm: () => void }
type JobResult = { entries: Entry[]; stopped: boolean }
type JobProgress = { count: number; total: number; latest: string }

function appTheme(mode: 'light' | 'dark') {
  const dark = mode === 'dark'
  return createTheme({
    palette: {
      mode,
      primary: { main: dark ? '#a3e8be' : '#235c40', contrastText: dark ? '#14231a' : '#ffffff' },
      success: { main: dark ? '#9cdfb3' : '#23653b' },
      warning: { main: dark ? '#efc58b' : '#8c540f' },
      error: { main: dark ? '#ffaaa3' : '#aa302e' },
      background: { default: dark ? '#121212' : '#f7f7f5', paper: dark ? '#1b1b1b' : '#ffffff' },
      text: { primary: dark ? '#f3f3f2' : '#171717', secondary: dark ? '#b4b5b2' : '#535550' },
      divider: dark ? '#323330' : '#dedfdb',
    },
    typography: { fontFamily: 'Roboto, system-ui, sans-serif', button: { textTransform: 'none', fontWeight: 500 } },
    shape: { borderRadius: 10 },
    components: {
      MuiButton: {
        defaultProps: { disableElevation: true },
        styleOverrides: { root: { minHeight: 35 }, outlined: { borderColor: dark ? '#454744' : '#bbbfba' } },
      },
      MuiDialog: { styleOverrides: { paper: { borderRadius: 19, backgroundImage: 'none' } } },
      MuiTableCell: {
        styleOverrides: { root: { paddingBlock: 8 }, head: { color: dark ? '#b4b5b2' : '#535550', fontSize: '.78rem' } },
      },
    },
  })
}

export default function App() {
  const [themeMode, setThemeMode] = useState<'light' | 'dark'>(() => localStorage.getItem('xiaomi-debloat-theme') === 'light' ? 'light' : 'dark')
  const theme = useMemo(() => appTheme(themeMode), [themeMode])
  const [page, setPage] = useState<Page>('overview')
  const [list, setList] = useState<DeviceList | null>(null)
  const [transport, setTransport] = useState<number | null>(null)
  const [loaded, setLoaded] = useState<{ transport: number; snap: Snapshot } | null>(null)
  const [loading, setLoading] = useState(false)
  const [snapshotError, setSnapshotError] = useState<string | null>(null)
  const [revision, setRevision] = useState(0)
  const [metadata, setMetadata] = useState<Metadata>(emptyMetadata)
  const [saved, setSaved] = useState<Preset[]>([])
  const [entries, setEntries] = useState<Entry[]>([])
  const [scoped, setScoped] = useState(true)
  const [notice, setNotice] = useState<Notice | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [progress, setProgress] = useState<JobProgress | null>(null)
  const [wireless, setWireless] = useState(false)
  const [review, setReview] = useState<{ info: string; result: Preview; advanced: boolean } | null>(null)
  const [confirm, setConfirm] = useState<Confirm | null>(null)
  const [saving, setSaving] = useState<string[] | null>(null)
  const [presetName, setPresetName] = useState('')
  const polling = useRef(false)
  const activeTask = useRef(false)
  const hasSelected = useRef(false)
  const selectedTransport = useRef<number | null>(null)

  const device = list?.devices.find(item => item.transport === transport)
  const readyState = device?.state === 'device'
  const activeUser = list?.activeUser?.transport === transport ? list.activeUser.user : null
  const userChanged = activeUser !== null && loaded?.transport === transport && loaded.snap.info.user !== activeUser
  const snap = readyState && loaded?.transport === transport && !userChanged ? loaded.snap : null
  const selection: Selection | null = snap && transport !== null ? { transport, identity: snap.info.identity } : null
  const historyIdentity = scoped ? snap?.info.identity ?? null : null

  useEffect(() => {
    document.documentElement.dataset.theme = themeMode
    localStorage.setItem('xiaomi-debloat-theme', themeMode)
  }, [themeMode])

  useEffect(() => { selectedTransport.current = transport }, [transport])

  useEffect(() => {
    if (userChanged) {
      setLoaded(null)
      setRevision(previous => previous + 1)
    }
  }, [userChanged])

  const scan = useCallback(async () => {
    if (polling.current) return
    polling.current = true
    try {
      const state = await call<DeviceList>('device_list', { activeTransport: selectedTransport.current })
      setList(state)
      setTransport(previous => {
        if (previous !== null && state.devices.some(item => item.transport === previous)) return previous
        const ready = state.devices.filter(item => item.state === 'device')
        if (!hasSelected.current && ready.length === 1) {
          hasSelected.current = true
          return ready[0].transport
        }
        return null
      })
    } catch (error) {
      setList(previous => ({ adbPath: previous?.adbPath ?? null, devices: [], error: errorText(error), activeUser: null }))
    } finally {
      polling.current = false
    }
  }, [])

  useEffect(() => {
    void scan()
    const timer = window.setInterval(() => void scan(), 3000)
    return () => window.clearInterval(timer)
  }, [scan])

  useEffect(() => {
    void call<Metadata>('metadata').then(setMetadata, error => setNotice({ severity: 'error', text: errorText(error) }))
  }, [])

  useEffect(() => {
    let current = true
    setSnapshotError(null)
    if (transport === null || !readyState) {
      setLoaded(null)
      setLoading(false)
      return
    }
    setLoading(true)
    void call<Snapshot>('snapshot', { transport }).then(value => {
      if (current) setLoaded({ transport, snap: value })
    }, error => {
      if (current) { setLoaded(null); setSnapshotError(errorText(error)) }
    }).finally(() => { if (current) setLoading(false) })
    return () => { current = false }
  }, [transport, readyState, revision])

  const loadPresets = useCallback(async () => {
    const items = await call<Preset[]>('presets')
    setSaved(items)
  }, [])

  useEffect(() => {
    if (page === 'presets') void loadPresets().catch(error => setNotice({ severity: 'error', text: errorText(error) }))
  }, [page, loadPresets])

  useEffect(() => {
    if (page !== 'activity') return
    let current = true
    void call<Entry[]>('history', { identity: historyIdentity }).then(items => {
      if (current) setEntries(items)
    }, error => { if (current) setNotice({ severity: 'error', text: errorText(error) }) })
    return () => { current = false }
  }, [page, historyIdentity, revision])

  function openPage(next: Page) {
    setPage(next)
    window.scrollTo(0, 0)
  }

  async function task<T>(label: string, action: () => Promise<T>): Promise<T | undefined> {
    if (activeTask.current) return
    activeTask.current = true
    setBusy(label)
    try {
      return await action()
    } catch (error) {
      setNotice({ severity: 'error', text: errorText(error) })
    } finally {
      activeTask.current = false
      setBusy(null)
    }
  }

  function refresh() {
    void scan()
    setRevision(previous => previous + 1)
  }

  function chooseAdb() {
    void task('Choosing ADB', async () => {
      if (await call<boolean>('configure_adb')) {
        refresh()
        setNotice({ severity: 'success', text: 'ADB location saved.' })
      }
    })
  }

  function guide(name: string) {
    void call('open_guide', { guide: name }).catch(error => setNotice({ severity: 'error', text: errorText(error) }))
  }

  function reviewActions(actions: Action[], advanced = false, xiaomiOnly = false) {
    if (!selection || !snap) return
    const info = `${snap.info.model} · User ${snap.info.user}`
    void task('Checking device and building preview', async () => {
      const result = await call<Preview>('preview', { request: { selection, actions, advanced, xiaomiOnly } })
      setReview({ info, result, advanced })
    })
  }

  function executePreview(preview: Preview) {
    setReview(null)
    void task('Applying device changes', async () => {
      setProgress({ count: 0, total: preview.rows.length, latest: '' })
      const channel = new Channel<Entry>()
      channel.onmessage = entry => setProgress(previous => previous && ({ ...previous, count: previous.count + 1, latest: entry.label }))
      try {
        const result = await call<JobResult>('execute', { token: preview.token, progress: channel })
        const succeeded = result.entries.filter(entry => entry.status === 'success').length
        const skipped = result.entries.filter(entry => entry.status === 'skipped').length
        const issues = result.entries.length - succeeded - skipped
        const text = [`${succeeded} applied`, skipped ? `${skipped} skipped` : '', issues ? `${issues} need attention` : '']
          .filter(Boolean).join(' · ')
        setNotice({ severity: issues || result.stopped ? 'warning' : 'success',
          text: `${text}. ${result.stopped ? 'The batch stopped early. ' : ''}See Activity for details.` })
      } finally {
        setProgress(null)
        refresh()
      }
    })
  }

  function saveSelection(packages: string[]) {
    setPresetName('')
    setSaving(packages)
  }

  function savePreset() {
    if (!saving) return
    void task('Saving preset', async () => {
      await call('save_preset', { preset: { name: presetName, version: 1, packages: saving } })
      setSaving(null)
      await loadPresets()
      setNotice({ severity: 'success', text: 'Preset saved.' })
    })
  }

  function confirmAction(value: Confirm) { setConfirm(value) }

  function deletePreset(name: string) {
    confirmAction({ title: 'Delete preset?', description: `Delete “${name}” from this PC?`, label: 'Delete preset', danger: true,
      onConfirm: () => { void task('Deleting preset', async () => { await call('delete_preset', { name }); await loadPresets() }) } })
  }

  function exportFile(command: string, args: Record<string, unknown>) {
    void task('Exporting file', async () => {
      if (await call<boolean>(command, args)) setNotice({ severity: 'success', text: 'Export saved.' })
    })
  }

  function showEntry(entry: Entry) {
    setNotice({
      severity: entry.status === 'success' ? 'success' : entry.status === 'failed' ? 'error' : 'warning',
      text: entry.detail || `${entry.label} · ${entry.status}`,
    })
  }

  function installFiles(files: ApkFile[], replace: boolean, split: boolean) {
    if (!selection || !snap) return
    confirmAction({ title: 'Review installation', label: 'Install',
      description: `Install ${files.map(file => file.name).join(', ')} on ${snap.info.model} (user ${snap.info.user}). ${replace ? 'Android shares app updates across users on the same device.' : 'Replacement is disabled.'}`,
      onConfirm: () => {
        void task('Installing APK files', async () => {
          try {
            showEntry(await call<Entry>('install', { selection, paths: files.map(file => file.path), replace, split }))
          } finally { refresh() }
        })
      },
    })
  }

  function undoEntry(entry: Entry) {
    if (!selection) return
    confirmAction({ title: 'Undo this change?', description: `Restore the previous state for “${entry.label}” on the current Android user.`,
      label: 'Undo change', onConfirm: () => { void task('Restoring prior state', async () => {
        try { showEntry(await call<Entry>('undo', { selection, id: entry.id })) }
        finally { refresh() }
      }) } })
  }

  function diagnostics() {
    if (!selection) return
    confirmAction({ title: 'Export Android diagnostics?', description: 'The most recent device logs may include private information from Android and its apps. Review the text file before sharing it.',
      label: 'Choose save location', onConfirm: () => exportFile('export_diagnostics', { selection }) })
  }

  const readyCount = list?.devices.filter(item => item.state === 'device').length ?? 0
  const error = snapshotError || list?.error || null
  const disabled = Boolean(busy)
  const readyRows = review?.result.rows.filter(row => !row.reason).length ?? 0
  return <ThemeProvider theme={theme}>
    <CssBaseline enableColorScheme />
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true"><PhoneIcon fontSize="small" /></div>
          <div className="brand-copy"><strong>Xiaomi Debloat</strong><span>Android device manager</span></div>
        </div>
        <nav className="nav" aria-label="Main navigation">
          {nav.map(item => <Button key={item.id} className="nav-item" aria-label={item.label} aria-current={page === item.id ? 'page' : undefined}
            onClick={() => openPage(item.id)}><item.icon fontSize="small" /><span className="nav-label">{item.label}</span></Button>)}
        </nav>
        <div className="sidebar-bottom">
          <Tooltip title={list?.adbPath || 'Select the official adb.exe'}>
            <Button className="nav-item" aria-label="ADB setup" disabled={disabled} onClick={chooseAdb}><AdbIcon fontSize="small" /><span className="nav-label">ADB setup</span></Button>
          </Tooltip>
          <div className="health"><span className={`dot ${list?.adbPath && !list.error ? 'good' : 'warn'}`} />
            <span>{list === null ? 'Looking for ADB' : list.error ? 'ADB unavailable' : list.adbPath ? 'ADB ready' : 'ADB setup required'}</span>
          </div>
        </div>
      </aside>
      <header className="topbar">
        <div className="device-picker">
          <Select size="small" displayEmpty value={transport?.toString() ?? ''} disabled={disabled || !list?.devices.length}
            onChange={event => {
              hasSelected.current = true
              setTransport(event.target.value === '' ? null : Number(event.target.value))
            }} inputProps={{ 'aria-label': 'Select Android device' }}>
            <MenuItem value="">{list === null ? 'Detecting devices…' : list.devices.length ? 'Select a device' : 'No devices detected'}</MenuItem>
            {list?.devices.map(item => <MenuItem value={item.transport.toString()} key={item.transport}>
              {item.model} · {item.state === 'device' ? item.connection : item.state}
            </MenuItem>)}
          </Select>
          {snap && <Chip size="small" label={`User ${snap.info.user}`} />}
          {!transport && readyCount > 1 && <span className="small muted">{readyCount} ready</span>}
        </div>
        <div className="top-actions">
          <Tooltip title="Refresh devices and inventory"><span>
            <IconButton size="small" aria-label="Refresh devices and inventory" disabled={disabled} onClick={refresh}><RefreshIcon fontSize="small" /></IconButton>
          </span></Tooltip>
          <Button size="small" startIcon={<WifiIcon fontSize="small" />} color="inherit" disabled={disabled} onClick={() => setWireless(true)}>Wireless</Button>
          <Tooltip title={themeMode === 'dark' ? 'Use light theme' : 'Use dark theme'}>
            <IconButton size="small" aria-label={themeMode === 'dark' ? 'Use light theme' : 'Use dark theme'}
              onClick={() => setThemeMode(themeMode === 'dark' ? 'light' : 'dark')}>{themeMode === 'dark' ? <LightIcon fontSize="small" /> : <DarkIcon fontSize="small" />}</IconButton>
          </Tooltip>
        </div>
      </header>
      <main className="main">
        {busy && <div className="job-progress" aria-live="polite">
          <LinearProgress variant={progress ? 'determinate' : 'indeterminate'} value={progress ? 100 * progress.count / progress.total : undefined} />
          <p className="small muted">{busy}{progress ? ` · ${progress.count}/${progress.total}${progress.latest ? ` · ${progress.latest}` : ''}` : '…'}</p>
        </div>}
        {page !== 'overview' && (error || loading) && <Alert severity={error ? 'warning' : 'info'} sx={{ mb: 2 }}>{error || 'Reading device inventory…'}</Alert>}
        {page === 'overview' && <Overview snap={snap} device={device ?? (list?.devices.length === 1 && !readyCount ? list.devices[0] : undefined)} readyCount={readyCount} hasAdb={!!list?.adbPath} error={error} busy={disabled}
          loading={loading || list === null} onAdb={chooseAdb} onWireless={() => setWireless(true)} onGuide={guide} onOpen={openPage} />}
        {page === 'apps' && <Apps key={snap?.info.identity ?? 'offline'} snap={snap} metadata={metadata} review={reviewActions} busy={disabled}
          savePreset={saveSelection} goOverview={() => openPage('overview')} />}
        {page === 'presets' && <Presets snap={snap} metadata={metadata} saved={saved} busy={disabled} review={reviewActions}
          onImport={() => { void task('Importing preset', async () => {
            const preset = await call<Preset | null>('import_preset')
            if (preset) { await loadPresets(); setNotice({ severity: 'success', text: `Imported ${preset.name}.` }) }
          }) }}
          onExport={name => exportFile('export_preset', { name })} onDelete={deletePreset} />}
        {page === 'tweaks' && <Tweaks snap={snap} metadata={metadata} busy={disabled} review={reviewActions} />}
        {page === 'install' && <Install snap={snap} busy={disabled} pick={split => task('Choosing APK files', () => call<ApkFile[]>('pick_apks', { split }))}
          onInstall={installFiles} />}
        {page === 'activity' && <Activity entries={entries} activeDevice={!!snap} scoped={scoped} busy={disabled} onScope={setScoped}
          onUndo={undoEntry} onCheck={entry => { if (selection) void task('Checking operation outcome', async () => {
            try { showEntry(await call<Entry>('reconcile', { selection, id: entry.id })) }
            finally { refresh() }
          }) }} onExport={format => exportFile('export_activity', { format, identity: historyIdentity })}
          onDiagnostics={diagnostics} />}
      </main>
    </div>
    {wireless && <WirelessDialog disabled={disabled} adbReady={!!list?.adbPath && !list.error}
      transport={device?.connection === 'Wireless' ? device.transport : undefined}
      onClose={() => setWireless(false)} onSetup={() => { setWireless(false); chooseAdb() }}
      onSubmit={request => { void task('Updating wireless connection', async () => {
        const text = await call<string>('connection', { request })
        setNotice({ severity: 'info', text })
        if (request.kind !== 'pair') setWireless(false)
        refresh()
      }) }} />}
    {review && <Dialog open maxWidth="sm" fullWidth onClose={() => setReview(null)} aria-labelledby="review-title">
      <DialogTitle id="review-title">Review {review.result.rows.length === 1 ? 'change' : 'changes'}</DialogTitle>
      <DialogContent dividers>
        <div className="section-head"><span className="small muted">{review.info}</span><Chip size="small" label={`${readyRows} ready`} /></div>
        {review.advanced && <Alert severity="warning" sx={{ mb: 1 }}>Other apps or system features may depend on this package.</Alert>}
        {review.result.rows.some(row => row.action.kind === 'package' && row.action.verb === 'remove' && !row.reason) &&
          <Alert severity="warning" sx={{ mb: 1 }}>Removal applies to this Android user. Android is asked to keep app data. Restoring some apps requires the original APK.</Alert>}
        {review.result.rows.map(row => <div className="review-row" key={row.action.id}>
          <strong className="small text-wrap">{row.label}</strong>
          {row.reason ? <span className="small warn-text">Skip · {row.reason}</span> :
            <span className="small muted">{row.current} → {row.next}</span>}
        </div>)}
      </DialogContent>
      <DialogActions>
        <Button onClick={() => setReview(null)}>{readyRows ? 'Cancel' : 'Close'}</Button>
        {readyRows > 0 && <Button variant="contained" color={review.result.rows.some(row => row.action.kind === 'package' && row.action.verb === 'remove' && !row.reason) ? 'error' : 'primary'}
          onClick={() => executePreview(review.result)}>Apply {readyRows} {readyRows === 1 ? 'change' : 'changes'}</Button>}
      </DialogActions>
    </Dialog>}
    {saving && <Dialog open fullWidth maxWidth="xs" onClose={() => { if (!disabled) setSaving(null) }} aria-labelledby="preset-title">
      <DialogTitle id="preset-title">Save a preset</DialogTitle>
      <DialogContent dividers>
        <p className="small muted" style={{ marginTop: 0 }}>Save {saving.length} package IDs for future disable previews.</p>
        <TextField autoFocus fullWidth size="small" label="Preset name" value={presetName} disabled={disabled}
          onChange={event => setPresetName(event.target.value)} slotProps={{ htmlInput: { maxLength: 64 } }}
          onKeyDown={event => { if (event.key === 'Enter' && presetName.trim()) savePreset() }} />
      </DialogContent>
      <DialogActions><Button disabled={disabled} onClick={() => setSaving(null)}>Cancel</Button>
        <Button variant="contained" disabled={!presetName.trim() || disabled} onClick={savePreset}>Save preset</Button></DialogActions>
    </Dialog>}
    {confirm && <Dialog open fullWidth maxWidth="sm" onClose={() => setConfirm(null)} aria-labelledby="confirm-title">
      <DialogTitle id="confirm-title">{confirm.title}</DialogTitle>
      <DialogContent dividers><p className="small text-wrap" style={{ margin: 0 }}>{confirm.description}</p></DialogContent>
      <DialogActions><Button onClick={() => setConfirm(null)}>Cancel</Button>
        <Button variant="contained" color={confirm.danger ? 'error' : 'primary'} onClick={() => { setConfirm(null); confirm.onConfirm() }}>{confirm.label}</Button></DialogActions>
    </Dialog>}
    <Snackbar open={!!notice} autoHideDuration={notice?.severity === 'error' ? 12000 : 7000}
      onClose={(_, reason) => { if (reason !== 'clickaway') setNotice(null) }} anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
      sx={{ zIndex: theme.zIndex.modal + 20 }}>
      {notice ? <Alert severity={notice.severity} variant="filled" onClose={() => setNotice(null)}
        sx={{ maxWidth: 510, overflowWrap: 'anywhere' }}>{notice.text}</Alert> : undefined}
    </Snackbar>
  </ThemeProvider>
}

type WirelessRequest = { kind: 'pair'; endpoint: string; code: string } |
  { kind: 'connect'; endpoint: string } | { kind: 'disconnect'; transport: number }

function WirelessDialog({ disabled, adbReady, transport, onClose, onSetup, onSubmit }: {
  disabled: boolean; adbReady: boolean; transport?: number; onClose: () => void; onSetup: () => void;
  onSubmit: (request: WirelessRequest) => void;
}) {
  const [mode, setMode] = useState<'pair' | 'connect'>('pair')
  const [pairAddress, setPairAddress] = useState('')
  const [connectAddress, setConnectAddress] = useState('')
  const [code, setCode] = useState('')
  const endpoint = mode === 'pair' ? pairAddress : connectAddress
  const canSubmit = adbReady && !disabled && endpoint.trim() && (mode === 'connect' || /^[0-9]{6}$/.test(code))
  const submit = () => {
    if (!canSubmit) return
    if (mode === 'pair') { onSubmit({ kind: 'pair', endpoint, code }); setCode('') }
    else onSubmit({ kind: 'connect', endpoint })
  }
  return <Dialog open maxWidth="sm" fullWidth onClose={disabled ? undefined : onClose} aria-labelledby="wireless-title">
    <DialogTitle id="wireless-title">Wireless debugging</DialogTitle>
    <DialogContent dividers>
      {!adbReady && <Alert severity="warning" action={<Button size="small" onClick={onSetup}>Choose ADB</Button>}>Set up ADB before pairing.</Alert>}
      <p className="small muted">Connect the PC and phone to the same network. On the phone, open Developer options and turn on Wireless debugging.</p>
      <Tabs value={mode} onChange={(_, value) => setMode(value)} aria-label="Wireless connection method">
        <Tab disabled={disabled} value="pair" label="Pair new device" /><Tab disabled={disabled} value="connect" label="Connect paired device" />
      </Tabs>
      {mode === 'pair' ? <div className="stack" style={{ paddingTop: 13 }}>
        <p className="small muted" style={{ margin: 0 }}>Tap “Pair device with pairing code” on the phone, then enter the address and six-digit code shown there.</p>
        <TextField size="small" label="Pairing IP and port" value={pairAddress} placeholder="192.168.1.20:37001" disabled={disabled}
          onChange={event => setPairAddress(event.target.value)} />
        <TextField size="small" label="Six-digit pairing code" type="password" value={code} disabled={disabled} autoComplete="off"
          slotProps={{ htmlInput: { inputMode: 'numeric', maxLength: 6 } }} onChange={event => setCode(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') submit() }} />
      </div> : <div className="stack" style={{ paddingTop: 13 }}>
        <p className="small muted" style={{ margin: 0 }}>Use the address on the main Wireless debugging screen. Android shows a separate port here.</p>
        <TextField size="small" label="Connection IP and port" value={connectAddress} placeholder="192.168.1.20:38901" disabled={disabled}
          onChange={event => setConnectAddress(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') submit() }} />
      </div>}
    </DialogContent>
    <DialogActions>
      {transport !== undefined && <Button color="error" sx={{ mr: 'auto' }} disabled={disabled}
        onClick={() => onSubmit({ kind: 'disconnect', transport })}>Disconnect</Button>}
      <Button disabled={disabled} onClick={onClose}>Close</Button>
      <Button variant="contained" disabled={!canSubmit} onClick={submit}>{mode === 'pair' ? 'Pair device' : 'Connect'}</Button>
    </DialogActions>
  </Dialog>
}
