## Xiaomi Debloat Tool

A dark desktop app for managing Android 11+ devices from Windows 10/11 or Linux x64 over USB or wireless ADB. Xiaomi, Redmi, and POCO phones get built-in debloat presets.

## Install

Get the Windows setup executable or the Linux Debian package or AppImage from [GitHub Releases](https://github.com/jqvxz/xiaomi-debloat-tool/releases). Make the AppImage executable before launching it.

## What it does

- Disable, remove, or restore apps for the active Android user, with batch previews and protection for identified core components.
- Apply Xiaomi presets or save, import, and export your own package selections.
- Adjust animations, display, and power settings. Install standalone or split APKs.
- Review local activity, undo supported changes, and export operation or diagnostic logs.

Android decides which package changes to allow. Some HyperOS builds block disabling system apps. Per-user removal is available for separate review, and restoring a removed app may require its original signed APK.

## Connect a device

1. Put ADB on your PATH or select it under **ADB setup**. [Android Platform-Tools](https://developer.android.com/tools/releases/platform-tools) includes `adb.exe` on Windows and `adb` on Linux.
2. For USB, enable **USB debugging**, plug in the phone, and approve the PC. For Wi-Fi, enable **Wireless debugging** and enter the pairing code in the app’s **Wireless** dialog. Use the main Wireless debugging address if the phone needs a separate connection step.
3. Select the device in the top bar and review the proposed changes before applying them.

## Build from source

Install Node.js 22+ and the [Tauri prerequisites for your OS](https://v2.tauri.app/start/prerequisites/). Use the MSVC Rust toolchain on Windows.

```powershell
npm ci
npm run tauri -- dev
npm run tauri -- build
```

Find the packages under `src-tauri/target/release/bundle/`.
