import { useEffect, useMemo, useState, type ReactNode } from 'react'
import {
  Alert, Button, Checkbox, Chip, FormControl, FormControlLabel, IconButton, InputAdornment, InputLabel,
  MenuItem, Select, Table, TableBody, TableCell, TableHead, TablePagination, TableRow, TextField, Tooltip,
} from '@mui/material'
import AppsIcon from '@mui/icons-material/AppsRounded'
import ArchiveIcon from '@mui/icons-material/Inventory2Outlined'
import ArrowIcon from '@mui/icons-material/ArrowForwardRounded'
import CloseIcon from '@mui/icons-material/CloseRounded'
import DownloadIcon from '@mui/icons-material/DownloadRounded'
import HistoryIcon from '@mui/icons-material/HistoryRounded'
import PackageIcon from '@mui/icons-material/AndroidRounded'
import PhoneIcon from '@mui/icons-material/PhoneAndroidRounded'
import SearchIcon from '@mui/icons-material/SearchRounded'
import ShieldIcon from '@mui/icons-material/ShieldOutlined'
import UploadIcon from '@mui/icons-material/FileUploadOutlined'
import UsbIcon from '@mui/icons-material/UsbRounded'
import WifiIcon from '@mui/icons-material/WifiRounded'
import TuneIcon from '@mui/icons-material/TuneRounded'
import InstallIcon from '@mui/icons-material/InstallMobileRounded'
import PresetIcon from '@mui/icons-material/ViewQuiltOutlined'
import type { Action, ApkFile, Device, Entry, Metadata, Package, Preset, Snapshot, Verb } from './api'

export type Page = 'overview' | 'apps' | 'presets' | 'tweaks' | 'install' | 'activity'
export type Review = (actions: Action[], advanced?: boolean, xiaomiOnly?: boolean) => void

export function Heading({ title, subtitle, actions }: { title: string; subtitle: string; actions?: ReactNode }) {
  return <header className="page-heading">
    <div><h1>{title}</h1><p>{subtitle}</p></div>
    {actions && <div className="actions">{actions}</div>}
  </header>
}

export function Empty({ title, text, icon, action }: { title: string; text: string; icon?: ReactNode; action?: ReactNode }) {
  return <div className="panel empty">
    {icon ?? <PhoneIcon fontSize="large" />}
    <h2>{title}</h2>
    <p className="muted small">{text}</p>
    {action}
  </div>
}

export function Overview({ snap, device, readyCount, hasAdb, error, loading, busy, onAdb, onWireless, onGuide, onOpen }: {
  snap: Snapshot | null; device?: Device; readyCount: number; hasAdb: boolean; error: string | null; loading: boolean; busy: boolean;
  onAdb: () => void; onWireless: () => void; onGuide: (name: string) => void; onOpen: (page: Page) => void;
}) {
  const state = device?.state
  const chooseDevice = readyCount > 0 && !device
  const counts = snap ? [
    ['Installed', snap.packages.filter(pkg => pkg.installed).length, <AppsIcon fontSize="small" />],
    ['System packages', snap.packages.filter(pkg => pkg.system).length, <ShieldIcon fontSize="small" />],
    ['Disabled or removed', snap.packages.filter(pkg => pkg.disabled || !pkg.installed).length, <ArchiveIcon fontSize="small" />],
  ] as const : []
  return <>
    <Heading title="Device overview" subtitle="Manage the apps and settings on your Android device." />
    <div className="overview-grid">
      {snap ? <>
        <section className="panel device-card" aria-label="Connected device">
          <div className="device-art"><PhoneIcon sx={{ fontSize: 59, strokeWidth: 0.5 }} /></div>
          <div className="device-copy">
            <div className="status-line"><span className="dot good" /> Connected over {device?.connection ?? 'ADB'}</div>
            <h2 className="text-wrap">{snap.info.model || device?.model}</h2>
            <p className="small muted text-wrap">{snap.info.build || snap.info.brand}</p>
          </div>
          <dl className="device-meta">
            <div><dt>Android</dt><dd>{snap.info.android}</dd></div>
            <div><dt>Active user</dt><dd>User {snap.info.user}</dd></div>
            <div><dt>Brand</dt><dd>{snap.info.brand || 'Unknown'}</dd></div>
          </dl>
        </section>
        <div className="stat-grid">{counts.map(([label, count, icon]) =>
          <div className="panel stat" key={label}><div className="stat-top">{icon}{label}</div><strong>{count}</strong></div>
        )}</div>
        <section className="panel panel-pad">
          <div className="section-head"><h2>Device tools</h2></div>
          <div className="quick-links">
            <Button className="quick-link" variant="outlined" onClick={() => onOpen('apps')}><span><AppsIcon /> Browse apps</span><ArrowIcon fontSize="small" /></Button>
            <Button className="quick-link" variant="outlined" onClick={() => onOpen('presets')}><span><PresetIcon /> Debloat presets</span><ArrowIcon fontSize="small" /></Button>
            <Button className="quick-link" variant="outlined" onClick={() => onOpen('tweaks')}><span><TuneIcon /> Tune settings</span><ArrowIcon fontSize="small" /></Button>
          </div>
        </section>
        {!snap.protectionReady && <Alert severity="warning">Android’s active roles could not be checked. Disable and removal are paused until a refresh succeeds.</Alert>}
      </> : <>
        <Empty title={loading ? 'Reading device' : chooseDevice ? 'Select an Android device' : state === 'unauthorized' ? 'Approve USB debugging' : device ? 'Device unavailable' : 'Connect an Android device'}
          text={loading ? 'Fetching packages and settings from the selected device.' : chooseDevice ? 'Choose a connected device from the top bar.' : state === 'unauthorized' ? 'Unlock the phone and allow USB debugging when Android prompts you.' : error || (device ? 'Check the connection, then refresh the device list.' : 'Use a USB data cable or pair with Android Wireless debugging.')}
          action={!chooseDevice && <Button variant="contained" color="primary" disabled={busy} startIcon={<WifiIcon />} onClick={onWireless}>Wireless setup</Button>} />
        <div className="setup-grid">
          <section className="panel panel-pad">
            <div className="section-head"><h2>Connect with USB</h2><UsbIcon className="muted" /></div>
            <ol className="setup-steps"><li>Enable Developer options and USB debugging.</li><li>Connect the phone with a USB data cable.</li><li>Unlock it and approve your computer.</li></ol>
            <div className="actions">
              <Button onClick={() => onGuide('usb')}>Android guide</Button>
              <Button onClick={() => onGuide('xiaomi')}>Xiaomi guide</Button>
            </div>
          </section>
          <section className="panel panel-pad">
            <div className="section-head"><h2>Android Platform-Tools</h2><span className={hasAdb ? 'dot good' : 'dot warn'} /></div>
            <p className="muted small">{hasAdb ? 'ADB is ready. You can switch to another Platform-Tools installation.' : 'Download and extract the Windows Platform-Tools, then select adb.exe.'}</p>
            <div className="actions">
              <Button variant="outlined" disabled={busy} onClick={onAdb}>Choose adb.exe</Button>
              <Button onClick={() => onGuide('platform-tools')}>Download</Button>
            </div>
          </section>
        </div>
      </>}
    </div>
  </>
}

function packageStatus(pkg: Package) {
  return !pkg.installed ? 'Removed for user' : pkg.disabled ? 'Disabled' : 'Enabled'
}

export function Apps({ snap, metadata, review, busy, savePreset, goOverview }: {
  snap: Snapshot | null; metadata: Metadata; review: Review; busy: boolean;
  savePreset: (packages: string[]) => void; goOverview: () => void;
}) {
  const [search, setSearch] = useState('')
  const [category, setCategory] = useState('all')
  const [status, setStatus] = useState('all')
  const [advanced, setAdvanced] = useState(false)
  const [active, setActive] = useState<string | null>(null)
  const [selected, setSelected] = useState<string[]>([])
  const [page, setPage] = useState(0)
  const [size, setSize] = useState(25)
  const catalog = useMemo(() => new Map(metadata.catalog.map(entry => [entry.id, entry])), [metadata.catalog])
  const detail = snap?.packages.find(pkg => pkg.id === active)
  const eligible = (pkg: Package) => pkg.installed && !pkg.protection && (!pkg.system || catalog.has(pkg.id))
  const filtered = snap?.packages.filter(pkg => {
    const query = search.trim().toLowerCase()
    if (query && !pkg.id.toLowerCase().includes(query) && !catalog.get(pkg.id)?.name.toLowerCase().includes(query)) return false
    if (category === 'system' && !pkg.system || category === 'user' && pkg.system || category === 'catalog' && !catalog.has(pkg.id)) return false
    return status === 'all' || status === 'enabled' && pkg.installed && !pkg.disabled ||
      status === 'disabled' && pkg.installed && pkg.disabled || status === 'removed' && !pkg.installed
  }) ?? []
  const currentPage = Math.min(page, Math.max(0, Math.ceil(filtered.length / size) - 1))
  const visible = filtered.slice(currentPage * size, (currentPage + 1) * size)
  const pageIds = visible.filter(eligible).map(pkg => pkg.id)
  const effectiveSelection = snap ? selected.filter(id => snap.packages.some(pkg => pkg.id === id && eligible(pkg))) : []
  const checkedPage = pageIds.length > 0 && pageIds.every(id => effectiveSelection.includes(id))
  const togglePage = () => setSelected(checkedPage ?
    effectiveSelection.filter(id => !pageIds.includes(id)) :
    [...new Set([...effectiveSelection, ...pageIds])].slice(0, 150))
  const batch = (verb: Verb) => review(effectiveSelection.map(id => ({ kind: 'package', id, verb })))
  const single = (pkg: Package, verb: Verb) => review([{ kind: 'package', id: pkg.id, verb }], advanced && pkg.system && !catalog.has(pkg.id))
  const detailRestricted = detail?.system && !catalog.has(detail.id)
  return <>
    <Heading title="Apps" subtitle={snap ? `Browse packages for Android user ${snap.info.user}.` : 'Browse and manage installed and system packages.'}
      actions={snap && <FormControlLabel control={<Checkbox size="small" checked={advanced} onChange={event => setAdvanced(event.target.checked)} />} label="Advanced mode" />} />
    {!snap ? <Empty title="No device selected" text="Connect a device to load its app inventory." icon={<AppsIcon fontSize="large" />}
      action={<Button onClick={goOverview}>Open connection setup</Button>} /> :
      <div className={detail ? 'package-grid with-detail' : 'package-grid'}>
        <section className="panel" aria-label="Package inventory">
          <div className="table-tools">
            <TextField size="small" className="search" placeholder="Search apps or package IDs" value={search}
              onChange={event => { setSearch(event.target.value); setPage(0) }}
              slotProps={{ htmlInput: { 'aria-label': 'Search packages' }, input: { startAdornment: <InputAdornment position="start"><SearchIcon fontSize="small" /></InputAdornment> } }} />
            <FormControl size="small" className="filter"><InputLabel id="category">Type</InputLabel>
              <Select labelId="category" label="Type" value={category} onChange={event => { setCategory(event.target.value); setPage(0) }}>
                <MenuItem value="all">All types</MenuItem><MenuItem value="system">System</MenuItem><MenuItem value="user">User apps</MenuItem><MenuItem value="catalog">Debloat catalog</MenuItem>
              </Select>
            </FormControl>
            <FormControl size="small" className="filter"><InputLabel id="package-status">State</InputLabel>
              <Select labelId="package-status" label="State" value={status} onChange={event => { setStatus(event.target.value); setPage(0) }}>
                <MenuItem value="all">All states</MenuItem><MenuItem value="enabled">Enabled</MenuItem><MenuItem value="disabled">Disabled</MenuItem><MenuItem value="removed">Removed</MenuItem>
              </Select>
            </FormControl>
          </div>
          {effectiveSelection.length > 0 && <div className="table-tools">
            <span className="small muted">{effectiveSelection.length} selected</span>
            <Button variant="contained" size="small" disabled={busy} onClick={() => batch('disable')}>Review disable</Button>
            <Button size="small" color="error" disabled={busy} onClick={() => batch('remove')}>Review removal</Button>
            <Button size="small" disabled={busy} onClick={() => savePreset(effectiveSelection)}>Save preset</Button>
            <Button size="small" onClick={() => setSelected([])}>Clear</Button>
          </div>}
          <div className="table-wrap">
            <Table size="small" aria-label="Android packages">
              <TableHead><TableRow>
                <TableCell padding="checkbox"><Checkbox size="small" disabled={pageIds.length === 0} checked={checkedPage}
                  indeterminate={!checkedPage && pageIds.some(id => effectiveSelection.includes(id))} onChange={togglePage}
                  slotProps={{ input: { 'aria-label': 'Select eligible packages on this page' } }} /></TableCell>
                <TableCell>Package</TableCell><TableCell>Type</TableCell><TableCell>State</TableCell>
              </TableRow></TableHead>
              <TableBody>{visible.map(pkg => {
                const known = catalog.get(pkg.id)
                const checked = effectiveSelection.includes(pkg.id)
                return <TableRow key={pkg.id} hover selected={pkg.id === active}>
                  <TableCell padding="checkbox"><Checkbox size="small" checked={checked}
                    disabled={!eligible(pkg) || !checked && effectiveSelection.length >= 150}
                    onChange={() => setSelected(checked ? effectiveSelection.filter(id => id !== pkg.id) : [...effectiveSelection, pkg.id])}
                    slotProps={{ input: { 'aria-label': `Select ${pkg.id}` } }} /></TableCell>
                  <TableCell><button className="package-link" onClick={() => setActive(pkg.id)}>
                    {known ? <><span className="package-name">{known.name}</span><span className="package-id mono">{pkg.id}</span></> :
                      <span className="package-name mono">{pkg.id}</span>}
                  </button></TableCell>
                  <TableCell><div className="chips"><Chip size="small" label={pkg.system ? 'System' : 'User'} />
                    {pkg.protection && <Tooltip title={pkg.protection}><ShieldIcon className="muted" fontSize="small" aria-label="Protected" /></Tooltip>}
                  </div></TableCell>
                  <TableCell><span className={`small ${pkg.disabled || !pkg.installed ? 'warn-text' : 'muted'}`}>{packageStatus(pkg)}</span></TableCell>
                </TableRow>
              })}{visible.length === 0 && <TableRow><TableCell colSpan={4}>No packages match these filters.</TableCell></TableRow>}</TableBody>
            </Table>
          </div>
          <TablePagination component="div" count={filtered.length} page={currentPage} rowsPerPage={size} rowsPerPageOptions={[25, 50, 100]}
            onPageChange={(_, value) => setPage(value)} onRowsPerPageChange={event => { setSize(Number(event.target.value)); setPage(0) }} />
        </section>
        {detail && <aside className="panel panel-pad package-detail" aria-label="Package details">
          <div className="detail-top"><h2 className="text-wrap">{catalog.get(detail.id)?.name ?? 'Package details'}</h2>
            <IconButton size="small" aria-label="Close package details" onClick={() => setActive(null)}><CloseIcon fontSize="small" /></IconButton>
          </div>
          <p className="mono small text-wrap">{detail.id}</p>
          <div className="chips"><Chip size="small" label={packageStatus(detail)} /><Chip size="small" label={detail.system ? 'System app' : 'User app'} /></div>
          <p className="small muted">{catalog.get(detail.id)?.description ?? 'Android reports the package ID and current state for this app.'}</p>
          {detail.protection && <Alert severity="info" sx={{ my: 1.5 }}>{detail.protection}</Alert>}
          {!detail.protection && detail.installed && detailRestricted && <p className="small warn-text">
            {advanced ? 'Other apps or system features may depend on this package.' : 'Turn on Advanced mode to review an individual change.'}
          </p>}
          <div className="actions">
            {!detail.installed && <Button variant="contained" disabled={busy} onClick={() => single(detail, 'restore')}>Restore</Button>}
            {detail.installed && detail.disabled && <Button variant="contained" disabled={busy} onClick={() => single(detail, 'enable')}>Enable</Button>}
            {detail.installed && !detail.protection && (!detailRestricted || advanced) && <>
              {!detail.disabled && <Button variant="contained" disabled={busy} onClick={() => single(detail, 'disable')}>Review disable</Button>}
              <Button color="error" disabled={busy} onClick={() => single(detail, 'remove')}>Review removal</Button>
            </>}
          </div>
        </aside>}
      </div>}
  </>
}

export function Presets({ snap, metadata, saved, busy, review, onImport, onExport, onDelete }: {
  snap: Snapshot | null; metadata: Metadata; saved: Preset[]; busy: boolean; review: Review;
  onImport: () => void; onExport: (name: string) => void; onDelete: (name: string) => void;
}) {
  const packages = new Set(snap?.packages.filter(pkg => pkg.installed && !pkg.disabled && !pkg.protection).map(pkg => pkg.id))
  const card = (preset: Preset, builtin: boolean) => {
    const matches = preset.packages.filter(id => packages.has(id)).length
    const available = !!snap && (!builtin || snap.info.xiaomi)
    return <article className="panel panel-pad" key={preset.name}>
      <h2>{preset.name}</h2>
      <p className="muted small">{builtin ? (preset.name === 'Ads and Analytics' ? 'Disable Xiaomi advertising and analytics packages.' :
        'Disable selected Xiaomi and partner preloads.') : `${preset.packages.length} saved package IDs.`}</p>
      <p className="small muted">{available ? `${matches} of ${preset.packages.length} packages currently enabled` :
        builtin ? 'Available on Xiaomi, Redmi, and POCO devices' : 'Connect a device to preview this preset'}</p>
      <div className="actions">
        <Button variant="outlined" disabled={!available || busy} onClick={() =>
          review(preset.packages.map(id => ({ kind: 'package', id, verb: 'disable' })), false, builtin)}>Review preset</Button>
        <Button color="error" disabled={!available || busy} onClick={() =>
          review(preset.packages.map(id => ({ kind: 'package', id, verb: 'remove' })), false, builtin)}>Review removal</Button>
        {!builtin && <>
          <Button disabled={busy} onClick={() => onExport(preset.name)}>Export</Button>
          <Button disabled={busy} color="error" onClick={() => onDelete(preset.name)}>Delete</Button>
        </>}
      </div>
    </article>
  }
  return <>
    <Heading title="Debloat presets" subtitle="Apply a curated selection or reuse one of your own."
      actions={<Button variant="outlined" startIcon={<UploadIcon />} disabled={busy} onClick={onImport}>Import JSON</Button>} />
    <div className="stack">
      <h2 className="section-title">Xiaomi presets</h2>
      <div className="preset-grid">{metadata.builtIn.map(preset => card(preset, true))}</div>
      <h2 className="section-title" style={{ marginTop: 8 }}>Saved presets</h2>
      {saved.length > 0 ? <div className="preset-grid">{saved.map(preset => card(preset, false))}</div> :
        <Empty title="No saved presets" text="Select packages in Apps to save a preset, or import a JSON preset." icon={<PresetIcon fontSize="large" />} />}
    </div>
  </>
}

export function Tweaks({ snap, metadata, busy, review }: {
  snap: Snapshot | null; metadata: Metadata; busy: boolean; review: Review;
}) {
  const [draft, setDraft] = useState<Record<string, string>>({})
  useEffect(() => { setDraft({}) }, [snap])
  const pending: Action[] = Object.entries(draft).map(([id, value]) => ({ kind: 'setting', id, value }))
  const animations = metadata.settings.filter(spec => spec.id.endsWith('_scale'))
  const canAnimate = !!snap && animations.length === 3 && animations.every(spec => !snap.unavailableSettings.includes(spec.id))
  return <>
    <Heading title="Device tweaks" subtitle="Adjust animations, display behavior, and power preferences."
      actions={<Button variant="contained" disabled={!pending.length || busy} onClick={() => review(pending)}>Review changes{pending.length ? ` (${pending.length})` : ''}</Button>} />
    {!snap ? <Empty title="No device selected" text="Connect a device to read and change its settings." icon={<TuneIcon fontSize="large" />} /> :
      <div className="stack">
        <section className="panel panel-pad">
          <div className="section-head">
            <div><h2>Faster Animations</h2><p className="muted small">Use 0.5× scales to shorten visual transitions.</p></div>
            <Button variant="outlined" disabled={!canAnimate || busy}
              onClick={() => review(animations.map(spec => ({ kind: 'setting', id: spec.id, value: '0.5' })))}>Review preset</Button>
          </div>
        </section>
        <section className="panel panel-pad">
          <div className="section-head"><h2>Android settings</h2></div>
          {metadata.settings.map(spec => {
            const actual = snap.settings[spec.id]
            const option = spec.options.find(([key]) => actual != null && Number(key) === Number(actual))?.[0]
            const current = option ?? ''
            const unavailable = snap.unavailableSettings.includes(spec.id)
            return <div className="tweak-row" key={spec.id}>
              <div>
                <strong className="small">{spec.label}</strong>
                <p className="muted small">{spec.description}</p>
                <p className="small subtle">{spec.namespace === 'global' ? 'Device-wide' : `User ${snap.info.user}`}</p>
              </div>
              <FormControl size="small">
                <Select displayEmpty value={draft[spec.id] ?? current} disabled={unavailable || busy}
                  inputProps={{ 'aria-label': spec.label }} onChange={event => setDraft(previous => {
                    const next = { ...previous }
                    if (event.target.value === current) delete next[spec.id]
                    else next[spec.id] = event.target.value
                    return next
                  })}>
                  {!option && <MenuItem value="">{unavailable ? 'Unavailable' : (actual ?? 'Device default')}</MenuItem>}
                  {spec.options.map(([key, label]) => <MenuItem key={key} value={key}>{label}</MenuItem>)}
                </Select>
              </FormControl>
            </div>
          })}
        </section>
      </div>}
  </>
}

export function Install({ snap, busy, pick, onInstall }: {
  snap: Snapshot | null; busy: boolean; pick: (split: boolean) => Promise<ApkFile[] | undefined>;
  onInstall: (files: ApkFile[], replace: boolean, split: boolean) => void;
}) {
  const [split, setSplit] = useState(false)
  const [files, setFiles] = useState<ApkFile[]>([])
  const [replace, setReplace] = useState(false)
  async function choose(mode: boolean) {
    const chosen = await pick(mode)
    if (chosen?.length) { setFiles(chosen); setSplit(mode) }
  }
  const ready = split ? files.length >= 2 && files.length <= 65 : files.length === 1
  return <>
    <Heading title="Install apps" subtitle="Sideload a local APK or a base APK with its split files." />
    <section className="panel panel-pad">
      <div className="section-head"><h2>APK files</h2><InstallIcon className="muted" /></div>
      <div className="actions">
        <Button variant="outlined" startIcon={<PackageIcon />} disabled={busy} onClick={() => void choose(false)}>Choose APK</Button>
        <Button variant="outlined" startIcon={<ArchiveIcon />} disabled={busy} onClick={() => void choose(true)}>Choose split APK set</Button>
      </div>
      <p className="muted small">{split ? 'Select the base APK and all required splits together.' : 'Choose a standalone .apk file to install.'}</p>
      {files.map(file => <div className="file-row" key={file.path}><span className="mono small">{file.name}</span>
        <IconButton size="small" aria-label={`Remove ${file.name} from selection`} disabled={busy}
          onClick={() => setFiles(previous => previous.filter(item => item.path !== file.path))}><CloseIcon fontSize="small" /></IconButton>
      </div>)}
      <FormControlLabel control={<Checkbox checked={replace} onChange={event => setReplace(event.target.checked)} />} label="Replace existing app when signatures match" />
      <div className="actions" style={{ marginTop: 10 }}>
        <Button variant="contained" disabled={!snap || !ready || busy} onClick={() => onInstall(files, replace, split)}>Review installation</Button>
        <span className="muted small">{snap ? `${snap.info.model} · User ${snap.info.user}` : 'Connect a device to install.'}</span>
      </div>
    </section>
  </>
}

export function Activity({ entries, activeDevice, scoped, busy, onScope, onUndo, onCheck, onExport, onDiagnostics }: {
  entries: Entry[]; activeDevice: boolean; scoped: boolean; busy: boolean; onScope: (value: boolean) => void;
  onUndo: (entry: Entry) => void; onCheck: (entry: Entry) => void; onExport: (format: 'json' | 'csv') => void; onDiagnostics: () => void;
}) {
  return <>
    <Heading title="Activity" subtitle="Review device changes, recover earlier settings, and export logs."
      actions={<>
        <Button variant="outlined" startIcon={<DownloadIcon />} disabled={busy} onClick={() => onExport('json')}>JSON</Button>
        <Button variant="outlined" startIcon={<DownloadIcon />} disabled={busy} onClick={() => onExport('csv')}>CSV</Button>
        <Button variant="outlined" disabled={busy || !activeDevice} onClick={onDiagnostics}>Export Android logs</Button>
      </>} />
    <div className="stack">
      <div className="panel panel-pad">
        <div className="section-head" style={{ marginBottom: entries.length ? 4 : 0 }}>
          <h2>Operation history</h2>
          {activeDevice && <FormControlLabel control={<Checkbox size="small" checked={scoped} onChange={event => onScope(event.target.checked)} />} label="Selected device and user" />}
        </div>
        {entries.map(entry => <div className="activity-row" key={entry.id}>
          <div>
            <strong className="small text-wrap">{entry.label}</strong>
            <p className="small muted">{new Date(entry.at).toLocaleString()} · {entry.device} · User {entry.user}</p>
            {entry.detail && <p className="small subtle text-wrap">{entry.detail}</p>}
          </div>
          <div className="actions">
            <Chip size="small" color={entry.status === 'success' ? 'success' : entry.status === 'failed' ? 'error' :
              entry.status === 'unknown' ? 'warning' : 'default'} label={entry.status} />
            {activeDevice && scoped && entry.canUndo && <Button size="small" disabled={busy} onClick={() => onUndo(entry)}>Undo</Button>}
            {activeDevice && scoped && entry.status === 'unknown' && entry.action && <Button size="small" disabled={busy} onClick={() => onCheck(entry)}>Check outcome</Button>}
          </div>
        </div>)}
      </div>
      {!entries.length && <Empty title="No operations recorded" text="Changes and install results will appear here after you run an operation." icon={<HistoryIcon fontSize="large" />} />}
    </div>
  </>
}
