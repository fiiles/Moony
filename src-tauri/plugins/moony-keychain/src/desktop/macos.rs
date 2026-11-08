//! The macOS native: Touch ID through a Secure Enclave key, in the Swift package under `macos/`
//! (CryptoKit + LocalAuthentication), linked by `build.rs` with swift-rs.
//!
//! The Swift side wraps the master key for a Secure Enclave key that only a Touch ID match against
//! today's fingerprints can use (`.biometryCurrentSet`), and keeps that key's handle inside the
//! blob, so the blob file is all there is: [`MacNative::forget`] has nothing to delete. The blob
//! format, the ECIES scheme and the error classification live in `MoonyBiometrics.swift`; this
//! side only frames the calls and decodes the status byte every answer starts with.
//!
//! The same package seals device sync's binding to this Mac ([`NativeMachine`]): a Secure Enclave
//! key without user presence, its handle inside the blob, so it needs no keychain entitlement and
//! survives app updates and reinstalls; a blob from another Mac is `Invalidated`, and a Mac without
//! a Secure Enclave (Intel without T2) is `Unsupported`. It also copies a secret to the pasteboard
//! with the concealed marker type and clears it later only while the change count is unchanged.
//!
//! Accepted residual, as on the phones: the secret crosses the bridge in swift-rs `SRData`
//! buffers (the argument to `moony_bio_wrap`, the answer of `moony_bio_unwrap`; the binding's the
//! same way), which are freed but not wiped. The Rust copies are `Zeroizing` and the Swift side
//! wipes its own. The clipboard text crosses as a swift-rs string and cannot be wiped at all.

use swift_rs::{swift, Bool, Int, SRData, SRString};
use zeroize::Zeroizing;

use super::{NativeBiometrics, NativeMachine};
use crate::{BiometryType, KeychainError, Result, Status};

swift!(fn moony_bio_status() -> Int);
swift!(fn moony_bio_wrap(reason: &SRString, secret: &SRData) -> SRData);
swift!(fn moony_bio_unwrap(reason: &SRString, blob: &SRData) -> SRData);
swift!(fn moony_bio_selftest() -> Bool);
swift!(fn moony_bind_seal(secret: &SRData) -> SRData);
swift!(fn moony_bind_open(blob: &SRData) -> SRData);
swift!(fn moony_clip_copy(text: &SRString) -> Int);
swift!(fn moony_clip_clear_if(count: Int) -> Bool);

/// Status bytes the Swift side starts every answer with.
const STATUS_OK: u8 = 0;
const STATUS_CANCELLED: u8 = 1;
const STATUS_UNAVAILABLE: u8 = 2;
const STATUS_INVALIDATED: u8 = 3;
const STATUS_UNSUPPORTED: u8 = 5;

/// Touch ID through the Swift package. Every call but `status` blocks on the prompt.
pub(super) struct MacNative;

impl NativeBiometrics for MacNative {
    fn status(&self) -> Result<Status> {
        // SAFETY: a plain call into the linked Swift package; no arguments, a scalar answer.
        Ok(status_from_code(unsafe { moony_bio_status() }))
    }

    fn wrap(&self, secret: &[u8; 32], reason: &str) -> Result<Vec<u8>> {
        let reason = SRString::from(reason);
        let secret = SRData::from(secret.as_slice());
        // SAFETY: both arguments are live swift-rs objects; `swift!` balances the retain counts.
        let answer = unsafe { moony_bio_wrap(&reason, &secret) };
        decode(&answer)
    }

    fn unwrap(&self, blob: &[u8], reason: &str) -> Result<Zeroizing<[u8; 32]>> {
        let reason = SRString::from(reason);
        let blob = SRData::from(blob);
        // SAFETY: as in `wrap`.
        let answer = unsafe { moony_bio_unwrap(&reason, &blob) };
        secret_from(decode(&answer)?, "Touch ID")
    }

    /// Nothing to do: the Secure Enclave key lives only inside the blob, and removing the blob
    /// file (which `Keychain::remove` does next) destroys it.
    fn forget(&self) -> Result<()> {
        Ok(())
    }
}

/// Device sync's binding and the clearing clipboard. None of these calls prompts.
impl NativeMachine for MacNative {
    fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>> {
        let secret = SRData::from(secret.as_slice());
        // SAFETY: a live swift-rs object; `swift!` balances the retain counts.
        let answer = unsafe { moony_bind_seal(&secret) };
        decode(&answer)
    }

    fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        let blob = SRData::from(blob);
        // SAFETY: as in `bind_seal`.
        let answer = unsafe { moony_bind_open(&blob) };
        secret_from(decode(&answer)?, "The binding")
    }

    fn copy_secret_text(&self, text: &str) -> Result<u64> {
        let text = SRString::from(text);
        // SAFETY: as in `bind_seal`; the answer is a scalar.
        let count = unsafe { moony_clip_copy(&text) };
        u64::try_from(count)
            .map_err(|_| KeychainError::Failed("the pasteboard refused the text".to_string()))
    }

    fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool> {
        // A token no change count can equal: the pasteboard is not ours to clear.
        let Ok(count) = Int::try_from(token) else {
            return Ok(false);
        };
        // SAFETY: a scalar argument and answer.
        Ok(unsafe { moony_clip_clear_if(count) })
    }
}

/// The 32-byte secret an answer's payload holds; `what` names the source in the message.
fn secret_from(payload: Vec<u8>, what: &str) -> Result<Zeroizing<[u8; 32]>> {
    let payload = Zeroizing::new(payload);
    let mut secret = Zeroizing::new([0u8; 32]);
    if payload.len() != secret.len() {
        return Err(KeychainError::Failed(format!(
            "{what} returned {} bytes instead of 32",
            payload.len()
        )));
    }
    secret.copy_from_slice(&payload);
    Ok(secret)
}

/// `moony_bio_status`'s answer: -1 no usable biometrics, 0 usable but of no type the app can name
/// (not offered: the copy says "fingerprint"), 1 Touch ID.
fn status_from_code(code: Int) -> Status {
    if code >= 1 {
        Status {
            available: true,
            biometry_type: BiometryType::TouchId,
        }
    } else {
        Status {
            available: false,
            biometry_type: BiometryType::None,
        }
    }
}

/// Splits an answer into its status byte and the rest: the payload when it is ok, otherwise the
/// error, with the Swift side's message for a failure. The payload may be the secret, so the
/// caller wipes it.
fn decode(answer: &[u8]) -> Result<Vec<u8>> {
    let Some((&status, rest)) = answer.split_first() else {
        return Err(KeychainError::Failed(
            "Touch ID returned an empty answer".to_string(),
        ));
    };
    match status {
        STATUS_OK => Ok(rest.to_vec()),
        STATUS_CANCELLED => Err(KeychainError::Cancelled),
        STATUS_UNAVAILABLE => Err(KeychainError::Unavailable),
        STATUS_INVALIDATED => Err(KeychainError::Invalidated),
        STATUS_UNSUPPORTED => Err(KeychainError::Unsupported),
        _ if rest.is_empty() => Err(KeychainError::Failed("Touch ID failed".to_string())),
        _ => Err(KeychainError::Failed(
            String::from_utf8_lossy(rest).into_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The Swift side's error classification for a failed key restore (stage 0) or key agreement
    // (stage 1), fed with synthetic errors; answers a status byte.
    swift!(fn moony_bio_classify(stage: Int, domain: &SRString, code: Int, wrapped: Bool) -> Int);

    const RESTORE: Int = 0;
    const AGREEMENT: Int = 1;
    const BINDING: Int = 2;
    const LA: &str = "com.apple.LocalAuthentication";
    const OS_STATUS: &str = "NSOSStatusErrorDomain";
    const TOKEN: &str = "CryptoTokenKit";

    fn classify(stage: Int, domain: &str, code: Int, wrapped: bool) -> Int {
        let domain = SRString::from(domain);
        // SAFETY: a live swift-rs string and scalars; the answer is a scalar.
        unsafe { moony_bio_classify(stage, &domain, code, wrapped) }
    }

    #[test]
    fn decode_maps_every_status_byte() {
        assert_eq!(decode(&[0, 7, 8, 9]), Ok(vec![7, 8, 9]));
        assert_eq!(decode(&[1]), Err(KeychainError::Cancelled));
        assert_eq!(decode(&[2, b'x']), Err(KeychainError::Unavailable));
        assert_eq!(decode(&[3]), Err(KeychainError::Invalidated));
        assert_eq!(decode(&[5]), Err(KeychainError::Unsupported));
        assert_eq!(
            decode(b"\x04boom"),
            Err(KeychainError::Failed("boom".to_string()))
        );
        assert_eq!(
            decode(&[4]),
            Err(KeychainError::Failed("Touch ID failed".to_string()))
        );
        assert!(matches!(decode(&[]), Err(KeychainError::Failed(_))));
    }

    #[test]
    fn native_selftest_round_trips() {
        // SAFETY: a plain call into the linked Swift package; no arguments, a scalar answer.
        assert!(unsafe { moony_bio_selftest() });
    }

    #[test]
    fn status_maps_codes() {
        for code in [-1, 0] {
            assert_eq!(
                status_from_code(code),
                Status {
                    available: false,
                    biometry_type: BiometryType::None
                },
                "code {code}"
            );
        }
        assert_eq!(
            status_from_code(1),
            Status {
                available: true,
                biometry_type: BiometryType::TouchId
            }
        );
    }

    /// Asks this Mac's LocalAuthentication; no prompt. On a Touch ID Mac with a fingerprint
    /// enrolled: `available: true, biometry_type: TouchId`.
    #[test]
    #[ignore = "depends on the machine's Touch ID hardware and enrolment"]
    fn mac_status_on_this_machine() {
        println!("{:?}", MacNative.status());
    }

    #[test]
    fn a_second_prompt_failing_at_agreement_never_reads_as_invalidated() {
        for wrapped in [false, true] {
            // userCancel, systemCancel, appCancel, userFallback, notInteractive → cancelled.
            for code in [-2, -4, -9, -3, -1004] {
                assert_eq!(classify(AGREEMENT, LA, code, wrapped), 1, "LA {code}");
            }
            // biometryLockout, biometryNotAvailable, biometryNotEnrolled → unavailable.
            for code in [-8, -6, -7] {
                assert_eq!(classify(AGREEMENT, LA, code, wrapped), 2, "LA {code}");
            }
            // authenticationFailed (a misread finger) and an unknown LA code → failed.
            assert_eq!(classify(AGREEMENT, LA, -1, wrapped), 4);
            assert_eq!(classify(AGREEMENT, LA, -10, wrapped), 4);
            // errSecUserCanceled → cancelled.
            assert_eq!(classify(AGREEMENT, OS_STATUS, -128, wrapped), 1);
        }
    }

    #[test]
    fn a_transient_agreement_failure_is_failed_not_invalidated() {
        for wrapped in [false, true] {
            // errSecInteractionNotAllowed, errSecNotAvailable, errSecMissingEntitlement.
            for code in [-25308, -25291, -34018] {
                assert_eq!(
                    classify(AGREEMENT, OS_STATUS, code, wrapped),
                    4,
                    "OSStatus {code}"
                );
            }
            // TKError communicationError → failed; canceledByUser → cancelled.
            assert_eq!(classify(AGREEMENT, TOKEN, -2, wrapped), 4);
            assert_eq!(classify(AGREEMENT, TOKEN, -4, wrapped), 1);
        }
    }

    #[test]
    fn a_key_refusing_an_authenticated_context_is_invalidated() {
        for wrapped in [false, true] {
            assert_eq!(classify(AGREEMENT, OS_STATUS, -25293, wrapped), 3); // errSecAuthFailed
            assert_eq!(classify(AGREEMENT, OS_STATUS, -25300, wrapped), 3); // errSecItemNotFound
            assert_eq!(classify(AGREEMENT, TOKEN, -3, wrapped), 3); // TKError corruptedData
        }
        assert_eq!(classify(AGREEMENT, "CryptoKit.CryptoKitError", 0, true), 3);
    }

    #[test]
    fn a_transient_restore_failure_is_failed_not_invalidated() {
        for wrapped in [false, true] {
            // errSecInteractionNotAllowed, errSecNotAvailable, errSecMissingEntitlement.
            for code in [-25308, -25291, -34018] {
                assert_eq!(
                    classify(RESTORE, OS_STATUS, code, wrapped),
                    4,
                    "OSStatus {code}"
                );
            }
            assert_eq!(classify(RESTORE, OS_STATUS, -128, wrapped), 1);
            assert_eq!(classify(RESTORE, LA, -2, wrapped), 1);
            assert_eq!(classify(RESTORE, LA, -8, wrapped), 2);
            assert_eq!(classify(RESTORE, TOKEN, -2, wrapped), 4);
            assert_eq!(classify(RESTORE, TOKEN, -4, wrapped), 1);
        }
    }

    #[test]
    fn an_unusable_key_handle_is_invalidated() {
        assert_eq!(classify(RESTORE, TOKEN, -3, false), 3);
        assert_eq!(classify(RESTORE, OS_STATUS, -25300, false), 3); // errSecItemNotFound
        assert_eq!(classify(RESTORE, "CryptoKit.CryptoKitError", 1, true), 3);
    }

    /// A binding key handle from another Mac's Secure Enclave (CryptoTokenKit `corruptedData`,
    /// "corrupted objectID detected"), a key that is gone and a key that refuses the agreement
    /// all say "not this machine": invalidated, which the app reads as `NotThisDevice`.
    #[test]
    fn a_binding_key_from_another_mac_is_invalidated() {
        for wrapped in [false, true] {
            assert_eq!(classify(BINDING, TOKEN, -3, wrapped), 3); // TKError corruptedData
            assert_eq!(classify(BINDING, OS_STATUS, -25300, wrapped), 3); // errSecItemNotFound
            assert_eq!(classify(BINDING, OS_STATUS, -25293, wrapped), 3); // errSecAuthFailed
            assert_eq!(classify(BINDING, OS_STATUS, -50, wrapped), 3); // errSecParam
        }
        assert_eq!(classify(BINDING, "CryptoKit.CryptoKitError", 1, true), 3);
        assert_eq!(classify(BINDING, "CryptoKit.CryptoKitError", 0, false), 3);
    }

    /// A refusal of the moment says nothing about the machine: failed (or cancelled), which the
    /// app reads as `Failed`, so the check lets this start go on instead of re-joining.
    #[test]
    fn a_transient_binding_failure_is_never_invalidated() {
        for wrapped in [false, true] {
            // errSecInteractionNotAllowed, errSecNotAvailable, errSecMissingEntitlement.
            for code in [-25308, -25291, -34018] {
                assert_eq!(
                    classify(BINDING, OS_STATUS, code, wrapped),
                    4,
                    "OSStatus {code}"
                );
            }
            assert_eq!(classify(BINDING, TOKEN, -2, wrapped), 4); // communicationError
            assert_eq!(classify(BINDING, TOKEN, -4, wrapped), 1); // canceledByUser
            assert_eq!(classify(BINDING, OS_STATUS, -128, wrapped), 1); // errSecUserCanceled
            assert_eq!(classify(BINDING, LA, -2, wrapped), 1); // userCancel
            assert_eq!(classify(BINDING, LA, -6, wrapped), 2); // biometryNotAvailable
        }
    }

    /// Seals with a new Secure Enclave key (no prompt) and opens it again; a flipped bit, a cut
    /// blob and a garbled key handle are invalidated. On a Mac without a Secure Enclave (Intel
    /// without T2) both calls are `Unsupported` instead. Run it with the screen unlocked: the key
    /// is `WhenUnlocked`, so behind the lock screen the seal fails with OSStatus -25308
    /// (`Failed`, never `Invalidated`).
    #[test]
    #[ignore = "needs a Mac with a Secure Enclave"]
    fn secure_enclave_binding_round_trips() {
        let secret: [u8; 32] = std::array::from_fn(|i| i as u8);
        let blob = match MacNative.bind_seal(&secret) {
            Err(KeychainError::Unsupported) => {
                assert_eq!(
                    MacNative.bind_open(&[1; 200]).map(|_| ()),
                    Err(KeychainError::Unsupported)
                );
                println!("no Secure Enclave on this Mac: Unsupported");
                return;
            }
            other => other.expect("seal"),
        };
        assert!(
            !blob.windows(32).any(|window| window == secret),
            "the secret is not in the blob"
        );
        assert_eq!(*MacNative.bind_open(&blob).expect("open"), secret);
        assert_eq!(*MacNative.bind_open(&blob).expect("open twice"), secret);

        let mut flipped = blob.clone();
        let last = flipped.len() - 20;
        flipped[last] ^= 1;
        let mut garbled_key = blob.clone();
        garbled_key[10] ^= 0xff;
        for broken in [
            flipped,
            blob[..blob.len() - 1].to_vec(),
            garbled_key,
            vec![],
        ] {
            assert_eq!(
                MacNative.bind_open(&broken).map(|_| ()),
                Err(KeychainError::Invalidated)
            );
        }
        // A Touch ID blob's format is not a binding's.
        let mut touch_id_like = blob.clone();
        touch_id_like[0] = 1;
        assert_eq!(
            MacNative.bind_open(&touch_id_like).map(|_| ()),
            Err(KeychainError::Invalidated)
        );
    }

    /// Copies, clears while unchanged, and leaves a later copy alone. Puts back the plain text
    /// the pasteboard held before.
    #[test]
    #[ignore = "touches the real pasteboard"]
    fn the_pasteboard_is_cleared_only_while_unchanged() {
        let saved = std::process::Command::new("pbpaste")
            .output()
            .expect("pbpaste")
            .stdout;

        let token = MacNative.copy_secret_text("ABCD-EFGH").expect("copy");
        assert_eq!(paste(), "ABCD-EFGH");
        assert_eq!(MacNative.clear_clipboard_if_unchanged(token), Ok(true));
        assert_eq!(paste(), "", "cleared");

        let stale = MacNative.copy_secret_text("ABCD-EFGH").expect("copy");
        let later = MacNative.copy_secret_text("something else").expect("copy");
        assert!(later > stale);
        assert_eq!(MacNative.clear_clipboard_if_unchanged(stale), Ok(false));
        assert_eq!(paste(), "something else", "a later copy stays");
        assert_eq!(MacNative.clear_clipboard_if_unchanged(later), Ok(true));

        let mut restore = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("pbcopy");
        std::io::Write::write_all(restore.stdin.as_mut().expect("stdin"), &saved).expect("write");
        restore.wait().expect("pbcopy done");
    }

    fn paste() -> String {
        let output = std::process::Command::new("pbpaste")
            .output()
            .expect("pbpaste");
        String::from_utf8(output.stdout).expect("utf-8")
    }
}
