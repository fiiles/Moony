use serde::{Deserialize, Serialize};

/// Which biometry the device offers, for the unlock button's icon and wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiometryType {
    #[serde(rename = "faceId")]
    FaceId,
    /// Touch ID on iOS; a fingerprint-only device on Android.
    #[serde(rename = "touchId")]
    TouchId,
    /// Any other biometry (Optic ID, or an Android device with several sensors).
    #[serde(rename = "biometric")]
    Biometric,
    /// Windows Hello on the desktop (face, fingerprint or PIN, whichever the person set up).
    #[serde(rename = "windowsHello")]
    WindowsHello,
    #[serde(rename = "none")]
    None,
}

/// The `status` answer: whether a biometry-protected secret can be stored and read right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    #[serde(rename = "available")]
    pub available: bool,
    #[serde(rename = "biometryType")]
    pub biometry_type: BiometryType,
}

/// Device sync's binding and private clipboard over the iOS bridge: the payloads, the replies and
/// the classification of a failed binding (review PF11). Host-compiled, so the table is tested on
/// every platform; only the iPhone build calls it (`mobile.rs`).
///
/// The Swift side (`ios/Sources/MoonyBinding.swift`) never decides that a blob belongs to another
/// phone. It answers the stage that failed with the error's domain and code (and the codes of the
/// errors underneath it), and [`classify_ios_binding`] reads them with the macOS table
/// (`MoonyBiometrics.swift`, `restoreFailure` / `momentaryFailure`): a blob that does not parse,
/// a key handle this Secure Enclave rejects, an agreement the key refuses and a box that does not
/// open are `Invalidated` ("not this device", a re-join); a refusal of the moment is `Failed` (the
/// start goes on) at every stage, under any other error. Unlike macOS, an error the table does not
/// know is `Failed` too (review PF11): on the phone a re-join costs a new device ID and a fresh
/// copy, and a locked phone must never cause one.
pub(crate) mod bridge {
    // Only the iPhone build calls these; every platform compiles and tests them.
    #![cfg_attr(not(target_os = "ios"), allow(dead_code))]

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Serialize};
    use zeroize::Zeroizing;

    use crate::{KeychainError, Result};

    /// The domain of the failures the Swift side finds itself (`MoonyBinding.swift`): 1 the blob is
    /// not of the binding's format or length, 2 a secret of the wrong length, 3 no AES-GCM box, 4
    /// no access control.
    pub const MOONY_BINDING_DOMAIN: &str = "MoonyBinding";
    /// The NSError domain of a Security OSStatus.
    const OS_STATUS_DOMAIN: &str = "NSOSStatusErrorDomain";
    /// CryptoTokenKit's `TKErrorDomain`, which the Secure Enclave's key operations answer in.
    const TOKEN_DOMAIN: &str = "CryptoTokenKit";
    /// CryptoKit's errors, by their case's index (`MoonyBinding.swift` `cryptoKitCode`:
    /// 2 is `authenticationFailure`, a box that does not open).
    const CRYPTO_KIT_DOMAIN: &str = "CryptoKit.CryptoKitError";
    /// LocalAuthentication's `LAErrorDomain`: no binding call prompts, so any such answer is one
    /// of the moment.
    const LOCAL_AUTHENTICATION_DOMAIN: &str = "com.apple.LocalAuthentication";

    /// OSStatus values that say nothing about the key: errSecInteractionNotAllowed (the phone is
    /// locked), errSecNotAvailable, errSecMissingEntitlement, errSecUserCanceled, errSecAllocate
    /// (memory), errSecIO, errSecDiskFull, errSecInternalComponent.
    pub const MOMENTARY_OS_STATUS: [i64; 8] = [-25308, -25291, -34018, -128, -108, -36, -34, -2070];
    /// `TKError` codes that say nothing about the key: communicationError, canceledByUser,
    /// tokenNotFound, authenticationNeeded.
    pub const MOMENTARY_TOKEN_ERRORS: [i64; 4] = [-2, -4, -7, -9];
    /// OSStatus values of a key this Secure Enclave will not use: errSecItemNotFound,
    /// errSecAuthFailed, errSecParam (the macOS table's binding rows).
    const KEY_REJECTED_OS_STATUS: [i64; 3] = [-25300, -25293, -50];
    /// `TKError` codes of a key this Secure Enclave will not use: corruptedData ("corrupted
    /// objectID detected", a handle from another Secure Enclave), authenticationFailed,
    /// objectNotFound.
    const KEY_REJECTED_TOKEN_ERRORS: [i64; 3] = [-3, -5, -6];

    /// `bindSeal`'s argument: the 32-byte value as base64 ([`encode_secret`]).
    #[derive(Serialize)]
    pub struct BindSealPayload<'a> {
        pub secret: &'a str,
    }

    /// `bindOpen`'s argument: the blob as base64 ([`encode_blob`]).
    #[derive(Serialize)]
    pub struct BindOpenPayload<'a> {
        pub blob: &'a str,
    }

    /// `copySecretText`'s argument.
    #[derive(Serialize)]
    pub struct CopySecretTextPayload<'a> {
        pub text: &'a str,
    }

    /// `copySecretText`'s answer: the pasteboard's change count after the copy.
    #[derive(Deserialize)]
    pub struct CopySecretTextReply {
        pub token: u64,
    }

    /// `clearClipboardIfUnchanged`'s argument: a token `copySecretText` answered.
    #[derive(Serialize)]
    pub struct ClearClipboardPayload {
        pub token: u64,
    }

    /// `clearClipboardIfUnchanged`'s answer: whether the pasteboard was emptied.
    #[derive(Deserialize)]
    pub struct ClearClipboardReply {
        pub cleared: bool,
    }

    /// `bindSeal`'s and `bindOpen`'s answer: the blob or the 32-byte value as base64, or
    /// `unsupported` (no Secure Enclave: the simulator), or the failure to classify.
    #[derive(Deserialize)]
    pub struct BindReply {
        #[serde(default)]
        pub value: Option<String>,
        #[serde(default)]
        pub unsupported: bool,
        #[serde(default)]
        pub failure: Option<NativeFailure>,
    }

    /// Where the binding failed, with the error's domain and code and those of the errors under it
    /// (`NSUnderlyingErrorKey`, outermost first): CryptoKit and CryptoTokenKit wrap the error that
    /// caused theirs, and the macOS table looks through them.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct NativeFailure {
        pub stage: BindStage,
        pub domain: String,
        pub code: i64,
        #[serde(default)]
        pub underlying: Vec<NativeError>,
    }

    /// One error under a [`NativeFailure`].
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct NativeError {
        pub domain: String,
        pub code: i64,
    }

    /// The step of the seal or the open that failed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    pub enum BindStage {
        /// Making the Secure Enclave key or sealing the value (`bindSeal`).
        #[serde(rename = "seal")]
        Seal,
        /// Reading the blob's format, length and ephemeral key.
        #[serde(rename = "parse")]
        Parse,
        /// Restoring the Secure Enclave key from the handle in the blob.
        #[serde(rename = "restore")]
        Restore,
        /// The key agreement on the Secure Enclave.
        #[serde(rename = "agree")]
        Agree,
        /// Opening the AES-GCM box.
        #[serde(rename = "open")]
        Open,
    }

    impl BindStage {
        fn name(self) -> &'static str {
            match self {
                Self::Seal => "seal",
                Self::Parse => "parse",
                Self::Restore => "restore",
                Self::Agree => "agree",
                Self::Open => "open",
            }
        }
    }

    impl NativeFailure {
        /// The error and the ones under it, outermost first.
        fn chain(&self) -> impl Iterator<Item = (&str, i64)> {
            std::iter::once((self.domain.as_str(), self.code)).chain(
                self.underlying
                    .iter()
                    .map(|error| (error.domain.as_str(), error.code)),
            )
        }
    }

    /// The binding's 32-byte value as base64, wiped when dropped. The JSON the bridge builds from
    /// it is not (the accepted residual of `lib.rs`).
    pub fn encode_secret(secret: &[u8; 32]) -> Zeroizing<String> {
        Zeroizing::new(STANDARD.encode(secret))
    }

    /// A blob as base64.
    pub fn encode_blob(blob: &[u8]) -> String {
        STANDARD.encode(blob)
    }

    /// Classifies a failed seal or open the way macOS does: `Invalidated` only for what proves the
    /// blob is not this phone's (see the module), `Failed` for everything else.
    pub fn classify_ios_binding(failure: &NativeFailure) -> KeychainError {
        if failure.chain().any(is_momentary) {
            return failed(failure);
        }
        match failure.stage {
            BindStage::Seal => failed(failure),
            BindStage::Parse => KeychainError::Invalidated,
            BindStage::Restore | BindStage::Agree | BindStage::Open => {
                if failure.chain().any(rejects_the_blob) {
                    KeychainError::Invalidated
                } else {
                    failed(failure)
                }
            }
        }
    }

    /// `bindOpen`'s answer: the 32-byte value, wiped when dropped.
    pub fn bind_reply_secret(reply: BindReply) -> Result<Zeroizing<[u8; 32]>> {
        let bytes = Zeroizing::new(reply_bytes(reply)?);
        let mut secret = Zeroizing::new([0u8; 32]);
        if bytes.len() != secret.len() {
            return Err(KeychainError::Failed(format!(
                "This phone's secure storage returned {} bytes instead of 32",
                bytes.len()
            )));
        }
        secret.copy_from_slice(&bytes);
        Ok(secret)
    }

    /// `bindSeal`'s answer: the blob.
    pub fn bind_reply_blob(reply: BindReply) -> Result<Vec<u8>> {
        let blob = reply_bytes(reply)?;
        if blob.is_empty() {
            return Err(KeychainError::Failed(
                "This phone's secure storage returned an empty blob".to_string(),
            ));
        }
        Ok(blob)
    }

    /// The decoded value of a reply, or its error. The base64 text may be the secret, so it is
    /// wiped once decoded.
    fn reply_bytes(reply: BindReply) -> Result<Vec<u8>> {
        if reply.unsupported {
            return Err(KeychainError::Unsupported);
        }
        if let Some(failure) = &reply.failure {
            return Err(classify_ios_binding(failure));
        }
        let Some(value) = reply.value.map(Zeroizing::new) else {
            return Err(KeychainError::Failed(
                "This phone's secure storage returned no answer".to_string(),
            ));
        };
        STANDARD.decode(value.as_bytes()).map_err(|_| {
            KeychainError::Failed("This phone's secure storage returned no base64".to_string())
        })
    }

    fn is_momentary((domain, code): (&str, i64)) -> bool {
        match domain {
            OS_STATUS_DOMAIN => MOMENTARY_OS_STATUS.contains(&code),
            TOKEN_DOMAIN => MOMENTARY_TOKEN_ERRORS.contains(&code),
            LOCAL_AUTHENTICATION_DOMAIN => true,
            _ => false,
        }
    }

    /// What only a blob that is not this phone's (or not a binding at all) gives once it has
    /// parsed: a key handle the Secure Enclave rejects, an agreement it refuses, a CryptoKit error
    /// over the blob's bytes (an authentication failure when the box does not open), or a box
    /// that holds no 32-byte value.
    fn rejects_the_blob((domain, code): (&str, i64)) -> bool {
        match domain {
            OS_STATUS_DOMAIN => KEY_REJECTED_OS_STATUS.contains(&code),
            TOKEN_DOMAIN => KEY_REJECTED_TOKEN_ERRORS.contains(&code),
            CRYPTO_KIT_DOMAIN | MOONY_BINDING_DOMAIN => true,
            _ => false,
        }
    }

    /// Names the phone's secure storage, never Face ID or Touch ID (the binding has no prompt),
    /// and quotes the stage, domains and codes only.
    fn failed(failure: &NativeFailure) -> KeychainError {
        let errors = failure
            .chain()
            .map(|(domain, code)| format!("{domain} {code}"))
            .collect::<Vec<_>>()
            .join(" < ");
        KeychainError::Failed(format!(
            "This phone's secure storage cannot be used right now ({}: {errors})",
            failure.stage.name()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::bridge::*;
    use super::*;
    use crate::KeychainError;

    #[test]
    fn status_parses_the_native_answer() {
        let status: Status =
            serde_json::from_str(r#"{"available":true,"biometryType":"faceId"}"#).expect("json");
        assert_eq!(
            status,
            Status {
                available: true,
                biometry_type: BiometryType::FaceId
            }
        );
    }

    #[test]
    fn every_biometry_type_round_trips_by_its_wire_name() {
        for (wire, kind) in [
            ("faceId", BiometryType::FaceId),
            ("touchId", BiometryType::TouchId),
            ("biometric", BiometryType::Biometric),
            ("windowsHello", BiometryType::WindowsHello),
            ("none", BiometryType::None),
        ] {
            let json = format!("\"{wire}\"");
            assert_eq!(
                serde_json::from_str::<BiometryType>(&json).expect("json"),
                kind
            );
            assert_eq!(serde_json::to_string(&kind).expect("json"), json);
        }
    }

    // --- Device sync's binding over the iOS bridge (review PF11) ---

    const OS_STATUS: &str = "NSOSStatusErrorDomain";
    const TOKEN: &str = "CryptoTokenKit";
    const CRYPTO_KIT: &str = "CryptoKit.CryptoKitError";
    const LA: &str = "com.apple.LocalAuthentication";
    const ALL_STAGES: [BindStage; 5] = [
        BindStage::Seal,
        BindStage::Parse,
        BindStage::Restore,
        BindStage::Agree,
        BindStage::Open,
    ];

    fn failure(stage: BindStage, domain: &str, code: i64) -> NativeFailure {
        NativeFailure {
            stage,
            domain: domain.to_string(),
            code,
            underlying: Vec::new(),
        }
    }

    /// `outer` with `inner` as its underlying error, the way CryptoKit and CryptoTokenKit wrap
    /// the error that caused theirs.
    fn wrapped(stage: BindStage, outer: (&str, i64), inner: (&str, i64)) -> NativeFailure {
        NativeFailure {
            underlying: vec![NativeError {
                domain: inner.0.to_string(),
                code: inner.1,
            }],
            ..failure(stage, outer.0, outer.1)
        }
    }

    fn reply(json: &str) -> BindReply {
        serde_json::from_str(json).expect("reply json")
    }

    #[test]
    fn bridge_payloads_round_trip_and_refuse_wrong_lengths() {
        let secret: [u8; 32] = std::array::from_fn(|i| i as u8);
        let encoded = encode_secret(&secret);
        assert_eq!(
            encoded.as_str(),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
        );
        assert_eq!(
            serde_json::to_string(&BindSealPayload { secret: &encoded }).expect("json"),
            r#"{"secret":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="}"#
        );
        let blob = encode_blob(&[0x42, 0, 1, 0xff]);
        assert_eq!(blob, "QgAB/w==");
        assert_eq!(
            serde_json::to_string(&BindOpenPayload { blob: &blob }).expect("json"),
            r#"{"blob":"QgAB/w=="}"#
        );

        // The secret comes back as it went.
        let opened = bind_reply_secret(reply(&format!(
            r#"{{"value":"{}","unsupported":false}}"#,
            encoded.as_str()
        )))
        .expect("secret");
        assert_eq!(*opened, secret);
        assert_eq!(
            bind_reply_blob(reply(r#"{"value":"QgAB/w==","unsupported":false}"#)),
            Ok(vec![0x42, 0, 1, 0xff])
        );

        // 31 and 33 bytes are no binding; neither is text that is not base64, nor no value.
        for wrong in [
            encode_blob(&[7; 31]),
            encode_blob(&[7; 33]),
            String::new(),
            "not base64!".to_string(),
        ] {
            let answer = bind_reply_secret(reply(&format!(r#"{{"value":"{wrong}"}}"#)));
            assert!(matches!(answer, Err(KeychainError::Failed(_))), "{wrong:?}");
        }
        for wrong in ["", "not base64!"] {
            let answer = bind_reply_blob(reply(&format!(r#"{{"value":"{wrong}"}}"#)));
            assert!(matches!(answer, Err(KeychainError::Failed(_))), "{wrong:?}");
        }
        assert!(matches!(
            bind_reply_secret(reply(r#"{"unsupported":false}"#)),
            Err(KeychainError::Failed(_))
        ));
        assert!(matches!(
            bind_reply_blob(reply("{}")),
            Err(KeychainError::Failed(_))
        ));

        // No Secure Enclave (the simulator): unsupported, whatever else came along.
        assert_eq!(
            bind_reply_blob(reply(r#"{"unsupported":true}"#)),
            Err(KeychainError::Unsupported)
        );
        assert_eq!(
            bind_reply_secret(reply(r#"{"unsupported":true,"value":"QgAB/w=="}"#)).map(|_| ()),
            Err(KeychainError::Unsupported)
        );

        // A failure is classified, with or without its underlying errors.
        assert_eq!(
            bind_reply_secret(reply(
                r#"{"unsupported":false,"failure":{"stage":"restore","domain":"CryptoTokenKit","code":-3}}"#
            ))
            .map(|_| ()),
            Err(KeychainError::Invalidated)
        );
        assert!(matches!(
            bind_reply_blob(reply(
                r#"{"failure":{"stage":"seal","domain":"NSOSStatusErrorDomain","code":-25308,"underlying":[]}}"#
            )),
            Err(KeychainError::Failed(_))
        ));
    }

    #[test]
    fn bind_stages_read_by_their_wire_names() {
        for (wire, stage) in [
            ("seal", BindStage::Seal),
            ("parse", BindStage::Parse),
            ("restore", BindStage::Restore),
            ("agree", BindStage::Agree),
            ("open", BindStage::Open),
        ] {
            assert_eq!(
                serde_json::from_str::<BindStage>(&format!("\"{wire}\"")).expect("json"),
                stage
            );
        }
        assert!(serde_json::from_str::<BindStage>("\"Restore\"").is_err());
    }

    /// The macOS table (`desktop/macos.rs`, `MoonyBiometrics.swift` `classified`), row by row,
    /// for the phone's binding, which Rust classifies itself.
    #[test]
    fn ios_binding_failures_classify_like_macos() {
        let invalidated = [
            // A blob that does not parse: whatever the parser said.
            failure(BindStage::Parse, MOONY_BINDING_DOMAIN, 1),
            failure(BindStage::Parse, CRYPTO_KIT, 3),
            failure(BindStage::Parse, "SomethingElse", 0),
            // A key handle another Secure Enclave rejects ("corrupted objectID detected").
            failure(BindStage::Restore, TOKEN, -3),
            wrapped(BindStage::Restore, (TOKEN, -3), (CRYPTO_KIT, 1)),
            failure(BindStage::Restore, CRYPTO_KIT, 1),
            failure(BindStage::Restore, CRYPTO_KIT, 0),
            failure(BindStage::Restore, OS_STATUS, -25300), // errSecItemNotFound
            failure(BindStage::Restore, OS_STATUS, -50),    // errSecParam
            // An agreement the key refuses.
            failure(BindStage::Agree, OS_STATUS, -25293), // errSecAuthFailed
            failure(BindStage::Agree, TOKEN, -3),
            wrapped(BindStage::Agree, (TOKEN, -3), (OS_STATUS, -25293)),
            // A box that does not open (CryptoKit authenticationFailure), or holds no secret.
            failure(BindStage::Open, CRYPTO_KIT, 2),
            failure(BindStage::Open, MOONY_BINDING_DOMAIN, 2),
        ];
        for row in &invalidated {
            assert_eq!(
                classify_ios_binding(row),
                KeychainError::Invalidated,
                "{row:?}"
            );
        }

        let failed = [
            // The phone is locked: errSecInteractionNotAllowed, bare or under a key error.
            failure(BindStage::Restore, OS_STATUS, -25308),
            wrapped(BindStage::Restore, (TOKEN, -3), (OS_STATUS, -25308)),
            wrapped(BindStage::Agree, (CRYPTO_KIT, 1), (OS_STATUS, -25308)),
            failure(BindStage::Agree, OS_STATUS, -25291), // errSecNotAvailable
            failure(BindStage::Agree, TOKEN, -2),         // communicationError
            failure(BindStage::Restore, TOKEN, -4),       // canceledByUser
            failure(BindStage::Restore, OS_STATUS, -128), // errSecUserCanceled
            failure(BindStage::Agree, LA, -6),
            // Sealing never says anything about another device.
            failure(BindStage::Seal, TOKEN, -3),
            failure(BindStage::Seal, CRYPTO_KIT, 2),
            failure(BindStage::Seal, OS_STATUS, -25300),
            failure(BindStage::Seal, MOONY_BINDING_DOMAIN, 2),
            // An error this table does not know is not proof of another device.
            failure(BindStage::Seal, "SomethingElse", 0),
            failure(BindStage::Restore, "SomethingElse", 0),
            failure(BindStage::Agree, "SomethingElse", 0),
            failure(BindStage::Open, "SomethingElse", 0),
            failure(BindStage::Restore, OS_STATUS, -99_999),
            failure(BindStage::Restore, TOKEN, -99),
        ];
        for row in &failed {
            assert!(
                matches!(classify_ios_binding(row), KeychainError::Failed(_)),
                "{row:?}"
            );
        }
    }

    /// A refusal of the moment says nothing about the phone: at every stage, bare or under any
    /// other error, it is `Failed` (the start goes on), never `Invalidated` (a re-join).
    #[test]
    fn no_momentary_failure_reads_as_invalidated() {
        let mut momentary: Vec<(&str, i64)> = MOMENTARY_OS_STATUS
            .iter()
            .map(|&code| (OS_STATUS, code))
            .collect();
        momentary.extend(MOMENTARY_TOKEN_ERRORS.iter().map(|&code| (TOKEN, code)));
        momentary.extend([(LA, -1), (LA, -2), (LA, -6), (LA, -8)]);
        assert!(MOMENTARY_OS_STATUS.contains(&-25308));
        assert!(MOMENTARY_OS_STATUS.contains(&-25291));
        for stage in ALL_STAGES {
            for &(domain, code) in &momentary {
                let rows = [
                    failure(stage, domain, code),
                    wrapped(stage, (TOKEN, -3), (domain, code)),
                    wrapped(stage, (CRYPTO_KIT, 2), (domain, code)),
                    wrapped(stage, (domain, code), (OS_STATUS, -25300)),
                ];
                for row in &rows {
                    assert!(
                        matches!(classify_ios_binding(row), KeychainError::Failed(_)),
                        "{row:?}"
                    );
                }
            }
        }
    }

    /// The binding has no prompt and is not biometric: its messages name the phone's secure
    /// storage, never Face ID or Touch ID, and quote codes only.
    #[test]
    fn binding_messages_name_the_secure_storage_not_biometry() {
        for stage in ALL_STAGES {
            for (domain, code) in [(OS_STATUS, -25308), (LA, -6), ("SomethingElse", 0)] {
                if stage == BindStage::Parse && domain == "SomethingElse" {
                    continue; // a blob that does not parse is invalidated, whatever the parser said
                }
                let KeychainError::Failed(message) =
                    classify_ios_binding(&failure(stage, domain, code))
                else {
                    panic!("{stage:?} {domain} {code} is a failure");
                };
                assert!(message.contains("secure storage"), "{message}");
                assert!(message.contains(&code.to_string()), "{message}");
                assert!(!message.contains("Face ID"), "{message}");
                assert!(!message.contains("Touch ID"), "{message}");
            }
        }
        let KeychainError::Failed(message) = bind_reply_secret(reply(r#"{"value":"AAAA"}"#))
            .map(|_| ())
            .expect_err("short")
        else {
            panic!("a failure");
        };
        assert!(!message.contains("Face ID") && !message.contains("Touch ID"));
    }
}
