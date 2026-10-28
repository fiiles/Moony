//! The Windows native: Windows Hello through `KeyCredentialManager` (WinRT).
//!
//! Hello keeps a per-app key pair in the TPM (named after the app identifier) that signs a
//! challenge only after the Hello prompt. [`hello_wrap`] turns that signature into the key that
//! wraps the master key; this side only drives the credential and maps its answers. There is no
//! DPAPI or password fallback: without Hello's signature the blob cannot be opened.
//!
//! `KeyCredentialManager` takes no prompt text (Windows words the dialog itself), so `reason` is
//! unused here. Its dialog belongs to no window of ours and can open behind the app, so while a
//! call waits for it, [`bring_dialog_forward`] raises it.
//!
//! Accepted residual: the signature (the key material) passes through WinRT buffers (`IBuffer`,
//! the `Array` it is copied out with), which are freed but not wiped. The Rust copies are
//! `Zeroizing`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::{Array, HRESULT, HSTRING};
use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};
use zeroize::Zeroizing;

use super::hello_wrap::{self, OpenError};
use super::NativeBiometrics;
use crate::{BiometryType, KeychainError, Result, Status};

/// The window class of the Windows Hello dialog.
const HELLO_DIALOG_CLASS: &str = "Credential Dialog Xaml Host";
/// How long to look for the dialog, and how often.
const RAISE_FOR: Duration = Duration::from_secs(3);
const RAISE_EVERY: Duration = Duration::from_millis(100);

/// What `DeleteAsync` fails with when there is no credential to delete (`NTE_NO_KEY`,
/// `NTE_NOT_FOUND`, `ERROR_NOT_FOUND`).
const NO_CREDENTIAL: [HRESULT; 3] = [
    HRESULT(0x8009_000D_u32 as i32),
    HRESULT(0x8009_0011_u32 as i32),
    HRESULT(0x8007_0490_u32 as i32),
];

/// Windows Hello with a credential named after the app. Every call but `status` blocks on the
/// Hello prompt.
pub(super) struct WinNative {
    credential_name: HSTRING,
}

impl WinNative {
    pub(super) fn new(app_identifier: &str) -> Self {
        Self {
            credential_name: HSTRING::from(app_identifier),
        }
    }
}

impl NativeBiometrics for WinNative {
    fn status(&self) -> Result<Status> {
        // The type stays Windows Hello even when Hello is not set up: the settings copy names it.
        Ok(Status {
            available: is_supported()?,
            biometry_type: BiometryType::WindowsHello,
        })
    }

    /// Creates a fresh credential (its Hello prompt is the person's confirmation), replacing any
    /// earlier one, so blobs wrapped before can no longer be opened.
    fn wrap(&self, secret: &[u8; 32], _reason: &str) -> Result<Vec<u8>> {
        require_hello()?;
        let operation = KeyCredentialManager::RequestCreateAsync(
            &self.credential_name,
            KeyCredentialCreationOption::ReplaceExisting,
        )
        .map_err(winrt_failure)?;
        let created = bring_dialog_forward(|| operation.join()).map_err(winrt_failure)?;
        check(created.Status().map_err(winrt_failure)?)?;
        let credential = created.Credential().map_err(winrt_failure)?;

        let challenge = hello_wrap::new_challenge();
        let signature = sign(&credential, &challenge)?;
        hello_wrap::seal(&challenge, &signature, secret)
    }

    fn unwrap(&self, blob: &[u8], _reason: &str) -> Result<Zeroizing<[u8; 32]>> {
        // A blob of another format can never be opened; say so before prompting.
        let challenge = hello_wrap::challenge_of(blob).ok_or(KeychainError::Invalidated)?;
        require_hello()?;
        let opened = KeyCredentialManager::OpenAsync(&self.credential_name)
            .and_then(|operation| operation.join())
            .map_err(winrt_failure)?;
        check(opened.Status().map_err(winrt_failure)?)?;
        let credential = opened.Credential().map_err(winrt_failure)?;

        let signature = sign(&credential, &challenge)?;
        hello_wrap::open(blob, &signature).map_err(open_failure)
    }

    fn forget(&self) -> Result<()> {
        match KeyCredentialManager::DeleteAsync(&self.credential_name)
            .and_then(|operation| operation.join())
        {
            Ok(()) => Ok(()),
            // Nothing to delete (never created, or already gone) is what forgetting wants.
            Err(error) if NO_CREDENTIAL.contains(&error.code()) => Ok(()),
            Err(error) => Err(winrt_failure(error)),
        }
    }
}

/// Whether Hello is set up (a PIN at least) on this account; asks without a prompt.
fn is_supported() -> Result<bool> {
    KeyCredentialManager::IsSupportedAsync()
        .and_then(|operation| operation.join())
        .map_err(winrt_failure)
}

/// `Unavailable` unless Hello is set up, so no call reaches a prompt that cannot work.
fn require_hello() -> Result<()> {
    if is_supported()? {
        Ok(())
    } else {
        Err(KeychainError::Unavailable)
    }
}

/// Hello's signature over `challenge`, after the prompt.
fn sign(credential: &KeyCredential, challenge: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>> {
    let data = CryptographicBuffer::CreateFromByteArray(challenge).map_err(winrt_failure)?;
    let operation = credential.RequestSignAsync(&data).map_err(winrt_failure)?;
    let signed = bring_dialog_forward(|| operation.join()).map_err(winrt_failure)?;
    check(signed.Status().map_err(winrt_failure)?)?;
    let buffer = signed.Result().map_err(winrt_failure)?;
    let mut bytes = Array::<u8>::new();
    CryptographicBuffer::CopyToByteArray(&buffer, &mut bytes).map_err(winrt_failure)?;
    Ok(Zeroizing::new(bytes.to_vec()))
}

/// A credential call's status as the keychain's error. Only a credential that is gone is
/// `Invalidated` (that switches biometric unlock off for good); every answer the person gives and
/// every passing condition stays retryable.
fn check(status: KeyCredentialStatus) -> Result<()> {
    match status {
        KeyCredentialStatus::Success => Ok(()),
        // "Use my PIN/password instead" answers the prompt the same way Cancel does.
        KeyCredentialStatus::UserCanceled | KeyCredentialStatus::UserPrefersPassword => {
            Err(KeychainError::Cancelled)
        }
        KeyCredentialStatus::NotFound => Err(KeychainError::Invalidated),
        // The TPM locked itself after too many attempts: no usable Hello right now, like a
        // biometry lockout on the other platforms.
        KeyCredentialStatus::SecurityDeviceLocked => Err(KeychainError::Unavailable),
        KeyCredentialStatus::UnknownError => {
            Err(KeychainError::Failed("Windows Hello failed".to_string()))
        }
        other => Err(KeychainError::Failed(format!(
            "Windows Hello answered status {}",
            other.0
        ))),
    }
}

/// A blob Hello's signature cannot open was wrapped for another credential (or altered): it can
/// never be opened again.
fn open_failure(error: OpenError) -> KeychainError {
    match error {
        OpenError::Malformed | OpenError::WrongKey => KeychainError::Invalidated,
        OpenError::KeyDerivation => {
            KeychainError::Failed("Windows Hello: could not derive the unwrap key".to_string())
        }
    }
}

fn winrt_failure(error: windows::core::Error) -> KeychainError {
    KeychainError::Failed(format!("Windows Hello: {error}"))
}

/// Runs `wait` (a blocking wait on a Hello call) while a helper thread looks for the Hello dialog
/// for up to [`RAISE_FOR`] and brings it to the front once it shows, so it does not open hidden
/// behind the app.
fn bring_dialog_forward<T>(wait: impl FnOnce() -> T) -> T {
    let done = Arc::new(AtomicBool::new(false));
    let helper = {
        let done = Arc::clone(&done);
        thread::spawn(move || {
            let class = HSTRING::from(HELLO_DIALOG_CLASS);
            let started = Instant::now();
            while !done.load(Ordering::Relaxed) && started.elapsed() < RAISE_FOR {
                // SAFETY: plain user32 calls with a live wide string; a handle whose window closed
                // in between only makes `SetForegroundWindow` return false.
                if let Ok(dialog) = unsafe { FindWindowW(&class, None) } {
                    if unsafe { SetForegroundWindow(dialog) }.as_bool() {
                        return;
                    }
                }
                thread::sleep(RAISE_EVERY);
            }
        })
    };
    let answer = wait();
    done.store(true, Ordering::Relaxed);
    // The helper only raises a window; a panic there leaves nothing to recover.
    let _ = helper.join();
    answer
}
