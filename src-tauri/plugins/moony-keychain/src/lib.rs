//! Moony's phone keychain: one secret per key, stored behind the device biometry (iOS Keychain
//! with `.biometryCurrentSet`, Android Keystore with an enrolment-bound AES-GCM key), plus the
//! privacy screen that covers the app in the app switcher / recents.
//!
//! Rust is the only caller of [`Keychain::store`] / [`Keychain::retrieve`] /
//! [`Keychain::remove`]: the default permission set grants the webview `status` and
//! `set_privacy_screen` only, so a stored secret never crosses into JavaScript.
//!
//! On the desktop the crate compiles and every call returns [`KeychainError::Unsupported`]; the
//! app registers the plugin under `#[cfg(mobile)]` only.

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

/// Builds the plugin. On a phone the native side starts with the privacy screen enabled.
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
