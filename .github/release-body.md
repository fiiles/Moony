Moony is a private personal finance app: your records stay on your computer, in an encrypted database. Earlier versions are in the [changelog](https://github.com/fiiles/Moony/blob/main/CHANGELOG.md).

## What's new in @VERSION@

@CHANGES@

## Download

| Your computer | File |
|---|---|
| Mac with Apple Silicon (M1, M2, M3, M4 …) | `Moony_@VERSION@_aarch64.dmg` |
| Mac with an Intel processor | `Moony_@VERSION@_x64.dmg` |
| Windows 10 or 11, 64-bit | `Moony_@VERSION@_x64-setup.exe` |
| Linux, any distribution | `Moony_@VERSION@_amd64.AppImage` |
| Debian, Ubuntu | `Moony_@VERSION@_amd64.deb` |
| Fedora, openSUSE | `Moony-@VERSION@-1.x86_64.rpm` |

Not sure which Mac you have? Apple menu → **About This Mac**: the *Chip* line says *Apple M…* or *Intel*.

The `.tar.gz`, `.sig` and `latest.json` files are for the built-in updater; you do not need to download them.

## First launch: the installers are not signed yet

Moony is not yet signed by Apple or Microsoft, so your system warns you the first time you open it. The app is not damaged. You need the step below once, for the copy you downloaded in your browser.

### macOS

1. Open the `.dmg` and drag **Moony** into **Applications**.
2. Open **Terminal** (Applications → Utilities → Terminal), paste this line and press Return:
   ```
   xattr -cr /Applications/Moony.app
   ```
   It removes the "downloaded from the internet" mark that makes macOS say *"Moony is damaged and can't be opened"*.
3. Open Moony as usual.

If macOS instead says that it *cannot verify the developer*, you can also skip Terminal: try to open Moony once, then go to **System Settings → Privacy & Security**, scroll to *Security* and click **Open Anyway** next to Moony.

### Windows

1. If the browser says the file is not commonly downloaded, keep it. In Edge: **⋯ → Keep → Show more → Keep anyway**.
2. Run the installer. When SmartScreen shows *"Windows protected your PC"*, click **More info**, then **Run anyway**.

### Linux

- **AppImage:** `chmod +x Moony_@VERSION@_amd64.AppImage`, then run it. Some distributions need the `libfuse2` package first.
- **Debian / Ubuntu:** `sudo apt install ./Moony_@VERSION@_amd64.deb`
- **Fedora / openSUSE:** `sudo dnf install ./Moony-@VERSION@-1.x86_64.rpm`

## After installing

Moony asks for a password and shows a **recovery key** on the first start; save the key somewhere safe. Later versions arrive through the built-in updater after you unlock the app (you can switch the check off in Settings → Security). More in the [README](https://github.com/fiiles/Moony#install).
