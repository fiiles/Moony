use std::path::Path;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use zeroize::Zeroizing;

#[cfg(target_os = "ios")]
use crate::models::bridge;
use crate::{KeychainError, Result, Status};

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

#[derive(Serialize)]
struct ExcludeFromBackupPayload<'a> {
    path: &'a str,
}

#[derive(Deserialize)]
struct RetrieveResponse {
    value: Option<String>,
}

#[derive(Deserialize)]
struct ExcludeFromBackupResponse {
    excluded: bool,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct CopyFromUriPayload<'a> {
    uri: &'a str,
    dir: &'a str,
}

#[cfg(target_os = "android")]
#[derive(Deserialize)]
struct CopyFromUriResponse {
    path: String,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct CopyToUriPayload<'a> {
    path: &'a str,
    uri: &'a str,
}

#[cfg(target_os = "android")]
fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| KeychainError::Failed("the path is not valid UTF-8".to_string()))
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

    /// Keeps the file or folder at `path` (a folder with everything in it) out of device backups
    /// and returns whether that holds afterwards. iOS sets `isExcludedFromBackup` on the path and
    /// reads the value back, so `Ok(false)` means the flag did not take; Android answers `Ok(false)`
    /// until its backup rules exist (plan Phase 7). The path must exist. Rust-only, like `store`.
    pub fn exclude_from_backup(&self, path: &Path) -> Result<bool> {
        let path = path
            .to_str()
            .ok_or_else(|| KeychainError::Failed("the path is not valid UTF-8".to_string()))?;
        self.0
            .run_mobile_plugin::<ExcludeFromBackupResponse>(
                "excludeFromBackup",
                ExcludeFromBackupPayload { path },
            )
            .map(|response| response.excluded)
            .map_err(Into::into)
    }

    /// Android only. Copies the document behind a `content://` URI (a picked file, a statement
    /// shared or opened in Moony) into `dir` under its display name, made safe and unique, and
    /// answers the copy's path. Rust-only, like `store`: the caller decides the folder.
    #[cfg(target_os = "android")]
    pub fn copy_from_uri(&self, uri: &str, dir: &Path) -> Result<std::path::PathBuf> {
        let dir = utf8(dir)?;
        self.0
            .run_mobile_plugin::<CopyFromUriResponse>(
                "copyFromUri",
                CopyFromUriPayload { uri, dir },
            )
            .map(|response| std::path::PathBuf::from(response.path))
            .map_err(Into::into)
    }

    /// Android only. Writes the file at `path` into the document behind a `content://` URI (the
    /// one the save picker created), replacing what it held. Rust-only: the caller checks that
    /// `path` is a staged export.
    #[cfg(target_os = "android")]
    pub fn copy_to_uri(&self, path: &Path, uri: &str) -> Result<()> {
        let path = utf8(path)?;
        self.0
            .run_mobile_plugin("copyToUri", CopyToUriPayload { path, uri })
            .map_err(Into::into)
    }

    /// Whether biometry-protected storage is usable now, and which biometry the device has.
    pub fn status(&self) -> Result<Status> {
        self.0.run_mobile_plugin("status", ()).map_err(Into::into)
    }

    /// Seals device sync's 32-byte binding to this phone and answers the blob. iOS: a new Secure
    /// Enclave key (no prompt, usable only while the phone is unlocked), its handle inside the
    /// blob, as on macOS (`MoonyBinding.swift`); `Unsupported` without a Secure Enclave (the
    /// Xcode 27 simulator has one: `SecureEnclave.isAvailable` is true there and the seal works).
    /// Android: `Unsupported`, so the binding stays off, as on Linux.
    pub fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>> {
        #[cfg(target_os = "ios")]
        {
            let secret = bridge::encode_secret(secret);
            let reply: bridge::BindReply = self
                .0
                .run_mobile_plugin("bindSeal", bridge::BindSealPayload { secret: &secret })?;
            bridge::bind_reply_blob(reply)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = secret;
            Err(KeychainError::Unsupported)
        }
    }

    /// Opens a blob [`Keychain::bind_seal`] made on this phone. `Invalidated`: it was sealed on
    /// another phone or is no binding (Rust classifies the native failure,
    /// [`bridge::classify_ios_binding`]); `Failed`: a refusal of the moment (the phone is locked).
    /// Android: `Unsupported`.
    pub fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        #[cfg(target_os = "ios")]
        {
            let blob = bridge::encode_blob(blob);
            let reply: bridge::BindReply = self
                .0
                .run_mobile_plugin("bindOpen", bridge::BindOpenPayload { blob: &blob })?;
            bridge::bind_reply_secret(reply)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = blob;
            Err(KeychainError::Unsupported)
        }
    }

    /// Copies `text` (the recovery code) to the pasteboard for this phone only (`.localOnly`: no
    /// Universal Clipboard) with a 60 s expiry, and answers the pasteboard's change count as the
    /// token [`Keychain::clear_clipboard_if_unchanged`] takes. The text crosses the bridge as JSON
    /// and lives in a Swift `String`, neither wiped: the residual of copying a secret at all.
    /// Android: `Unsupported`; the UI copies itself and asks the person to clear the clipboard.
    pub fn copy_secret_text(&self, text: &str) -> Result<u64> {
        #[cfg(target_os = "ios")]
        {
            self.0
                .run_mobile_plugin::<bridge::CopySecretTextReply>(
                    "copySecretText",
                    bridge::CopySecretTextPayload { text },
                )
                .map(|reply| reply.token)
                .map_err(Into::into)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = text;
            Err(KeychainError::Unsupported)
        }
    }

    /// Empties the pasteboard if its change count is still `token`, that is, if nothing was copied
    /// since [`Keychain::copy_secret_text`]; answers whether it did. At lock or in the background
    /// iOS may limit pasteboard access or `changeCount` freshness, so this clear can miss; the 60 s
    /// `.expirationDate` set at the copy is the backstop. Android: `Unsupported`.
    pub fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool> {
        #[cfg(target_os = "ios")]
        {
            // A token no change count can equal (Swift's `Int`): the pasteboard is not ours to clear.
            if i64::try_from(token).is_err() {
                return Ok(false);
            }
            self.0
                .run_mobile_plugin::<bridge::ClearClipboardReply>(
                    "clearClipboardIfUnchanged",
                    bridge::ClearClipboardPayload { token },
                )
                .map(|reply| reply.cleared)
                .map_err(Into::into)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = token;
            Err(KeychainError::Unsupported)
        }
    }
}
