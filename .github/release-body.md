Moony is a private personal finance app: your records stay on your computer, in an encrypted database. Earlier versions are in the [changelog](https://github.com/fiiles/Moony/blob/main/CHANGELOG.md).

## What's new in @VERSION@

@CHANGES@

## Download

| Your computer | File |
|---|---|
| Mac with Apple Silicon (M1, M2, M3, M4 …) | `Moony_@VERSION@_aarch64.dmg` |
| Mac with an Intel processor | `Moony_@VERSION@_x64.dmg` |
| Windows 10 or 11, 64-bit | `Moony_@VERSION@_x64-setup.exe` |
| Linux (x86_64), any distribution | `Moony_@VERSION@_amd64.AppImage` |
| Debian, Ubuntu (x86_64) | `Moony_@VERSION@_amd64.deb` |
| Fedora, openSUSE (x86_64) | `Moony-@VERSION@-1.x86_64.rpm` |
| Linux on ARM (aarch64), any distribution | `Moony_@VERSION@_aarch64.AppImage` |
| Debian, Ubuntu on ARM (arm64) | `Moony_@VERSION@_arm64.deb` |
| Fedora, openSUSE on ARM (aarch64) | `Moony-@VERSION@-1.aarch64.rpm` |

Not sure which Mac you have? Apple menu → **About This Mac**: the *Chip* line says *Apple M…* or *Intel*. On Linux, `uname -m` prints `x86_64` or `aarch64`.

The `.tar.gz`, `.sig` and `latest.json` files are for the built-in updater; you do not need to download them.

## Opening Moony on a Mac

Moony for macOS is signed with an Apple Developer ID and notarized by Apple, so it opens like any other app: open the `.dmg`, drag **Moony** into **Applications** and open it.

## First launch on Windows: the installer is not signed yet

The Windows installer is not signed yet, so Windows warns you the first time you open it. The app is not harmful. You need the steps below once, for the copy you downloaded in your browser.

1. If the browser says the file is not commonly downloaded, keep it. In Edge: **⋯ → Keep → Show more → Keep anyway**.
2. Run the installer. When SmartScreen shows *"Windows protected your PC"*, click **More info**, then **Run anyway**.

## Linux

The commands use the x86_64 files; on ARM, use `aarch64.AppImage`, `arm64.deb` and `aarch64.rpm` instead.

- **AppImage:** `chmod +x Moony_@VERSION@_amd64.AppImage`, then run it. Some distributions need the `libfuse2` package first.
- **Debian / Ubuntu:** `sudo apt install ./Moony_@VERSION@_amd64.deb`
- **Fedora / openSUSE:** `sudo dnf install ./Moony-@VERSION@-1.x86_64.rpm`

## After installing

Moony asks for a password and shows a **recovery key** on the first start; save the key somewhere safe. Later versions arrive through the built-in updater after you unlock the app (you can switch the check off in Settings → Security). More in the [README](https://github.com/fiiles/Moony#download-and-install).
