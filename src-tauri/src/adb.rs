use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub stop: bool,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stop: false,
        }
    }

    pub fn stopped(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stop: true,
        }
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

pub trait Runner: Send + Sync {
    fn run(
        &self,
        exe: &Path,
        args: &[&str],
        input: Option<&str>,
        timeout: Duration,
    ) -> Result<String>;
}

pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(
        &self,
        exe: &Path,
        args: &[&str],
        input: Option<&str>,
        timeout: Duration,
    ) -> Result<String> {
        let mut command = Command::new(exe);
        command
            .env_remove("ADB_SERVER_SOCKET")
            .env_remove("ANDROID_ADB_SERVER_PORT")
            .env_remove("ANDROID_ADB_SERVER_ADDRESS")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| Failure::stopped(format!("Could not start ADB. {e}")))?;
        let out = read_pipe(child.stdout.take().expect("ADB stdout is piped"));
        let err = read_pipe(child.stderr.take().expect("ADB stderr is piped"));
        if let Some(input) = input {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = writeln!(stdin, "{input}");
            }
        }
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(15)),
                value => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Failure::stopped(match value {
                        Err(e) => format!("Could not wait for ADB. {e}"),
                        _ => "ADB timed out. Refresh the device before retrying.".into(),
                    }));
                }
            }
        };
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        if stdout.len() > MAX_OUTPUT || stderr.len() > MAX_OUTPUT {
            return Err(Failure::stopped("ADB output exceeded the capture limit."));
        }
        let stdout = String::from_utf8_lossy(&stdout);
        let stderr = String::from_utf8_lossy(&stderr);
        if !status.success() {
            let message = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            let lower = message.to_ascii_lowercase();
            let stop = [
                "device offline",
                "device unauthorized",
                "no devices",
                "connection closed",
                "transport",
                "protocol fault",
            ]
            .iter()
            .any(|word| lower.contains(word))
                || (lower.contains("device") && lower.contains("not found"));
            return Err(Failure {
                message: if message.is_empty() {
                    format!("ADB exited with {status}.")
                } else {
                    message.into()
                },
                stop,
            });
        }
        Ok(stdout.into_owned())
    }
}

const MAX_OUTPUT: usize = 8 * 1024 * 1024;

fn read_pipe(pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes);
        bytes
    })
}

pub fn find_adb() -> Option<PathBuf> {
    let sdk_paths = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .into_iter()
        .filter_map(env::var_os)
        .map(|dir| PathBuf::from(dir).join("platform-tools"));
    let local_sdk = env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("Android/SDK/platform-tools"));
    env::var_os("PATH")
        .iter()
        .flat_map(env::split_paths)
        .chain(sdk_paths)
        .chain(local_sdk)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("adb.exe"))
        .find(|path| path.is_file())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub transport: u32,
    #[serde(skip_serializing)]
    pub serial: String,
    pub model: String,
    pub state: String,
    pub connection: &'static str,
}

fn parse_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices attached"))
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let serial = columns.next()?;
            let mut state = columns.next()?.to_owned();
            if state == "no" {
                state = "no permissions".into();
            }
            let fields: Vec<_> = columns.collect();
            let transport = fields
                .iter()
                .find_map(|field| field.strip_prefix("transport_id:"))?
                .parse()
                .ok()?;
            let model = fields.iter().find_map(|field| field.strip_prefix("model:"));
            let connection = if serial.starts_with("emulator-") {
                "Emulator"
            } else if serial.contains(':') || serial.contains("._adb-tls-connect.") {
                "Wireless"
            } else {
                "USB"
            };
            Some(Device {
                transport,
                serial: serial.into(),
                model: model.unwrap_or("Android device").replace('_', " "),
                state,
                connection,
            })
        })
        .collect()
}

#[derive(Clone)]
pub struct Adb {
    pub exe: PathBuf,
    runner: Arc<dyn Runner>,
}

impl Adb {
    pub fn new(exe: PathBuf, runner: Arc<dyn Runner>) -> Self {
        Self { exe, runner }
    }

    pub fn host(&self, args: &[&str]) -> Result<String> {
        self.runner
            .run(&self.exe, args, None, Duration::from_secs(12))
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        Ok(parse_devices(&self.host(&["devices", "-l"])?))
    }

    pub fn active_user(&self, transport: u32) -> Result<u32> {
        parse_user(self.host(&[
            "-t",
            &transport.to_string(),
            "shell",
            "-T",
            "am",
            "get-current-user",
        ])?)
    }

    pub fn pair(&self, endpoint: &str, code: &str) -> Result<()> {
        let reply = self.runner.run(
            &self.exe,
            &["pair", endpoint],
            Some(code),
            Duration::from_secs(25),
        );
        match reply {
            Ok(reply) if reply.to_ascii_lowercase().contains("successfully paired") => Ok(()),
            _ => Err(Failure::new(
                "Pairing failed. Check the pairing address and use a fresh six-digit code.",
            )),
        }
    }

    pub fn target(&self, transport: u32) -> Result<Target<'_>> {
        let device = self.device(transport)?;
        let mut target = Target {
            adb: self,
            device,
            user: 0,
            sdk: 0,
            identity: String::new(),
        };
        target.user = target.current_user()?;
        target.sdk = target
            .prop("ro.build.version.sdk")?
            .parse()
            .map_err(|_| Failure::stopped("Could not read the Android version."))?;
        if target.sdk < 30 {
            return Err(Failure::new("This app supports Android 11 and later."));
        }
        target.identity = target.read_identity()?;
        Ok(target)
    }

    fn device(&self, transport: u32) -> Result<Device> {
        self.devices()?
            .into_iter()
            .find(|d| d.transport == transport && d.state == "device")
            .ok_or_else(|| {
                Failure::stopped("The selected device is no longer ready. Reconnect and refresh.")
            })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub model: String,
    pub brand: String,
    pub android: String,
    pub build: String,
    pub user: u32,
    pub xiaomi: bool,
    pub identity: String,
}

pub struct Target<'a> {
    adb: &'a Adb,
    pub device: Device,
    pub user: u32,
    pub sdk: u32,
    pub identity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageState {
    pub installed: bool,
    pub enabled: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub id: String,
    pub system: bool,
    pub installed: bool,
    pub disabled: bool,
    pub protection: Option<&'static str>,
}

impl Target<'_> {
    pub fn run(&self, args: &[&str], timeout: Duration) -> Result<String> {
        let transport = self.device.transport.to_string();
        let mut targeted = vec!["-t", &transport];
        targeted.extend_from_slice(args);
        self.adb.runner.run(&self.adb.exe, &targeted, None, timeout)
    }

    pub fn shell(&self, args: &[&str]) -> Result<String> {
        let mut targeted = vec!["shell", "-T"];
        targeted.extend_from_slice(args);
        self.run(&targeted, Duration::from_secs(12))
    }

    fn current_user(&self) -> Result<u32> {
        parse_user(self.shell(&["am", "get-current-user"])?)
    }

    fn prop(&self, key: &str) -> Result<String> {
        Ok(self.shell(&["getprop", key])?.trim().into())
    }

    fn read_identity(&self) -> Result<String> {
        let android_id = match self.setting("secure", "android_id") {
            Ok(value) => {
                value.filter(|id| id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
            }
            Err(error) if error.stop => return Err(error),
            Err(_) => None,
        };
        let serial = self.prop("ro.serialno")?;
        let physical = if !serial.is_empty() && !serial.eq_ignore_ascii_case("unknown") {
            format!("serial:{serial}:{}", android_id.unwrap_or_default())
        } else if let Some(android_id) = android_id {
            format!("android-id:{android_id}")
        } else if matches!(self.device.connection, "USB" | "Emulator") {
            format!("adb:{}", self.device.serial)
        } else {
            return Err(Failure::stopped(
                "ADB could not establish a stable identity for this wireless device. Try USB.",
            ));
        };
        Ok(format!(
            "{:x}",
            Sha256::digest(format!("xiaomi-debloat:{physical}:{}", self.user))
        ))
    }

    pub fn verify(&self) -> Result<()> {
        let current = self.adb.device(self.device.transport)?;
        if current.serial != self.device.serial
            || self.current_user()? != self.user
            || self.read_identity()? != self.identity
        {
            return Err(Failure::stopped(
                "The active device or Android user changed. Refresh and review the operation again.",
            ));
        }
        Ok(())
    }

    pub fn info(&self) -> Result<DeviceInfo> {
        let brand = self.prop("ro.product.brand")?;
        let manufacturer = self.prop("ro.product.manufacturer")?;
        let xiaomi = [brand.as_str(), manufacturer.as_str()].iter().any(|value| {
            ["xiaomi", "redmi", "poco"]
                .iter()
                .any(|brand| value.eq_ignore_ascii_case(brand))
        });
        let hyper = self.prop("ro.mi.os.version.name")?;
        let miui = self.prop("ro.miui.ui.version.name")?;
        let display = self.prop("ro.build.display.id")?;
        let build = if !hyper.is_empty() {
            format!("HyperOS {hyper}")
        } else if !miui.is_empty() {
            format!("MIUI {miui}")
        } else {
            display
        };
        Ok(DeviceInfo {
            model: self.prop("ro.product.model")?,
            brand,
            android: self.prop("ro.build.version.release")?,
            build,
            user: self.user,
            xiaomi,
            identity: self.identity.clone(),
        })
    }

    fn package_list(&self, options: &[&str]) -> Result<String> {
        let user = self.user.to_string();
        let mut args = vec!["cmd", "package", "list", "packages", "--user", &user];
        args.extend_from_slice(options);
        let output = self.shell(&args)?;
        if output
            .lines()
            .any(|line| !line.trim().is_empty() && !line.starts_with("package:"))
        {
            return Err(Failure::stopped(
                "Unexpected Android package list. Refresh before making changes.",
            ));
        }
        Ok(output)
    }

    pub fn inventory(&self) -> Result<Inventory> {
        let installed = package_ids(&self.package_list(&[])?);
        let all = package_ids(&self.package_list(&["-u"])?);
        let system = package_ids(&self.package_list(&["-u", "-s"])?);
        let disabled = package_ids(&self.package_list(&["-d"])?);
        let apex = package_ids(&self.package_list(&["--apex-only"])?);
        let uids = self.package_list(&["-U"])?;
        let platform_uids: HashSet<_> = uids
            .lines()
            .filter_map(|line| {
                let (id, uid) = line.strip_prefix("package:")?.split_once(" uid:")?;
                (uid.trim().parse::<u32>().ok()? % 100_000 < 10_000).then_some(id)
            })
            .collect();
        let dynamic = self.active_protected();
        let protection_ready = dynamic.is_ok();
        let dynamic = dynamic.unwrap_or_default();
        let packages = all
            .union(&installed)
            .chain(apex.iter())
            .map(|id| {
                let is_system = system.contains(id) || apex.contains(id);
                let protection =
                    if apex.contains(id) || platform_uids.contains(id.as_str()) || core_package(id)
                    {
                        Some("Core Android or manufacturer component")
                    } else if dynamic.contains(id) {
                        Some("Active system role or input service")
                    } else if !protection_ready {
                        Some("Couldn't verify active system roles")
                    } else {
                        None
                    };
                (
                    id.clone(),
                    Package {
                        id: id.clone(),
                        system: is_system,
                        installed: installed.contains(id) || apex.contains(id),
                        disabled: disabled.contains(id),
                        protection,
                    },
                )
            })
            .collect();
        Ok(Inventory {
            packages,
            protection_ready,
        })
    }

    fn active_protected(&self) -> Result<HashSet<String>> {
        let user = self.user.to_string();
        let mut protected = HashSet::new();
        if let Some(value) = self.setting("secure", "default_input_method")? {
            let id = value
                .split_once('/')
                .map(|(id, _)| id)
                .filter(|id| valid_package(id))
                .ok_or_else(|| Failure::new("Could not verify the active input method."))?;
            protected.insert(id.into());
        }
        let resolve = |intent: &[&str]| {
            let mut args = vec![
                "cmd",
                "package",
                "resolve-activity",
                "--brief",
                "--user",
                &user,
            ];
            args.extend_from_slice(intent);
            self.shell(&args)
        };
        let mut intents: Vec<&[&str]> = vec![
            &[
                "-a",
                "android.intent.action.MAIN",
                "-c",
                "android.intent.category.HOME",
            ],
            &["-a", "android.settings.SETTINGS"],
        ];
        if self.sdk < 31 {
            intents.extend([
                &["-a", "android.intent.action.DIAL", "-d", "tel:123"][..],
                &["-a", "android.intent.action.SENDTO", "-d", "smsto:123"][..],
            ]);
        }
        for intent in intents {
            let reply = resolve(intent)?;
            if let Some(id) = reply.lines().rev().find_map(|line| {
                let (id, _) = line.trim().split_once('/')?;
                valid_package(id).then_some(id)
            }) {
                protected.insert(id.into());
            } else if !reply.trim().eq_ignore_ascii_case("No activity found") {
                return Err(Failure::new(
                    "Could not resolve Android's active system roles.",
                ));
            }
        }
        if self.sdk >= 31 {
            for role in ["HOME", "DIALER", "SMS"] {
                let output = self.shell(&[
                    "cmd",
                    "role",
                    "get-role-holders",
                    "--user",
                    &user,
                    &format!("android.app.role.{role}"),
                ])?;
                for id in output
                    .trim()
                    .split([';', '\n'])
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    if !valid_package(id) {
                        return Err(Failure::new("Could not verify Android's role holders."));
                    }
                    protected.insert(id.into());
                }
            }
        }
        Ok(protected)
    }

    pub fn package_state(&self, id: &str) -> Result<Option<PackageState>> {
        debug_assert!(valid_package(id));
        let output = self.shell(&["dumpsys", "package", id])?;
        parse_package_state(&output, id, self.user)
    }

    pub fn pm(&self, verb: &str, id: &str) -> Result<()> {
        debug_assert!(valid_package(id));
        let user = self.user.to_string();
        let mut args = vec!["pm", verb];
        if verb == "uninstall" {
            args.push("-k");
        }
        args.extend(["--user", &user, id]);
        let output = self.shell(&args).map_err(|mut failure| {
            if !failure.stop {
                failure.message = package_error(verb, &failure.message);
            }
            failure
        })?;
        if output.lines().any(|line| {
            let line = line.trim().to_ascii_lowercase();
            line.starts_with("failure") || line.starts_with("error") || line.contains("exception")
        }) || (verb == "uninstall" && output.trim() != "Success")
        {
            return Err(Failure::new(package_error(verb, &output)));
        }
        Ok(())
    }

    pub fn setting(&self, namespace: &str, key: &str) -> Result<Option<String>> {
        let user = self.user.to_string();
        let value = self.shell(&["settings", "--user", &user, "get", namespace, key])?;
        let value = value.trim();
        if value.contains('\n') || value.starts_with("Error") || value.contains("Exception") {
            return Err(Failure::new(format!("Android could not read {key}.")));
        }
        Ok((value != "null" && !value.is_empty()).then(|| value.to_owned()))
    }

    pub fn write_setting(&self, namespace: &str, key: &str, value: Option<&str>) -> Result<()> {
        let user = self.user.to_string();
        let mut args = vec![
            "settings",
            "--user",
            &user,
            if value.is_some() { "put" } else { "delete" },
            namespace,
            key,
        ];
        if let Some(value) = value {
            if !numeric_setting(value) {
                return Err(Failure::new(
                    "The prior setting value cannot be restored safely.",
                ));
            }
            args.push(value);
        }
        let output = self.shell(&args)?;
        if output.contains("Error") || output.contains("Exception") {
            return Err(Failure::new(format!("Android rejected {key}.")));
        }
        if !same_setting(self.setting(namespace, key)?.as_deref(), value) {
            return Err(Failure::new(format!(
                "Android did not retain the requested value for {key}."
            )));
        }
        Ok(())
    }
}

fn parse_user(output: String) -> Result<u32> {
    output
        .trim()
        .parse()
        .map_err(|_| Failure::stopped("Could not identify the active Android user."))
}

fn package_error(verb: &str, output: &str) -> String {
    if output.contains("Cannot disable system packages") && verb == "disable-user" {
        return "Android blocks disabling this system app. You can review per-user removal separately.".into();
    }
    let detail = output
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("java.lang.SecurityException: "))
        .or_else(|| output.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("Android rejected the package change.");
    if verb == "install-existing" {
        format!(
            "{detail} If Android no longer has the package, reinstall it from its original signed APK."
        )
    } else {
        detail.to_owned()
    }
}

pub struct Inventory {
    pub packages: BTreeMap<String, Package>,
    pub protection_ready: bool,
}

fn parse_package_state(output: &str, id: &str, user: u32) -> Result<Option<PackageState>> {
    let header = format!("Package [{id}]");
    let user_label = format!("User {user}:");
    let mut found = false;
    for line in output.lines().map(str::trim) {
        if line.starts_with("Package [") {
            found = line.starts_with(&header);
        }
        if found && line.starts_with(&user_label) {
            let flag = |name: &str| {
                line.split_whitespace()
                    .find_map(|token| token.strip_prefix(name))
            };
            let installed = match flag("installed=") {
                Some("true") => true,
                Some("false") => false,
                _ => {
                    return Err(Failure::stopped(
                        "Could not read the package's per-user state.",
                    ));
                }
            };
            let enabled = flag("enabled=")
                .and_then(|v| v.parse().ok())
                .filter(|v| *v <= 4)
                .ok_or_else(|| Failure::stopped("Could not read the package's enabled state."))?;
            return Ok(Some(PackageState {
                installed,
                enabled: Some(enabled),
            }));
        }
    }
    if output.contains("Unable to find package") || output.trim().is_empty() {
        Ok(None)
    } else {
        Err(Failure::stopped(
            "Android did not report the package's per-user state.",
        ))
    }
}

fn package_ids(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("package:"))
        .map(str::trim)
        .filter(|id| valid_package(id))
        .map(str::to_owned)
        .collect()
}

pub fn valid_package(id: &str) -> bool {
    id.len() <= 255
        && !id.is_empty()
        && id.split('.').all(|part| {
            part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

pub fn numeric_setting(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 24
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-'))
        && value.parse::<f64>().is_ok_and(f64::is_finite)
}

pub fn same_setting(left: Option<&str>, right: Option<&str>) -> bool {
    left == right
        || matches!((left, right), (Some(a), Some(b))
        if numeric_setting(a) && numeric_setting(b) && a.parse::<f64>().ok() == b.parse::<f64>().ok())
}

fn core_package(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    matches!(
        id.as_str(),
        "android"
            | "com.android.shell"
            | "com.android.systemui"
            | "com.android.phone"
            | "com.android.settings"
            | "com.android.server.telecom"
            | "com.google.android.gms"
            | "com.google.android.gsf"
            | "com.android.vending"
            | "com.miui.home"
            | "com.mi.android.globallauncher"
            | "com.miui.securitycenter"
            | "com.miui.securitycore"
            | "com.miui.securityadd"
            | "com.miui.powerkeeper"
            | "com.miui.core"
            | "com.miui.freeform"
            | "com.xiaomi.xmsf"
            | "com.xiaomi.xmsfkeeper"
            | "com.lbe.security.miui"
            | "com.xiaomi.finddevice"
            | "com.xiaomi.account"
            | "com.miui.daemon"
            | "com.miui.rom"
            | "com.miui.guardprovider"
            | "com.miui.miuisdk"
            | "com.miui.system"
    ) || id.starts_with("com.android.providers.")
        || [
            ".packageinstaller",
            ".permissioncontroller",
            ".systemui",
            ".telephony",
            ".networkstack",
            ".connectivity",
            ".updater",
            ".setupwizard",
            ".devicepolicy",
            ".keychain",
            ".credentialmanager",
            ".externalstorage",
            ".documentsui",
            ".webview",
        ]
        .iter()
        .any(|part| id.contains(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready_and_unauthorized_transports() {
        let rows = parse_devices(
            "List of devices attached\r\nA12\tunauthorized transport_id:1\r\n10.0.0.2:53221 device model:Pixel_7 transport_id:3\r\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].state, "unauthorized");
        assert_eq!(rows[1].connection, "Wireless");
        assert_eq!(rows[1].model, "Pixel 7");
    }

    #[test]
    fn reads_exact_package_and_user_and_rejects_untrusted_arguments() {
        let parsed = parse_package_state("  Package [com.test] (0):\n    User 0: installed=true enabled=0\n    User 10: installed=false enabled=3", "com.test", 10).unwrap();
        assert_eq!(
            parsed,
            Some(PackageState {
                installed: false,
                enabled: Some(3)
            })
        );
        assert!(!valid_package("com.test;id"));
        assert!(!valid_package("-r"));
        assert!(!numeric_setting("1;id"));
        assert!(same_setting(Some("0.50"), Some("0.5")));
    }
}
