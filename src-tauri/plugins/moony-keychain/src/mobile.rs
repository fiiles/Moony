use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{Result, Status};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.moonyfinance.keychain";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_moony_keychain);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<Keychain<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "KeychainPlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_moony_keychain)?;
    Ok(Keychain(handle))
}

/// The phone keychain. Every call blocks until the native side answers (`retrieve` until the
/// person answers the biometric prompt), so call it from `spawn_blocking` or an async command,
/// never from the main thread.
pub struct Keychain<R: Runtime>(PluginHandle<R>);

#[derive(Serialize)]
struct StorePayload<'a> {
    key: &'a str,
    value: &'a str,
    reason: &'a str,
}

#[derive(Serialize)]
struct RetrievePayload<'a> {
    key: &'a str,
    reason: &'a str,
}

#[derive(Serialize)]
struct KeyPayload<'a> {
    key: &'a str,
}

#[derive(Serialize)]
struct PrivacyScreenPayload {
    enabled: bool,
}

#[derive(Deserialize)]
struct RetrieveResponse {
    value: Option<String>,
}

impl<R: Runtime> Keychain<R> {
    /// Stores `value_hex` under `key` behind the current biometric enrolment, replacing any
    /// earlier value. Both platforms prompt with `reason` first: iOS evaluates Face ID / Touch ID
    /// (so the system's permission alert comes in context) and stores with that context; Android
    /// uses it as the title (its key is auth-bound for encryption too).
    pub fn store(&self, key: &str, value_hex: &str, reason: &str) -> Result<()> {
        self.0
            .run_mobile_plugin(
                "store",
                StorePayload {
                    key,
                    value: value_hex,
                    reason,
                },
            )
            .map_err(Into::into)
    }

    /// Prompts the person with `reason` and returns the stored value; `Ok(None)` when nothing
    /// is stored (no prompt then). `Err(KeychainError::Invalidated)` when the enrolment changed
    /// since `store`, `Err(KeychainError::Cancelled)` when the prompt was dismissed.
    pub fn retrieve(&self, key: &str, reason: &str) -> Result<Option<String>> {
        self.0
            .run_mobile_plugin::<RetrieveResponse>("retrieve", RetrievePayload { key, reason })
            .map(|response| response.value)
            .map_err(Into::into)
    }

    /// Removes the value stored under `key` (no prompt). Removing a missing key succeeds.
    pub fn remove(&self, key: &str) -> Result<()> {
        self.0
            .run_mobile_plugin("remove", KeyPayload { key })
            .map_err(Into::into)
    }

    /// Turns the app-switcher / recents privacy screen on or off (on by default).
    pub fn set_privacy_screen(&self, enabled: bool) -> Result<()> {
        self.0
            .run_mobile_plugin("setPrivacyScreen", PrivacyScreenPayload { enabled })
            .map_err(Into::into)
    }

    /// Whether biometry-protected storage is usable now, and which biometry the device has.
    pub fn status(&self) -> Result<Status> {
        self.0.run_mobile_plugin("status", ()).map_err(Into::into)
    }
}
