use crate::adb::{self, Failure, Inventory, PackageState, Target};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum Action {
    Package { id: String, verb: Verb },
    Setting { id: String, value: String },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Verb {
    Disable,
    Remove,
    Enable,
    Restore,
}

impl Action {
    pub fn id(&self) -> &str {
        match self {
            Self::Package { id, .. } | Self::Setting { id, .. } => id,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Package { id, verb } => format!(
                "{} {}",
                match verb {
                    Verb::Disable => "Disable",
                    Verb::Remove => "Remove for user",
                    Verb::Enable => "Enable",
                    Verb::Restore => "Restore",
                },
                id
            ),
            Self::Setting { id, .. } => {
                format!("Set {}", setting(id).map_or(id.as_str(), |spec| spec.label))
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Value {
    Package(Option<PackageState>),
    Setting(Option<String>),
}

impl Value {
    pub fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Setting(a), Self::Setting(b)) => adb::same_setting(a.as_deref(), b.as_deref()),
            _ => self == other,
        }
    }

    pub fn to_label(&self) -> String {
        match self {
            Self::Package(Some(state)) if !state.installed => "Removed for user".into(),
            Self::Package(Some(state)) if state.enabled.is_some_and(|value| value >= 2) => {
                "Disabled".into()
            }
            Self::Package(Some(_)) => "Enabled".into(),
            Self::Package(None) => "Unavailable on device".into(),
            Self::Setting(value) => value.clone().unwrap_or_else(|| "Device default".into()),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub namespace: &'static str,
    pub options: &'static [(&'static str, &'static str)],
}

const SCALES: &[(&str, &str)] = &[
    ("0", "Off"),
    ("0.5", "0.5×"),
    ("1", "1×"),
    ("1.5", "1.5×"),
    ("2", "2×"),
];

pub const SETTINGS: &[Setting] = &[
    Setting {
        id: "window_animation_scale",
        label: "Window animation",
        description: "Speed of opening and closing windows.",
        namespace: "global",
        options: SCALES,
    },
    Setting {
        id: "transition_animation_scale",
        label: "Transition animation",
        description: "Speed of transitions between screens.",
        namespace: "global",
        options: SCALES,
    },
    Setting {
        id: "animator_duration_scale",
        label: "Animator duration",
        description: "Duration of animations inside apps.",
        namespace: "global",
        options: SCALES,
    },
    Setting {
        id: "screen_off_timeout",
        label: "Screen timeout",
        description: "How long the display stays on while idle.",
        namespace: "system",
        options: &[
            ("15000", "15 seconds"),
            ("30000", "30 seconds"),
            ("60000", "1 minute"),
            ("120000", "2 minutes"),
            ("300000", "5 minutes"),
            ("600000", "10 minutes"),
        ],
    },
    Setting {
        id: "screen_brightness_mode",
        label: "Adaptive brightness",
        description: "Adjust brightness to ambient light on supported devices.",
        namespace: "system",
        options: &[("0", "Off"), ("1", "On")],
    },
    Setting {
        id: "stay_on_while_plugged_in",
        label: "Stay awake while charging",
        description: "Keep the display awake while connected to power.",
        namespace: "global",
        options: &[("0", "Off"), ("2", "USB only"), ("15", "Any charger")],
    },
];

fn setting(id: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|spec| spec.id == id)
}

#[derive(Clone, Copy, Serialize)]
pub struct CatalogApp {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const CATALOG: &[CatalogApp] = &[
    CatalogApp {
        id: "com.miui.analytics",
        name: "MIUI Analytics",
        description: "Collects Xiaomi device and app analytics.",
    },
    CatalogApp {
        id: "com.miui.msa.global",
        name: "MIUI System Ads",
        description: "Supports ads and promotions in some Xiaomi apps.",
    },
    CatalogApp {
        id: "com.miui.bugreport",
        name: "Xiaomi Feedback",
        description: "Lets you submit bug reports and feedback to Xiaomi.",
    },
    CatalogApp {
        id: "com.xiaomi.glgm",
        name: "Xiaomi Game Center",
        description: "Offers game discovery on some regional builds.",
    },
    CatalogApp {
        id: "com.facebook.appmanager",
        name: "Meta App Manager",
        description: "Manages some preloaded Meta apps.",
    },
    CatalogApp {
        id: "com.facebook.services",
        name: "Meta Services",
        description: "Provides background services for preloaded Meta apps.",
    },
    CatalogApp {
        id: "com.facebook.system",
        name: "Meta App Installer",
        description: "Supports provisioning of preloaded Meta apps.",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    pub version: u8,
    pub name: String,
    pub packages: Vec<String>,
}

pub fn builtin_presets() -> Vec<Preset> {
    [
        ("Ads and Analytics", &CATALOG[..2]),
        ("Optional Preloads", &CATALOG[2..]),
    ]
    .into_iter()
    .map(|(name, entries)| Preset {
        version: 1,
        name: name.into(),
        packages: entries.iter().map(|entry| entry.id.into()).collect(),
    })
    .collect()
}

pub const MAX_ITEMS: usize = 150;

pub fn validate_preset(mut preset: Preset) -> Result<Preset, String> {
    preset.name = preset.name.trim().into();
    if preset.version != 1
        || preset.name.is_empty()
        || preset.name.chars().count() > 64
        || preset.name.chars().any(char::is_control)
        || preset.packages.is_empty()
        || preset.packages.len() > MAX_ITEMS
        || preset.packages.iter().any(|id| !adb::valid_package(id))
    {
        return Err(
            "A preset needs version 1, a name of 1–64 characters, and 1–150 valid package IDs."
                .into(),
        );
    }
    let mut seen = HashSet::new();
    preset.packages.retain(|id| seen.insert(id.clone()));
    Ok(preset)
}

pub fn validate_action(action: &Action) -> adb::Result<()> {
    match action {
        Action::Package { id, .. } if adb::valid_package(id) => Ok(()),
        Action::Setting { id, value }
            if setting(id)
                .is_some_and(|spec| spec.options.iter().any(|(allowed, _)| value == allowed)) =>
        {
            Ok(())
        }
        _ => Err(Failure::new(
            "Invalid package or unsupported setting value.",
        )),
    }
}

pub fn assess(
    action: &Action,
    inventory: &Inventory,
    advanced: bool,
    batch: bool,
) -> adb::Result<()> {
    if let Action::Package { id, verb } = action {
        let package = inventory
            .packages
            .get(id)
            .ok_or_else(|| Failure::new("Package is unavailable on this device."))?;
        let destructive = matches!(verb, Verb::Disable | Verb::Remove);
        if destructive {
            if let Some(reason) = package.protection {
                return Err(Failure::new(reason));
            }
            if package.system && !CATALOG.iter().any(|entry| entry.id == id) && (!advanced || batch)
            {
                return Err(Failure::new(
                    "Unclassified system apps require an individual Advanced action.",
                ));
            }
        }
        let available = match verb {
            Verb::Disable => package.installed && !package.disabled,
            Verb::Remove => package.installed,
            Verb::Enable => package.installed && package.disabled,
            Verb::Restore => !package.installed,
        };
        if !available {
            return Err(Failure::new(
                "The package is already in the requested state.",
            ));
        }
    }
    Ok(())
}

pub fn capture(target: &Target<'_>, action: &Action) -> adb::Result<Value> {
    match action {
        Action::Package { id, .. } => Ok(Value::Package(target.package_state(id)?)),
        Action::Setting { id, .. } => {
            let spec = setting(id).ok_or_else(|| Failure::new("Unknown setting."))?;
            Ok(Value::Setting(target.setting(spec.namespace, spec.id)?))
        }
    }
}

pub fn check_before(action: &Action, value: &Value) -> adb::Result<()> {
    match (action, value) {
        (Action::Package { .. }, Value::Package(Some(_))) => Ok(()),
        (Action::Setting { value: desired, .. }, Value::Setting(current)) => {
            if adb::same_setting(current.as_deref(), Some(desired)) {
                Err(Failure::new("The setting already has this value."))
            } else if current
                .as_deref()
                .is_some_and(|value| !adb::numeric_setting(value))
            {
                Err(Failure::new(
                    "The current setting value cannot be restored safely.",
                ))
            } else {
                Ok(())
            }
        }
        _ => Err(Failure::new("The prior state could not be captured.")),
    }
}

pub fn apply(target: &Target<'_>, action: &Action) -> adb::Result<()> {
    match action {
        Action::Package { id, verb } => target.pm(
            match verb {
                Verb::Disable => "disable-user",
                Verb::Remove => "uninstall",
                Verb::Enable => "enable",
                Verb::Restore => "install-existing",
            },
            id,
        ),
        Action::Setting { id, value } => {
            let spec = setting(id).ok_or_else(|| Failure::new("Unknown setting."))?;
            target.write_setting(spec.namespace, spec.id, Some(value))
        }
    }
}

pub fn applied(action: &Action, value: &Value) -> bool {
    match (action, value) {
        (
            Action::Package {
                verb: Verb::Remove, ..
            },
            Value::Package(None),
        ) => true,
        (Action::Package { verb, .. }, Value::Package(Some(state))) => match verb {
            Verb::Disable => state.installed && state.enabled == Some(3),
            Verb::Remove => !state.installed,
            Verb::Enable => state.installed && state.enabled == Some(1),
            Verb::Restore => state.installed,
        },
        (
            Action::Setting {
                value: expected, ..
            },
            Value::Setting(actual),
        ) => adb::same_setting(Some(expected), actual.as_deref()),
        _ => false,
    }
}

pub fn revert(
    target: &Target<'_>,
    action: &Action,
    before: &Value,
    current: &Value,
) -> adb::Result<()> {
    match (action, before, current) {
        (Action::Setting { id, .. }, Value::Setting(prior), Value::Setting(_)) => {
            let spec = setting(id).ok_or_else(|| Failure::new("Unknown setting."))?;
            target.write_setting(spec.namespace, spec.id, prior.as_deref())
        }
        (Action::Package { id, .. }, Value::Package(Some(prior)), Value::Package(Some(now))) => {
            let makes_unavailable =
                !prior.installed || prior.enabled.is_some_and(|value| value >= 2);
            if makes_unavailable {
                let inventory = target.inventory()?;
                if !inventory
                    .packages
                    .get(id)
                    .is_some_and(|pkg| pkg.protection.is_none())
                {
                    return Err(Failure::new(
                        "Restoring that state would affect an active or protected package.",
                    ));
                }
            }
            target.verify()?;
            if !prior.installed {
                if now.installed {
                    target.pm("uninstall", id)?;
                }
                return Ok(());
            }
            if !now.installed {
                target.pm("install-existing", id)?;
                target.verify()?;
            }
            if target.package_state(id)?.and_then(|state| state.enabled) != prior.enabled {
                let verb = match prior.enabled {
                    Some(0) => "default-state",
                    Some(1) => "enable",
                    Some(2) => "disable",
                    Some(3) => "disable-user",
                    Some(4) => "disable-until-used",
                    _ => return Err(Failure::new("The prior enabled state is unavailable.")),
                };
                target.pm(verb, id)?;
            }
            Ok(())
        }
        _ => Err(Failure::new("This change needs to be restored manually.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adb::Package;

    #[test]
    fn preset_import_rejects_shell_arguments_and_unknown_fields() {
        let bad = Preset {
            name: "Name".into(),
            version: 1,
            packages: vec!["com.example;id".into()],
        };
        assert!(validate_preset(bad).is_err());
        assert!(
            serde_json::from_str::<Preset>(
                r#"{"name":"name","version":1,"packages":["com.test"],"cmd":"uninstall"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn advanced_app_changes_require_one_package_and_never_bypass_protection() {
        let id = "com.oem.component".to_string();
        let action = Action::Package {
            id: id.clone(),
            verb: Verb::Remove,
        };
        let mut inventory = Inventory {
            packages: [(
                id.clone(),
                Package {
                    id: id.clone(),
                    installed: true,
                    system: true,
                    disabled: false,
                    protection: None,
                },
            )]
            .into(),
            protection_ready: true,
        };
        assert!(assess(&action, &inventory, false, false).is_err());
        assert!(assess(&action, &inventory, true, true).is_err());
        assert!(assess(&action, &inventory, true, false).is_ok());
        inventory.packages.get_mut(&id).unwrap().protection = Some("Launcher");
        assert!(assess(&action, &inventory, true, false).is_err());
    }
}
