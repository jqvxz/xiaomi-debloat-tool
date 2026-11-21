use crate::{
    adb::{self, Adb, Device, DeviceInfo, Failure, Inventory, Package, ProcessRunner, Target},
    operations::{self, Action, CatalogApp, Preset, Setting, Value},
    store::{Entry, Status, Store},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type Result<T> = std::result::Result<T, String>;

struct AppState {
    store: Store,
    runner: Arc<dyn adb::Runner>,
    job: Mutex<()>,
    preview: Mutex<Option<PendingPreview>>,
    next_token: AtomicU32,
}

impl AppState {
    fn new(store: Store, runner: Arc<dyn adb::Runner>) -> Self {
        Self {
            store,
            runner,
            job: Mutex::new(()),
            preview: Mutex::new(None),
            next_token: AtomicU32::new(1),
        }
    }

    fn adb(&self) -> Result<Option<Adb>> {
        let configured = self
            .store
            .preference("adb")?
            .map(PathBuf::from)
            .filter(|path| adb::is_executable(path));
        Ok(configured
            .or_else(adb::find_adb)
            .map(|path| Adb::new(path, Arc::clone(&self.runner))))
    }

    fn require_adb(&self) -> Result<Adb> {
        self.adb()?.ok_or_else(|| {
            format!(
                "Choose {} from Android Platform-Tools first.",
                adb::ADB_FILENAME
            )
        })
    }

    fn start_job(&self) -> Result<MutexGuard<'_, ()>> {
        self.job
            .try_lock()
            .map_err(|_| "Another device operation is in progress.".into())
    }

    fn target<'a>(&self, adb: &'a Adb, selection: &Selection) -> Result<Target<'a>> {
        let target = adb.target(selection.transport).map_err(message)?;
        if target.identity != selection.identity {
            return Err("The device or Android user changed. Refresh before continuing.".into());
        }
        Ok(target)
    }
}

fn message(failure: Failure) -> String {
    failure.message
}

async fn worker<T: Send + 'static>(
    app: AppHandle,
    task: impl FnOnce(&AppHandle, &AppState) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        task(&app, &state)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceList {
    adb_path: Option<String>,
    devices: Vec<Device>,
    error: Option<String>,
    active_user: Option<ActiveUser>,
}

#[derive(Serialize)]
struct ActiveUser {
    transport: u32,
    user: u32,
}

#[tauri::command]
async fn device_list(app: AppHandle, active_transport: Option<u32>) -> Result<DeviceList> {
    worker(app, move |_, state| {
        let Some(adb) = state.adb()? else {
            return Ok(DeviceList {
                adb_path: None,
                devices: Vec::new(),
                error: None,
                active_user: None,
            });
        };
        let adb_path = Some(adb.exe.to_string_lossy().into_owned());
        Ok(match adb.devices() {
            Ok(devices) => {
                let active_user = active_transport
                    .filter(|transport| {
                        devices
                            .iter()
                            .any(|d| d.transport == *transport && d.state == "device")
                    })
                    .and_then(|transport| {
                        adb.active_user(transport)
                            .ok()
                            .map(|user| ActiveUser { transport, user })
                    });
                DeviceList {
                    adb_path,
                    devices,
                    error: None,
                    active_user,
                }
            }
            Err(failure) => DeviceList {
                adb_path,
                devices: Vec::new(),
                error: Some(failure.message),
                active_user: None,
            },
        })
    })
    .await
}

#[tauri::command]
async fn configure_adb(app: AppHandle) -> Result<bool> {
    worker(app, |app, state| {
        let _job = state.start_job()?;
        let dialog = app.dialog().file().set_title(format!(
            "Select the Android Platform-Tools {}",
            adb::ADB_FILENAME
        ));
        #[cfg(windows)]
        let dialog = dialog.add_filter("Windows executable", &["exe"]);
        let Some(path) = dialog.blocking_pick_file() else {
            return Ok(false);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        let expected_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                if cfg!(windows) {
                    name.eq_ignore_ascii_case(adb::ADB_FILENAME)
                } else {
                    name == adb::ADB_FILENAME
                }
            });
        if !path.is_absolute() || !expected_name || !adb::is_executable(&path) {
            return Err(format!(
                "Select the {} executable from Android Platform-Tools.",
                adb::ADB_FILENAME
            ));
        }
        let adb = Adb::new(path.clone(), Arc::clone(&state.runner));
        if !adb
            .host(&["version"])
            .map_err(message)?
            .contains("Android Debug Bridge version")
        {
            return Err(
                "The selected executable did not identify itself as Android Debug Bridge.".into(),
            );
        }
        state
            .store
            .save_preference("adb", path.to_str().ok_or("Unsupported ADB path.")?)?;
        Ok(true)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum Connection {
    Pair { endpoint: String, code: String },
    Connect { endpoint: String },
    Disconnect { transport: u32 },
}

fn endpoint(input: &str) -> Result<String> {
    input
        .trim()
        .parse::<SocketAddr>()
        .ok()
        .filter(|addr| addr.port() != 0)
        .map(|addr| addr.to_string())
        .ok_or_else(|| "Enter the IP address and port shown on the phone.".into())
}

#[tauri::command]
async fn connection(app: AppHandle, request: Connection) -> Result<String> {
    worker(app, move |_, state| {
        let _job = state.start_job()?;
        let adb = state.require_adb()?;
        match request {
            Connection::Pair { endpoint: input, code } => {
                if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("Enter the six-digit code shown on the phone.".into());
                }
                adb.pair(&endpoint(&input)?, &code).map_err(message)?;
                Ok("Paired. If the phone doesn't appear, connect using its main Wireless debugging address.".into())
            }
            Connection::Connect { endpoint: input } => {
                let reply = adb.host(&["connect", &endpoint(&input)?]).map_err(message)?;
                let reply = reply.trim();
                if reply.starts_with("connected to") || reply.starts_with("already connected to") {
                    Ok("Wireless connection requested.".into())
                } else { Err(reply.into()) }
            }
            Connection::Disconnect { transport } => {
                let device = adb.devices().map_err(message)?.into_iter()
                    .find(|d| d.transport == transport && d.connection == "Wireless").ok_or("Wireless device not found.")?;
                let reply = adb.host(&["disconnect", &device.serial]).map_err(message)?;
                if reply.contains("error") || reply.contains("no such device") {
                    return Err(reply.trim().into());
                }
                Ok("Wireless device disconnected.".into())
            }
        }
    }).await
}

#[tauri::command]
fn open_guide(app: AppHandle, guide: &str) -> Result<()> {
    let url = match guide {
        "platform-tools" => "https://developer.android.com/tools/releases/platform-tools",
        "usb" => "https://developer.android.com/tools/adb#Enabling",
        "xiaomi" => "https://www.mi.com/global/support/faq/details/KA-168765/",
        _ => return Err("Unknown setup guide.".into()),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    settings: &'static [Setting],
    catalog: &'static [CatalogApp],
    built_in: Vec<Preset>,
}

#[tauri::command]
fn metadata() -> Metadata {
    Metadata {
        settings: operations::SETTINGS,
        catalog: operations::CATALOG,
        built_in: operations::builtin_presets(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    info: DeviceInfo,
    packages: Vec<Package>,
    protection_ready: bool,
    settings: BTreeMap<String, Option<String>>,
    unavailable_settings: Vec<&'static str>,
}

#[tauri::command]
async fn snapshot(app: AppHandle, transport: u32) -> Result<Snapshot> {
    worker(app, move |_, state| {
        let adb = state.require_adb()?;
        let target = adb.target(transport).map_err(message)?;
        let info = target.info().map_err(message)?;
        let inventory = target.inventory().map_err(message)?;
        let mut settings = BTreeMap::new();
        let mut unavailable_settings = Vec::new();
        for spec in operations::SETTINGS {
            match target.setting(spec.namespace, spec.id) {
                Ok(value) => {
                    settings.insert(spec.id.to_owned(), value);
                }
                Err(error) if error.stop => return Err(error.message),
                Err(_) => unavailable_settings.push(spec.id),
            }
        }
        target.verify().map_err(message)?;
        Ok(Snapshot {
            info,
            packages: inventory.packages.into_values().collect(),
            protection_ready: inventory.protection_ready,
            settings,
            unavailable_settings,
        })
    })
    .await
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    transport: u32,
    identity: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewRequest {
    selection: Selection,
    actions: Vec<Action>,
    advanced: bool,
    xiaomi_only: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRow {
    action: Action,
    label: String,
    current: Option<String>,
    next: Option<String>,
    reason: Option<String>,
}

#[derive(Serialize)]
struct Preview {
    token: u32,
    rows: Vec<PreviewRow>,
}

struct PendingPreview {
    token: u32,
    request: PreviewRequest,
    rows: Vec<(PreviewRow, Option<Value>)>,
    created: Instant,
}

fn next_label(action: &Action) -> String {
    use operations::Verb;
    match action {
        Action::Package {
            verb: Verb::Disable,
            ..
        } => "Disabled".into(),
        Action::Package {
            verb: Verb::Remove, ..
        } => "Removed for active user".into(),
        Action::Package {
            verb: Verb::Enable | Verb::Restore,
            ..
        } => "Available for active user".into(),
        Action::Setting { id, value } => operations::SETTINGS
            .iter()
            .find(|spec| spec.id == id)
            .and_then(|spec| spec.options.iter().find(|(key, _)| value == key))
            .map_or_else(|| value.clone(), |(_, label)| (*label).into()),
    }
}

fn prepare(state: &AppState, request: PreviewRequest) -> Result<Preview> {
    let _job = state.start_job()?;
    if request.actions.is_empty() || request.actions.len() > operations::MAX_ITEMS {
        return Err("Choose between 1 and 150 actions.".into());
    }
    let mut unique = HashSet::new();
    for action in &request.actions {
        operations::validate_action(action).map_err(message)?;
        if !unique.insert(action.id()) {
            return Err("The same item cannot be changed twice in one job.".into());
        }
    }
    let adb = state.require_adb()?;
    let target = state.target(&adb, &request.selection)?;
    if request.xiaomi_only && !target.info().map_err(message)?.xiaomi {
        return Err("This preset applies to Xiaomi, Redmi, and POCO devices.".into());
    }
    let inventory = if request
        .actions
        .iter()
        .any(|a| matches!(a, Action::Package { .. }))
    {
        target.inventory().map_err(message)?
    } else {
        Inventory {
            packages: BTreeMap::new(),
            protection_ready: true,
        }
    };
    let batch = request.actions.len() > 1;
    let mut rows = Vec::new();
    for action in &request.actions {
        let assessment =
            operations::assess(action, &inventory, request.advanced, batch).and_then(|_| {
                let value = operations::capture(&target, action)?;
                operations::check_before(action, &value)?;
                Ok(value)
            });
        let (before, reason) = match assessment {
            Ok(value) => (Some(value), None),
            Err(error) if error.stop => return Err(error.message),
            Err(error) => (None, Some(error.message)),
        };
        rows.push((
            PreviewRow {
                action: action.clone(),
                label: action.label(),
                current: before.as_ref().map(Value::to_label),
                next: before.as_ref().map(|_| next_label(action)),
                reason,
            },
            before,
        ));
    }
    target.verify().map_err(message)?;
    let token = state.next_token.fetch_add(1, Ordering::Relaxed);
    let rendered = Preview {
        token,
        rows: rows.iter().map(|(row, _)| row.clone()).collect(),
    };
    *state
        .preview
        .lock()
        .map_err(|_| "Could not cache the operation preview.")? = Some(PendingPreview {
        token,
        request,
        rows,
        created: Instant::now(),
    });
    Ok(rendered)
}

#[tauri::command]
async fn preview(app: AppHandle, request: PreviewRequest) -> Result<Preview> {
    worker(app, move |_, state| prepare(state, request)).await
}

#[derive(Serialize)]
struct JobResult {
    entries: Vec<Entry>,
    stopped: bool,
}

fn run_prepared(state: &AppState, token: u32, on_entry: impl Fn(&Entry)) -> Result<JobResult> {
    let _job = state.start_job()?;
    let mut cache = state
        .preview
        .lock()
        .map_err(|_| "Could not read the operation preview.")?;
    if !cache
        .as_ref()
        .is_some_and(|job| job.token == token && job.created.elapsed() < Duration::from_secs(300))
    {
        return Err("This preview expired. Review the operation again.".into());
    }
    let job = cache.take().ok_or("Review the operation first.")?;
    drop(cache);
    let adb = state.require_adb()?;
    let target = state.target(&adb, &job.request.selection)?;
    let model = target.info().map_err(message)?.model;
    let batch = job.rows.len() > 1;
    let mut result = JobResult {
        entries: Vec::new(),
        stopped: false,
    };
    for (row, before) in job.rows {
        let mut entry = Entry::new(
            &model,
            target.user,
            row.label,
            Some(row.action.clone()),
            before.clone(),
        );
        let Some(before) = before else {
            entry.status = Status::Skipped;
            entry.detail = row.reason;
            let entry = state.store.add(&target.identity, entry)?;
            on_entry(&entry);
            result.entries.push(entry);
            continue;
        };
        let preflight = (|| {
            target.verify()?;
            if matches!(row.action, Action::Package { .. }) {
                operations::assess(
                    &row.action,
                    &target.inventory()?,
                    job.request.advanced,
                    batch,
                )?;
            }
            let current = operations::capture(&target, &row.action)?;
            if !current.matches(&before) {
                return Err(Failure::new(
                    "The item changed after preview. Review it again.",
                ));
            }
            Ok(())
        })();
        if let Err(error) = preflight {
            entry.status = Status::Skipped;
            entry.detail = Some(error.message);
            let entry = state.store.add(&target.identity, entry)?;
            on_entry(&entry);
            result.entries.push(entry);
            if error.stop {
                result.stopped = true;
                break;
            }
            continue;
        }
        entry = state.store.add(&target.identity, entry)?;
        let write = target
            .verify()
            .and_then(|_| operations::apply(&target, &row.action));
        let stop = record_outcome(
            &mut entry,
            write,
            || {
                target
                    .verify()
                    .and_then(|_| operations::capture(&target, &row.action))
            },
            |actual| operations::applied(&row.action, actual),
        );
        state.store.update(&entry)?;
        on_entry(&entry);
        result.entries.push(entry);
        if stop {
            result.stopped = true;
            break;
        }
    }
    Ok(result)
}

fn record_outcome(
    entry: &mut Entry,
    write: adb::Result<()>,
    read: impl FnOnce() -> adb::Result<Value>,
    succeeded: impl FnOnce(&Value) -> bool,
) -> bool {
    entry.detail = write.as_ref().err().map(|error| error.message.clone());
    if write.as_ref().err().is_some_and(|error| error.stop) {
        entry.status = Status::Unknown;
        return true;
    }
    let actual = match read() {
        Ok(value) => value,
        Err(error) => {
            entry.status = Status::Unknown;
            entry.detail = Some(error.message);
            return error.stop;
        }
    };
    entry.status = if succeeded(&actual) {
        Status::Success
    } else if entry
        .before
        .as_ref()
        .is_some_and(|before| before.matches(&actual))
    {
        Status::Failed
    } else {
        Status::Unknown
    };
    if entry.status != Status::Success && entry.detail.is_none() {
        entry.detail = Some("The device did not return the expected state.".into());
    }
    if entry.status == Status::Success {
        entry.detail = matches!(actual, Value::Package(None)).then(|| {
            "Reinstall this app with a copy of its APK signed by the same publisher.".into()
        });
    }
    entry.after = Some(actual);
    false
}

#[tauri::command]
async fn execute(
    app: AppHandle,
    token: u32,
    progress: tauri::ipc::Channel<Entry>,
) -> Result<JobResult> {
    worker(app, move |_, state| {
        run_prepared(state, token, |entry| {
            let _ = progress.send(entry.clone());
        })
    })
    .await
}

#[tauri::command]
async fn history(app: AppHandle, identity: Option<String>) -> Result<Vec<Entry>> {
    worker(app, move |_, state| {
        state.store.history(identity.as_deref(), Some(400))
    })
    .await
}

#[tauri::command]
async fn undo(app: AppHandle, selection: Selection, id: i64) -> Result<Entry> {
    worker(app, move |_, state| {
        let _job = state.start_job()?;
        let adb = state.require_adb()?;
        let target = state.target(&adb, &selection)?;
        let original = state.store.history(Some(&target.identity), None)?.into_iter()
            .find(|entry| entry.id == id && entry.can_undo).ok_or("This entry cannot be undone for the selected device and user.")?;
        let action = original.action.ok_or("This action has no stored reverse operation.")?;
        let before = original.before.ok_or("The prior state is missing.")?;
        let current = operations::capture(&target, &action).map_err(message)?;
        if !original.after.as_ref().is_some_and(|after| after.matches(&current)) {
            return Err("The current state has changed since this action. Undo would overwrite that change.".into());
        }
        let mut entry = Entry::new(&original.device, target.user, format!("Undo {}", original.label),
            Some(action.clone()), Some(current.clone()));
        entry.undo_of = Some(id);
        entry = state.store.add(&target.identity, entry)?;
        let write = target.verify().and_then(|_| operations::revert(&target, &action, &before, &current));
        record_outcome(&mut entry, write, || target.verify().and_then(|_| operations::capture(&target, &action)),
            |actual| before.matches(actual));
        state.store.update(&entry)?;
        Ok(entry)
    }).await
}

#[tauri::command]
async fn reconcile(app: AppHandle, selection: Selection, id: i64) -> Result<Entry> {
    worker(app, move |_, state| {
        let _job = state.start_job()?;
        let adb = state.require_adb()?;
        let target = state.target(&adb, &selection)?;
        let history = state.store.history(Some(&target.identity), None)?;
        let mut entry = history
            .iter()
            .find(|entry| entry.id == id && entry.status == Status::Unknown)
            .cloned()
            .ok_or("This entry cannot be checked for the selected device and user.")?;
        let action = entry
            .action
            .clone()
            .ok_or("Refresh Apps to check the result of this installation.")?;
        if history.iter().any(|later| {
            later.id > id
                && matches!(
                    later.status,
                    Status::Success | Status::Pending | Status::Unknown
                )
                && later
                    .action
                    .as_ref()
                    .is_some_and(|later_action| later_action.id() == action.id())
        }) {
            return Err("A newer operation already affected this item.".into());
        }
        let actual = operations::capture(&target, &action).map_err(message)?;
        target.verify().map_err(message)?;
        let applied = if let Some(original_id) = entry.undo_of {
            history
                .iter()
                .find(|e| e.id == original_id)
                .and_then(|e| e.before.as_ref())
                .is_some_and(|v| v.matches(&actual))
        } else {
            operations::applied(&action, &actual)
        };
        entry.status = if applied {
            Status::Success
        } else if entry
            .before
            .as_ref()
            .is_some_and(|before| before.matches(&actual))
        {
            Status::Failed
        } else {
            Status::Unknown
        };
        entry.after = Some(actual);
        entry.detail = Some(
            if entry.status == Status::Unknown {
                "The device's current state remains inconclusive."
            } else {
                "Result checked against the current device state."
            }
            .into(),
        );
        state.store.update(&entry)?;
        Ok(entry)
    })
    .await
}

#[derive(Serialize)]
struct ApkFile {
    path: String,
    name: String,
}

#[tauri::command]
async fn pick_apks(app: AppHandle, split: bool) -> Result<Vec<ApkFile>> {
    worker(app, move |app, _| {
        let dialog = app
            .dialog()
            .file()
            .set_title(if split {
                "Select the base APK and its splits"
            } else {
                "Select an APK"
            })
            .add_filter("Android APK", &["apk"]);
        let files = if split {
            dialog.blocking_pick_files().unwrap_or_default()
        } else {
            dialog.blocking_pick_file().into_iter().collect()
        };
        files
            .into_iter()
            .map(|file| {
                let path = file.into_path().map_err(|e| e.to_string())?;
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("Could not read the APK name.")?
                    .to_owned();
                let path = path.to_str().ok_or("Unsupported APK path.")?.to_owned();
                Ok(ApkFile { path, name })
            })
            .collect()
    })
    .await
}

#[tauri::command]
async fn install(
    app: AppHandle,
    selection: Selection,
    paths: Vec<String>,
    replace: bool,
    split: bool,
) -> Result<Entry> {
    worker(app, move |_, state| {
        let _job = state.start_job()?;
        if (!split && paths.len() != 1) || (split && !(2..=65).contains(&paths.len())) {
            return Err("Select one standalone APK or a base APK with 1–64 splits.".into());
        }
        let mut names = Vec::new();
        let mut canonical_paths = Vec::new();
        let mut unique = BTreeSet::new();
        for path in paths {
            let original = Path::new(&path);
            if !original.is_absolute()
                || !original
                    .extension()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.eq_ignore_ascii_case("apk"))
            {
                return Err("Only local APK files are supported.".into());
            }
            let resolved = original
                .canonicalize()
                .map_err(|_| "An APK is no longer accessible.")?;
            if !resolved.is_file() || !unique.insert(resolved.clone()) {
                return Err("Select distinct APK files.".into());
            }
            names.push(
                original
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("Unsupported APK name.")?
                    .to_owned(),
            );
            canonical_paths.push(resolved);
        }
        let adb = state.require_adb()?;
        let target = state.target(&adb, &selection)?;
        let user = target.user.to_string();
        let mut args = vec![
            if split { "install-multiple" } else { "install" },
            "--user",
            user.as_str(),
        ];
        // Modern ADB replaces existing APKs by default. -R explicitly disables replacement.
        args.push(if replace { "-r" } else { "-R" });
        for path in &canonical_paths {
            args.push(path.to_str().ok_or("Unsupported APK path.")?);
        }
        let label = if split {
            format!("Install split set ({})", names.join(", "))
        } else {
            format!("Install {}", names[0])
        };
        let model = target.info().map_err(message)?.model;
        target.verify().map_err(message)?;
        let mut entry = state.store.add(
            &target.identity,
            Entry::new(&model, target.user, label, None, None),
        )?;
        let response = target
            .verify()
            .and_then(|_| target.run(&args, Duration::from_secs(300)));
        match response {
            Ok(output) if output.lines().any(|line| line.trim() == "Success") => {
                entry.status = Status::Success
            }
            response => {
                let (status, mut detail) = match response {
                    Ok(output) => (Status::Failed, output.trim().to_owned()),
                    Err(error) => (
                        if error.stop {
                            Status::Unknown
                        } else {
                            Status::Failed
                        },
                        error.message,
                    ),
                };
                for (path, name) in canonical_paths.iter().zip(names.iter()) {
                    let absolute = path.to_string_lossy();
                    detail = detail
                        .replace(absolute.as_ref(), name)
                        .replace(absolute.trim_start_matches(r"\\?\"), name);
                }
                entry.status = status;
                entry.detail = Some(if detail.is_empty() {
                    "ADB did not report a successful installation.".into()
                } else {
                    detail
                });
            }
        }
        state.store.update(&entry)?;
        Ok(entry)
    })
    .await
}

#[tauri::command]
async fn presets(app: AppHandle) -> Result<Vec<Preset>> {
    worker(app, |_, state| state.store.presets()).await
}

#[tauri::command]
async fn save_preset(app: AppHandle, preset: Preset) -> Result<()> {
    worker(app, move |_, state| {
        state
            .store
            .save_preset(&operations::validate_preset(preset)?)
    })
    .await
}

#[tauri::command]
async fn delete_preset(app: AppHandle, name: String) -> Result<()> {
    worker(app, move |_, state| state.store.delete_preset(&name)).await
}

#[tauri::command]
async fn import_preset(app: AppHandle) -> Result<Option<Preset>> {
    worker(app, |app, state| {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Import a debloat preset")
            .add_filter("JSON preset", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65_536 {
            return Err("Preset files must be under 64 KB.".into());
        }
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        let preset =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid preset file. {e}"))?;
        let preset = operations::validate_preset(preset)?;
        state.store.save_preset(&preset)?;
        Ok(Some(preset))
    })
    .await
}

#[tauri::command]
async fn export_preset(app: AppHandle, name: String) -> Result<bool> {
    worker(app, move |app, state| {
        let preset = state
            .store
            .presets()?
            .into_iter()
            .find(|p| p.name == name)
            .ok_or("Preset not found.")?;
        let Some(path) = save_path(app, "Export a debloat preset", "preset.json", "json")? else {
            return Ok(false);
        };
        write_file(
            &path,
            &serde_json::to_vec_pretty(&preset).map_err(|e| e.to_string())?,
        )?;
        Ok(true)
    })
    .await
}

fn save_path(app: &AppHandle, title: &str, filename: &str, ext: &str) -> Result<Option<PathBuf>> {
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(filename)
        .add_filter(ext.to_uppercase(), &[ext])
        .blocking_save_file()
        .map(|file| file.into_path().map_err(|e| e.to_string()))
        .transpose()
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or("Invalid export location.")?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    temp.write_all(data).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

fn csv_cell(value: &str) -> String {
    // Spreadsheet programs may evaluate text taken from a device as formulas.
    let dangerous =
        value.trim_start().starts_with(['=', '+', '-', '@']) || value.starts_with(['\t', '\r']);
    format!(
        "\"{}{}\"",
        if dangerous { "'" } else { "" },
        value.replace('"', "\"\"")
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ActivityFormat {
    Json,
    Csv,
}

#[tauri::command]
async fn export_activity(
    app: AppHandle,
    format: ActivityFormat,
    identity: Option<String>,
) -> Result<bool> {
    worker(app, move |app, state| {
        let (filename, ext) = match format {
            ActivityFormat::Json => ("activity.json", "json"),
            ActivityFormat::Csv => ("activity.csv", "csv"),
        };
        let Some(path) = save_path(app, "Export activity history", filename, ext)? else {
            return Ok(false);
        };
        let history = state.store.history(identity.as_deref(), None)?;
        let data = match format {
            ActivityFormat::Json => {
                serde_json::to_vec_pretty(&history).map_err(|e| e.to_string())?
            }
            ActivityFormat::Csv => {
                let mut body = String::from(
                    "Id,UTC timestamp,Device,Android user,Operation,Status,Details\r\n",
                );
                for e in history {
                    let cells = [
                        &e.id.to_string(),
                        &e.at,
                        &e.device,
                        &e.user.to_string(),
                        &e.label,
                        e.status.label(),
                        e.detail.as_deref().unwrap_or_default(),
                    ];
                    body.push_str(
                        &cells
                            .iter()
                            .map(|cell| csv_cell(cell))
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    body.push_str("\r\n");
                }
                body.into_bytes()
            }
        };
        write_file(&path, &data)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
async fn export_diagnostics(app: AppHandle, selection: Selection) -> Result<bool> {
    worker(app, move |app, state| {
        let _job = state.start_job()?;
        let Some(path) = save_path(
            app,
            "Export recent Android logs",
            "android-logcat.txt",
            "txt",
        )?
        else {
            return Ok(false);
        };
        let adb = state.require_adb()?;
        let target = state.target(&adb, &selection)?;
        target.verify().map_err(message)?;
        let output = target
            .run(
                &["logcat", "-d", "-t", "1000", "-v", "threadtime"],
                Duration::from_secs(20),
            )
            .map_err(message)?;
        write_file(&path, output.as_bytes())?;
        Ok(true)
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            app.manage(AppState::new(
                Store::open(&dir).map_err(std::io::Error::other)?,
                Arc::new(ProcessRunner),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            device_list,
            configure_adb,
            connection,
            open_guide,
            metadata,
            snapshot,
            preview,
            execute,
            history,
            undo,
            reconcile,
            pick_apks,
            install,
            presets,
            save_preset,
            delete_preset,
            import_preset,
            export_preset,
            export_activity,
            export_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Xiaomi Debloat Tool");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakePhone {
        user: u32,
        fail_on: Option<String>,
        values: BTreeMap<String, String>,
        writes: Vec<String>,
    }

    #[derive(Default)]
    struct FakeAdb(Mutex<FakePhone>);

    impl adb::Runner for FakeAdb {
        fn run(
            &self,
            _: &Path,
            args: &[&str],
            _: Option<&str>,
            _: Duration,
        ) -> adb::Result<String> {
            let mut phone = self.0.lock().unwrap();
            if args == ["devices", "-l"] {
                return Ok(
                    "List of devices attached\nABC device model:Fake_Phone transport_id:7\n".into(),
                );
            }
            let ["-t", "7", "shell", "-T", shell @ ..] = args else {
                panic!("Unexpected ADB arguments: {args:?}");
            };
            let reply = match shell {
                ["am", "get-current-user"] => phone.user.to_string(),
                ["getprop", "ro.build.version.sdk"] => "36".into(),
                ["getprop", "ro.serialno"] => "fake-serial".into(),
                ["getprop", "ro.product.model"] => "Fake Phone".into(),
                ["getprop", _] => String::new(),
                ["settings", "--user", _, "get", _, "android_id"] => "1092837465abcdef".into(),
                ["settings", "--user", _, "get", _, key] => phone
                    .values
                    .get(*key)
                    .cloned()
                    .unwrap_or_else(|| "null".into()),
                ["settings", "--user", _, "put", _, key, value] => {
                    phone.writes.push((*key).into());
                    if phone.fail_on.as_deref() == Some(key) {
                        return Err(Failure::stopped("device offline"));
                    }
                    phone.values.insert((*key).into(), (*value).into());
                    String::new()
                }
                ["settings", "--user", _, "delete", _, key] => {
                    phone.values.remove(*key);
                    "Deleted 1 rows".into()
                }
                _ => panic!("Unexpected shell arguments: {shell:?}"),
            };
            Ok(reply)
        }
    }

    fn fixture(path: &Path, runner: Arc<FakeAdb>) -> (AppState, Selection) {
        let store = Store::open(path).unwrap();
        store
            .save_preference("adb", std::env::current_exe().unwrap().to_str().unwrap())
            .unwrap();
        let state = AppState::new(store, runner);
        let identity = state.require_adb().unwrap().target(7).unwrap().identity;
        (
            state,
            Selection {
                transport: 7,
                identity,
            },
        )
    }

    fn request(selection: Selection) -> PreviewRequest {
        PreviewRequest {
            selection,
            actions: operations::SETTINGS[..3]
                .iter()
                .map(|spec| Action::Setting {
                    id: spec.id.into(),
                    value: "0.5".into(),
                })
                .collect(),
            advanced: false,
            xiaomi_only: false,
        }
    }

    #[test]
    fn switched_users_invalidate_confirmed_preview() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeAdb::default());
        let (state, selection) = fixture(dir.path(), Arc::clone(&runner));
        let preview = prepare(&state, request(selection)).unwrap();
        runner.0.lock().unwrap().user = 10;
        assert!(run_prepared(&state, preview.token, |_| {}).is_err());
        assert!(runner.0.lock().unwrap().writes.is_empty());
    }

    #[test]
    fn preserves_verified_work_after_disconnect_and_restores_missing_setting() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeAdb::default());
        let (state, selection) = fixture(dir.path(), Arc::clone(&runner));
        let preview = prepare(&state, request(selection.clone())).unwrap();
        runner.0.lock().unwrap().fail_on = Some("transition_animation_scale".into());
        let result = run_prepared(&state, preview.token, |_| {}).unwrap();
        assert!(result.stopped);
        assert_eq!(
            result.entries.iter().map(|e| e.status).collect::<Vec<_>>(),
            [Status::Success, Status::Unknown]
        );
        assert_eq!(
            runner.0.lock().unwrap().writes,
            ["window_animation_scale", "transition_animation_scale"]
        );
        let journal = state
            .store
            .history(Some(&selection.identity), None)
            .unwrap();
        let verified = journal.iter().find(|entry| entry.can_undo).unwrap();
        let adb = state.require_adb().unwrap();
        let target = adb.target(7).unwrap();
        operations::revert(
            &target,
            verified.action.as_ref().unwrap(),
            verified.before.as_ref().unwrap(),
            verified.after.as_ref().unwrap(),
        )
        .unwrap();
        assert!(runner.0.lock().unwrap().values.is_empty());
    }

    #[test]
    fn changed_values_are_skipped_and_other_actions_continue() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeAdb::default());
        let (state, selection) = fixture(dir.path(), Arc::clone(&runner));
        let preview = prepare(&state, request(selection)).unwrap();
        runner
            .0
            .lock()
            .unwrap()
            .values
            .insert("window_animation_scale".into(), "2.0".into());
        let result = run_prepared(&state, preview.token, |_| {}).unwrap();
        assert!(!result.stopped);
        assert_eq!(
            result.entries.iter().map(|e| e.status).collect::<Vec<_>>(),
            [Status::Skipped, Status::Success, Status::Success]
        );
        assert_eq!(
            runner.0.lock().unwrap().values["window_animation_scale"],
            "2.0"
        );
    }
}
