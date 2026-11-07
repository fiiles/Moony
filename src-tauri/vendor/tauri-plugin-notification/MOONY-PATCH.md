# Vendored tauri-plugin-notification 2.8.0 (patched)

This directory is an exact copy of the crates.io release `tauri-plugin-notification` 2.8.0
(checksum `5bc6c194a95a535abdfbe3bc1af9a43935b8b9b29c216b8e71af8b59132703cf`), wired in through
`[patch.crates-io]` in `src-tauri/Cargo.toml`. Its licenses (`LICENSE_APACHE-2.0`, `LICENSE_MIT`,
`LICENSE.spdx`) apply unchanged.

The only change is in `ios/Sources/NotificationHandler.swift`:

- `toActiveNotification` no longer force-unwraps `notificationsMap[request.identifier]`. The map
  only holds the notifications scheduled by the running process, so a notification scheduled by an
  earlier run crashed the app when it was tapped (`didReceive`), presented in the foreground
  (`willPresent`) or listed (`active`). `sound` and `attachments` now fall back to empty.
- `ActiveNotification` carries `extra`, read from the request's own `userInfo["__EXTRA__"]` (where
  the plugin stores it when scheduling), so a tap reports the payload across app runs. Moony puts
  the target screen there (`extra.path`, `src/mobile/lib/notification-target.ts`).

When upgrading the plugin: check whether upstream has fixed the force unwrap and returns `extra`
on iOS; if so, drop this directory and the patch entry. Otherwise copy the new release here and
apply the same two changes.
