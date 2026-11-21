## Xiaomi Debloat Tool

A Tauri and Rust desktop app for managing Android 11+ devices from Windows 10/11 x64 over USB or wireless ADB. Xiaomi, Redmi, and POCO phones get built-in debloat presets.

## What it does

- Disable, remove, or restore apps for the active Android user, with batch previews and protection for identified core components.
- Apply Xiaomi presets or save, import, and export your own package selections.
- Adjust animations, display, and power settings. Install standalone or split APKs.
- Review local activity, undo supported changes, and export operation or diagnostic logs.

Android decides which package changes to allow. Some HyperOS builds block disabling system apps. Per-user removal is available for separate review, and restoring a removed app may require its original signed APK.

## Connect a device

1. If ADB is missing, extract [Android Platform-Tools](https://developer.android.com/tools/releases/platform-tools) and select `adb.exe` under **ADB setup**.
2. For USB, enable **USB debugging**, plug in the phone, and approve the PC. For Wi-Fi, enable **Wireless debugging** and enter the pairing code in the app’s **Wireless** dialog. Use the main Wireless debugging address if the phone needs a separate connection step.
3. Select the device in the top bar and review the proposed changes before applying them.

## Build from source

Install Node.js 22+ and the [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/), including the MSVC Rust toolchain.

```powershell
npm ci
npm run tauri -- dev
npm run tauri -- build --target x86_64-pc-windows-msvc
```

Find the unsigned installer under `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`.
