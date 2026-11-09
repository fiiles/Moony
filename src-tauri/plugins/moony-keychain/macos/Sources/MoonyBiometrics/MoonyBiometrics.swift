// moony-keychain, macOS half: Touch ID unlock with a Secure Enclave key, device sync's binding to
// this Mac, and a clipboard copy that clears itself.
//
// The master key is wrapped for a Secure Enclave P-256 key-agreement key whose access control is
// `[.privateKeyUsage, .biometryCurrentSet]`: only this Mac's Secure Enclave can use it, and only
// after a Touch ID match against the fingerprints enrolled when it was made. Adding or removing a
// fingerprint invalidates it for good. The key never leaves the Secure Enclave; what the app keeps
// is its `dataRepresentation` (an opaque handle only this Secure Enclave can use) inside the blob,
// so deleting the blob file deletes the key. No keychain item and no entitlement are involved.
//
// The wrap is ECIES: an ephemeral P-256 key agrees with the Secure Enclave key's public half (no
// prompt needed for that), HKDF-SHA256 turns the shared secret into an AES-256-GCM key, and the
// ephemeral public key travels in the blob. Unwrapping repeats the agreement on the Secure
// Enclave's side, which is the step the finger unlocks.
//
// Blob v1, built and parsed only here:
//   0x01 | u16 BE length | SE key dataRepresentation | ephemeral public key x963 (65 B)
//        | AES.GCM combined (12 B nonce + 32 B ciphertext + 16 B tag)
//
// Device sync's binding (`moony_bind_seal` / `moony_bind_open`) is the same ECIES with its own
// first byte (0x42) and HKDF label, for a Secure Enclave key whose access control is
// `.privateKeyUsage` alone: no Touch ID, no prompt, but still only this Mac's Secure Enclave can
// use it, and only while the Mac is unlocked. Its handle travels in the blob as above, so there is
// no keychain item, no entitlement, and nothing an app update or reinstall loses. A blob from
// another Mac fails to restore its key there: "invalidated", which the app reads as "not this
// device". A Mac without a Secure Enclave (Intel without T2) answers "unsupported".
//
// Every exported result is an `SRData` whose first byte is a status code the Rust side decodes
// (`desktop/macos.rs`): 0 ok (the payload follows), 1 cancelled, 2 unavailable, 3 invalidated,
// 4 failed (a UTF-8 message follows), 5 unsupported. The secret only ever lives in `Data` and byte
// arrays, which are wiped when this file is done with them, never in a `String`, and no message
// quotes it. The `SRData` buffers that carry it across the bridge are swift-rs's and are freed, not
// wiped.
//
// The Touch ID calls block until the person answers the prompt, so Rust makes them off the main
// thread. The binding and clipboard calls never prompt.

import AppKit
import CryptoKit
import Foundation
import LocalAuthentication
import Security
import SwiftRs

/// The first byte and the HKDF label of a blob: Touch ID's and the binding's differ, so neither
/// blob opens as the other.
private struct BlobFormat {
  let version: UInt8
  let info: Data
}

private let touchIdFormat = BlobFormat(version: 1, info: Data("moony-touchid-v1".utf8))
private let bindingFormat = BlobFormat(version: 0x42, info: Data("moony-binding-v1".utf8))
/// How a message names what failed: Touch ID unlock speaks of Touch ID; the binding, which has no
/// prompt and is not biometric, speaks of this Mac's secure storage and never of Touch ID.
private struct Wording {
  /// What failed, as a sentence subject: "Touch ID failed: …".
  let subject: String
  /// The key, as a sentence subject: "The binding key is gone: …".
  let keyName: String
}

private let touchIdWording = Wording(subject: "Touch ID", keyName: "The Touch ID key")
private let bindingWording = Wording(
  subject: "This Mac's secure storage", keyName: "The device binding key")

/// The marker types clipboard managers honour (nspasteboard.org): concealed (keep no copy, show
/// no preview) and transient (the item is temporary, do not record it).
private let concealedType = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
private let transientType = NSPasteboard.PasteboardType("org.nspasteboard.TransientType")
private let secretLength = 32
private let x963Length = 65
private let sealedLength = 12 + 32 + 16

private enum StatusCode: UInt8 {
  case ok = 0
  case cancelled = 1
  case unavailable = 2
  case invalidated = 3
  case failed = 4
  case unsupported = 5
}

private struct BiometricsError: Error {
  let code: StatusCode
  let message: String

  static func cancelled() -> BiometricsError {
    BiometricsError(code: .cancelled, message: "")
  }

  static func unsupported() -> BiometricsError {
    BiometricsError(code: .unsupported, message: "")
  }
}

/// The key the blob is wrapped for: the Secure Enclave key in the app, a software key in the
/// selftest. `keyData` is what goes into the blob to find the key again.
private struct Recipient {
  let keyData: Data
  let publicKey: P256.KeyAgreement.PublicKey
  let agree: (P256.KeyAgreement.PublicKey) throws -> SharedSecret
}

private struct ParsedBlob {
  let keyData: Data
  let ephemeralPublicKey: Data
  let sealed: Data
}

// MARK: - Exports

/// -1: no usable biometrics (no sensor, nothing enrolled, locked out, or no Secure Enclave);
/// 0: biometrics usable but of no type the app can name; 1: Touch ID (any other type on a Mac).
@_cdecl("moony_bio_status")
public func moonyBioStatus() -> Int {
  let context = LAContext()
  var policyError: NSError?
  guard SecureEnclave.isAvailable,
    context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &policyError)
  else {
    return -1
  }
  return context.biometryType == .none ? 0 : 1
}

/// Confirms with Touch ID (the person switching biometric unlock on), then makes a new Secure
/// Enclave key and wraps the 32-byte secret for it. Payload: the blob.
@_cdecl("moony_bio_wrap")
public func moonyBioWrap(reason: SRString, secret: SRData) -> SRData {
  return reply {
    var bytes = secret.toArray()
    defer { wipe(&bytes) }
    guard bytes.count == secretLength else {
      throw BiometricsError(code: .failed, message: "The secret is not \(secretLength) bytes")
    }

    let context = biometricContext(reason: reason.toString())
    try authenticate(context)

    var accessError: Unmanaged<CFError>?
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        [.privateKeyUsage, .biometryCurrentSet], &accessError)
    else {
      let detail = accessError.map { describe($0.takeRetainedValue()) } ?? "unknown"
      throw BiometricsError(
        code: .failed, message: "Could not create the access control: \(detail)")
    }
    let key: SecureEnclave.P256.KeyAgreement.PrivateKey
    do {
      key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
        accessControl: access, authenticationContext: context)
    } catch {
      throw BiometricsError(
        code: .failed, message: "Could not create the Secure Enclave key: \(describe(error))")
    }

    let recipient = Recipient(
      keyData: key.dataRepresentation, publicKey: key.publicKey,
      agree: key.sharedSecretFromKeyAgreement)
    return Array(try seal(bytes, for: recipient, format: touchIdFormat))
  }
}

/// Unwraps a blob `moony_bio_wrap` made, behind a Touch ID prompt showing `reason`. Payload: the
/// 32-byte secret.
@_cdecl("moony_bio_unwrap")
public func moonyBioUnwrap(reason: SRString, blob: SRData) -> SRData {
  return reply {
    let context = biometricContext(reason: reason.toString())
    return try open(
      Data(blob.toArray()), format: touchIdFormat,
      restore: { keyData in
        // Restoring needs no prompt; the key only asks for the finger when it is used, and then
        // takes the context authenticated below instead of asking again.
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
          dataRepresentation: keyData, authenticationContext: context)
        return Recipient(
          keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
      },
      authorize: { try authenticate(context) }, wording: touchIdWording)
  }
}

/// Seals device sync's 32-byte binding for a new Secure Enclave key that needs no user presence.
/// Payload: the blob. Unsupported without a Secure Enclave.
@_cdecl("moony_bind_seal")
public func moonyBindSeal(secret: SRData) -> SRData {
  return reply(bindingWording) {
    var bytes = secret.toArray()
    defer { wipe(&bytes) }
    guard bytes.count == secretLength else {
      throw BiometricsError(code: .failed, message: "The secret is not \(secretLength) bytes")
    }
    guard SecureEnclave.isAvailable else {
      throw BiometricsError.unsupported()
    }

    var accessError: Unmanaged<CFError>?
    guard
      let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage],
        &accessError)
    else {
      let detail = accessError.map { describe($0.takeRetainedValue()) } ?? "unknown"
      throw BiometricsError(
        code: .failed, message: "Could not create the access control: \(detail)")
    }
    let key: SecureEnclave.P256.KeyAgreement.PrivateKey
    do {
      key = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: access)
    } catch {
      throw BiometricsError(
        code: .failed, message: "Could not create the device binding key: \(describe(error))")
    }

    let recipient = Recipient(
      keyData: key.dataRepresentation, publicKey: key.publicKey,
      agree: key.sharedSecretFromKeyAgreement)
    return Array(try seal(bytes, for: recipient, format: bindingFormat))
  }
}

/// Opens a blob `moony_bind_seal` made, without a prompt. Payload: the 32-byte binding.
/// Invalidated: the blob was sealed on another Mac (its key handle does not restore here) or is
/// not a binding blob. Unsupported without a Secure Enclave.
@_cdecl("moony_bind_open")
public func moonyBindOpen(blob: SRData) -> SRData {
  return reply(bindingWording) {
    guard SecureEnclave.isAvailable else {
      throw BiometricsError.unsupported()
    }
    return try open(
      Data(blob.toArray()), format: bindingFormat,
      restore: { keyData in
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(dataRepresentation: keyData)
        return Recipient(
          keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
      },
      authorize: {}, wording: bindingWording)
  }
}

/// Puts `text` on the general pasteboard as the only item, for this Mac only (`.currentHostOnly`:
/// never handed to Universal Clipboard on the person's other devices), with the concealed and
/// transient marker types, and answers the pasteboard's change count after taking it over (-1
/// when the write was refused).
/// The text crosses the bridge as a swift-rs string and lives here as a Swift `String`, neither of
/// which can be wiped: the accepted residual of copying a secret at all.
@_cdecl("moony_clip_copy")
public func moonyClipCopy(text: SRString) -> Int {
  let pasteboard = NSPasteboard.general
  // Taking the pasteboard over is what changes its count; writing the item does not.
  let count = pasteboard.prepareForNewContents(with: .currentHostOnly)
  let item = NSPasteboardItem()
  guard item.setString(text.toString(), forType: .string),
    item.setData(Data(), forType: concealedType),
    item.setData(Data(), forType: transientType),
    pasteboard.writeObjects([item])
  else {
    pasteboard.clearContents()
    return -1
  }
  return count
}

/// Empties the general pasteboard if its change count is still `count`, that is, if nobody copied
/// anything since `moony_clip_copy` answered it.
@_cdecl("moony_clip_clear_if")
public func moonyClipClearIf(count: Int) -> Bool {
  let pasteboard = NSPasteboard.general
  guard pasteboard.changeCount == count else {
    return false
  }
  pasteboard.clearContents()
  return true
}

/// The wrap and unwrap code above with a software P-256 key in place of the Secure Enclave key, so
/// a test can prove the blob format and the ECIES round trip without a finger on the sensor.
@_cdecl("moony_bio_selftest")
public func moonyBioSelftest() -> Bool {
  let softwareKey = P256.KeyAgreement.PrivateKey()
  let restore: (Data) throws -> Recipient = { keyData in
    let key = try P256.KeyAgreement.PrivateKey(rawRepresentation: keyData)
    return Recipient(
      keyData: keyData, publicKey: key.publicKey, agree: key.sharedSecretFromKeyAgreement)
  }
  let secret = (0..<secretLength).map { UInt8($0) }
  let recipient = Recipient(
    keyData: softwareKey.rawRepresentation, publicKey: softwareKey.publicKey,
    agree: softwareKey.sharedSecretFromKeyAgreement)
  let refused: (Data, BlobFormat) -> Bool = { blob, format in
    do {
      _ = try open(blob, format: format, restore: restore, authorize: {}, wording: touchIdWording)
      return false
    } catch let error as BiometricsError where error.code == .invalidated {
      return true
    } catch {
      return false
    }
  }
  do {
    for (format, other) in [(touchIdFormat, bindingFormat), (bindingFormat, touchIdFormat)] {
      let blob = try seal(secret, for: recipient, format: format)
      guard blob.first == format.version,
        try open(blob, format: format, restore: restore, authorize: {}, wording: touchIdWording)
          == secret
      else { return false }

      // A flipped ciphertext bit, a cut blob and the other format must all be refused as
      // invalidated.
      var tampered = blob
      tampered[tampered.count - 20] ^= 0x01
      var relabelled = blob
      relabelled[0] = other.version
      for broken in [tampered, Data(blob.prefix(blob.count - 1))] {
        guard refused(broken, format) else { return false }
      }
      guard refused(blob, other), refused(relabelled, other) else { return false }
    }
    return true
  } catch {
    return false
  }
}

/// For the Rust tests: the status code `restoreFailure` (`stage` 0), `agreementFailure` (`stage`
/// 1) or the binding's `restoreFailure` (`stage` 2: its key needs no prompt, so a restore and an
/// agreement failure are classified alike) gives an error in `domain` with `code`, bare or
/// (`wrapped`) as the underlying error of an unrelated outer error, the way CryptoKit and
/// CryptoTokenKit wrap them.
@_cdecl("moony_bio_classify")
public func moonyBioClassify(stage: Int, domain: SRString, code: Int, wrapped: Bool) -> Int {
  return Int(classified(stage: stage, domain: domain.toString(), code: code, wrapped: wrapped)
    .code.rawValue)
}

/// For the Rust tests: the message `moony_bio_classify`'s classification carries.
@_cdecl("moony_bio_classify_message")
public func moonyBioClassifyMessage(stage: Int, domain: SRString, code: Int, wrapped: Bool)
  -> SRString
{
  return SRString(
    classified(stage: stage, domain: domain.toString(), code: code, wrapped: wrapped).message)
}

private func classified(stage: Int, domain: String, code: Int, wrapped: Bool) -> BiometricsError {
  var error = NSError(domain: domain, code: code)
  if wrapped {
    error = NSError(
      domain: "CryptoTokenKit", code: -3, userInfo: [NSUnderlyingErrorKey: error])
  }
  switch stage {
  case 0: return restoreFailure(error, wording: touchIdWording)
  case 1: return agreementFailure(error, wording: touchIdWording)
  default: return restoreFailure(error, wording: bindingWording)
  }
}

// MARK: - ECIES

private func seal(
  _ secret: [UInt8], for recipient: Recipient, format: BlobFormat
) throws -> Data {
  let ephemeral = P256.KeyAgreement.PrivateKey()
  let ephemeralPublic = ephemeral.publicKey.x963Representation
  let sealed: Data
  var plaintext = Data(secret)
  defer { wipe(&plaintext) }
  do {
    let shared = try ephemeral.sharedSecretFromKeyAgreement(with: recipient.publicKey)
    let key = wrappingKey(
      shared, ephemeralPublic: ephemeralPublic,
      recipientPublic: recipient.publicKey.x963Representation, info: format.info)
    guard let combined = try AES.GCM.seal(plaintext, using: key).combined else {
      throw BiometricsError(code: .failed, message: "AES-GCM gave no combined box")
    }
    sealed = combined
  } catch let error as BiometricsError {
    throw error
  } catch {
    throw BiometricsError(code: .failed, message: "Could not wrap the secret: \(describe(error))")
  }

  guard recipient.keyData.count <= Int(UInt16.max) else {
    throw BiometricsError(code: .failed, message: "The key handle is too long for the blob")
  }
  var blob = Data([format.version])
  blob.append(UInt8(recipient.keyData.count >> 8))
  blob.append(UInt8(recipient.keyData.count & 0xff))
  blob.append(recipient.keyData)
  blob.append(ephemeralPublic)
  blob.append(sealed)
  return blob
}

/// Parses the blob, finds the key again, lets `authorize` prompt, then agrees and decrypts. A blob
/// that does not parse, a key that is gone, an agreement the key refuses and a box that does not
/// open are all "invalidated": the blob can never be read again. `wording` words the messages.
private func open(
  _ blob: Data, format: BlobFormat, restore: (Data) throws -> Recipient,
  authorize: () throws -> Void, wording: Wording
) throws -> [UInt8] {
  let parsed = try parse(blob, format: format)
  let recipient: Recipient
  let ephemeralPublic: P256.KeyAgreement.PublicKey
  do {
    ephemeralPublic = try P256.KeyAgreement.PublicKey(x963Representation: parsed.ephemeralPublicKey)
  } catch {
    throw BiometricsError(code: .invalidated, message: "The blob's ephemeral key is malformed")
  }
  do {
    recipient = try restore(parsed.keyData)
  } catch {
    throw restoreFailure(error, wording: wording)
  }

  try authorize()

  let shared: SharedSecret
  do {
    shared = try recipient.agree(ephemeralPublic)
  } catch {
    throw agreementFailure(error, wording: wording)
  }

  let key = wrappingKey(
    shared, ephemeralPublic: parsed.ephemeralPublicKey,
    recipientPublic: recipient.publicKey.x963Representation, info: format.info)
  var plaintext: Data
  do {
    plaintext = try AES.GCM.open(AES.GCM.SealedBox(combined: parsed.sealed), using: key)
  } catch {
    throw BiometricsError(code: .invalidated, message: "The blob does not open")
  }
  defer { wipe(&plaintext) }
  guard plaintext.count == secretLength else {
    throw BiometricsError(
      code: .invalidated, message: "The blob holds no \(secretLength)-byte secret")
  }
  return Array(plaintext)
}

private func parse(_ blob: Data, format: BlobFormat) throws -> ParsedBlob {
  // `Data` slices keep their parent's indices; work on a copy that starts at zero.
  let bytes = Data(blob)
  guard bytes.count >= 3, bytes[0] == format.version else {
    throw BiometricsError(code: .invalidated, message: "The blob is not of the expected format")
  }
  let keyLength = Int(bytes[1]) << 8 | Int(bytes[2])
  guard bytes.count == 3 + keyLength + x963Length + sealedLength else {
    throw BiometricsError(code: .invalidated, message: "The blob has the wrong length")
  }
  let keyEnd = 3 + keyLength
  let ephemeralEnd = keyEnd + x963Length
  return ParsedBlob(
    keyData: Data(bytes[3..<keyEnd]),
    ephemeralPublicKey: Data(bytes[keyEnd..<ephemeralEnd]),
    sealed: Data(bytes[ephemeralEnd...]))
}

/// HKDF-SHA256 over the shared secret, salted with both public keys so the AES key is bound to this
/// exact pair, and labelled with the format (`info`).
private func wrappingKey(
  _ shared: SharedSecret, ephemeralPublic: Data, recipientPublic: Data, info: Data
) -> SymmetricKey {
  return shared.hkdfDerivedSymmetricKey(
    using: SHA256.self, salt: ephemeralPublic + recipientPublic, sharedInfo: info,
    outputByteCount: 32)
}

// MARK: - Touch ID

/// Biometry only: no "Enter Password" button, so the Mac login password never releases the key.
/// LAContext refuses an empty reason, hence the app's name in its place.
private func biometricContext(reason: String) -> LAContext {
  let context = LAContext()
  context.localizedReason = reason.isEmpty ? "Moony" : reason
  context.localizedFallbackTitle = ""
  return context
}

/// Shows the Touch ID prompt and blocks until the person answers it. The reply comes on a private
/// queue, so waiting here never deadlocks, whichever thread the caller is on (not the main one).
private func authenticate(_ context: LAContext) throws {
  let answered = DispatchSemaphore(value: 0)
  let outcome = PromptOutcome()
  context.evaluatePolicy(
    .deviceOwnerAuthenticationWithBiometrics, localizedReason: context.localizedReason
  ) { success, error in
    if !success {
      outcome.failure =
        error ?? BiometricsError(code: .failed, message: "Touch ID failed without a reason")
    }
    answered.signal()
  }
  answered.wait()
  if let failure = outcome.failure {
    throw classify(failure, wording: touchIdWording)
  }
}

/// The prompt's answer, written on LocalAuthentication's queue and read after the semaphore, which
/// orders the two.
private final class PromptOutcome: @unchecked Sendable {
  var failure: Error?
}

private func classify(_ error: Error, wording: Wording) -> BiometricsError {
  if let error = error as? BiometricsError {
    return error
  }
  guard let laError = error as? LAError else {
    return BiometricsError(
      code: .failed, message: "\(wording.subject) failed: \(describe(error))")
  }
  switch laError.code {
  case .userCancel, .appCancel, .systemCancel, .userFallback, .notInteractive:
    // `.notInteractive`: the prompt came while the app could not show it; the person tries again.
    return .cancelled()
  case .biometryNotAvailable, .biometryNotEnrolled, .biometryLockout, .passcodeNotSet:
    return BiometricsError(
      code: .unavailable,
      message: "\(wording.subject) is unavailable: \(laError.localizedDescription)")
  case .authenticationFailed:
    return BiometricsError(
      code: .failed, message: "\(wording.subject) did not recognise the fingerprint")
  default:
    return BiometricsError(
      code: .failed, message: "\(wording.subject) failed: \(laError.localizedDescription)")
  }
}

/// The Secure Enclave key could not be restored from its handle (before any prompt). Invalidated
/// only when the handle itself is unusable; a refusal of the moment (no interaction allowed in
/// this session, the Secure Enclave not reachable, a missing entitlement) is a failure, so the app
/// keeps biometric unlock on and the next attempt can work.
///
/// For the binding, "unusable" is what a handle from another Mac's Secure Enclave gives
/// (CryptoTokenKit `corruptedData`, "corrupted objectID detected"), and anything this does not
/// recognise as momentary counts as such: a needless re-join beats a check that silently passes.
private func restoreFailure(_ error: Error, wording: Wording) -> BiometricsError {
  if let answer = momentaryFailure(error, wording: wording) {
    return answer
  }
  return BiometricsError(
    code: .invalidated, message: "\(wording.keyName) is gone: \(describe(error))")
}

/// The key agreement failed after a successful prompt. If the Secure Enclave asked again, its
/// LocalAuthentication answer (a cancel, a misread finger, a lockout) is classified like the first
/// prompt's, and a refusal of the moment is a failure as in `restoreFailure`, so none of them
/// switches biometric unlock off. Anything else means the key refused an authenticated context:
/// the enrolment it was bound to is gone.
private func agreementFailure(_ error: Error, wording: Wording) -> BiometricsError {
  if let answer = momentaryFailure(error, wording: wording) {
    return answer
  }
  return BiometricsError(
    code: .invalidated, message: "\(wording.keyName) refused the unlock: \(describe(error))")
}

/// The answers that say nothing about the key itself, so biometric unlock stays on: a
/// LocalAuthentication answer (classified like the prompt's), a refusal of the moment (no
/// interaction allowed in this session, the Secure Enclave not reachable, a missing entitlement,
/// the token not answering) and a cancel. `nil` leaves the error to the caller.
private func momentaryFailure(_ error: Error, wording: Wording) -> BiometricsError? {
  if let laError = laError(in: error) {
    return classify(laError, wording: wording)
  }
  let transient: Set<Int> = [
    Int(errSecInteractionNotAllowed), Int(errSecNotAvailable), Int(errSecMissingEntitlement),
  ]
  if let status = osStatus(in: error), transient.contains(status) {
    return BiometricsError(
      code: .failed, message: "\(wording.keyName) cannot be used right now: \(describe(error))")
  }
  if osStatus(in: error) == Int(errSecUserCanceled)
    || hasTokenError(in: error, code: tokenCanceledByUser)
  {
    return .cancelled()
  }
  if hasTokenError(in: error, code: tokenCommunicationError) {
    return BiometricsError(
      code: .failed, message: "The Secure Enclave did not answer: \(describe(error))")
  }
  return nil
}

/// The first LocalAuthentication error in `error` or its underlying errors (CryptoKit and
/// CryptoTokenKit wrap them).
private func laError(in error: Error) -> LAError? {
  return firstError(in: error) { $0.domain == LAErrorDomain }.flatMap { found in
    LAError.Code(rawValue: found.code).map { LAError($0, userInfo: found.userInfo) }
  }
}

/// The first OSStatus in `error` or its underlying errors.
private func osStatus(in error: Error) -> Int? {
  return firstError(in: error) { $0.domain == NSOSStatusErrorDomain }?.code
}

/// CryptoTokenKit's error domain (`TKErrorDomain`) and the two `TKError` codes that are not about
/// the key: `canceledByUser` (-4) and `communicationError` (-2). Spelled out so the package does
/// not link CryptoTokenKit for three constants.
private let tokenErrorDomain = "CryptoTokenKit"
private let tokenCanceledByUser = -4
private let tokenCommunicationError = -2

/// Whether `error` or any of its underlying errors is the CryptoTokenKit error `code`. Any, not the
/// first: the code can sit under an outer error of the same domain.
private func hasTokenError(in error: Error, code: Int) -> Bool {
  return firstError(in: error) { $0.domain == tokenErrorDomain && $0.code == code } != nil
}

private func firstError(in error: Error, where matches: (NSError) -> Bool) -> NSError? {
  var current: NSError? = error as NSError
  while let candidate = current {
    if matches(candidate) {
      return candidate
    }
    current = candidate.userInfo[NSUnderlyingErrorKey] as? NSError
  }
  return nil
}

// MARK: - Helpers

/// Runs `body` and frames its answer for Rust: the status byte, then the payload or the message,
/// worded by `wording`. The payload array is wiped once it is copied into the reply.
private func reply(_ wording: Wording = touchIdWording, _ body: () throws -> [UInt8]) -> SRData {
  do {
    var payload = try body()
    defer { wipe(&payload) }
    var framed = [UInt8]()
    framed.reserveCapacity(1 + payload.count)
    framed.append(StatusCode.ok.rawValue)
    framed.append(contentsOf: payload)
    return SRData(framed)
  } catch {
    let failure = classify(error, wording: wording)
    return SRData([failure.code.rawValue] + Array(failure.message.utf8))
  }
}

/// Domain and code only: an error's description could in principle quote a value it was given.
private func describe(_ error: Error) -> String {
  let failure = error as NSError
  return "\(failure.domain) \(failure.code)"
}

private func wipe(_ bytes: inout [UInt8]) {
  bytes.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}

private func wipe(_ data: inout Data) {
  data.withUnsafeMutableBytes { buffer in
    guard let base = buffer.baseAddress else { return }
    _ = memset_s(base, buffer.count, 0, buffer.count)
  }
}
