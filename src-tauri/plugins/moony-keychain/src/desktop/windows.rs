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
//!
//! [`WinMachine`] is device sync's side: the binding sealed with DPAPI in machine scope
//! (`CRYPTPROTECT_LOCAL_MACHINE`), which another machine cannot open (`dpapi_errors` classifies the
//! answers). A copy on this same machine, by this or another user, opens it: that case is left to
//! the relay's own-log check (plan correction 10). And the clearing clipboard: the secret goes on
//! as `CF_UNICODETEXT` together with `ExcludeClipboardContentFromMonitorProcessing`,
//! `CanIncludeInClipboardHistory` = 0 and `CanUploadToCloudClipboard` = 0, which keep it out of
//! the clipboard history and the cloud clipboard, and is taken off later only while the clipboard
//! sequence number is unchanged. Residual: DPAPI's output buffer is wiped
//! before it is freed, but the clipboard's copy of the text belongs to the system once set.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::{w, Array, HRESULT, HSTRING, PCWSTR};
use windows::Security::Credentials::{
    KeyCredential, KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::Foundation::{GlobalFree, LocalFree, HANDLE, HGLOBAL, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
    CRYPT_INTEGER_BLOB,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};
use zeroize::{Zeroize, Zeroizing};

use super::dpapi_errors;
use super::hello_wrap::{self, OpenError};
use super::{NativeBiometrics, NativeMachine};
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

/// How often to try opening the clipboard while another program holds it open, and how long apart.
const CLIPBOARD_OPEN_ATTEMPTS: u32 = 10;
const CLIPBOARD_OPEN_RETRY: Duration = Duration::from_millis(20);

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

/// Device sync's binding (DPAPI, machine scope) and the clearing clipboard. Nothing prompts.
pub(super) struct WinMachine;

impl NativeMachine for WinMachine {
    fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: 32,
            // DPAPI only reads its input; the pointer is mutable in the signature alone.
            pbData: secret.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at 32 live bytes; DPAPI allocates `output` with LocalAlloc, and
        // `DpapiOutput` frees it.
        unsafe {
            CryptProtectData(
                &input,
                w!("Moony sync binding"),
                None,
                None,
                None,
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|error| dpapi_errors::seal_failure(error.code().0))?;
        Ok(DpapiOutput(output).bytes().to_vec())
    }

    fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        let length = u32::try_from(blob.len()).map_err(|_| KeychainError::Invalidated)?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: length,
            pbData: blob.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `bind_seal`; no description is asked for, so none is allocated.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|error| dpapi_errors::open_failure(error.code().0))?;
        let opened = DpapiOutput(output);
        // DPAPI opened it, so it was sealed here, but not as a binding: not this binding.
        let bytes: &[u8; 32] = opened
            .bytes()
            .try_into()
            .map_err(|_| KeychainError::Invalidated)?;
        Ok(Zeroizing::new(*bytes))
    }

    /// Empties the clipboard and puts the text on it with the three privacy formats, then answers
    /// the clipboard sequence number as the token.
    ///
    /// The token is read just after `CloseClipboard`, not while the clipboard is still open:
    /// Windows may count the changes of a write only when the writer closes the clipboard, and a
    /// token read too early would never match, so the code would stay on the clipboard for good,
    /// the worse failure for a secret. In the moment between the close and the read another
    /// program can write the clipboard. The token is then that program's, and if nothing else is
    /// copied in the next 60 s the clear empties that program's content rather than ours. Nothing
    /// leaks that way (the code was already replaced): at worst the person loses what was copied in
    /// a window of microseconds right after pressing "Copy code", which is accepted.
    fn copy_secret_text(&self, text: &str) -> Result<u64> {
        let privacy = privacy_formats()?;
        let utf16: Zeroizing<Vec<u8>> = Zeroizing::new(
            text.encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        );
        {
            let _clipboard = OpenedClipboard::open()?;
            // SAFETY: the clipboard is open (the guard closes it).
            unsafe { EmptyClipboard() }.map_err(clipboard_failure)?;
            // The markers first, so the text never sits on the clipboard without them. Each is a
            // DWORD zero: "no" for the two `Can…` formats, and "any data" for the exclusion one.
            for format in privacy {
                set_clipboard_bytes(format, &0u32.to_le_bytes())?;
            }
            set_clipboard_bytes(CF_UNICODETEXT.0.into(), &utf16)?;
        }
        // Read once the clipboard is closed, so every change of this write is counted.
        // SAFETY: a plain user32 query.
        let token = unsafe { GetClipboardSequenceNumber() };
        if token == 0 {
            // No clipboard access in this window station: nothing could ever take the code back.
            if let Ok(_clipboard) = OpenedClipboard::open() {
                // SAFETY: the clipboard is open.
                let _ = unsafe { EmptyClipboard() };
            }
            return Err(KeychainError::Failed(
                "the clipboard has no sequence number here".to_string(),
            ));
        }
        Ok(u64::from(token))
    }

    fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool> {
        let _clipboard = OpenedClipboard::open()?;
        // Compared while the clipboard is held open, so nobody can write in between.
        // SAFETY: a plain user32 query.
        if u64::from(unsafe { GetClipboardSequenceNumber() }) != token {
            return Ok(false);
        }
        // SAFETY: the clipboard is open.
        unsafe { EmptyClipboard() }.map_err(clipboard_failure)?;
        Ok(true)
    }
}

/// A buffer DPAPI allocated: wiped (it may hold the binding) and freed on drop.
struct DpapiOutput(CRYPT_INTEGER_BLOB);

impl DpapiOutput {
    fn bytes(&self) -> &[u8] {
        if self.0.pbData.is_null() {
            return &[];
        }
        // SAFETY: DPAPI set `pbData` to `cbData` bytes it allocated and that live until `drop`.
        unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) }
    }
}

impl Drop for DpapiOutput {
    fn drop(&mut self) {
        if self.0.pbData.is_null() {
            return;
        }
        // SAFETY: as in `bytes`; the buffer is ours to wipe and to free with LocalFree, once.
        unsafe {
            std::slice::from_raw_parts_mut(self.0.pbData, self.0.cbData as usize).zeroize();
            LocalFree(Some(HLOCAL(self.0.pbData.cast())));
        }
    }
}

/// The registered formats that keep an item private: out of clipboard monitors, the clipboard
/// history (Win+V) and the cloud clipboard (`ExcludeClipboardContentFromMonitorProcessing`), and
/// explicitly out of the history (`CanIncludeInClipboardHistory` = 0) and the cloud clipboard
/// (`CanUploadToCloudClipboard` = 0).
fn privacy_formats() -> Result<[u32; 3]> {
    Ok([
        registered_format(w!("ExcludeClipboardContentFromMonitorProcessing"))?,
        registered_format(w!("CanIncludeInClipboardHistory"))?,
        registered_format(w!("CanUploadToCloudClipboard"))?,
    ])
}

fn registered_format(name: PCWSTR) -> Result<u32> {
    // SAFETY: a static wide string; registering an existing name returns its id.
    match unsafe { RegisterClipboardFormatW(name) } {
        0 => Err(clipboard_failure(windows::core::Error::from_thread())),
        format => Ok(format),
    }
}

/// The clipboard, open for this thread until dropped. Another program may hold it for a moment,
/// so opening is retried briefly.
struct OpenedClipboard;

impl OpenedClipboard {
    fn open() -> Result<Self> {
        let mut attempt = 1;
        loop {
            // SAFETY: no owner window; closed again in `drop`.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(error) if attempt >= CLIPBOARD_OPEN_ATTEMPTS => {
                    return Err(clipboard_failure(error))
                }
                Err(_) => {
                    attempt += 1;
                    thread::sleep(CLIPBOARD_OPEN_RETRY);
                }
            }
        }
    }
}

impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        // SAFETY: this thread opened the clipboard in `open`.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Puts `bytes` on the open clipboard as `format`, in a global block the system owns afterwards.
fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: a fresh movable block of `bytes.len()` bytes, locked only while it is written; on
    // every failure it is wiped and freed here, since the system took no ownership.
    unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(clipboard_failure)?;
        let target = GlobalLock(block).cast::<u8>();
        if target.is_null() {
            let error = windows::core::Error::from_thread();
            let _ = GlobalFree(Some(block));
            return Err(clipboard_failure(error));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        // Unlocking the last lock "fails" with ERROR_SUCCESS; nothing to act on.
        let _ = GlobalUnlock(block);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(block.0))) {
            wipe_and_free(block, bytes.len());
            return Err(clipboard_failure(error));
        }
    }
    Ok(())
}

/// Wipes and frees a global block the clipboard did not take.
///
/// # Safety
/// `block` is a live movable block of at least `length` bytes that nothing else owns.
unsafe fn wipe_and_free(block: HGLOBAL, length: usize) {
    // SAFETY: per the contract above.
    unsafe {
        let target = GlobalLock(block).cast::<u8>();
        if !target.is_null() {
            std::slice::from_raw_parts_mut(target, length).zeroize();
            let _ = GlobalUnlock(block);
        }
        let _ = GlobalFree(Some(block));
    }
}

fn clipboard_failure(error: windows::core::Error) -> KeychainError {
    KeychainError::Failed(format!("the clipboard: 0x{:08X}", error.code().0 as u32))
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::GlobalSize;

    #[test]
    fn dpapi_binding_round_trips() {
        let secret: [u8; 32] = std::array::from_fn(|i| i as u8);
        let blob = WinMachine.bind_seal(&secret).expect("seal");
        assert!(
            !blob.windows(32).any(|window| window == secret),
            "the secret is not in the blob"
        );
        assert_eq!(*WinMachine.bind_open(&blob).expect("open"), secret);

        let mut flipped = blob.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        for broken in [flipped, blob[..blob.len() / 2].to_vec(), vec![0u8; 64]] {
            assert!(matches!(
                WinMachine.bind_open(&broken),
                Err(KeychainError::Invalidated)
            ));
        }
    }

    #[test]
    fn the_clipboard_is_cleared_only_while_unchanged() {
        let _restore = SavedClipboardText::take();

        let token = WinMachine.copy_secret_text("ABCD-EFGH").expect("copy");
        assert_eq!(clipboard_text().as_deref(), Some("ABCD-EFGH"));
        for format in privacy_formats().expect("formats") {
            assert!(has_format(format), "every privacy format is set");
        }
        assert_eq!(WinMachine.clear_clipboard_if_unchanged(token), Ok(true));
        assert!(!has_format(CF_UNICODETEXT.0.into()), "cleared");

        let stale = WinMachine.copy_secret_text("ABCD-EFGH").expect("copy");
        let later = WinMachine.copy_secret_text("something else").expect("copy");
        assert_ne!(later, stale);
        assert_eq!(WinMachine.clear_clipboard_if_unchanged(stale), Ok(false));
        assert_eq!(
            clipboard_text().as_deref(),
            Some("something else"),
            "a later copy stays"
        );
        assert_eq!(WinMachine.clear_clipboard_if_unchanged(later), Ok(true));
    }

    fn has_format(format: u32) -> bool {
        // SAFETY: a plain user32 query; no clipboard ownership needed.
        unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    }

    /// The clipboard's text, if it holds any.
    fn clipboard_text() -> Option<String> {
        let _clipboard = OpenedClipboard::open().expect("open");
        // SAFETY: the clipboard is open; the handle is the clipboard's, read under a lock and
        // never past `GlobalSize`.
        unsafe {
            let handle = GetClipboardData(CF_UNICODETEXT.0.into()).ok()?;
            let block = HGLOBAL(handle.0);
            let units = GlobalSize(block) / 2;
            let text = GlobalLock(block).cast::<u16>();
            if text.is_null() {
                return None;
            }
            let all = std::slice::from_raw_parts(text, units);
            let end = all.iter().position(|&unit| unit == 0).unwrap_or(units);
            let read = String::from_utf16_lossy(&all[..end]);
            let _ = GlobalUnlock(block);
            Some(read)
        }
    }

    /// Takes the person's clipboard text before a test and puts it back afterwards, also when the
    /// test fails (other formats, such as an image, are not kept).
    struct SavedClipboardText(Option<String>);

    impl SavedClipboardText {
        fn take() -> Self {
            Self(clipboard_text())
        }
    }

    impl Drop for SavedClipboardText {
        fn drop(&mut self) {
            let Ok(_clipboard) = OpenedClipboard::open() else {
                return;
            };
            // SAFETY: the clipboard is open.
            let _ = unsafe { EmptyClipboard() };
            if let Some(text) = &self.0 {
                let utf16: Vec<u8> = text
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                let _ = set_clipboard_bytes(CF_UNICODETEXT.0.into(), &utf16);
            }
        }
    }
}
