//! Moony's phone keychain: one secret per key, stored behind the device biometry (iOS Keychain
//! with `.biometryCurrentSet`, Android Keystore with an enrolment-bound AES-GCM key), plus the
//! privacy screen that covers the app in the app switcher / recents, and the call that keeps the
//! app's data out of iOS device backups.
//!
//! Rust is the only caller of [`Keychain::store`] / [`Keychain::retrieve`] /
//! [`Keychain::remove`]: the default permission set grants the webview `status` and
//! `set_privacy_screen` only, so a stored secret never crosses into JavaScript. The same goes for
//! [`Keychain::exclude_from_backup`], which keeps the app's data folder, the webview's storage and
//! the staged and received files out of device backups.
//!
//! Accepted residual: on its way to and from the native side the secret (the app's master key as
//! hex) is plain text that `zeroize::Zeroizing` cannot reach: the JSON the plugin bridge carries
//! (the serialized `StorePayload`, the reply `RetrieveResponse` is parsed from) and the Swift
//! `String` (Kotlin `String` on Android) on the native side. The app wipes its own copies (the
//! parsed value moves into a `Zeroizing` in `services/biometric_unlock.rs`); the bridge buffers
//! live only for the call and are freed, not wiped.
//!
//! On the desktop the crate compiles and registers on every platform. The desktop backend wraps
//! the secret with a key that only the platform's biometry can use and keeps the wrapped blob in a
//! private file in the app data directory (the same one as the database); on macOS that is Touch ID
//! through a Secure Enclave key, on Windows it is Windows Hello through a `KeyCredentialManager`
//! credential. On Linux every call returns [`KeychainError::Unsupported`]. The desktop webview is
//! not granted any of the plugin's permissions: it reaches biometrics only through the app's
//! commands.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(desktop)]
mod desktop;
mod error;
#[cfg(mobile)]
mod mobile;
mod models;

#[cfg(desktop)]
pub use desktop::Keychain;
pub use error::{KeychainError, Result};
#[cfg(mobile)]
pub use mobile::Keychain;
pub use models::{BiometryType, Status};

/// Plugin name: the `plugin:moony-keychain|…` prefix and the `moony-keychain:` permission prefix.
pub const PLUGIN_NAME: &str = "moony-keychain";

/// Access to the [`Keychain`] from an `App`, `AppHandle`, `Window` or `Webview`.
pub trait KeychainExt<R: Runtime> {
    fn keychain(&self) -> &Keychain<R>;
}

impl<R: Runtime, T: Manager<R>> KeychainExt<R> for T {
    fn keychain(&self) -> &Keychain<R> {
        self.state::<Keychain<R>>().inner()
    }
}

/// Builds the plugin. On a phone the native side starts with the privacy screen enabled; on the
/// desktop it takes the app data directory and identifier from the app.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|app, api| {
            #[cfg(mobile)]
            let keychain = mobile::init(app, api)?;
            #[cfg(desktop)]
            let keychain = desktop::init(app, api);
            app.manage(keychain);
            Ok(())
        })
        .build()
}
