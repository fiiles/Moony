//! The macOS native: Touch ID through a Secure Enclave key, in the Swift package under `macos/`
//! (CryptoKit + LocalAuthentication), linked by `build.rs` with swift-rs.
//!
//! The Swift side wraps the master key for a Secure Enclave key that only a Touch ID match against
//! today's fingerprints can use (`.biometryCurrentSet`), and keeps that key's handle inside the
//! blob, so the blob file is all there is: [`MacNative::forget`] has nothing to delete. The blob
//! format, the ECIES scheme and the error classification live in `MoonyBiometrics.swift`; this
//! side only frames the calls and decodes the status byte every answer starts with.
//!
//! Accepted residual, as on the phones: the secret crosses the bridge in swift-rs `SRData`
//! buffers (the argument to `moony_bio_wrap`, the answer of `moony_bio_unwrap`), which are freed
//! but not wiped. The Rust copies are `Zeroizing` and the Swift side wipes its own.

use swift_rs::{swift, Bool, Int, SRData, SRString};
use zeroize::Zeroizing;

use super::NativeBiometrics;
use crate::{BiometryType, KeychainError, Result, Status};

swift!(fn moony_bio_status() -> Int);
swift!(fn moony_bio_wrap(reason: &SRString, secret: &SRData) -> SRData);
swift!(fn moony_bio_unwrap(reason: &SRString, blob: &SRData) -> SRData);
swift!(fn moony_bio_selftest() -> Bool);

/// Status bytes the Swift side starts every answer with.
const STATUS_OK: u8 = 0;
const STATUS_CANCELLED: u8 = 1;
const STATUS_UNAVAILABLE: u8 = 2;
const STATUS_INVALIDATED: u8 = 3;

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
        let payload = Zeroizing::new(decode(&answer)?);
        let mut secret = Zeroizing::new([0u8; 32]);
        if payload.len() != secret.len() {
            return Err(KeychainError::Failed(format!(
                "Touch ID returned {} bytes instead of 32",
                payload.len()
            )));
        }
        secret.copy_from_slice(&payload);
        Ok(secret)
    }

    /// Nothing to do: the Secure Enclave key lives only inside the blob, and removing the blob
    /// file (which `Keychain::remove` does next) destroys it.
    fn forget(&self) -> Result<()> {
        Ok(())
    }
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
    const LA: &str = "com.apple.LocalAuthentication";
    const OS_STATUS: &str = "NSOSStatusErrorDomain";

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
        assert!(unsafe { moony_bio_selftest() });
    }

    #[test]
    fn status_maps_codes() {
        assert_eq!(
            status_from_code(-1),
            Status {
                available: false,
                biometry_type: BiometryType::None
            }
        );
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
    fn a_key_refusing_an_authenticated_context_is_invalidated() {
        assert_eq!(classify(AGREEMENT, OS_STATUS, -25293, false), 3); // errSecAuthFailed
        assert_eq!(classify(AGREEMENT, "CryptoTokenKit", -3, false), 3);
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
        }
    }

    #[test]
    fn an_unusable_key_handle_is_invalidated() {
        assert_eq!(classify(RESTORE, "CryptoTokenKit", -3, false), 3);
        assert_eq!(classify(RESTORE, OS_STATUS, -25300, false), 3); // errSecItemNotFound
        assert_eq!(classify(RESTORE, "CryptoKit.CryptoKitError", 1, true), 3);
    }
}
